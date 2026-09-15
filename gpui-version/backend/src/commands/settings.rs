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
    s.settings = settings;
    storage::save_store(&s).map_err(|e| e.to_string())
}
