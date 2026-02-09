use crate::state::{AmmState, Config};
use core::mem::size_of;

use pinocchio::{
    cpi::{Seed, Signer},
    error::ProgramError,
    AccountView, Address, ProgramResult,
};
use pinocchio::sysvars::Sysvar;
use pinocchio_token::instructions::{MintTo, Transfer};
use pinocchio_token::ID as TOKEN_PROGRAM_ID;

/// ========== 存入流动性指令所需的账户 ==========
/// 
/// 用户向 AMM 流动性池提供流动性时所需的所有账户。
pub struct DepositAccounts<'a> {
    /// 提供流动性的用户账户（必须是签名者）
    pub user: &'a AccountView,
    /// LP 代币的 mint 账户（用于铸造 LP 代币给用户）
    pub mint_lp: &'a AccountView,
    /// 代币 X 的金库账户（接收用户的代币 X）
    pub vault_x: &'a AccountView,
    /// 代币 Y 的金库账户（接收用户的代币 Y）
    pub vault_y: &'a AccountView,
    /// 用户的代币 X 关联代币账户（ATA）
    pub user_x_ata: &'a AccountView,
    /// 用户的代币 Y 关联代币账户（ATA）
    pub user_y_ata: &'a AccountView,
    /// 用户的 LP 代币关联代币账户（ATA）
    pub user_lp_ata: &'a AccountView,
    /// AMM 配置账户（包含池参数）
    pub config: &'a AccountView,
    /// SPL Token 程序
    pub token_program: &'a AccountView,
}

impl<'a> TryFrom<&'a [AccountView]> for DepositAccounts<'a> {
    type Error = ProgramError;

    /// 验证和提取存入指令所需的账户
    fn try_from(accounts: &'a [AccountView]) -> Result<Self, Self::Error> {
        let [user, mint_lp, vault_x, vault_y, user_x_ata, user_y_ata, user_lp_ata, config, token_program] =
            accounts
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

        // ============ 其他账户基本检查 ============
        // 注：完整的验证（如 mint_lp 的 Mint 格式检查）可以在后续步骤中进行

        // ============ 金库 PDA 验证 ============
        // 从 Config 加载数据以验证金库地址是否匹配
        let cfg = Config::load(config)?;

        // 验证 vault_x 是否为正确的 PDA
        let (derived_vault_x, _) = Address::find_program_address(
            &[
                config.address().as_ref(),
                token_program.address().as_ref(),
                cfg.mint_x().as_ref(),
            ],
            &pinocchio_associated_token_account::ID,
        );
        if derived_vault_x != *vault_x.address() {
            return Err(ProgramError::InvalidAccountData);
        }

        // 验证 vault_y 是否为正确的 PDA
        let (derived_vault_y, _) = Address::find_program_address(
            &[
                config.address().as_ref(),
                token_program.address().as_ref(),
                cfg.mint_y().as_ref(),
            ],
            &pinocchio_associated_token_account::ID,
        );
        if derived_vault_y != *vault_y.address() {
            return Err(ProgramError::InvalidAccountData);
        }

        Ok(Self {
            user,
            mint_lp,
            vault_x,
            vault_y,
            user_x_ata,
            user_y_ata,
            user_lp_ata,
            config,
            token_program,
        })
    }
}

/// ========== 存入流动性指令的数据结构 ==========
/// 
/// 包含用户提供流动性时的所有参数。
#[repr(C, packed)] 
pub struct DepositInstructionData {
    /// 用户想要铸造的 LP 代币数量
    pub amount: u64,
    /// 愿意支付的最大代币 X 数量（滑点保护）
    pub max_x: u64,
    /// 愿意支付的最大代币 Y 数量（滑点保护）
    pub max_y: u64,
    /// 交易过期时间（Unix 时间戳，0 表示不限制）
    pub expiration: i64,
}

impl<'a> TryFrom<&'a [u8]> for DepositInstructionData {
    type Error = ProgramError;

    /// 从字节数据解析存入指令参数，进行有效性检查
    fn try_from(data: &'a [u8]) -> Result<Self, Self::Error> {
        // 验证数据长度与预期结构相符
        const DEPOSIT_DATA_LEN: usize = size_of::<u64>() * 3 + size_of::<i64>();
        if data.len() != DEPOSIT_DATA_LEN {
            return Err(ProgramError::InvalidInstructionData);
        }

        let instruction_data = unsafe { (data.as_ptr() as *const Self).read_unaligned() };

        // 验证 LP 数量大于 0
        if instruction_data.amount == 0 {
            return Err(ProgramError::InvalidArgument);
        }

        // ============ 过期时间检查 ==========
        // 如果设置了过期时间，验证当前时间未超过
        let clock = pinocchio::sysvars::clock::Clock::get()?;
        if instruction_data.expiration != 0 && clock.unix_timestamp > instruction_data.expiration {
            return Err(ProgramError::Custom(0));
        }

        Ok(instruction_data)
    }
}

/// ========== Deposit 指令实现 ==========
/// 
/// 用户向流动性池提供流动性，获得 LP 代币作为凭证。
pub struct Deposit<'a> {
    /// 所需的账户
    pub accounts: DepositAccounts<'a>,
    /// 指令参数
    pub instruction_data: DepositInstructionData,
}

impl<'a> TryFrom<(&'a [u8], &'a [AccountView])> for Deposit<'a> {
    type Error = ProgramError;

    /// 构建 Deposit 指令，进行全面的合法性检查
    fn try_from((data, accounts): (&'a [u8], &'a [AccountView])) -> Result<Self, Self::Error> {
        let accounts = DepositAccounts::try_from(accounts)?;
        let instruction_data = DepositInstructionData::try_from(data)?;
        
        // ============ Config 状态验证 ============
        // 验证 AMM 是否已初始化且处于可用状态
        let config = Config::load_mut(accounts.config)?;

        if config.state() != (AmmState::Initialized as u8) {
            return Err(ProgramError::InvalidAccountData);
        }

        Ok(Self {
            accounts,
            instruction_data,
        })
    }
}

impl<'a> Deposit<'a> {
    /// 该指令的鉴别器值（1 表示存入指令）
    pub const DISCRIMINATOR: &'a u8 = &1;

    /// 执行存入流程
    /// 
    /// 将用户的代币转入金库，并铸造对应的 LP 代币给用户。
    pub fn process(&mut self) -> ProgramResult {
        // ============ 步骤1：转移代币 X ==========
        // 将用户的代币 X 从其 ATA 转入金库
        if self.instruction_data.max_x > 0 {
            Transfer {
                from: self.accounts.user_x_ata,
                to: self.accounts.vault_x,
                authority: self.accounts.user,
                amount: self.instruction_data.max_x,
            }
            .invoke()?;
        }

        // ============ 步骤2：转移代币 Y ==========
        // 将用户的代币 Y 从其 ATA 转入金库
        if self.instruction_data.max_y > 0 {
            Transfer {
                from: self.accounts.user_y_ata,
                to: self.accounts.vault_y,
                authority: self.accounts.user,
                amount: self.instruction_data.max_y,
            }
            .invoke()?;
        }

        // ============ 步骤3：铸造 LP 代币 ==========
        // 向用户铸造 LP 代币作为流动性凭证
        // Config PDA 是 mint authority，需要其签名
        if self.instruction_data.amount > 0 {
            let config = Config::load(self.accounts.config)?;
            let seed_bytes = config.seed().to_le_bytes();
            let bump = config.config_bump();

            // 准备 Config PDA 的签名种子
            let config_seeds = [
                Seed::from(b"config"),
                Seed::from(&seed_bytes),
                Seed::from(config.mint_x().as_ref()),
                Seed::from(config.mint_y().as_ref()),
                Seed::from(&bump),
            ];

            let signer = [Signer::from(&config_seeds)];

            MintTo {
                mint: self.accounts.mint_lp,
                account: self.accounts.user_lp_ata,
                mint_authority: self.accounts.config,
                amount: self.instruction_data.amount,
            }
            .invoke_signed(&signer)?;
        }

        Ok(())
    }
}
