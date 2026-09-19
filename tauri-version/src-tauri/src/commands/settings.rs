use tauri::{AppHandle, State};
use crate::storage;
use crate::models::{AppSettings, StoreData};
use std::sync::Mutex;

#[tauri::command]
pub fn get_settings(
    store: State<'_, Mutex<StoreData>>,
) -> Result<AppSettings, String> {
    let s = store.lock().map_err(|e| e.to_string())?;
    Ok(s.settings.clone())
}

#[tauri::command]
pub fn save_settings(
    app: AppHandle,
    store: State<'_, Mutex<StoreData>>,
    settings: AppSettings,
) -> Result<(), String> {
    // 同步开机自启状态
    #[cfg(desktop)]
    {
        use tauri_plugin_autostart::ManagerExt;
        let mgr = app.autolaunch();
        let enabled = mgr.is_enabled().unwrap_or(false);
        if settings.auto_start && !enabled {
            let _ = mgr.enable();
        } else if !settings.auto_start && enabled {
            let _ = mgr.disable();
        }
    }
    let mut s = store.lock().map_err(|e| e.to_string())?;
    s.settings = settings;
    storage::save_store(&s).map_err(|e| e.to_string())
}
