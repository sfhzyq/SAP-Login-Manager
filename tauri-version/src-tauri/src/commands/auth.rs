use tauri::State;
use crate::storage;
use crate::dpapi;
use std::sync::Mutex;
use crate::models::StoreData;

#[tauri::command]
pub fn set_master_password(
    store: State<'_, Mutex<StoreData>>,
    password: String,
) -> Result<(), String> {
    let mut s = store.lock().map_err(|e| e.to_string())?;
    storage::set_master_password(&mut s, &password).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn verify_master_password(
    store: State<'_, Mutex<StoreData>>,
    password: String,
) -> Result<bool, String> {
    let mut s = store.lock().map_err(|e| e.to_string())?;
    let verified = storage::verify_master_password(&s, &password).map_err(|e| e.to_string())?;
    // 验证成功后，若使用旧版密钥派生或旧版加密算法则自动迁移到 V2 + GCM
    if verified && (s.key_derivation_version == 0 || s.encryption_version == 0) {
        storage::migrate_key_derivation(&mut s, &password).map_err(|e| e.to_string())?;
    }
    Ok(verified)
}

#[tauri::command]
pub fn change_master_password(
    store: State<'_, Mutex<StoreData>>,
    old_password: String,
    new_password: String,
) -> Result<(), String> {
    let mut s = store.lock().map_err(|e| e.to_string())?;
    storage::change_master_password(&mut s, &old_password, &new_password).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn is_password_set(
    store: State<'_, Mutex<StoreData>>,
) -> Result<bool, String> {
    let s = store.lock().map_err(|e| e.to_string())?;
    Ok(storage::is_password_set(&s))
}

/// 免密模式：用 Windows DPAPI 加密主密码并保存到 store.remembered_master。
/// 调用前应先用 verify_master_password 校验过该密码正确。
#[tauri::command]
pub fn remember_master_password(
    store: State<'_, Mutex<StoreData>>,
    password: String,
) -> Result<(), String> {
    let verified = {
        let s = store.lock().map_err(|e| e.to_string())?;
        storage::verify_master_password(&s, &password).map_err(|e| e.to_string())?
    };
    if !verified {
        return Err("主密码不正确，无法开启免密".to_string());
    }
    let encrypted = dpapi::encrypt(&password)?;
    let mut s = store.lock().map_err(|e| e.to_string())?;
    s.remembered_master = encrypted;
    storage::save_store(&s).map_err(|e| e.to_string())
}

/// 免密模式：读取并用 DPAPI 解密已记忆的主密码。
/// 返回空字符串表示未记忆或解密失败（例如换了 Windows 账户）。
#[tauri::command]
pub fn get_remembered_password(
    store: State<'_, Mutex<StoreData>>,
) -> Result<String, String> {
    let cipher = {
        let s = store.lock().map_err(|e| e.to_string())?;
        s.remembered_master.clone()
    };
    if cipher.is_empty() {
        return Ok(String::new());
    }
    match dpapi::decrypt(&cipher) {
        Ok(pwd) => Ok(pwd),
        Err(_) => Ok(String::new()),
    }
}

/// 关闭免密模式：清除已记忆的主密码。
#[tauri::command]
pub fn clear_remembered_password(
    store: State<'_, Mutex<StoreData>>,
) -> Result<(), String> {
    let mut s = store.lock().map_err(|e| e.to_string())?;
    if s.remembered_master.is_empty() {
        return Ok(());
    }
    s.remembered_master = String::new();
    storage::save_store(&s).map_err(|e| e.to_string())
}
