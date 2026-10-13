use crate::models::{AppSettings, StoreData};
use crate::storage;
use std::sync::Mutex;

pub fn get_settings(
    store: &Mutex<StoreData>,
) -> Result<AppSettings, String> {
    let s = store.lock().map_err(|e| e.to_string())?;
    Ok(s.settings.clone())
}

/// 保存设置。
/// 注意：开机自启由 UI 层（app crate）负责同步 Windows 注册表，
/// 此处仅持久化设置。
pub fn save_settings(
    store: &Mutex<StoreData>,
    settings: AppSettings,
) -> Result<(), String> {
    let mut s = store.lock().map_err(|e| e.to_string())?;
    let old = std::mem::replace(&mut s.settings, settings);
    // 持久化失败则回滚内存，避免内存/磁盘分叉
    if let Err(e) = storage::save_store(&s) {
        s.settings = old;
        return Err(e.to_string());
    }
    Ok(())
}
