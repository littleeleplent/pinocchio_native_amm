use crate::state::Config;
use core::mem::size_of;

use constant_product_curve::ConstantProduct;
use pinocchio::sysvars::Sysvar;
use pinocchio::{
    cpi::{Seed, Signer},
    error::ProgramError,
    AccountView, Address, ProgramResult,
};
use pinocchio_token::instructions::{Burn, Transfer};
use pinocchio_token::state::{Mint, TokenAccount};
use pinocchio_token::ID as TOKEN_PROGRAM_ID;

/// ========== 提取流动性指令所需的账户 ==========
/// 
/// 用户从 AMM 流动性池提取流动性时所需的所有账户。
pub struct WithdrawAccounts<'a> {
    /// 提取流动性的用户账户（必须是签名者）
    pub user: &'a AccountView,
    /// LP 代币的 mint 账户（用于销毁 LP 代币）
    pub mint_lp: &'a AccountView,
    /// 代币 X 的金库账户（转出代币 X 给用户）
    pub vault_x: &'a AccountView,
    /// 代币 Y 的金库账户（转出代币 Y 给用户）
    pub vault_y: &'a AccountView,
    /// 用户的代币 X 关联代币账户（接收代币 X）
    pub user_x_ata: &'a AccountView,
    /// 用户的代币 Y 关联代币账户（接收代币 Y）
    pub user_y_ata: &'a AccountView,
    /// 用户的 LP 代币关联代币账户（销毁 LP 代币）
    pub user_lp_ata: &'a AccountView,
    /// AMM 配置账户（包含池参数）
    pub config: &'a AccountView,
    /// SPL Token 程序
    pub token_program: &'a AccountView,
}

impl<'a> TryFrom<&'a [AccountView]> for WithdrawAccounts<'a> {
    type Error = ProgramError;

    /// 验证和提取提取指令所需的账户
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

        // ============ LP Mint 账户验证 ============
        // 验证 LP mint 的格式和所有权
        if mint_lp.data_len() != Mint::LEN || !mint_lp.owned_by(token_program.address()) {
            return Err(ProgramError::InvalidAccountData);
        }

        // ============ 金库 PDA 验证 ============
        // 从 Config 加载数据并验证金库地址是否为正确的 PDA
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

        // ============ 代币账户格式和所有权验证 ============
        // 验证所有相关的代币账户都由 Token 程序拥有
        if vault_x.data_len() != TokenAccount::LEN || !vault_x.owned_by(token_program.address()) {
            return Err(ProgramError::InvalidAccountOwner);
        }
        if vault_y.data_len() != TokenAccount::LEN || !vault_y.owned_by(token_program.address()) {
            return Err(ProgramError::InvalidAccountOwner);
        }
        if user_x_ata.data_len() != TokenAccount::LEN || !user_x_ata.owned_by(token_program.address()) {
            return Err(ProgramError::InvalidAccountOwner);
        }
        if user_y_ata.data_len() != TokenAccount::LEN || !user_y_ata.owned_by(token_program.address()) {
            return Err(ProgramError::InvalidAccountOwner);
        }
        if user_lp_ata.data_len() != TokenAccount::LEN || !user_lp_ata.owned_by(token_program.address()) {
            return Err(ProgramError::InvalidAccountOwner);
        }

        Ok(WithdrawAccounts {
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

/// ========== 提取流动性指令的数据结构 ==========
/// 
/// 包含用户提取流动性时的所有参数。
#[repr(C, packed)] 
pub struct WithdrawInstructionData {
    /// 用户想要销毁的 LP 代币数量
    pub amount: u64,
    /// 愿意接收的最少代币 X 数量（滑点保护）
    pub min_x: u64,
    /// 愿意接收的最少代币 Y 数量（滑点保护）
    pub min_y: u64,
    /// 交易过期时间（Unix 时间戳，0 表示不限制）
    pub expiration: i64,
}

impl<'a> TryFrom<&'a [u8]> for WithdrawInstructionData {
    type Error = ProgramError;

    /// 从字节数据解析提取指令参数，进行有效性检查
    fn try_from(data: &'a [u8]) -> Result<Self, Self::Error> {
        // 验证数据长度与预期结构相符
        const WITHDRAW_DATA_LEN: usize = size_of::<u64>() * 3 + size_of::<i64>();
        if data.len() != WITHDRAW_DATA_LEN {
            return Err(ProgramError::InvalidInstructionData);
        }

        let instruction_data = unsafe { (data.as_ptr() as *const Self).read_unaligned() };

        // 验证 LP 销毁数量大于 0
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

/// ========== Withdraw 指令实现 ==========
/// 
/// 用户从流动性池提取流动性，销毁 LP 代币获得底层代币。
pub struct Withdraw<'a> {
    /// 所需的账户
    pub accounts: WithdrawAccounts<'a>,
    /// 指令参数
    pub instruction_data: WithdrawInstructionData,
}

impl<'a> TryFrom<(&'a [u8], &'a [AccountView])> for Withdraw<'a> {
    type Error = ProgramError;

    /// 构建 Withdraw 指令，计算用户将获得的代币数量并进行滑点检查
    fn try_from((data, accounts): (&'a [u8], &'a [AccountView])) -> Result<Self, Self::Error> {
        let accounts = WithdrawAccounts::try_from(accounts)?;
        let instruction_data = WithdrawInstructionData::try_from(data)?;

        // ============ 计算输出代币数量 ============
        // 根据 LP 代币数量和池中代币数量计算用户将获得的代币 X 和 Y
        let mint_lp = unsafe { Mint::from_account_view_unchecked(accounts.mint_lp)? };
        let vault_x = unsafe { TokenAccount::from_account_view_unchecked(accounts.vault_x)? };
        let vault_y = unsafe { TokenAccount::from_account_view_unchecked(accounts.vault_y)? };

        // 如果用户销毁全部 LP，直接返回池中所有代币
        let (x, y) = if mint_lp.supply() == instruction_data.amount {
            (vault_x.amount(), vault_y.amount())
        } else {
            // 否则使用常数乘积曲线计算按比例获得的数量
            let amounts = ConstantProduct::xy_withdraw_amounts_from_l(
                vault_x.amount(),
                vault_y.amount(),
                mint_lp.supply(),
                instruction_data.amount,
                6,
            )
            .map_err(|_| ProgramError::InvalidArgument)?;

            (amounts.x, amounts.y)
        };

        // ============ 滑点保护检查 ============
        // 验证计算出的数量满足用户的最小要求
        if !(x >= instruction_data.min_x && y >= instruction_data.min_y) {
            return Err(ProgramError::InvalidArgument);
        }

        Ok(Self {
            accounts,
            instruction_data,
        })
    }
}

impl<'a> Withdraw<'a> {
    /// 该指令的鉴别器值（2 表示提取指令）
    pub const DISCRIMINATOR: &'a u8 = &2;

    /// 执行提取流程
    /// 
    /// 销毁用户的 LP 代币，将对应的底层代币转给用户。
    pub fn process(&mut self) -> ProgramResult {
        // ============ 步骤1：再次计算输出数量 ============
        // 为了避免存储额外数据，在执行时重新计算（可以与 try_from 中的计算对应）
        let mint_lp = unsafe { Mint::from_account_view_unchecked(self.accounts.mint_lp)? };
        let vault_x = unsafe { TokenAccount::from_account_view_unchecked(self.accounts.vault_x)? };
        let vault_y = unsafe { TokenAccount::from_account_view_unchecked(self.accounts.vault_y)? };

        let (x, y) = if mint_lp.supply() == self.instruction_data.amount {
            (vault_x.amount(), vault_y.amount())
        } else {
            let amounts = ConstantProduct::xy_withdraw_amounts_from_l(
                vault_x.amount(),
                vault_y.amount(),
                mint_lp.supply(),
                self.instruction_data.amount,
                6,
            )
            .map_err(|_| ProgramError::InvalidArgument)?;

            (amounts.x, amounts.y)
        };

        // ============ 步骤2：销毁用户的 LP 代币 ============
        // 用户授权销毁操作
        Burn {
            account: self.accounts.user_lp_ata,
            mint: self.accounts.mint_lp,
            authority: self.accounts.user,
            amount: self.instruction_data.amount,
        }
        .invoke()?;

        // ============ 步骤3：从金库转出代币给用户 ============
        // Config PDA 是金库的权限方，需要其签名
        let cfg = Config::load(self.accounts.config)?;
        let seed_bytes = cfg.seed().to_le_bytes();
        let bump = cfg.config_bump();
        let config_seeds = [
            Seed::from(b"config"),
            Seed::from(&seed_bytes),
            Seed::from(cfg.mint_x().as_ref()),
            Seed::from(cfg.mint_y().as_ref()),
            Seed::from(&bump),
        ];
        let signer = [Signer::from(&config_seeds)];

        // 转出代币 X
        if x > 0 {
            Transfer {
                from: self.accounts.vault_x,
                to: self.accounts.user_x_ata,
                authority: self.accounts.config,
                amount: x,
            }
            .invoke_signed(&signer)?;
        }

        // 转出代币 Y
        if y > 0 {
            Transfer {
                from: self.accounts.vault_y,
                to: self.accounts.user_y_ata,
                authority: self.accounts.config,
                amount: y,
            }
            .invoke_signed(&signer)?;
        }

        Ok(())
    }
}

