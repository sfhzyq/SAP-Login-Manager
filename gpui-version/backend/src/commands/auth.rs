use crate::commands::{CmdResult, CommandError};
use crate::crypto::KDF_VERSION_LATEST;
use crate::models::StoreData;
use crate::storage;
use std::sync::Mutex;

/// v5 Should #3：本文件所有写路径统一「工作副本 → 落盘成功 → 提交到 guard」。
/// storage 层的 set/change/migrate 内部就会调用 `save_store(&副本)`；只有保存
/// 成功才把副本写回全局 guard，磁盘满 / EDR 占用时内存与磁盘不会分叉。
pub fn set_master_password(store: &Mutex<StoreData>, password: &str) -> CmdResult<()> {
    let mut guard = store
        .lock()
        .map_err(|e| CommandError::Other(e.to_string()))?;
    let mut s = guard.clone();
    storage::set_master_password(&mut s, password)?;
    *guard = s;
    Ok(())
}

pub fn verify_master_password(store: &Mutex<StoreData>, password: &str) -> CmdResult<bool> {
    let mut guard = store
        .lock()
        .map_err(|e| CommandError::Other(e.to_string()))?;
    let mut s = guard.clone();
    let verified = storage::verify_master_password(&s, password)?;
    // 验证成功后，若使用旧版密钥派生或旧版加密算法则自动迁移到最新版（Argon2id + GCM）
    if verified && (s.key_derivation_version < KDF_VERSION_LATEST || s.encryption_version == 0) {
        storage::migrate_key_derivation(&mut s, password)?;
        *guard = s;
    }
    Ok(verified)
}

pub fn change_master_password(
    store: &Mutex<StoreData>,
    old_password: &str,
    new_password: &str,
) -> CmdResult<()> {
    let mut guard = store
        .lock()
        .map_err(|e| CommandError::Other(e.to_string()))?;
    let mut s = guard.clone();
    storage::change_master_password(&mut s, old_password, new_password)?;
    // 清除历史版本遗留的 DPAPI 记忆密文（旧密码已失效，且现在不再写入新密文）
    s.remembered_master = String::new();
    // 无论 storage::change_master_password 内部是否已落盘，此处再保存一次
    // 以确保 remembered_master 的清除被持久化
    storage::save_store(&s)?;
    *guard = s;
    Ok(())
}

pub fn is_password_set(store: &Mutex<StoreData>) -> CmdResult<bool> {
    let s = store
        .lock()
        .map_err(|e| CommandError::Other(e.to_string()))?;
    Ok(storage::is_password_set(&s))
}

/// 免密模式（仅本次运行内有效）：校验主密码，不把主密码写入磁盘。
///
/// v4 Should #14：旧实现用 DPAPI 把主密码加密进 store.remembered_master，
/// 但全应用从未读取该字段（冷启动固定走解锁页），等于多存一份当前用户
/// 可解密的主密码副本而零功能收益。这里顺带清理历史遗留的密文。
pub fn remember_master_password(store: &Mutex<StoreData>, password: &str) -> CmdResult<()> {
    let mut guard = store
        .lock()
        .map_err(|e| CommandError::Other(e.to_string()))?;
    let mut s = guard.clone();
    if !storage::verify_master_password(&s, password)? {
        return Err(CommandError::Other(
            "主密码不正确，无法开启免密".to_string(),
        ));
    }
    if !s.remembered_master.is_empty() {
        s.remembered_master = String::new();
        storage::save_store(&s)?;
        *guard = s;
    }
    Ok(())
}

/// 关闭免密模式：清除已记忆的主密码（同样走工作副本，失败不提交）。
pub fn clear_remembered_password(store: &Mutex<StoreData>) -> CmdResult<()> {
    let mut guard = store
        .lock()
        .map_err(|e| CommandError::Other(e.to_string()))?;
    if guard.remembered_master.is_empty() {
        return Ok(());
    }
    let mut s = guard.clone();
    s.remembered_master = String::new();
    storage::save_store(&s)?;
    *guard = s;
    Ok(())
}
