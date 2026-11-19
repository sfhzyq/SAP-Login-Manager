//! SAP Login Manager 后端（从 Tauri 版完整迁移，剥离 Tauri 依赖）
//!
//! 数据格式与 Tauri 版完全兼容：共享同一份 store.json，可双向切换。

pub mod bat_parser;
pub mod commands;
pub mod crypto;
pub mod dpapi;
pub mod models;
pub mod sap_automation;
pub mod sap_parser;
pub mod sap_scripting;
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
pub struct Store {
    data: Arc<Mutex<StoreData>>,
    /// v5 Must #1：`init_store` 失败的原因。非空表示本次会话使用**内存空库**
    /// 且 storage 层已禁止写盘（防止覆盖未读出的真实数据）。UI 必须在启动时
    /// 把这条错误显示给用户，不能只写 no-op 日志。
    init_error: Arc<Option<String>>,
}

impl Store {
    /// 初始化存储（读取 store.json；失败时保留原文件并禁用本次会话写盘）
    pub fn init() -> Self {
        match storage::init_store() {
            Ok(data) => Self {
                data: Arc::new(Mutex::new(data)),
                init_error: Arc::new(None),
            },
            Err(e) => {
                let msg = e.to_string();
                log::error!("init_store 失败，本次会话使用内存空库且不写盘: {msg}");
                Self {
                    data: Arc::new(Mutex::new(storage::create_empty_store())),
                    init_error: Arc::new(Some(msg)),
                }
            }
        }
    }

    /// 启动时的存储错误（供主窗口 push_notification 展示）；正常启动返回 None。
    pub fn init_error(&self) -> Option<&str> {
        self.init_error.as_deref()
    }

    /// 加锁访问数据。
    /// 毒锁恢复是显式选择（保持应用可用），但必须留下日志痕迹：
    /// 持锁线程 panic 后数据可能处于中间态，排障时需要知道发生过毒锁。
    pub fn lock(&self) -> MutexGuard<'_, StoreData> {
        self.data.lock().unwrap_or_else(|e| {
            log::warn!("store 锁中毒，已恢复数据继续运行（数据可能处于中间态）: {e}");
            // v4 Should #9：into_inner 不清除 poison 标志，而 commands 层经 Deref
            // 拿到的是 &Mutex<StoreData>、走的是 Mutex::lock()，此后每次都会拿
            // PoisonError —— app 能读写、所有命令却永久失败。恢复时一并清毒标。
            self.data.clear_poison();
            e.into_inner()
        })
    }
}

impl Deref for Store {
    type Target = Mutex<StoreData>;

    fn deref(&self) -> &Self::Target {
        self.data.as_ref()
    }
}

impl Default for Store {
    fn default() -> Self {
        // 注意：Default 有副作用（读/初始化 store.json）。保留是为兼容
        // `Store::default()` 调用点；新代码请用显式的 `Store::init()`。
        Self::init()
    }
}
