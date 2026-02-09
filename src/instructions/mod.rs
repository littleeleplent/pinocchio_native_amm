/// ========== 全部指令模块导入 ==========
/// 
/// 本模块组织和导出所有 AMM 智能合约支持的指令实现。
/// 每个指令在单独的文件中定义，本模块负责协调它们。

/// 提取流动性的指令实现
pub mod deposit;
/// 初始化新的 AMM 流动性池
pub mod initialize;
/// 代币交换的指令实现
pub mod swap;
/// 提取流动性的指令实现
pub mod withdraw;

// 将所有指令导出到顶层，为外部模块和指令分发提供便利
pub use deposit::*;
pub use initialize::*;
pub use swap::*;
pub use withdraw::*;