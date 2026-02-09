use crate::state::{AmmState, Config};
use constant_product_curve::{ConstantProduct, LiquidityPair};
use core::mem::size_of;

use pinocchio::{
    cpi::{Seed, Signer},
    error::ProgramError,
    sysvars::{clock::Clock, Sysvar},
    AccountView, ProgramResult,
};
use pinocchio_token::instructions::Transfer;
use pinocchio_token::state::TokenAccount;
use pinocchio_token::ID as TOKEN_PROGRAM_ID;

/// ========== 交换指令所需的账户 ==========
/// 
/// 用户进行代币交换时所需的所有账户。
pub struct SwapAccounts<'a> {
    /// 执行交换的用户账户（必须是签名者）
    pub user: &'a AccountView,
    /// 用户的代币 X 关联代币账户（可能是输入或输出账户）
    pub user_x_ata: &'a AccountView,
    /// 用户的代币 Y 关联代币账户（可能是输入或输出账户）
    pub user_y_ata: &'a AccountView,
    /// 代币 X 的金库账户（接收或转出代币 X）
    pub vault_x: &'a AccountView,
    /// 代币 Y 的金库账户（接收或转出代币 Y）
    pub vault_y: &'a AccountView,
    /// AMM 配置账户（包含池参数）
    pub config: &'a AccountView,
    /// SPL Token 程序
    pub token_program: &'a AccountView,
}

impl<'a> TryFrom<&'a [AccountView]> for SwapAccounts<'a> {
    type Error = ProgramError;

    /// 验证和提取交换指令所需的账户
    fn try_from(accounts: &'a [AccountView]) -> Result<Self, Self::Error> {
        let [user, user_x_ata, user_y_ata, vault_x, vault_y, config, token_program] = accounts
        else {
            return Err(ProgramError::NotEnoughAccountKeys);
        };

        // 验证用户是交易签名者
        if !user.is_signer() {
            return Err(ProgramError::MissingRequiredSignature);
        }

        // ============ Config 账户验证 ============
        // 验证 Config 账户的大小和所有权
        if config.data_len() != Config::LEN {
            return Err(ProgramError::InvalidAccountData);
        }
        if !config.owned_by(&crate::ID) {
            return Err(ProgramError::InvalidAccountOwner);
        }

        // ============ Token Program 验证 ============
        // 确保使用的是真实的 SPL Token 程序
        if token_program.address() != &TOKEN_PROGRAM_ID {
            return Err(ProgramError::InvalidArgument);
        }

        // ============ 代币账户格式和所有权验证 ============
        // 验证所有相关的代币账户都由 Token 程序拥有且格式正确
        if user_x_ata.data_len() != TokenAccount::LEN
            || !user_x_ata.owned_by(token_program.address())
        {
            return Err(ProgramError::InvalidAccountOwner);
        }
        if user_y_ata.data_len() != TokenAccount::LEN
            || !user_y_ata.owned_by(token_program.address())
        {
            return Err(ProgramError::InvalidAccountOwner);
        }
        if vault_x.data_len() != TokenAccount::LEN || !vault_x.owned_by(token_program.address()) {
            return Err(ProgramError::InvalidAccountOwner);
        }
        if vault_y.data_len() != TokenAccount::LEN || !vault_y.owned_by(token_program.address()) {
            return Err(ProgramError::InvalidAccountOwner);
        }

        Ok(SwapAccounts {
            user,
            user_x_ata,
            user_y_ata,
            vault_x,
            vault_y,
            config,
            token_program,
        })
    }
}

/// ========== 交换指令的数据结构 ==========
/// 
/// 包含用户交换代币时的所有参数。
#[repr(C, packed)]
pub struct SwapInstructionData {
    /// 标志位：true 表示用 X 换 Y，false 表示用 Y 换 X
    pub is_x: bool,
    /// 用户想要交换的代币数量（输入代币的数量）
    pub amount: u64,
    /// 用户愿意接收的最少输出代币数量（滑点保护）
    pub min: u64,
    /// 交易过期时间（Unix 时间戳，0 表示不限制）
    pub expiration: i64,
}

impl<'a> TryFrom<&'a [u8]> for SwapInstructionData {
    type Error = ProgramError;

    /// 从字节数据解析交换指令参数，进行有效性检查
    fn try_from(data: &'a [u8]) -> Result<Self, Self::Error> {
        // 验证数据长度与预期结构相符
        const SWAP_DATA_LEN: usize = size_of::<bool>() + size_of::<u64>() * 2 + size_of::<i64>();
        if data.len() != SWAP_DATA_LEN {
            return Err(ProgramError::InvalidInstructionData);
        }

        let instruction_data = unsafe { (data.as_ptr() as *const Self).read_unaligned() };

        // ============ 过期时间检查 ==========
        // 如果设置了过期时间，验证当前时间未超过
        let clock = pinocchio::sysvars::clock::Clock::get()?;
        if instruction_data.expiration != 0 && clock.unix_timestamp > instruction_data.expiration {
            return Err(ProgramError::Custom(0));
        }

        // ============ 金额有效性检查 ==========
        // 验证交换数量大于 0
        if instruction_data.amount == 0 {
            return Err(ProgramError::InvalidArgument);
        }
        // 验证最小输出数量大于 0
        if instruction_data.min == 0 {
            return Err(ProgramError::InvalidArgument);
        }
        
        Ok(instruction_data)
    }
}

/// ========== Swap 指令实现 ==========
/// 
/// 用户使用一种代币交换另一种代币，根据常数乘积曲线计算汇率。
pub struct Swap<'a> {
    /// 所需的账户
    pub accounts: SwapAccounts<'a>,
    /// 指令参数
    pub instruction_data: SwapInstructionData,
}

impl<'a> TryFrom<(&'a [u8], &'a [AccountView])> for Swap<'a> {
    type Error = ProgramError;

    /// 构建 Swap 指令
    fn try_from((data, accounts): (&'a [u8], &'a [AccountView])) -> Result<Self, Self::Error> {
        let accounts = SwapAccounts::try_from(accounts)?;
        let instruction_data = SwapInstructionData::try_from(data)?;

        Ok(Self {
            accounts,
            instruction_data,
        })
    }
}

impl<'a> Swap<'a> {
    /// 该指令的鉴别器值（3 表示交换指令）
    pub const DISCRIMINATOR: &'a u8 = &3;

    /// 执行交换流程
    /// 
    /// 根据 is_x 标志，执行 X→Y 或 Y→X 交换，
    /// 使用常数乘积曲线计算输出数量。
    pub fn process(&mut self) -> ProgramResult {
        // ============ 步骤1：加载金库数据 ============
        // 获取当前金库中的代币数量
        let vault_x = unsafe { TokenAccount::from_account_view_unchecked(self.accounts.vault_x)? };
        let vault_y = unsafe { TokenAccount::from_account_view_unchecked(self.accounts.vault_y)? };
        
        // ============ 步骤2：加载配置和验证状态 ============
        // 获取 Config PDA 的数据备用（稍后用于生成签名）
        let cfg2 = Config::load(self.accounts.config)?;
        
        // 验证 AMM 已初始化且处于可用状态
        if cfg2.state() != (AmmState::Initialized as u8) {
            return Err(ProgramError::InvalidAccountData);
        }

        // ============ 步骤3：初始化常数乘积曲线模型 ============
        // 创建曲线模型，用于计算交换数量
        let mut curve = ConstantProduct::init(
            vault_x.amount(),              // X 金库当前余额
            vault_y.amount(),              // Y 金库当前余额
            vault_x.amount(),              // X 金库的初始供应（用于计费）
            cfg2.fee(),                    // 交换费用（以 bps 计）
            None,                          // 自定义参数（无）
        )
        .map_err(|_| ProgramError::ArithmeticOverflow)?;

        // ============ 步骤4：确定交换方向 ============
        // 根据 is_x 确定用户输入的是 X 还是 Y
        let p = match self.instruction_data.is_x {
            true => LiquidityPair::X,    // 用户输入 X，输出 Y
            false => LiquidityPair::Y,   // 用户输入 Y，输出 X
        };

        // ============ 步骤5：计算交换结果 ============
        // 使用常数乘积曲线计算输入和输出代币的数量
        let swap_result = curve
            .swap(p, self.instruction_data.amount, self.instruction_data.min)
            .map_err(|_| ProgramError::InvalidArgument)?;

        // 验证计算结果的有效性
        if swap_result.deposit == 0 || swap_result.withdraw == 0 {
            return Err(ProgramError::InvalidArgument);
        }

        // ============ 步骤6：准备 Config PDA 签名 ============
        // 构造用于签署转账交易的 PDA 签名种子
        let seed_bytes = cfg2.seed().to_le_bytes();
        let bump = cfg2.config_bump();
        let config_seeds = [
            Seed::from(b"config"),
            Seed::from(&seed_bytes),
            Seed::from(cfg2.mint_x().as_ref()),
            Seed::from(cfg2.mint_y().as_ref()),
            Seed::from(&bump),
        ];
        let signer = [Signer::from(&config_seeds)];

        // ============ 步骤7：执行代币转账 ============
        // 根据交换方向进行相应的转账操作
        if self.instruction_data.is_x {
            // 用户交换 X → Y 的情况
            
            // 将用户的代币 X 转入金库（用户签名）
            Transfer {
                from: self.accounts.user_x_ata,
                to: self.accounts.vault_x,
                authority: self.accounts.user,
                amount: swap_result.deposit,
            }
            .invoke()?;

            // 将金库的代币 Y 转给用户（Config PDA 签名）
            Transfer {
                from: self.accounts.vault_y,
                to: self.accounts.user_y_ata,
                authority: self.accounts.config,
                amount: swap_result.withdraw,
            }
            .invoke_signed(&signer)?;
        } else {
            // 用户交换 Y → X 的情况
            
            // 将用户的代币 Y 转入金库（用户签名）
            Transfer {
                from: self.accounts.user_y_ata,
                to: self.accounts.vault_y,
                authority: self.accounts.user,
                amount: swap_result.deposit,
            }
            .invoke()?;

            // 将金库的代币 X 转给用户（Config PDA 签名）
            Transfer {
                from: self.accounts.vault_x,
                to: self.accounts.user_x_ata,
                authority: self.accounts.config,
                amount: swap_result.withdraw,
            }
            .invoke_signed(&signer)?;
        }

        Ok(())
    }
}
