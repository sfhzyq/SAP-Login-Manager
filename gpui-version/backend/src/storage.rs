use std::fs;
use std::path::PathBuf;
use crate::models::{StoreData, Group};
use crate::crypto::{
    generate_salt, hash_password, verify_password, derive_key, encrypt, decrypt, encrypt_gcm,
    decrypt_gcm, CryptoError, KDF_VERSION_LATEST,
};
use chrono::Utc;
use serde_json;

#[derive(Debug, thiserror::Error)]
pub enum StorageError {
    #[error("IO 错误: {0}")]
    Io(#[from] std::io::Error),
    #[error("JSON 错误: {0}")]
    Json(#[from] serde_json::Error),
    #[error("加密错误: {0}")]
    Crypto(#[from] CryptoError),
    #[error("未设置主密码")]
    MasterPasswordNotSet,
    #[error("主密码错误")]
    WrongMasterPassword,
    #[error("系统分组不可修改或删除")]
    SystemGroupProtected,
    #[error("分组不存在")]
    GroupNotFound,
}

/// 便携版：数据存储在 exe 同目录下的 data/ 文件夹
fn get_app_data_dir() -> Result<PathBuf, StorageError> {
    let dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|p| p.to_path_buf()))
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")))
        .join("data");
    fs::create_dir_all(&dir)?;
    Ok(dir)
}

fn get_store_path() -> Result<PathBuf, StorageError> {
    Ok(get_app_data_dir()?.join("store.json"))
}

/// 系统分组定义（单一数据源，消除重复）
fn system_group_defs() -> Vec<(&'static str, &'static str, bool, bool)> {
    // (id, name, is_system, is_default)
    vec![
        ("group-default", "默认分组", false, true),
        ("group-production", "生产环境", true, false),
        ("group-test", "测试环境", true, false),
        ("group-development", "开发环境", true, false),
        ("group-configuration", "配置环境", true, false),
    ]
}

/// 创建初始系统分组
fn create_default_groups() -> Vec<Group> {
    let now = Utc::now();
    system_group_defs().into_iter().map(|(id, name, is_system, is_default)| {
        Group {
            id: id.to_string(),
            group_name: name.to_string(),
            entries: Vec::new(),
            is_system,
            is_default,
            created_at: now,
            updated_at: now,
        }
    }).collect()
}

/// 确保系统分组存在（兼容旧数据迁移）
fn ensure_default_groups(store: &mut StoreData) {
    let now = Utc::now();
    for (id, name, is_system, is_default) in system_group_defs() {
        let exists = store.groups.iter().any(|g| g.id == id);
        if !exists {
            store.groups.push(Group {
                id: id.to_string(),
                group_name: name.to_string(),
                entries: Vec::new(),
                is_system,
                is_default,
                created_at: now,
                updated_at: now,
            });
        }
    }
}

/// 创建空存储（init_store 失败时的兜底）
pub fn create_empty_store() -> StoreData {
    StoreData {
        salt: generate_salt(),
        master_password_hash: String::new(),
        credentials: Vec::new(),
        groups: create_default_groups(),
        settings: crate::models::AppSettings::default(),
        created_at: Utc::now(),
        updated_at: Utc::now(),
        key_derivation_version: 1,
        encryption_version: 1,
        remembered_master: String::new(),
    }
}

/// 初始化存储（首次运行时创建，损坏时尝试备份恢复）
pub fn init_store() -> Result<StoreData, StorageError> {
    let path = get_store_path()?;
    if path.exists() {
        let content = fs::read_to_string(&path);
        match content {
            Ok(c) => {
                match serde_json::from_str::<StoreData>(&c) {
                    Ok(mut store) => {
                        ensure_default_groups(&mut store);
                        return Ok(store);
                    }
                    Err(e) => {
                        // JSON 解析失败，尝试从备份恢复
                        log::error!("store.json 解析失败: {}，尝试从备份恢复", e);
                        let backup = path.with_extension("json.bak");
                        if backup.exists() {
                            if let Ok(backup_content) = fs::read_to_string(&backup) {
                                if let Ok(mut store) = serde_json::from_str::<StoreData>(&backup_content) {
                                    log::info!("从备份成功恢复 store");
                                    ensure_default_groups(&mut store);
                                    // 恢复后立即保存
                                    let _ = save_store(&store);
                                    return Ok(store);
                                }
                            }
                        }
                        // 备份也失败，将损坏文件重命名后创建新 store
                        let corrupted = path.with_extension("json.corrupted");
                        let _ = fs::rename(&path, &corrupted);
                        log::warn!("无法恢复，已将损坏文件重命名为 {}", corrupted.display());
                    }
                }
            }
            Err(e) => {
                log::error!("读取 store.json 失败: {}", e);
            }
        }
    }

    let store = StoreData {
        salt: generate_salt(),
        master_password_hash: String::new(),
        credentials: Vec::new(),
        groups: create_default_groups(),
        settings: crate::models::AppSettings::default(),
        created_at: Utc::now(),
        updated_at: Utc::now(),
        key_derivation_version: 1,
        encryption_version: 1,
        remembered_master: String::new(),
    };
    save_store(&store)?;
    Ok(store)
}

/// 带退避重试的 rename（应对 Windows 杀毒/EDR 瞬时文件占用导致的 os error 5/32）
#[cfg(windows)]
fn rename_with_retry(from: &std::path::Path, to: &std::path::Path) -> std::io::Result<()> {
    const MAX_RETRY: u32 = 8;
    let mut last_err: Option<std::io::Error> = None;
    for attempt in 0..MAX_RETRY {
        match fs::rename(from, to) {
            Ok(()) => return Ok(()),
            Err(e) => {
                // 仅对“拒绝访问(5)”与“占用中(32)”重试，其它错误立即返回
                let code = e.raw_os_error().unwrap_or(0);
                if code != 5 && code != 32 {
                    return Err(e);
                }
                last_err = Some(e);
                // 指数退避：20ms, 40ms, 80ms ...（最大约 2.5s）
                let wait = 20u64 << attempt.min(7);
                std::thread::sleep(std::time::Duration::from_millis(wait));
            }
        }
    }
    Err(last_err.unwrap_or_else(|| std::io::Error::other("rename 重试失败")))
}

/// 保存存储数据（原子写入：先写临时文件 + fsync，再 rename 覆盖目标，保留备份）
pub fn save_store(store: &StoreData) -> Result<(), StorageError> {
    let path = get_store_path()?;
    let content = serde_json::to_string_pretty(store)?;

    let tmp_path = path.with_extension("json.tmp");

    // 写入临时文件
    fs::write(&tmp_path, &content)?;

    // fsync 确保数据落盘（防止断电丢数据）
    #[cfg(windows)]
    {
        use std::os::windows::io::AsRawHandle;
        let file = fs::OpenOptions::new().write(true).open(&tmp_path)?;
        let handle = file.as_raw_handle();
        unsafe {
            windows_sys::Win32::Storage::FileSystem::FlushFileBuffers(handle as _);
        }
        drop(file);
    }
    #[cfg(not(windows))]
    {
        let file = fs::OpenOptions::new().write(true).open(&tmp_path)?;
        file.sync_all()?;
    }

    // 保留备份：如果目标文件已存在，先备份
    let backup_path = path.with_extension("json.bak");
    #[cfg(windows)]
    {
        // Windows: 使用 rename 直接覆盖目标（MoveFileEx REPLACE_EXISTING）
        // 先备份旧文件到 .bak
        if path.exists() {
            let _ = fs::remove_file(&backup_path);
            let _ = fs::rename(&path, &backup_path);
        }
        // rename tmp -> path
        // 注意：Windows 上杀毒软件/EDR 会在文件写入后瞬时扫描并占用文件句柄，
        // 导致 rename 抛出 “拒绝访问 (os error 5)” 或 “占用中 (os error 32)”。
        // 这里加入带退避的重试，避免删除/保存偶发失败。
        match rename_with_retry(&tmp_path, &path) {
            Ok(()) => {}
            // 如果 rename 仍失败，尝试 remove + rename 再重试一次
            Err(_) => {
                let _ = fs::remove_file(&path);
                if let Err(e) = rename_with_retry(&tmp_path, &path) {
                    // 最后的兜底：直接写入目标文件（放弃原子性，保证数据不丢）
                    fs::write(&path, &content).map_err(|_| e)?;
                    let _ = fs::remove_file(&tmp_path);
                }
            }
        }
    }

    #[cfg(not(windows))]
    {
        fs::rename(&tmp_path, &path)?;
    }

    Ok(())
}

/// 设置主密码
pub fn set_master_password(store: &mut StoreData, password: &str) -> Result<(), StorageError> {
    if !store.master_password_hash.is_empty() {
        return Err(StorageError::WrongMasterPassword);
    }
    store.master_password_hash = hash_password(password, &store.salt, KDF_VERSION_LATEST)?;
    store.key_derivation_version = KDF_VERSION_LATEST;
    store.encryption_version = 1;
    store.updated_at = Utc::now();
    save_store(store)?;
    Ok(())
}

/// 验证主密码
/// 如果使用旧版密钥派生(version 0)且验证成功，自动迁移到 V2
pub fn verify_master_password(store: &StoreData, password: &str) -> Result<bool, StorageError> {
    if store.master_password_hash.is_empty() {
        return Err(StorageError::MasterPasswordNotSet);
    }
    verify_password(password, &store.salt, &store.master_password_hash, store.key_derivation_version)
        .map_err(StorageError::from)
}

/// 迁移密钥派生算法到最新版（Argon2id）+ GCM 加密（在旧版验证成功后调用）
///
/// 原子性保证：先在临时缓冲区解密所有凭据，全部成功后再统一更新 store。
/// 若任一凭据解密失败，store 保持原状不被修改。
pub fn migrate_key_derivation(
    store: &mut StoreData,
    password: &str,
) -> Result<(), StorageError> {
    if store.key_derivation_version >= KDF_VERSION_LATEST && store.encryption_version >= 1 {
        return Ok(());
    }

    let old_key = derive_key(password, &store.salt, store.key_derivation_version)?;
    let new_key = derive_key(password, &store.salt, KDF_VERSION_LATEST)?;

    // 阶段 1：先解密所有凭据到临时缓冲区（不修改 store）
    let mut reencrypted: Vec<(usize, String)> = Vec::new();
    for (idx, cred) in store.credentials.iter().enumerate() {
        if !cred.encrypted_password.is_empty() {
            // 用旧密钥和旧加密版本解密
            let plain = if store.encryption_version >= 1 {
                decrypt_gcm(&cred.encrypted_password, &old_key)?
            } else {
                decrypt(&cred.encrypted_password, &old_key)?
            };
            // 用新密钥和 GCM 重新加密
            let new_encrypted = encrypt_gcm(&plain, &new_key)?;
            reencrypted.push((idx, new_encrypted));
        }
    }

    // 阶段 2：全部解密成功后，统一更新 store
    for (idx, new_encrypted) in reencrypted {
        store.credentials[idx].encrypted_password = new_encrypted;
    }

    store.master_password_hash = hash_password(password, &store.salt, KDF_VERSION_LATEST)?;
    store.key_derivation_version = KDF_VERSION_LATEST;
    store.encryption_version = 1;
    store.updated_at = Utc::now();
    save_store(store)?;
    Ok(())
}

/// 修改主密码
///
/// 原子性保证：先在临时缓冲区解密+重加密所有凭据，全部成功后再统一更新 store。
/// 同时迁移到最新密钥派生（Argon2id）+ GCM 加密（如果还在用旧版）。
pub fn change_master_password(
    store: &mut StoreData,
    old_password: &str,
    new_password: &str,
) -> Result<(), StorageError> {
    if !verify_master_password(store, old_password)? {
        return Err(StorageError::WrongMasterPassword);
    }

    let version = store.key_derivation_version;
    let old_key = derive_key(old_password, &store.salt, version)?;
    let new_key = derive_key(new_password, &store.salt, KDF_VERSION_LATEST)?;

    // 阶段 1：先解密+重加密所有凭据到临时缓冲区（不修改 store）
    let mut reencrypted: Vec<(usize, String)> = Vec::new();
    for (idx, cred) in store.credentials.iter().enumerate() {
        if !cred.encrypted_password.is_empty() {
            // 用旧密钥和旧加密版本解密
            let plain = if store.encryption_version >= 1 {
                decrypt_gcm(&cred.encrypted_password, &old_key)?
            } else {
                decrypt(&cred.encrypted_password, &old_key)?
            };
            // 用新密钥和 GCM 重新加密
            let new_encrypted = encrypt_gcm(&plain, &new_key)?;
            reencrypted.push((idx, new_encrypted));
        }
    }

    // 阶段 2：全部成功后，统一更新 store
    for (idx, new_encrypted) in reencrypted {
        store.credentials[idx].encrypted_password = new_encrypted;
    }

    store.master_password_hash = hash_password(new_password, &store.salt, KDF_VERSION_LATEST)?;
    store.key_derivation_version = KDF_VERSION_LATEST;
    store.encryption_version = 1;
    store.updated_at = Utc::now();
    save_store(store)?;
    Ok(())
}

/// 加密凭据密码（使用 GCM 如果 encryption_version >= 1，否则 CBC）
pub fn encrypt_credential_password(
    store: &StoreData,
    master_password: &str,
    plain_password: &str,
) -> Result<String, StorageError> {
    let key = derive_key(master_password, &store.salt, store.key_derivation_version)?;
    if store.encryption_version >= 1 {
        encrypt_gcm(plain_password, &key).map_err(StorageError::Crypto)
    } else {
        encrypt(plain_password, &key).map_err(StorageError::Crypto)
    }
}

/// 解密凭据密码（根据 encryption_version 选择算法）
pub fn decrypt_credential_password(
    store: &StoreData,
    master_password: &str,
    encrypted: &str,
) -> Result<String, StorageError> {
    let key = derive_key(master_password, &store.salt, store.key_derivation_version)?;
    if store.encryption_version >= 1 {
        decrypt_gcm(encrypted, &key).map_err(StorageError::Crypto)
    } else {
        decrypt(encrypted, &key).map_err(StorageError::Crypto)
    }
}

/// 检查是否已设置主密码
pub fn is_password_set(store: &StoreData) -> bool {
    !store.master_password_hash.is_empty()
}

/// 获取默认分组 ID
pub fn get_default_group_id(store: &StoreData) -> Option<String> {
    store.groups.iter()
        .find(|g| g.is_default)
        .map(|g| g.id.clone())
}

/// 检查分组是否受保护（系统分组或默认分组）
pub fn is_group_protected(store: &StoreData, group_id: &str) -> bool {
    store.groups.iter()
        .any(|g| g.id == group_id && (g.is_system || g.is_default))
}

/// 将凭据 ID 添加到分组
pub fn add_credential_to_group(
    store: &mut StoreData,
    credential_id: &str,
    group_id: &str,
) -> Result<(), StorageError> {
    let group = store.groups.iter_mut()
        .find(|g| g.id == group_id)
        .ok_or(StorageError::GroupNotFound)?;

    if !group.entries.contains(&credential_id.to_string()) {
        group.entries.push(credential_id.to_string());
        group.updated_at = Utc::now();
    }
    Ok(())
}
