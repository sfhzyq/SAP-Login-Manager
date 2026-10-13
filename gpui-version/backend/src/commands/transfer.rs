use crate::models::{ExportBundle, StoreData};
use crate::storage;
use chrono::Utc;
use std::fs;
use std::sync::Mutex;

/// 导出凭据到指定文件路径（需要主密码验证，明文密码仅写入文件，不返回前端）
pub fn export_credentials(
    store: &Mutex<StoreData>,
    master_password: String,
    save_path: String,
) -> Result<(), String> {
    let s = store.lock().map_err(|e| e.to_string())?;

    // 验证主密码后才允许导出
    if !storage::verify_master_password(&s, &master_password).map_err(|e| e.to_string())? {
        return Err("主密码错误".to_string());
    }

    let mut credentials = s.credentials.clone();
    // 解密密码用于导出（失败则跳过该凭据的密码）
    for cred in &mut credentials {
        if !cred.encrypted_password.is_empty() {
            cred.password = storage::decrypt_credential_password(&s, &master_password, &cred.encrypted_password)
                .unwrap_or_default();
        }
        // 导出后清除加密密码（导入时会重新加密）
        cred.encrypted_password = String::new();
    }

    let bundle = ExportBundle {
        app_version: "1.0.0".to_string(),
        exported_at: Utc::now(),
        group_count: s.groups.len(),
        connection_count: s.credentials.len(),
        has_landscape: false,
        source_salt: s.salt.clone(),
        credentials,
        groups: s.groups.clone(),
    };

    let content = serde_json::to_string_pretty(&bundle).map_err(|e| e.to_string())?;
    fs::write(&save_path, content).map_err(|e| e.to_string())?;

    Ok(())
}

/// 从导出包导入凭据（跳过已存在的 connection_id）
pub fn import_credentials(
    store: &Mutex<StoreData>,
    master_password: String,
    bundle: ExportBundle,
) -> Result<usize, String> {
    let mut s = store.lock().map_err(|e| e.to_string())?;

    let mut imported = 0;
    for mut cred in bundle.credentials {
        // 跳过已存在的同名连接
        let exists = s.credentials.iter()
            .any(|c| c.connection_id == cred.connection_id);
        if exists {
            continue;
        }

        if !cred.password.is_empty() {
            cred.encrypted_password = storage::encrypt_credential_password(
                &s, &master_password, &cred.password
            ).map_err(|e| e.to_string())?;
            cred.password = String::new();
        }
        cred.created_at = Utc::now();
        cred.updated_at = Utc::now();
        s.credentials.push(cred);
        imported += 1;
    }

    if imported > 0 {
        storage::save_store(&s).map_err(|e| e.to_string())?;
    }
    Ok(imported)
}
