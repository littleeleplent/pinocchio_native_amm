use pinocchio::{
    AccountView, Address, ProgramResult, address::declare_id, entrypoint, error::ProgramError,
};
entrypoint!(process_instruction);

/// 指令模块 - 包含所有智能合约可执行的指令
pub mod instructions;
pub use instructions::*;

/// 状态模块 - 定义了 AMM 合约的数据结构体
pub mod state;
pub use state::*;

declare_id!("22222222222222222222222222222222222222222222");

/// 主指令入口函数
/// 
/// 该函数是 Solana 智能合约的核心入口点，负责路由指令到不同的处理器。
/// 
/// # 参数
/// * `_program_id` - 当前程序的 ID（通常不使用）
/// * `accounts` - 指令涉及的所有账户
/// * `instruction_data` - 指令的二进制数据，第一个字节是指令鉴别器（discriminator）
///
/// # 返回值
/// * `ProgramResult` - 执行结果，包含成功或错误信息
fn process_instruction(
    _program_id: &Address,
    accounts: &[AccountView],
    instruction_data: &[u8],
) -> ProgramResult {
    // 从指令数据中提取鉴别器和真实数据
    match instruction_data.split_first() {
        Some((discriminator, data)) => {
            // 根据鉴别器路由到对应的指令处理器
            match *discriminator {
                0 => Initialize::try_from((data, accounts))?.process(),      // 初始化 AMM
                1 => Deposit::try_from((data, accounts))?.process(),         // 存入流动性
                2 => Withdraw::try_from((data, accounts))?.process(),        // 提取流动性
                3 => Swap::try_from((data, accounts))?.process(),            // 交换代币
                _ => Err(ProgramError::InvalidInstructionData),              // 未知指令
            }
        }
        None => Err(ProgramError::InvalidInstructionData),                   // 空指令数据
    }
}
