use crate::crypto::{
    decrypt, decrypt_gcm, derive_key, encrypt_gcm, encrypt_legacy_cbc, generate_salt,
    hash_password, verify_password, CryptoError, KDF_VERSION_LATEST,
};
use crate::models::{Group, StoreData};
use chrono::Utc;
use serde_json;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use zeroize::Zeroizing;

/// v5 Must #1：初始化读取失败后禁止任何写盘。
/// 读到的是「文件存在但读不出来」，此时内存里是空库；若允许后续 save，
/// 就会用空库覆盖尚未读出的真实数据，造成不可恢复的丢失。置位后所有
/// `save_store` 直接返回 `WritesBlockedAfterInitFailure`，强制用户排除
/// 占用/权限问题并重启。
static INIT_READ_FAILED: AtomicBool = AtomicBool::new(false);

/// 当前是否处于「初始化失败、禁止写盘」状态。
pub fn is_write_blocked() -> bool {
    INIT_READ_FAILED.load(Ordering::Relaxed)
}

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
    #[error("主密码已设置（请走修改主密码流程）")]
    MasterPasswordAlreadySet,
    #[error("主密码错误")]
    WrongMasterPassword,
    #[error("系统分组不可修改或删除")]
    SystemGroupProtected,
    #[error("分组不存在")]
    GroupNotFound,
    #[error("读取 store.json 失败（{path}）：为保护原数据未写入空库，请检查文件占用或权限后重试。原因：{source}")]
    StoreReadFailed {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error("备份/隔离 store.json 失败（{path} → {backup}）：为保护原数据已中止。原因：{source}")]
    StoreBackupFailed {
        path: String,
        backup: String,
        #[source]
        source: std::io::Error,
    },
    #[error(
        "保存 store.json 失败（{path}）：原文件已保留在 {backup}，新内容暂存于 {tmp}。rename: {rename_error}；写入: {write_error}"
    )]
    StorePersistFailed {
        path: String,
        backup: String,
        tmp: String,
        rename_error: String,
        write_error: String,
    },
    #[error(
        "初始化读取失败后已禁用写盘（防止用内存空库覆盖原数据），请排除文件占用/权限问题并重启应用"
    )]
    WritesBlockedAfterInitFailure,
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

/// 脚本存储根目录：<exe_dir>/data/scripts（按系统分子目录）。
/// 供 `sap_scripting::store` 复用，集中管理脚本文件位置。
pub fn scripts_root() -> PathBuf {
    let dir = get_app_data_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
        .join("scripts");
    let _ = fs::create_dir_all(&dir);
    dir
}

/// 脚本执行日志目录：<exe_dir>/data/logs（供 `sap_scripting::runlog` 使用）。
pub fn logs_root() -> PathBuf {
    let dir = get_app_data_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
        .join("logs");
    let _ = fs::create_dir_all(&dir);
    dir
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
    system_group_defs()
        .into_iter()
        .map(|(id, name, is_system, is_default)| Group {
            id: id.to_string(),
            group_name: name.to_string(),
            entries: Vec::new(),
            is_system,
            is_default,
            created_at: now,
            updated_at: now,
        })
        .collect()
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

/// 创建空存储（init_store 失败时的兜底）。
/// 构造逻辑与 [`init_store`] 的兜底分支共用 `empty_store_template`，避免两份漂移。
pub fn create_empty_store() -> StoreData {
    empty_store_template()
}

/// 空存储模板（单一事实来源）
fn empty_store_template() -> StoreData {
    StoreData {
        salt: generate_salt(),
        master_password_hash: String::new(),
        credentials: Vec::new(),
        groups: create_default_groups(),
        settings: crate::models::AppSettings::default(),
        created_at: Utc::now(),
        updated_at: Utc::now(),
        key_derivation_version: KDF_VERSION_LATEST,
        encryption_version: 1,
        remembered_master: String::new(),
    }
}

/// 初始化存储（首次运行时创建，损坏时尝试备份恢复）。
///
/// v5 Must #1：**读取失败绝不落盘空库**。读不出来时返回 Err 并置位
/// `INIT_READ_FAILED`，本次会话后续所有 `save_store` 都会被拒绝。
pub fn init_store() -> Result<StoreData, StorageError> {
    let path = get_store_path()?;
    match init_store_from(&path) {
        Ok(mut store) => {
            // v5 Consider #26：历史 DPAPI 密文只在用户改密 / 切换免密开关时
            // 被惰性清理，老用户升级后若不触碰这些开关就永久留盘（应用永不
            // 读取）。冷启动统一清一次并落盘；明文副本用 Zeroizing 清零。
            let legacy = Zeroizing::new(std::mem::take(&mut store.remembered_master));
            if !legacy.is_empty() {
                if let Err(e) = save_store(&store) {
                    log::warn!("清理历史 remembered_master 落盘失败（内存已清除）: {e}");
                } else {
                    log::info!("已清理历史版本遗留的 remembered_master 密文");
                }
            }
            Ok(store)
        }
        Err(e) => {
            INIT_READ_FAILED.store(true, Ordering::Relaxed);
            Err(e)
        }
    }
}

/// 按显式路径初始化（抽出来供单元测试覆盖「读失败 / 损坏 / 首次运行」分支）。
fn init_store_from(path: &Path) -> Result<StoreData, StorageError> {
    match load_store_from(path)? {
        Some(store) => Ok(store),
        None => {
            // 只有「文件确实不存在」才允许落盘空库：解析失败分支会先把损坏
            // 文件重命名隔离，读失败分支已在上面 return Err，两者都到不了这里。
            if path.exists() {
                return Err(StorageError::Io(std::io::Error::new(
                    std::io::ErrorKind::AlreadyExists,
                    format!("{} 存在且无法解析，拒绝覆盖", path.display()),
                )));
            }
            let store = empty_store_template();
            let content = serde_json::to_string_pretty(&store)?;
            write_store_atomic(path, &content)?;
            Ok(store)
        }
    }
}

/// 从 `path` 载入数据。
/// `Ok(None)` 表示「可以安全新建」（文件不存在，或已把损坏文件隔离）；
/// `Err` 表示「原文件还在但不该被覆盖」，调用方必须中止而不是继续写空库。
fn load_store_from(path: &Path) -> Result<Option<StoreData>, StorageError> {
    if !path.exists() {
        return Ok(None);
    }
    // 读取失败（瞬时句柄占用 / 权限波动 / 目录占位）：直接返回 Err。
    // 不 rename、不写盘 —— 原文件原样保留，重启即可找回。
    let content = fs::read_to_string(path).map_err(|source| StorageError::StoreReadFailed {
        path: path.display().to_string(),
        source,
    })?;

    match serde_json::from_str::<StoreData>(&content) {
        Ok(mut store) => {
            ensure_default_groups(&mut store);
            return Ok(Some(store));
        }
        Err(e) => {
            log::error!("store.json 解析失败: {}，先隔离损坏文件再尝试备份恢复", e);
        }
    }

    // 先把损坏文件改名保留（带时间戳，不覆盖上一次的隔离件），
    // 之后无论恢复成功与否都不会再被写盘覆盖。
    let corrupted =
        path.with_extension(format!("json.corrupted.{}", Utc::now().timestamp_millis()));
    fs::rename(path, &corrupted).map_err(|source| StorageError::StoreBackupFailed {
        path: path.display().to_string(),
        backup: corrupted.display().to_string(),
        source,
    })?;

    // 再从 .bak 恢复；只读不写盘，避免 save_store 把这份好备份删掉。
    let backup = path.with_extension("json.bak");
    if let Ok(backup_content) = fs::read_to_string(&backup) {
        if let Ok(mut store) = serde_json::from_str::<StoreData>(&backup_content) {
            log::info!("从备份成功恢复 store（本次仅载入内存，未覆盖原文件）");
            ensure_default_groups(&mut store);
            return Ok(Some(store));
        }
    }
    log::warn!(
        "无法恢复，已将损坏文件隔离为 {}；将新建空库",
        corrupted.display()
    );
    Ok(None)
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

/// 保存存储数据（原子写入：先写临时文件 + fsync，再 rename 覆盖目标，保留备份）。
///
/// v5 Must #1：`init_store` 读失败后禁止写盘，避免用内存空库覆盖真实数据。
pub fn save_store(store: &StoreData) -> Result<(), StorageError> {
    if is_write_blocked() {
        return Err(StorageError::WritesBlockedAfterInitFailure);
    }
    let path = get_store_path()?;
    let content = serde_json::to_string_pretty(store)?;
    write_store_atomic(&path, &content)
}

/// 按显式路径原子写入（抽出来供单元测试覆盖备份 / 兜底分支）。
fn write_store_atomic(path: &Path, content: &str) -> Result<(), StorageError> {
    let tmp_path = path.with_extension("json.tmp");

    // 写入临时文件
    fs::write(&tmp_path, content)?;

    // fsync 确保数据落盘（防止断电丢数据）
    #[cfg(windows)]
    {
        use std::os::windows::io::AsRawHandle;
        let file = fs::OpenOptions::new().write(true).open(&tmp_path)?;
        let handle = file.as_raw_handle();
        // SAFETY: handle 来自本函数以 write(true) 打开的 tmp 文件，drop(file) 前有效；
        // FlushFileBuffers 只接受可写句柄，不接收调用方指针或缓冲区。
        unsafe {
            if windows_sys::Win32::Storage::FileSystem::FlushFileBuffers(handle as _) == 0 {
                return Err(std::io::Error::last_os_error().into());
            }
        }
        drop(file);
    }
    #[cfg(not(windows))]
    {
        let file = fs::OpenOptions::new().write(true).open(&tmp_path)?;
        file.sync_all()?;
    }

    #[cfg(windows)]
    {
        write_store_atomic_windows(path, &tmp_path, content)
    }
    #[cfg(not(windows))]
    {
        fs::rename(&tmp_path, path)?;
        Ok(())
    }
}

/// Windows 分支：先确保旧文件真的备份成功，再 rename tmp 到目标。
///
/// v5 Should #9 修复要点：
/// - 删除旧 `.bak` / `rename(path → .bak)` 不再 `let _ =` 吞错；备份不成功
///   立刻返回 Err，绝不继续。
/// - 兜底路径不再 `remove_file(path)` —— 此时 `path` 已被备份走，删它既无
///   意义又会在极端情况下毁掉原数据。
/// - 最终失败返回的是**真实写入错误**，同时把 `.bak` / `.tmp` 路径写进错误
///   消息，用户可据此手工恢复；不再用旧的 rename 错误盖掉根因。
#[cfg(windows)]
fn write_store_atomic_windows(
    path: &Path,
    tmp_path: &Path,
    content: &str,
) -> Result<(), StorageError> {
    let backup_path = path.with_extension("json.bak");
    if path.exists() {
        if backup_path.exists() {
            fs::remove_file(&backup_path).map_err(|source| StorageError::StoreBackupFailed {
                path: path.display().to_string(),
                backup: backup_path.display().to_string(),
                source,
            })?;
        }
        fs::rename(path, &backup_path).map_err(|source| StorageError::StoreBackupFailed {
            path: path.display().to_string(),
            backup: backup_path.display().to_string(),
            source,
        })?;
    }

    // Windows 上杀毒软件/EDR 会在文件写入后瞬时扫描并占用句柄，导致 rename
    // 抛 os error 5 / 32；带退避重试两轮，期间 .bak 与 .tmp 都完整保留。
    match rename_with_retry(tmp_path, path) {
        Ok(()) => Ok(()),
        Err(first) => match rename_with_retry(tmp_path, path) {
            Ok(()) => Ok(()),
            Err(second) => {
                // 最后兜底：直接写目标文件（放弃原子性，保证数据不丢）。
                // 失败时保留 .bak / .tmp 并回报真实写入错误。
                match fs::write(path, content) {
                    Ok(()) => {
                        let _ = fs::remove_file(tmp_path);
                        Ok(())
                    }
                    Err(write_err) => Err(StorageError::StorePersistFailed {
                        path: path.display().to_string(),
                        backup: backup_path.display().to_string(),
                        tmp: tmp_path.display().to_string(),
                        rename_error: format!("{first}; {second}"),
                        write_error: write_err.to_string(),
                    }),
                }
            }
        },
    }
}

/// 设置主密码
pub fn set_master_password(store: &mut StoreData, password: &str) -> Result<(), StorageError> {
    if !store.master_password_hash.is_empty() {
        // 语义：主密码已存在（应走 change_master_password），不是「密码错误」
        return Err(StorageError::MasterPasswordAlreadySet);
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
    verify_password(
        password,
        &store.salt,
        &store.master_password_hash,
        store.key_derivation_version,
    )
    .map_err(StorageError::from)
}

/// 迁移密钥派生算法到最新版（Argon2id）+ GCM 加密（在旧版验证成功后调用）
///
/// 原子性保证：先在临时缓冲区解密所有凭据，全部成功后再统一更新 store。
/// 若任一凭据解密失败，store 保持原状不被修改。
pub fn migrate_key_derivation(store: &mut StoreData, password: &str) -> Result<(), StorageError> {
    if store.key_derivation_version >= KDF_VERSION_LATEST && store.encryption_version >= 1 {
        return Ok(());
    }

    // Zeroizing 析构自动清零——循环内 `?` 早退路径同样覆盖（此前早退
    // 会跳过函数尾部的手动 zeroize，密钥/明文随缓冲区未清零释放）
    let old_key = Zeroizing::new(derive_key(
        password,
        &store.salt,
        store.key_derivation_version,
    )?);
    let new_key = Zeroizing::new(derive_key(password, &store.salt, KDF_VERSION_LATEST)?);

    // 阶段 1：先解密所有凭据到临时缓冲区（不修改 store）
    let mut reencrypted: Vec<(usize, String)> = Vec::new();
    for (idx, cred) in store.credentials.iter().enumerate() {
        if !cred.encrypted_password.is_empty() {
            // 用旧密钥和旧加密版本解密（明文缓冲析构自动清零）
            let plain = Zeroizing::new(if store.encryption_version >= 1 {
                decrypt_gcm(&cred.encrypted_password, &old_key)?
            } else {
                decrypt(&cred.encrypted_password, &old_key)?
            });
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
    // Zeroizing 析构自动清零，覆盖循环内 `?` 早退路径
    let old_key = Zeroizing::new(derive_key(old_password, &store.salt, version)?);
    let new_key = Zeroizing::new(derive_key(new_password, &store.salt, KDF_VERSION_LATEST)?);

    // 阶段 1：先解密+重加密所有凭据到临时缓冲区（不修改 store）
    let mut reencrypted: Vec<(usize, String)> = Vec::new();
    for (idx, cred) in store.credentials.iter().enumerate() {
        if !cred.encrypted_password.is_empty() {
            // 用旧密钥和旧加密版本解密（明文缓冲析构自动清零）
            let plain = Zeroizing::new(if store.encryption_version >= 1 {
                decrypt_gcm(&cred.encrypted_password, &old_key)?
            } else {
                decrypt(&cred.encrypted_password, &old_key)?
            });
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
    // Zeroizing：加解密失败/成功路径都保证密钥缓冲析构清零
    let key = Zeroizing::new(derive_key(
        master_password,
        &store.salt,
        store.key_derivation_version,
    )?);
    if store.encryption_version >= 1 {
        encrypt_gcm(plain_password, &key).map_err(StorageError::Crypto)
    } else {
        encrypt_legacy_cbc(plain_password, &key).map_err(StorageError::Crypto)
    }
}

/// 解密凭据密码（根据 encryption_version 选择算法）
pub fn decrypt_credential_password(
    store: &StoreData,
    master_password: &str,
    encrypted: &str,
) -> Result<String, StorageError> {
    let key = Zeroizing::new(derive_key(
        master_password,
        &store.salt,
        store.key_derivation_version,
    )?);
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
    store
        .groups
        .iter()
        .find(|g| g.is_default)
        .map(|g| g.id.clone())
}

/// 检查分组是否受保护（系统分组或默认分组）
pub fn is_group_protected(store: &StoreData, group_id: &str) -> bool {
    store
        .groups
        .iter()
        .any(|g| g.id == group_id && (g.is_system || g.is_default))
}

/// 将凭据 ID 添加到分组
pub fn add_credential_to_group(
    store: &mut StoreData,
    credential_id: &str,
    group_id: &str,
) -> Result<(), StorageError> {
    let group = store
        .groups
        .iter_mut()
        .find(|g| g.id == group_id)
        .ok_or(StorageError::GroupNotFound)?;

    if !group.entries.contains(&credential_id.to_string()) {
        group.entries.push(credential_id.to_string());
        group.updated_at = Utc::now();
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicU64;

    static SEQ: AtomicU64 = AtomicU64::new(0);

    /// 每个用例独占一个临时目录，退出时清理；测试不碰真实的 data/store.json。
    struct TestDir(PathBuf);

    impl TestDir {
        fn new(tag: &str) -> Self {
            let millis = Utc::now().timestamp_millis();
            let seq = SEQ.fetch_add(1, Ordering::Relaxed);
            let dir = std::env::temp_dir().join(format!(
                "sap-store-v5-{tag}-{}-{millis}-{seq}",
                std::process::id()
            ));
            fs::create_dir_all(&dir).expect("create test dir");
            Self(dir)
        }

        fn store(&self) -> PathBuf {
            self.0.join("store.json")
        }

        fn bak(&self) -> PathBuf {
            self.0.join("store.json.bak")
        }

        fn tmp(&self) -> PathBuf {
            self.0.join("store.json.tmp")
        }

        fn isolated_count(&self) -> usize {
            fs::read_dir(&self.0)
                .expect("read test dir")
                .flatten()
                .filter(|e| e.file_name().to_string_lossy().contains("json.corrupted."))
                .count()
        }
    }

    impl Drop for TestDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    /// v5 Must #1：文件存在但读不出来时，绝不落盘空库覆盖原文件。
    /// 用「目录占位」跨平台稳定地制造 read_to_string 失败。
    #[test]
    fn read_failure_returns_err_and_never_overwrites_original() {
        let d = TestDir::new("readfail");
        let p = d.store();
        fs::create_dir(&p).expect("directory placeholder");

        let res = init_store_from(&p);
        assert!(
            matches!(res, Err(StorageError::StoreReadFailed { .. })),
            "读取失败必须返回 StoreReadFailed，实际: {res:?}"
        );
        // 原位置仍是那个占位目录，没有被空库覆盖，也没有生成 .bak / .tmp
        assert!(p.is_dir(), "原文件位置不得被写盘覆盖");
        assert!(!d.bak().exists());
        assert!(!d.tmp().exists());
        assert_eq!(d.isolated_count(), 0, "读失败不应隔离/重命名原文件");
    }

    /// 解析失败时：损坏文件先被隔离，好备份只读恢复，且不被 save 删掉。
    #[test]
    fn corrupt_store_is_isolated_and_recovered_from_bak_without_losing_bak() {
        let d = TestDir::new("corrupt");
        let p = d.store();
        fs::write(&p, b"{ this is not json").expect("write corrupt json");

        let mut good = empty_store_template();
        good.master_password_hash = "sentinel-from-bak".to_string();
        fs::write(
            d.bak(),
            serde_json::to_string_pretty(&good).expect("serialize backup"),
        )
        .expect("write backup");

        let loaded = init_store_from(&p).expect("recover from backup");
        assert_eq!(loaded.master_password_hash, "sentinel-from-bak");
        assert!(!p.exists(), "损坏文件必须已被移走，不得被空库覆盖");
        assert!(d.bak().exists(), "好备份不得被 save_store 删除");
        assert_eq!(d.isolated_count(), 1, "损坏文件应保留一份隔离件");
    }

    /// 无备份可恢复时，先隔离损坏文件再新建空库（旧行为会直接覆盖）。
    #[test]
    fn corrupt_store_without_bak_is_isolated_before_new_store_created() {
        let d = TestDir::new("nobak");
        let p = d.store();
        fs::write(&p, b"garbage").expect("write corrupt json");

        let loaded = init_store_from(&p).expect("create empty store");
        assert!(loaded.master_password_hash.is_empty());
        assert_eq!(d.isolated_count(), 1);
        let raw = fs::read_to_string(&p).expect("new store.json");
        let parsed: StoreData = serde_json::from_str(&raw).expect("valid json");
        assert_eq!(parsed.groups.len(), 5);
    }

    /// 首次运行（文件确实不存在）才允许落盘空库。
    #[test]
    fn first_run_creates_empty_store_on_disk() {
        let d = TestDir::new("firstrun");
        let p = d.store();
        assert!(!p.exists());
        init_store_from(&p).expect("create store");
        assert!(p.exists());
        let raw = fs::read_to_string(&p).expect("read store");
        serde_json::from_str::<StoreData>(&raw).expect("valid json");
    }

    /// v5 Should #9：备份动作失败时立即返回 Err，原文件与 tmp 都还在。
    #[cfg(windows)]
    #[test]
    fn backup_failure_returns_err_and_preserves_original_and_tmp() {
        let d = TestDir::new("bakfail");
        let p = d.store();
        fs::write(&p, b"original-credential-data").expect("write original");
        // 让旧 .bak 无法被 remove_file 删除（目录占位）
        fs::create_dir(d.bak()).expect("blocking backup directory");

        let res = write_store_atomic(&p, "new-content");
        assert!(
            matches!(res, Err(StorageError::StoreBackupFailed { .. })),
            "备份失败必须显式返回 Err，实际: {res:?}"
        );
        assert_eq!(
            fs::read_to_string(&p).expect("original intact"),
            "original-credential-data",
            "原 store.json 不得被删除"
        );
        assert!(d.tmp().exists(), "新内容应留在 tmp 供人工恢复");
    }

    /// 正常写入时 .bak 保存上一版内容，可据此回滚。
    #[cfg(windows)]
    #[test]
    fn successful_write_keeps_previous_version_in_bak() {
        let d = TestDir::new("roundtrip");
        let p = d.store();
        let mut s = empty_store_template();
        s.master_password_hash = "sentinel-v1".to_string();
        write_store_atomic(&p, &serde_json::to_string_pretty(&s).expect("v1")).expect("write v1");
        s.master_password_hash = "sentinel-v2".to_string();
        write_store_atomic(&p, &serde_json::to_string_pretty(&s).expect("v2")).expect("write v2");

        let current = fs::read_to_string(&p).expect("current");
        let backup = fs::read_to_string(d.bak()).expect("backup");
        assert!(current.contains("sentinel-v2"));
        assert!(backup.contains("sentinel-v1"));
        assert!(!backup.contains("sentinel-v2"));
    }
}
