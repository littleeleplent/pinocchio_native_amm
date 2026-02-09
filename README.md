#Pinocchio Native AMM

Solana 区块链上的原生自动做市商（AMM）实现，采用常数乘积曲线模型。

## 项目概述

Pinocchio Native AMM 是一个基于 Solana 区块链的去中心化交易协议实现，使用 Pinocchio 框架开发。该项目实现了流动性池和代币交换功能，支持用户提供流动性和进行代币交换操作。

## 主要特性

- **流动性管理**：支持用户存入和提取流动性
- **常数乘积曲线**：采用 x*y=k 的常数乘积公式实现价格发现机制
- **代币交换**：支持池内代币的原子交换
- **Solana 原生集成**：完全基于 Solana Smart Contract 标准开发

## 技术栈

- **编程语言**：Rust
- **框架**：Pinocchio 0.10.1
- **曲线库**：constant-product-curve
- **关键依赖**：
  - pinocchio-token：SPL Token 集成
  - pinocchio-associated-token-account：关联代币账户支持
  - solana-address：Solana 地址和密码学操作
  - pinocchio-system：系统程序集成

## 项目结构

```
.
├── Cargo.toml                 # 项目依赖配置
├── src/
│   ├── lib.rs               # 程序入口和指令分发
│   ├── state.rs             # 数据结构定义
│   └── instructions/        # 指令实现
│       ├── mod.rs           # 指令模块入口
│       ├── initialize.rs    # 初始化指令 (0)
│       ├── deposit.rs       # 存入流动性指令 (1)
│       ├── withdraw.rs      # 提取流动性指令 (2)
│       └── swap.rs          # 交换指令 (3)
└── target/                  # 编译输出目录
```

## 核心指令

| 指令 | ID | 功能 | 描述 |
|------|-----|------|------|
| Initialize | 0 | 初始化 | 创建新的 AMM 流动性池 |
| Deposit | 1 | 存入 | 用户向流动性池存入代币 |
| Withdraw | 2 | 提取 | 用户从流动性池提取代币 |
| Swap | 3 | 交换 | 在池内进行代币交换 |

## 快速开始

### 建立开发环境

```bash
# 安装 Rust（如未安装）
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# 为 Solana 配置切换 Rust 版本
rustup override set 1.81

# 安装 Solana CLI
sh -c "$(curl -sSfL https://release.solana.com/stable/install)"
```

### 编译项目

```bash
# 编译为 BPF（Berkley Packet Filter）目标
cargo build --release --target sbpf-solana-solana
```

### 本地测试

```bash
# 启动本地 Solana 验证器
solana-test-validator

# 部署合约（在另一个终端）
solana program deploy target/sbpf-solana-solana/release/blueshift_native_amm.so
```

## 使用示例

### 初始化流动性池

```rust
// 构建初始化指令
// 包含：池账户、token A 的 mint、token B 的 mint 等
```

### 提供流动性

```rust
// 调用 Deposit 指令
// 用户存入等值的 Token A 和 Token B
```

### 交换代币

```rust
// 调用 Swap 指令
// 指定输入代币数量，系统自动计算输出数量
```

### 提取流动性

```rust
// 调用 Withdraw 指令
// 用户提取所有流动性或部分流动性
```

## 合约地址

```
22222222222222222222222222222222222222222222
```

注：这是开发/测试地址，生产环境请使用实际部署后的地址。

## 常见问题

**Q: 如何修改流动性池的参数？**  
A: 在 [src/state.rs](src/state.rs) 中修改状态结构定义，在 [src/instructions/initialize.rs](src/instructions/initialize.rs) 中修改初始化逻辑。

**Q: 如何集成到我的项目中？**  
A: 在 `Cargo.toml` 中添加该项目作为依赖，或调用其生成的智能合约。

**Q: 支持多少个代币对？**  
A: 理论上从数量上没有限制，但需要为每个代币对创建单独的流动性池。

## 开发和贡献

欢迎提交 Issue 和 Pull Request 来改进项目。

### 开发建议

- 遵循 Rust 编码规范
- 为新功能编写单元测试
- 更新相关文档

## 许可证

本项目的许可证信息请参考项目根目录的 LICENSE 文件。

## 参考资源

- [Solana 官方文档](https://docs.solana.com/)
- [Pinocchio 框架](https://github.com/magicblock-labs/pinocchio)
- [常数乘积曲线](https://github.com/deanmlittle/constant-product-curve)
- [Uniswap V2 白皮书](https://uniswap.org/whitepaper.pdf)

## 联系方式

如有问题，请提交 GitHub Issues 或直接联系项目维护者。
