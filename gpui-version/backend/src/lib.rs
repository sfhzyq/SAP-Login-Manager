//! SAP Login Manager 后端（从 Tauri 版完整迁移，剥离 Tauri 依赖）
//!
//! 数据格式与 Tauri 版完全兼容：共享同一份 store.json，可双向切换。

pub mod commands;
pub mod crypto;
pub mod dpapi;
pub mod models;
pub mod sap_automation;
pub mod sap_parser;
pub mod storage;
pub mod window_ctl;

use std::ops::Deref;
use std::sync::{Arc, Mutex, MutexGuard};

pub use models::StoreData;

/// 全局存储句柄：UI 通过它访问 store.json
///
/// `Arc` 便于克隆到后台线程执行耗时操作（PBKDF2 派生、sapshcut 登录等），
/// `std::sync::Mutex` 跨线程加锁；注意不要在持锁期间执行 await 或长时间计算。
#[derive(Clone)]
pub struct Store(Arc<Mutex<StoreData>>);

impl Store {
    /// 初始化存储（读取 store.json；失败时创建空数据）
    pub fn init() -> Self {
        let data = storage::init_store()
            .unwrap_or_else(|_| storage::create_empty_store());
        Self(Arc::new(Mutex::new(data)))
    }

    /// 加锁访问数据
    pub fn lock(&self) -> MutexGuard<'_, StoreData> {
        self.0.lock().unwrap_or_else(|e| e.into_inner())
    }
}

impl Deref for Store {
    type Target = Mutex<StoreData>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl Default for Store {
    fn default() -> Self {
        Self::init()
    }
}
