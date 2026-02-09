use core::mem::size_of;
use pinocchio::{
    AccountView, Address,
    account::{Ref, RefMut},
    error::ProgramError,
};

/// ========== AMM 配置账户结构 ==========
/// 
/// Config 是 AMM 流动性池的核心配置账户，存储了整个流动性池的元数据信息。
/// 该结构体使用 `#[repr(C, packed)]` 以保证内存布局与 Solana 兼容。
#[repr(C, packed)]
pub struct Config {
    /// AMM 的状态（未初始化、已初始化、禁用、仅提取等）
    state: u8,
    /// 用于生成配置账户 PDA 的种子值
    seed: [u8; 8],
    /// AMM 的权限管理员地址（可以升级或暂停 AMM）
    authority: Address,
    /// 代币 X 的 mint 地址（流动性池中的第一种代币）
    mint_x: Address,
    /// 代币 Y 的 mint 地址（流动性池中的第二种代币）
    mint_y: Address,
    /// 交换费用（以 bps 计，即万分之一，例如 0-9999 表示 0%-99.99%）
    fee: [u8; 2],
    /// 生成配置账户 PDA 时的 bump seed 值
    config_bump: [u8; 1],
}

/// ========== AMM 状态枚举 ==========
/// 
/// 定义了 AMM 可能处于的各种状态，用来控制池的可用性和操作限制。
#[repr(u8)]
pub enum AmmState {
    /// 状态 0：未初始化，池账户已创建但数据未填充
    Uninitialized = 0u8,
    /// 状态 1：已初始化，池可以正常使用所有功能
    Initialized = 1u8,
    /// 状态 2：禁用，池不能交换或提供流动性
    Disabled = 2u8,
    /// 状态 3：仅提取，池只能提取流动性，不能交换或提供新流动性
    WithdrawOnly = 3u8,
}

impl Config {
    /// 配置账户的固定大小（以字节为单位）
    pub const LEN: usize = size_of::<Config>();

    /// ========== 加载 Config 账户数据（只读） ==========
    /// 
    /// 从 Solana 账户中安全地加载 Config 结构体的只读副本。
    /// 该方法会进行所有权验证以确保账户由当前程序拥有。
    /// 
    /// # 参数
    /// * `account_view` - 账户视图，包含账户的元数据和数据
    ///
    /// # 返回值
    /// * `Result<Ref<'a, Self>, ProgramError>` - 成功时返回受控引用，失败时返回错误
    #[inline(always)]
    pub fn load<'a>(account_view: &'a AccountView) -> Result<Ref<'a, Self>, ProgramError> {
        // 检查账户数据长度是否匹配
        if account_view.data_len() != Self::LEN {
            return Err(ProgramError::InvalidAccountData);
        }

        // 验证账户所有者是否为当前程序
        // 在 Pinocchio 这种追求极限性能的底层框架中，unsafe 的存在是为了获得零成本抽象。
        // 我们已通过所有者检查来确保安全性。
        let is_owner_valid = unsafe { account_view.owner() == &crate::ID };
        if !is_owner_valid {
            return Err(ProgramError::InvalidAccountOwner);
        }

        // 借用账户数据并映射到 Config 结构体引用
        let data = account_view.try_borrow()?;

        Ok(Ref::map(data, |data| unsafe {
            Self::from_bytes_unchecked(data)
        }))
    }

    /// ========== 加载 Config 账户数据（只读，不检查） ==========
    /// 
    /// 无安全检查地加载 Config 结构体，仅在调用者已验证账户时使用。
    ///
    /// # Safety
    /// 调用者必须确保：
    /// 1. 账户数据长度正确
    /// 2. 账户归程序所有
    /// 3. 不会出现并发修改
    /// 
    /// # 返回值
    /// * `Result<&Self, ProgramError>` - Config 的不可变引用
    #[inline(always)]
    pub unsafe fn load_unchecked(account_view: &AccountView) -> Result<&Self, ProgramError> {
        if account_view.data_len() != Self::LEN {
            return Err(ProgramError::InvalidAccountData);
        }
        let is_owner_valid = unsafe { account_view.owner() == &crate::ID };
        if !is_owner_valid {
            return Err(ProgramError::InvalidAccountOwner);
        }
        Ok(unsafe { Self::from_bytes_unchecked(account_view.borrow_unchecked()) })
    }

    /// ========== 从字节数组构造 Config（不检查对齐） ==========
    ///
    /// 将原始字节数据直接解释为 Config 结构体，无需复制。
    ///
    /// # Safety
    ///
    /// 调用者必须确保：
    /// 1. `bytes` 包含有效的 Config 数据表示
    /// 2. 字节数据正确对齐（Config 要求 1 字节对齐）
    /// 3. 字节数组长度至少为 size_of::<Config>()
    ///
    /// # 返回值
    /// * `&Self` - Config 的不可变引用
    #[inline(always)]
    pub unsafe fn from_bytes_unchecked(bytes: &[u8]) -> &Self {
        unsafe { &*(bytes.as_ptr() as *const Config) }
    }

    /// ========== 从字节数组构造 Config（可变，不检查） ==========
    ///
    /// 将原始字节数据直接解释为可变 Config 结构体。
    ///
    /// # Safety
    /// 调用者必须确保字节数据表示有效的 Config 结构体。
    #[inline(always)]
    pub unsafe fn from_bytes_unchecked_mut(bytes: &mut [u8]) -> &mut Self {
        unsafe { &mut *(bytes.as_mut_ptr() as *mut Config) }
    }

    // ========== 获取器方法（Getter 方法） ==========
    // 这些方法提供对各个字段的安全只读访问

    /// 获取 AMM 当前的状态值
    #[inline(always)]
    pub fn state(&self) -> u8 {
        self.state
    }

    /// 获取池的种子值（8 字节，转换为 u64）
    #[inline(always)]
    pub fn seed(&self) -> u64 {
        u64::from_le_bytes(self.seed)
    }

    /// 获取权限管理员地址的引用
    #[inline(always)]
    pub fn authority(&self) -> &Address {
        &self.authority
    }

    /// 获取代币 X mint 地址的引用
    #[inline(always)]
    pub fn mint_x(&self) -> &Address {
        &self.mint_x
    }

    /// 获取代币 Y mint 地址的引用
    #[inline(always)]
    pub fn mint_y(&self) -> &Address {
        &self.mint_y
    }

    /// 获取交换费用（以 bps 计，范围 0-9999）
    #[inline(always)]
    pub fn fee(&self) -> u16 {
        u16::from_le_bytes(self.fee)
    }

    /// 获取配置账户的 bump seed
    #[inline(always)]
    pub fn config_bump(&self) -> [u8; 1] {
        self.config_bump
    }

    /// ========== 加载 Config 账户数据（可变） ==========
    /// 
    /// 安全地加载 Config 结构体的可变引用，用于修改池的配置。
    /// 进行所有权和大小验证。
    ///
    /// # 参数
    /// * `account_view` - 账户视图
    ///
    /// # 返回值
    /// * `Result<RefMut<'a, Self>, ProgramError>` - 成功返回可变受控引用
    #[inline(always)]
    pub fn load_mut<'a>(account_view: &'a AccountView) -> Result<RefMut<'a, Self>, ProgramError> {
        if account_view.data_len() != Self::LEN {
            return Err(ProgramError::InvalidAccountData);
        }
        let is_owner_valid = unsafe { account_view.owner() == &crate::ID };

        if !is_owner_valid {
            return Err(ProgramError::InvalidAccountOwner);
        }
        Ok(RefMut::map(account_view.try_borrow_mut()?, |data| unsafe {
            Self::from_bytes_unchecked_mut(data)
        }))
    }

    // ========== 设置器方法（Setter 方法） ==========
    // 这些方法提供对各个字段的安全写入访问，包含验证逻辑

    /// 设置 AMM 的状态，并验证状态值的有效性
    #[inline(always)]
    pub fn set_state(&mut self, state: u8) -> Result<(), ProgramError> {
        // 确保状态不超过 WithdrawOnly
        if state.ge(&(AmmState::WithdrawOnly as u8)) {
            return Err(ProgramError::InvalidAccountData);
        }
        self.state = state;
        Ok(())
    }

    /// 设置池的种子值
    #[inline(always)]
    pub fn set_seed(&mut self, seed: u64) {
        self.seed = seed.to_le_bytes();
    }

    /// 设置权限管理员地址
    #[inline(always)]
    pub fn set_authority(&mut self, authority: Address) {
        self.authority = authority;
    }

    /// 设置代币 X 的 mint 地址
    #[inline(always)]
    pub fn set_mint_x(&mut self, mint_x: Address) {
        self.mint_x = mint_x;
    }

    /// 设置代币 Y 的 mint 地址
    #[inline(always)]
    pub fn set_mint_y(&mut self, mint_y: Address) {
        self.mint_y = mint_y;
    }

    /// 设置交换费用，并验证费用不超过 10000 bps（100%）
    #[inline(always)]
    pub fn set_fee(&mut self, fee: u16) -> Result<(), ProgramError> {
        if fee.ge(&10_000) {
            return Err(ProgramError::InvalidAccountData);
        }
        self.fee = fee.to_le_bytes();
        Ok(())
    }

    /// 设置配置账户的 bump seed
    #[inline(always)]
    pub fn set_config_bump(&mut self, config_bump: [u8; 1]) {
        self.config_bump = config_bump;
    }

    #[inline(always)]
    pub fn set_inner(
        &mut self,
        seed: u64,
        authority: Address,
        mint_x: Address,
        mint_y: Address,
        fee: u16,
        config_bump: [u8; 1],
    ) -> Result<(), ProgramError> {
        self.set_state(AmmState::Initialized as u8)?;
        self.set_seed(seed);
        self.set_authority(authority);
        self.set_mint_x(mint_x);
        self.set_mint_y(mint_y);
        self.set_fee(fee)?;
        self.set_config_bump(config_bump);
        Ok(())
    }

    #[inline(always)]
    pub fn has_authority(&self) -> Option<Address> {
        // read_unaligned：处理“对齐”问题
        // // 1. 用“不检查对齐”的方法，把 self.authority 里的内容强行拷贝一份给 auth
        // We use read_unaligned to safely copy the Address bytes into the 'auth' variable
        let auth = unsafe { core::ptr::addr_of!(self.authority).read_unaligned() };

        // 2. 检查复印出来的这个地址是不是全 0（默认地址）
        if auth == Address::default() {
            None // 全 0 说明没设置，返回“空”
        } else {
            // 3. 返回我们刚才“复印”好的 auth，而不是原件
            // 这样就不用从原结构体里“撕”数据了，编译器就不会报错
            // Return 'auth' (the local copy) instead of 'self.authority'
            Some(auth)
        }
    }

    /// 强制以可变引用加载账户数据，不检查所有者 (用于初始化)
    /// # Safety
    /// 调用者必须确保账户空间足够且已由程序控制
    #[allow(clippy::mut_from_ref)]
    #[inline(always)]
    pub unsafe fn load_mut_unchecked(
        account_view: &AccountView,
    ) -> Result<&mut Self, ProgramError> {
        if account_view.data_len() != Self::LEN {
            return Err(ProgramError::InvalidAccountData);
        }
        // 直接获取账户数据的原始指针并转换为可变结构体引用
        Ok(unsafe { Self::from_bytes_unchecked_mut(account_view.borrow_unchecked_mut()) })
    }
}