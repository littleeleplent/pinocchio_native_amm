use crate::state::Config;
use core::mem::size_of;
use core::mem::MaybeUninit;
use pinocchio::{
    cpi::{Seed, Signer},
    error::ProgramError,
    AccountView, Address, ProgramResult,
};
use pinocchio_system::instructions::CreateAccount;
use pinocchio_system::ID as SYSTEM_PROGRAM_ID;
use pinocchio_token::instructions::InitializeMint2;
use pinocchio_token::ID as TOKEN_PROGRAM_ID;

/// ========== 初始化指令所需的账户 ==========
/// 
/// 初始化 AMM 流动性池需要的所有账户引用，包括初始化者、各个账户和所需的程序。
pub struct InitializeAccounts<'a> {
    /// 初始化 AMM 的用户账户（必须是签名者）
    pub initializer: &'a AccountView,
    /// LP 代币的 mint 账户（创建时为空，需要初始化）
    pub mint_lp: &'a AccountView,
    /// AMM 配置账户（PDA，存储所有池参数）
    pub config: &'a AccountView,
    /// Solana 系统程序（用于创建账户）
    pub system_program: &'a AccountView,
    /// SPL Token 程序（用于初始化 mint 和 token 账户）
    pub token_program: &'a AccountView,
}

impl<'a> TryFrom<&'a [AccountView]> for InitializeAccounts<'a> {
    type Error = ProgramError;

    /// 从原始账户数组构建 InitializeAccounts，进行所有验证
    fn try_from(accounts: &'a [AccountView]) -> Result<Self, Self::Error> {
        let [initializer, mint_lp, config, system_program, token_program] = accounts else {
            return Err(ProgramError::NotEnoughAccountKeys);
        };

        // ============ Initializer 验证 ============
        // 确保初始化者是交易的签名者（已支付交易费用）
        if !initializer.is_signer() {
            return Err(ProgramError::MissingRequiredSignature);
        }
        
        // ============ System Program 验证 ============
        // 验证提供的是真实的 Solana 系统程序
        if system_program.address() != &SYSTEM_PROGRAM_ID {
            return Err(ProgramError::InvalidArgument);
        }

        // ============ Token Program 验证 ============
        // 验证提供的是真实的 SPL Token 程序
        if token_program.address() != &TOKEN_PROGRAM_ID {
            return Err(ProgramError::InvalidArgument);
        }

        Ok(Self {
            initializer,
            mint_lp,
            config,
            system_program,
            token_program,
        })
    }
}

/// ========== 初始化指令的数据结构 ==========
/// 
/// 包含初始化 AMM 所需的所有参数信息，采用紧凑布局便于序列化/反序列化。
#[repr(C, packed)]
pub struct InitializeInstructionData {
    /// 用于生成 PDA 的种子值
    pub seed: u64,
    /// 交换费用（bps，范围 0-9999）
    pub fee: u16,
    /// 代币 X 的 mint 地址（32 字节）
    pub mint_x: [u8; 32],
    /// 代币 Y 的 mint 地址（32 字节）
    pub mint_y: [u8; 32],
    /// 配置 PDA 的 bump seed
    pub config_bump: [u8; 1],
    /// LP mint PDA 的 bump seed
    pub lp_bump: [u8; 1],
    /// 权限管理员地址（可选，如果不提供则为零地址）
    pub authority: [u8; 32],
}

impl TryFrom<&[u8]> for InitializeInstructionData {
    type Error = ProgramError;

    /// 从字节数组解析初始化数据，支持带或不带 authority 的格式
    fn try_from(data: &[u8]) -> Result<Self, Self::Error> {
        // 计算不同格式的数据长度
        const INITIALIZE_DATA_LEN_WITH_AUTHORITY: usize = size_of::<InitializeInstructionData>();
        const INITIALIZE_DATA_LEN: usize =
            INITIALIZE_DATA_LEN_WITH_AUTHORITY - size_of::<[u8; 32]>();

        let instruction_data = match data.len() {
            // 完整格式：包含 authority 字段
            INITIALIZE_DATA_LEN_WITH_AUTHORITY => unsafe {
                (data.as_ptr() as *const Self).read_unaligned()
            },
            // 简化格式：不包含 authority，需要补充零字节
            INITIALIZE_DATA_LEN => {
                let mut raw: MaybeUninit<[u8; INITIALIZE_DATA_LEN_WITH_AUTHORITY]> =
                    MaybeUninit::uninit();
                let raw_ptr = raw.as_mut_ptr() as *mut u8;
                unsafe {
                    // 复制已提供的数据
                    core::ptr::copy_nonoverlapping(data.as_ptr(), raw_ptr, INITIALIZE_DATA_LEN);
                    // 将 authority 字段填充为零（表示无特定权限管理员）
                    core::ptr::write_bytes(raw_ptr.add(INITIALIZE_DATA_LEN), 0, 32);
                    // 转换为目标结构体
                    (raw.as_ptr() as *const Self).read_unaligned()
                }
            }
            _ => return Err(ProgramError::InvalidInstructionData),
        };

        Ok(instruction_data)
    }
}

/// ========== Initialize 指令实现 ==========
/// 
/// 初始化新的 AMM 流动性池，创建配置 PDA、LP mint 和相关账户。
pub struct Initialize<'a> {
    /// 初始化所需的账户
    pub accounts: InitializeAccounts<'a>,
    /// 初始化指令的参数
    pub instruction_data: InitializeInstructionData,
}

impl<'a> TryFrom<(&'a [u8], &'a [AccountView])> for Initialize<'a> {
    type Error = ProgramError;

    /// 从指令数据和账户数组构建 Initialize 结构体
    fn try_from((data, accounts): (&'a [u8], &'a [AccountView])) -> Result<Self, Self::Error> {
        let accounts = InitializeAccounts::try_from(accounts)?;
        let instruction_data: InitializeInstructionData =
            InitializeInstructionData::try_from(data)?;

        // 验证 mint_x 和 mint_y 是不同的代币
        if instruction_data.mint_x == instruction_data.mint_y {
            return Err(ProgramError::InvalidArgument);
        }

        Ok(Self {
            accounts,
            instruction_data,
        })
    }
}

impl<'a> Initialize<'a> {
    /// 该指令的鉴别器值（0 表示初始化指令）
    pub const DISCRIMINATOR: &'a u8 = &0;

    /// 执行初始化流程
    /// 
    /// 整个流程包括：
    /// 1. 创建配置 PDA 账户并初始化
    /// 2. 创建 LP mint PDA 账户并初始化
    pub fn process(&mut self) -> ProgramResult {
        use pinocchio::sysvars::Sysvar;

        // ============ 准备 Config PDA 的签名种子 ============
        let seed_binding = self.instruction_data.seed.to_le_bytes();
        let config_seeds = [
            Seed::from(b"config"),
            Seed::from(&seed_binding),
            Seed::from(&self.instruction_data.mint_x),
            Seed::from(&self.instruction_data.mint_y),
            Seed::from(&self.instruction_data.config_bump),
        ];

        // 将字节数组转换为 Address 类型
        let mint_x = Address::new_from_array(self.instruction_data.mint_x);
        let mint_y = Address::new_from_array(self.instruction_data.mint_y);
        let authority = Address::new_from_array(self.instruction_data.authority);

        // ============ 第1步：创建 Config PDA 账户 ============
        // 获取当前租期信息以计算创建账户所需的 lamports
        let rent = pinocchio::sysvars::rent::Rent::get()?;
        let config_lamports = rent
            .try_minimum_balance(Config::LEN)
            .map_err(|_| ProgramError::Custom(1))?;
        
        // 创建 Config 账户，使用生成的 PDA 进行签名
        let cfsigner = [Signer::from(&config_seeds)];
        CreateAccount {
            from: self.accounts.initializer,
            to: self.accounts.config,
            lamports: config_lamports,
            space: Config::LEN as u64,
            owner: &crate::ID,
        }
        .invoke_signed(&cfsigner)?;
        
        // ============ 第2步：初始化 Config PDA 账户数据 ============
        // 将所有配置参数写入新创建的账户
        {
            let mut config = Config::load_mut(self.accounts.config)?;
            config.set_inner(
                self.instruction_data.seed,
                authority,
                mint_x,
                mint_y,
                self.instruction_data.fee,
                self.instruction_data.config_bump,
            )?;
        }

        // ============ 第3步：创建 LP Mint PDA 账户 ============
        let mint_lp_seeds = [
            Seed::from(b"mint_lp"),
            Seed::from(self.accounts.config.address().as_ref()),
            Seed::from(&self.instruction_data.lp_bump),
        ];

        // 计算 Mint 账户所需的 lamports（SPL Mint 固定大小为 82 字节）
        let mint_lamports = rent
            .try_minimum_balance(82)
            .map_err(|_| ProgramError::Custom(2))?;

        CreateAccount {
            from: self.accounts.initializer,
            to: self.accounts.mint_lp,
            lamports: mint_lamports,
            space: 82,
            owner: &TOKEN_PROGRAM_ID,
        }
        .invoke_signed(&[Signer::from(&mint_lp_seeds)])?;

        // ============ 第4步：初始化 LP Mint 数据 ============
        InitializeMint2 {
            mint: self.accounts.mint_lp,
            decimals: 6,
            mint_authority: self.accounts.config.address(),
            freeze_authority: None,
        }
        .invoke()?;

        Ok(())
    }
}
