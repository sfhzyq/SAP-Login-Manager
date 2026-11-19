use crate::commands::{CmdResult, CommandError};
use crate::crypto::{decrypt, decrypt_gcm, derive_key};
use crate::models::{Credential, Group, StoreData};
use crate::storage;
use chrono::Utc;
use std::sync::Mutex;
use uuid::Uuid;

/// 锁中毒（持锁线程 panic 后）转为命令错误的统一写法
fn poisoned(e: impl std::fmt::Display) -> CommandError {
    CommandError::Other(format!("存储锁不可用: {e}"))
}

/// 编辑保存时把「表单不管理」的字段从原凭据带回。
/// collect() 每次从零重建 Credential，缺了这几行会让一次编辑静默丢掉
/// 脚本指针 / 服务 UUID / 收藏排序 / 消息服务器端口 / 创建时间（v4 Must #1）。
fn preserve_unmanaged_fields(credential: &mut Credential, existing: &Credential) {
    credential.script_path = existing.script_path.clone();
    credential.uuid = existing.uuid.clone();
    credential.favorite_order = existing.favorite_order;
    credential.message_server_port = existing.message_server_port.clone();
    credential.created_at = existing.created_at;
}

/// 写入前校验主密码。派生密钥只依赖 密码+salt+version，与 hash 无关，
/// 写错密码当下不会报错，但改密后会话仍持旧密码时，新凭据重启后永远解不开
///（v4 Must #3）。校验把「静默写坏数据」变成可见的 BadMasterPassword。
fn ensure_master_password(store: &StoreData, password: &str) -> CmdResult<()> {
    if storage::verify_master_password(store, password)? {
        Ok(())
    } else {
        Err(CommandError::BadMasterPassword)
    }
}

pub fn get_credentials(store: &Mutex<StoreData>) -> CmdResult<Vec<Credential>> {
    let s = store.lock().map_err(poisoned)?;
    let mut creds = s.credentials.clone();
    // 排序：收藏优先 → 登录次数降序 → 最后登录时间降序
    creds.sort_by(|a, b| {
        b.is_favorite
            .cmp(&a.is_favorite)
            .then_with(|| b.login_count.cmp(&a.login_count))
            .then_with(|| b.last_login_at.cmp(&a.last_login_at))
    });
    Ok(creds)
}

pub fn get_credential(store: &Mutex<StoreData>, id: String) -> CmdResult<Option<Credential>> {
    let s = store.lock().map_err(poisoned)?;
    Ok(s.credentials.iter().find(|c| c.id == id).cloned())
}

pub fn add_credential(
    store: &Mutex<StoreData>,
    master_password: &str,
    mut credential: Credential,
) -> CmdResult<Credential> {
    let mut guard = store.lock().map_err(poisoned)?;
    let mut s = guard.clone(); // 工作副本：持久化成功后才提交，避免内存/磁盘分叉

    // 写入前校验主密码：改密后会话仍持旧密码时，新凭据会用旧密钥加密，
    // 重启后 GCM 认证失败、密码永久丢失（v4 Must #3）
    ensure_master_password(&s, master_password)?;
    credential.id = Uuid::new_v4().to_string();
    credential.encrypted_password =
        storage::encrypt_credential_password(&s, master_password, &credential.password)?;
    credential.password = String::new();

    credential.login_count = 0;
    credential.last_login_at = None;
    credential.created_at = Utc::now();
    credential.updated_at = Utc::now();

    // 根据环境自动归入对应系统分组
    let group_id = resolve_group_id(&s, &credential);
    credential.group_id = group_id.clone();

    if let Some(ref gid) = group_id {
        if !gid.is_empty() {
            // entries 仅为兼容冗余轨，失败不阻断（归属以 group_id 为事实源）
            let _ = storage::add_credential_to_group(&mut s, &credential.id, gid);
        }
    }

    s.credentials.push(credential.clone());
    storage::save_store(&s)?;
    *guard = s; // 持久化成功，提交内存

    Ok(credential)
}

pub fn update_credential(
    store: &Mutex<StoreData>,
    master_password: &str,
    mut credential: Credential,
) -> CmdResult<Credential> {
    let mut guard = store.lock().map_err(poisoned)?;
    let mut s = guard.clone(); // 工作副本：持久化成功后才提交，避免内存/磁盘分叉

    let idx = s
        .credentials
        .iter()
        .position(|c| c.id == credential.id)
        .ok_or_else(|| CommandError::NotFound {
            entity: "凭据",
            id: credential.id.clone(),
        })?;

    // 获取旧分组 ID
    let old_group_id = s.credentials[idx].group_id.clone();

    if !credential.password.is_empty() {
        // 只有真正重新加密时才需要验密（留空保持原密码的路径不派生新密钥）
        ensure_master_password(&s, master_password)?;
        credential.encrypted_password =
            storage::encrypt_credential_password(&s, master_password, &credential.password)?;
        credential.password = String::new();
    } else if credential.encrypted_password.is_empty() {
        // 密码未修改时，保留原有的加密密码
        credential.encrypted_password = s.credentials[idx].encrypted_password.clone();
    }

    // 保留原有的 login_count 和 last_login_at
    credential.login_count = s.credentials[idx].login_count;
    credential.last_login_at = s.credentials[idx].last_login_at;
    credential.updated_at = Utc::now();

    // 根据环境重新解析分组
    let new_group_id = resolve_group_id(&s, &credential);
    credential.group_id = new_group_id.clone();

    // 表单不管理这些字段，整体替换前必须从原凭据带回（v4 Must #1）
    preserve_unmanaged_fields(&mut credential, &s.credentials[idx]);
    s.credentials[idx] = credential.clone();

    // 如果分组变更，同步更新分组的 entries 列表
    if old_group_id != new_group_id {
        let cred_id = credential.id.clone();

        // 从所有分组中移除该凭据
        for group in &mut s.groups {
            group.entries.retain(|eid| eid != &cred_id);
        }

        // 添加到新分组（entries 冗余轨失败不阻断）
        if let Some(ref gid) = new_group_id {
            if !gid.is_empty() {
                let _ = storage::add_credential_to_group(&mut s, &cred_id, gid);
            }
        }
    }

    storage::save_store(&s)?;
    *guard = s; // 持久化成功，提交内存

    Ok(credential)
}

pub fn delete_credential(store: &Mutex<StoreData>, id: String) -> CmdResult<()> {
    let mut guard = store.lock().map_err(poisoned)?;
    let mut s = guard.clone(); // 工作副本：持久化成功后才提交
    s.credentials.retain(|c| c.id != id);
    for group in &mut s.groups {
        group.entries.retain(|eid| eid != &id);
    }
    storage::save_store(&s).map_err(|e| {
        CommandError::Other(format!("保存失败: {e}（可能被杀毒软件占用，请稍后重试）"))
    })?;
    *guard = s; // 持久化成功，提交内存
    Ok(())
}

/// 撤销删除：原样恢复凭据（保留 id/加密密码/统计/收藏/分组归属）
pub fn restore_credentials(
    store: &Mutex<StoreData>,
    credentials: Vec<Credential>,
) -> CmdResult<usize> {
    let mut guard = store.lock().map_err(poisoned)?;
    let mut s = guard.clone(); // 工作副本：持久化成功后才提交
    let mut restored = 0;
    for mut cred in credentials {
        // 已存在同 ID 则跳过，避免重复
        if s.credentials.iter().any(|c| c.id == cred.id) {
            continue;
        }
        cred.password = String::new();
        cred.updated_at = Utc::now();
        // 目标分组不存在（或为空）时回退默认分组，
        // 避免悬挂 group_id 使凭据在 UI 分组筛选中不可见
        let target = match cred.group_id.clone() {
            Some(ref gid) if !gid.is_empty() && s.groups.iter().any(|g| &g.id == gid) => {
                Some(gid.clone())
            }
            _ => storage::get_default_group_id(&s),
        };
        cred.group_id = target.clone();
        if let Some(ref gid) = target {
            // entries 冗余轨，失败不阻断
            let _ = storage::add_credential_to_group(&mut s, &cred.id, gid);
        }
        s.credentials.push(cred);
        restored += 1;
    }
    if restored > 0 {
        storage::save_store(&s)?;
        *guard = s; // 持久化成功，提交内存
    }
    Ok(restored)
}

/// 获取解密后的密码（用于复制到剪贴板，前端配合自动清空策略）
pub fn get_decrypted_password(
    store: &Mutex<StoreData>,
    master_password: &str,
    credential_id: String,
) -> CmdResult<String> {
    let (encrypted, salt, kdv, ev) = {
        let s = store.lock().map_err(poisoned)?;
        let cred = s
            .credentials
            .iter()
            .find(|c| c.id == credential_id)
            .ok_or_else(|| CommandError::NotFound {
                entity: "凭据",
                id: credential_id.clone(),
            })?;
        (
            cred.encrypted_password.clone(),
            s.salt.clone(),
            s.key_derivation_version,
            s.encryption_version,
        )
    };
    if encrypted.is_empty() {
        return Ok(String::new());
    }
    let key = derive_key(master_password, &salt, kdv)?;
    if ev >= 1 {
        decrypt_gcm(&encrypted, &key).map_err(CommandError::from)
    } else {
        decrypt(&encrypted, &key).map_err(CommandError::from)
    }
}

/// 根据环境类型解析目标分组 ID
/// 优先级：自建分组 > 环境对应系统分组 > 默认分组
fn resolve_group_id(store: &StoreData, credential: &Credential) -> Option<String> {
    // 如果用户指定了自建分组（非系统、非默认分组），使用它
    if let Some(ref gid) = credential.group_id {
        if !gid.is_empty() {
            if let Some(g) = store.groups.iter().find(|g| g.id == *gid) {
                if !g.is_system && !g.is_default {
                    return Some(gid.clone());
                }
            }
        }
    }

    // 根据环境类型归入对应系统分组（分组不存在时回退默认分组，
    // 避免悬挂 group_id 或 add_credential 整体失败）
    let system_group = match credential.environment.as_str() {
        "production" => Some("group-production"),
        "test" => Some("group-test"),
        "development" => Some("group-development"),
        "configuration" => Some("group-configuration"),
        _ => None,
    };
    if let Some(gid) = system_group {
        if store.groups.iter().any(|g| g.id == gid) {
            return Some(gid.to_string());
        }
    }
    storage::get_default_group_id(store)
}

// === Groups ===

pub fn get_groups(store: &Mutex<StoreData>) -> CmdResult<Vec<Group>> {
    let s = store.lock().map_err(poisoned)?;
    Ok(s.groups.clone())
}

pub fn add_group(store: &Mutex<StoreData>, mut group: Group) -> CmdResult<Group> {
    let mut guard = store.lock().map_err(poisoned)?;
    let mut s = guard.clone(); // 工作副本：持久化成功后才提交
                               // 用户创建的分组永远不是系统分组或默认分组
    group.id = Uuid::new_v4().to_string();
    group.is_system = false;
    group.is_default = false;
    group.created_at = Utc::now();
    group.updated_at = Utc::now();
    s.groups.push(group.clone());
    storage::save_store(&s)?;
    *guard = s; // 持久化成功，提交内存
    Ok(group)
}

pub fn update_group(store: &Mutex<StoreData>, mut group: Group) -> CmdResult<Group> {
    let mut guard = store.lock().map_err(poisoned)?;
    let mut s = guard.clone(); // 工作副本：持久化成功后才提交

    let idx = s
        .groups
        .iter()
        .position(|g| g.id == group.id)
        .ok_or_else(|| CommandError::NotFound {
            entity: "分组",
            id: group.id.clone(),
        })?;

    // 系统分组和默认分组不可修改名称
    if storage::is_group_protected(&s, &group.id) {
        return Err(CommandError::Other("系统分组不可修改".to_string()));
    }

    group.updated_at = Utc::now();
    s.groups[idx] = group.clone();
    storage::save_store(&s)?;
    *guard = s; // 持久化成功，提交内存
    Ok(group)
}

/// 重命名分组
pub fn rename_group(store: &Mutex<StoreData>, group_id: String, new_name: String) -> CmdResult<()> {
    let mut guard = store.lock().map_err(poisoned)?;
    let mut s = guard.clone(); // 工作副本：持久化成功后才提交

    let idx = s
        .groups
        .iter()
        .position(|g| g.id == group_id)
        .ok_or_else(|| CommandError::NotFound {
            entity: "分组",
            id: group_id.clone(),
        })?;

    // 系统分组和默认分组不可修改名称
    if storage::is_group_protected(&s, &group_id) {
        return Err(CommandError::Other("系统分组不可修改".to_string()));
    }

    s.groups[idx].group_name = new_name;
    s.groups[idx].updated_at = Utc::now();
    storage::save_store(&s)?;
    *guard = s; // 持久化成功，提交内存
    Ok(())
}

/// 批量移动凭据到指定分组
pub fn batch_move_to_group(
    store: &Mutex<StoreData>,
    credential_ids: Vec<String>,
    group_id: String,
) -> CmdResult<usize> {
    let mut guard = store.lock().map_err(poisoned)?;
    // 目标分组不存在时直接拒绝，避免「entries 已移除但 group_id 未更新」的半程状态
    if !guard.groups.iter().any(|g| g.id == group_id) {
        return Err(CommandError::Other("目标分组不存在".to_string()));
    }
    let mut s = guard.clone(); // 工作副本：持久化成功后才提交

    let mut moved = 0;

    for id in &credential_ids {
        // 从所有分组中移除该凭据
        for group in &mut s.groups {
            group.entries.retain(|eid| eid != id);
        }
        // 添加到目标分组（已前置校验存在，不会 GroupNotFound）
        if storage::add_credential_to_group(&mut s, id, &group_id).is_ok() {
            moved += 1;
        }
        // 更新凭据的 group_id
        if let Some(cred) = s.credentials.iter_mut().find(|c| &c.id == id) {
            cred.group_id = Some(group_id.clone());
            cred.updated_at = Utc::now();
        }
    }

    storage::save_store(&s)?;
    *guard = s; // 持久化成功，提交内存
    Ok(moved)
}

/// 批量设置收藏（true=收藏，false=取消收藏）
pub fn batch_favorite(
    store: &Mutex<StoreData>,
    credential_ids: Vec<String>,
    favorite: bool,
) -> CmdResult<usize> {
    let mut guard = store.lock().map_err(poisoned)?;
    let mut s = guard.clone(); // 工作副本：持久化成功后才提交
    let mut affected = 0;

    // 收藏排序序号：新增收藏时取当前最大值
    let max_order = s
        .credentials
        .iter()
        .filter_map(|c| c.favorite_order)
        .max()
        .unwrap_or(0);
    let mut next_order = max_order + 1;

    for id in &credential_ids {
        if let Some(cred) = s.credentials.iter_mut().find(|c| &c.id == id) {
            if favorite && !cred.is_favorite {
                cred.is_favorite = true;
                cred.favorite_order = Some(next_order);
                next_order += 1;
            } else if !favorite && cred.is_favorite {
                cred.is_favorite = false;
                cred.favorite_order = None;
            } else {
                continue;
            }
            cred.updated_at = Utc::now();
            affected += 1;
        }
    }

    storage::save_store(&s)?;
    *guard = s; // 持久化成功，提交内存
    Ok(affected)
}

/// 批量设置登录语言（ZH / EN / JA）
pub fn batch_set_language(
    store: &Mutex<StoreData>,
    credential_ids: Vec<String>,
    language: String,
) -> CmdResult<usize> {
    if !matches!(language.as_str(), "ZH" | "EN" | "JA") {
        return Err(CommandError::InvalidInput(format!(
            "无效的语言代码: {language}"
        )));
    }
    let mut guard = store.lock().map_err(poisoned)?;
    let mut s = guard.clone(); // 工作副本：持久化成功后才提交
    let mut affected = 0;

    for id in &credential_ids {
        if let Some(cred) = s.credentials.iter_mut().find(|c| &c.id == id) {
            if cred.language == language {
                continue; // 幂等：无变化项不计入 affected
            }
            cred.language = language.clone();
            cred.updated_at = Utc::now();
            affected += 1;
        }
    }

    storage::save_store(&s)?;
    *guard = s; // 持久化成功，提交内存
    Ok(affected)
}

/// 批量设置环境分类（production / test / development / configuration / ""=未分类）
pub fn batch_set_environment(
    store: &Mutex<StoreData>,
    credential_ids: Vec<String>,
    environment: String,
) -> CmdResult<usize> {
    if !matches!(
        environment.as_str(),
        "production" | "test" | "development" | "configuration" | ""
    ) {
        return Err(CommandError::InvalidInput(format!(
            "无效的环境分类: {environment}"
        )));
    }
    let mut guard = store.lock().map_err(poisoned)?;
    let mut s = guard.clone(); // 工作副本：持久化成功后才提交
    let mut affected = 0;

    for id in &credential_ids {
        if let Some(cred) = s.credentials.iter_mut().find(|c| &c.id == id) {
            if cred.environment == environment {
                continue; // 幂等：无变化项不计入 affected
            }
            cred.environment = environment.clone();
            cred.updated_at = Utc::now();
            affected += 1;
        }
    }

    storage::save_store(&s)?;
    *guard = s; // 持久化成功，提交内存
    Ok(affected)
}

pub fn delete_group(store: &Mutex<StoreData>, id: String) -> CmdResult<()> {
    let mut guard = store.lock().map_err(poisoned)?;
    let mut s = guard.clone(); // 工作副本：持久化成功后才提交

    // 系统分组和默认分组不可删除
    if storage::is_group_protected(&s, &id) {
        return Err(CommandError::Other("系统分组不可删除".to_string()));
    }

    // 默认行为：将凭据移入默认分组，避免孤儿数据
    let default_group_id =
        storage::get_default_group_id(&s).ok_or_else(|| CommandError::NotFound {
            entity: "默认分组",
            id: "group-default".to_string(),
        })?;
    for cred in &mut s.credentials {
        if cred.group_id.as_deref() == Some(&id) {
            cred.group_id = Some(default_group_id.clone());
        }
    }
    // 从默认分组的 entries 中无需操作（凭据已通过 group_id 关联）
    s.groups.retain(|g| g.id != id);
    storage::save_store(&s)?;
    *guard = s; // 持久化成功，提交内存
    Ok(())
}

/// 删除分组（带选项）
/// mode: "move" = 将凭据移入默认分组后删除分组
///       "delete" = 删除分组内所有凭据后删除分组
pub fn delete_group_with_options(
    store: &Mutex<StoreData>,
    id: String,
    mode: String,
) -> CmdResult<()> {
    let mut guard = store.lock().map_err(poisoned)?;
    let mut s = guard.clone(); // 工作副本：持久化成功后才提交

    // 系统分组和默认分组不可删除
    if storage::is_group_protected(&s, &id) {
        return Err(CommandError::Other("系统分组不可删除".to_string()));
    }

    // 获取分组内的凭据 ID 列表（以 cred.group_id 为事实源；
    // group.entries 是兼容冗余轨可能滞后：旧数据会漏删凭据、
    // 甚至误删已移到其他分组的凭据）
    let entry_ids: Vec<String> = s
        .credentials
        .iter()
        .filter(|c| c.group_id.as_deref() == Some(&id))
        .map(|c| c.id.clone())
        .collect();

    match mode.as_str() {
        "move" => {
            // 将凭据移入默认分组
            let default_group_id =
                storage::get_default_group_id(&s).ok_or_else(|| CommandError::NotFound {
                    entity: "默认分组",
                    id: "group-default".to_string(),
                })?;
            for eid in &entry_ids {
                // 更新凭据的 group_id
                if let Some(cred) = s.credentials.iter_mut().find(|c| c.id == *eid) {
                    cred.group_id = Some(default_group_id.clone());
                    cred.updated_at = Utc::now();
                }
            }
            // 将凭据 ID 添加到默认分组的 entries
            if let Some(default_group) = s.groups.iter_mut().find(|g| g.id == default_group_id) {
                for eid in &entry_ids {
                    if !default_group.entries.contains(eid) {
                        default_group.entries.push(eid.clone());
                    }
                }
                default_group.updated_at = Utc::now();
            }
        }
        "delete" => {
            // 删除分组内所有凭据
            s.credentials.retain(|c| !entry_ids.contains(&c.id));
        }
        other => {
            return Err(CommandError::InvalidInput(format!(
                "无效的删除模式: {other}"
            )))
        }
    }

    // 删除分组本身
    s.groups.retain(|g| g.id != id);
    storage::save_store(&s)?;
    *guard = s; // 持久化成功，提交内存
    Ok(())
}

/// 将凭据移动到指定分组
pub fn move_credential_to_group(
    store: &Mutex<StoreData>,
    credential_id: String,
    group_id: String,
) -> CmdResult<()> {
    let mut guard = store.lock().map_err(poisoned)?;
    // 先校验目标分组存在，再做任何变更（失败不产生半程状态）
    if !guard.groups.iter().any(|g| g.id == group_id) {
        return Err(CommandError::Other("目标分组不存在".to_string()));
    }
    let mut s = guard.clone(); // 工作副本：持久化成功后才提交

    // 从所有分组中移除该凭据
    for group in &mut s.groups {
        group.entries.retain(|eid| eid != &credential_id);
    }

    // 添加到目标分组（已前置校验，entries 冗余轨失败不阻断）
    let _ = storage::add_credential_to_group(&mut s, &credential_id, &group_id);

    // 更新凭据的 group_id
    if let Some(cred) = s.credentials.iter_mut().find(|c| c.id == credential_id) {
        cred.group_id = Some(group_id);
        cred.updated_at = Utc::now();
    }

    storage::save_store(&s)?;
    *guard = s; // 持久化成功，提交内存
    Ok(())
}

// === 收藏 ===

/// 切换凭据收藏状态
pub fn toggle_favorite(store: &Mutex<StoreData>, credential_id: String) -> CmdResult<bool> {
    let mut guard = store.lock().map_err(poisoned)?;
    let mut s = guard.clone(); // 工作副本：持久化成功后才提交

    let max_order = s
        .credentials
        .iter()
        .filter_map(|c| c.favorite_order)
        .max()
        .unwrap_or(0);

    let cred = s
        .credentials
        .iter_mut()
        .find(|c| c.id == credential_id)
        .ok_or_else(|| CommandError::NotFound {
            entity: "凭据",
            id: credential_id.clone(),
        })?;

    cred.is_favorite = !cred.is_favorite;
    if cred.is_favorite {
        // 设置排序序号为当前最大+1
        cred.favorite_order = Some(max_order + 1);
    } else {
        cred.favorite_order = None;
    }
    cred.updated_at = Utc::now();

    let is_fav = cred.is_favorite;
    storage::save_store(&s)?;
    *guard = s; // 持久化成功，提交内存
    Ok(is_fav)
}

/// 重排序收藏列表
pub fn reorder_favorites(store: &Mutex<StoreData>, ordered_ids: Vec<String>) -> CmdResult<()> {
    let mut guard = store.lock().map_err(poisoned)?;
    let mut s = guard.clone(); // 工作副本：持久化成功后才提交

    for (idx, id) in ordered_ids.iter().enumerate() {
        if let Some(cred) = s.credentials.iter_mut().find(|c| &c.id == id) {
            cred.favorite_order = Some(idx as u32);
            cred.updated_at = Utc::now();
        }
    }

    storage::save_store(&s)?;
    *guard = s; // 持久化成功，提交内存
    Ok(())
}

// === 连接分享 ===

/// 生成与 SAP Logon 字段一致的分享文本（不含密码）
fn format_share_text(cred: &Credential) -> String {
    // 系统标识 = SID（3 位，如 PRD）；老数据 system_id 为空时回退 connection_id
    let sid = if cred.system_id.is_empty() {
        &cred.connection_id
    } else {
        &cred.system_id
    };
    let mut lines = Vec::new();
    lines.push(format!("系统标识: {}", sid));
    if let Some(ref dn) = cred.display_name {
        if !dn.is_empty() {
            lines.push(format!("显示名称: {}", dn));
        }
    }
    lines.push(format!("客户端: {}", cred.client));
    lines.push(format!("用户名: {}", cred.username));
    lines.push(format!("语言: {}", cred.language));
    // 服务器信息（按连接类型输出，与 SAP Logon 字段对应）
    if cred.connection_type == "load_balancing" {
        lines.push(format!("消息服务器: {}", cred.message_server));
        if !cred.message_server_port.is_empty() {
            lines.push(format!("消息服务器端口: {}", cred.message_server_port));
        }
        if !cred.logon_group.is_empty() {
            lines.push(format!("登录组: {}", cred.logon_group));
        }
    } else {
        lines.push(format!("应用服务器: {}", cred.app_server));
        if !cred.system_number.is_empty() {
            lines.push(format!("实例编号: {}", cred.system_number));
        }
    }
    if !cred.environment.is_empty() {
        let env_name = match cred.environment.as_str() {
            "production" => "生产环境",
            "test" => "测试环境",
            "development" => "开发环境",
            "configuration" => "配置环境",
            _ => "未分类",
        };
        lines.push(format!("环境: {}", env_name));
    }
    lines.join("\n")
}

/// 剪贴板写入（可能被其他进程阻塞，必须在释放 store 锁之后调用）
fn write_clipboard(text: &str) -> CmdResult<()> {
    let mut cb = arboard::Clipboard::new().map_err(|e| CommandError::External(e.to_string()))?;
    cb.set_text(text.to_string())
        .map_err(|e| CommandError::External(e.to_string()))?;
    Ok(())
}

/// 分享单条连接信息（不含凭据）到剪贴板。
/// 构造文本时持锁，剪贴板操作在释放锁之后执行。
pub fn share_credential(store: &Mutex<StoreData>, credential_id: String) -> CmdResult<String> {
    let text = {
        let s = store.lock().map_err(poisoned)?;
        let cred = s
            .credentials
            .iter()
            .find(|c| c.id == credential_id)
            .ok_or_else(|| CommandError::NotFound {
                entity: "凭据",
                id: credential_id.clone(),
            })?;
        format_share_text(cred)
    };
    write_clipboard(&text)?;
    Ok(text)
}

/// v5 Should #11：批量分享把全部文本拼成一段后**一次**写入剪贴板。
/// 逐条调 `share_credential` 时每条都覆盖剪贴板，最终只剩最后一条却报
/// 「已分享 N 个」，属假成功。返回 `(成功条数, 失败条数)`。
pub fn share_credentials_batch(
    store: &Mutex<StoreData>,
    credential_ids: Vec<String>,
) -> CmdResult<(usize, usize)> {
    let mut texts = Vec::new();
    let mut failed = 0usize;
    {
        let s = store.lock().map_err(poisoned)?;
        for id in &credential_ids {
            match s.credentials.iter().find(|c| c.id == *id) {
                Some(cred) => texts.push(format_share_text(cred)),
                None => {
                    log::warn!("批量分享跳过不存在的凭据: {id}");
                    failed += 1;
                }
            }
        }
    }
    if !texts.is_empty() {
        write_clipboard(&texts.join("\n\n"))?;
    }
    Ok((texts.len(), failed))
}

// === 批量登录 ===

/// 批量登录多个凭据
/// 不在 sleep 期间持有锁，避免阻塞其他操作
pub fn batch_login(
    store: &Mutex<StoreData>,
    master_password: &str,
    credential_ids: Vec<String>,
    interval_secs: u64,
) -> CmdResult<usize> {
    // 阶段1：加锁，克隆所需数据，派生密钥（仅一次），释放锁。
    // 密钥用 Zeroizing 包装：整个批量登录期间驻留，函数结束自动清零
    let (creds_to_login, key, enc_version, custom_sap_path) = {
        let s = store.lock().map_err(poisoned)?;
        let key = zeroize::Zeroizing::new(derive_key(
            master_password,
            &s.salt,
            s.key_derivation_version,
        )?);
        let enc_version = s.encryption_version;
        let custom_sap_path = s.settings.sap_logon_path.clone();
        let creds: Vec<Credential> = credential_ids
            .iter()
            .filter_map(|id| s.credentials.iter().find(|c| &c.id == id).cloned())
            .filter(|c| !c.username.is_empty())
            .collect();
        (creds, key, enc_version, custom_sap_path)
    };

    let mut success_count = 0;
    let now = Utc::now();
    let mut success_ids: Vec<String> = Vec::new();

    /// 把已成功条目的登录记账落盘（提前退出路径共用）。
    /// 记账属尽力而为：锁毒化/保存失败仅记日志，不覆盖真正的业务错误。
    fn apply_success_accounting(
        store: &Mutex<StoreData>,
        success_ids: &[String],
        now: chrono::DateTime<Utc>,
    ) {
        if success_ids.is_empty() {
            return;
        }
        let mut s = match store.lock() {
            Ok(s) => s,
            Err(e) => {
                log::warn!("登录记账：存储锁不可用: {e}");
                return;
            }
        };
        for sid in success_ids {
            if let Some(c) = s.credentials.iter_mut().find(|c| &c.id == sid) {
                c.login_count += 1;
                c.last_login_at = Some(now);
            }
        }
        if let Err(e) = storage::save_store(&s) {
            log::warn!("登录记账落盘失败（内存已更新）: {e}");
        }
    }

    // 阶段2：不加锁，执行 SAP 登录 + sleep
    for (idx, cred) in creds_to_login.iter().enumerate() {
        if idx > 0 && interval_secs > 0 {
            std::thread::sleep(std::time::Duration::from_secs(interval_secs));
        }

        // 解密失败同样要先记账已成功条目，否则前面几次成功登录的计数丢失
        let password = if enc_version >= 1 {
            match decrypt_gcm(&cred.encrypted_password, &key) {
                Ok(p) => p,
                Err(e) => {
                    apply_success_accounting(store, &success_ids, now);
                    return Err(CommandError::External(format!(
                        "解密 {} 失败: {}（已成功登录 {} 个，请勿重复登录）",
                        cred.connection_id, e, success_count
                    )));
                }
            }
        } else {
            match decrypt(&cred.encrypted_password, &key) {
                Ok(p) => p,
                Err(e) => {
                    apply_success_accounting(store, &success_ids, now);
                    return Err(CommandError::External(format!(
                        "解密 {} 失败: {}（已成功登录 {} 个，请勿重复登录）",
                        cred.connection_id, e, success_count
                    )));
                }
            }
        };

        let params = crate::sap_automation::SapLoginParams {
            connection_id: cred.connection_id.clone(),
            client: cred.client.clone(),
            username: cred.username.clone(),
            password,
            language: cred.language.clone(),
            connection_type: cred.connection_type.clone(),
            system_id: cred.system_id.clone(),
            app_server: cred.app_server.clone(),
            system_number: cred.system_number.clone(),
            message_server: cred.message_server.clone(),
            message_server_port: cred.message_server_port.clone(),
            logon_group: cred.logon_group.clone(),
            saprouter: cred.saprouter.clone(),
            uuid: cred.uuid.clone(),
            snc_enabled: cred.snc_enabled,
            snc_name: cred.snc_name.clone(),
            snc_qop: cred.snc_qop.clone(),
            snc_sso: cred.snc_sso,
            post_login_action: cred.post_login_action.clone(),
            post_login_action_type: cred.post_login_action_type.clone(),
            custom_sap_path: custom_sap_path.clone(),
        };

        match crate::sap_automation::login_to_sap(&params) {
            Ok(()) => {
                success_ids.push(cred.id.clone());
                success_count += 1;
            }
            Err(e) => {
                // 保存已成功的登录计数后再返回错误（与解密失败路径共用记账函数）
                apply_success_accounting(store, &success_ids, now);
                return Err(CommandError::External(format!(
                    "登录 {} 失败: {}（已成功登录 {} 个，请勿重复登录）",
                    cred.connection_id, e, success_count
                )));
            }
        }
    }

    // 阶段3：记账（尽力而为语义，见 apply_success_accounting）——此前
    // save_store 失败会 ? 提前返回：全部已成功登录的记账丢失且报存储
    // 错误，但 SAP 会话实际已全部打开，用户重试将开出重复会话
    apply_success_accounting(store, &success_ids, now);
    Ok(success_count)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 回归 v4 Must #1：编辑保存不得丢失表单不管理的字段
    #[test]
    fn update_preserves_fields_not_owned_by_form() {
        let existing = Credential {
            script_path: Some("C:\\scripts\\auto.vbs".to_string()),
            uuid: "service-uuid".to_string(),
            favorite_order: Some(3),
            message_server_port: "3600".to_string(),
            created_at: chrono::DateTime::from_timestamp(1_700_000_000, 0).unwrap(),
            ..Default::default()
        };
        let mut form = Credential::default();
        preserve_unmanaged_fields(&mut form, &existing);
        assert_eq!(form.script_path, existing.script_path);
        assert_eq!(form.uuid, existing.uuid);
        assert_eq!(form.favorite_order, existing.favorite_order);
        assert_eq!(form.message_server_port, existing.message_server_port);
        assert_eq!(form.created_at, existing.created_at);
    }

    /// 回归 v4 Must #3：写凭据前必须验主密码，错误密码不得静默加密落盘
    #[test]
    fn credential_writes_reject_wrong_master_password() {
        let mut s = storage::create_empty_store();
        s.master_password_hash =
            crate::crypto::hash_password("correct", &s.salt, s.key_derivation_version).unwrap();
        assert!(ensure_master_password(&s, "correct").is_ok());
        assert!(matches!(
            ensure_master_password(&s, "wrong"),
            Err(CommandError::BadMasterPassword)
        ));
    }

    fn store_with_master(password: &str) -> Mutex<StoreData> {
        let mut s = storage::create_empty_store();
        s.master_password_hash =
            crate::crypto::hash_password(password, &s.salt, s.key_derivation_version).unwrap();
        Mutex::new(s)
    }

    /// v5 Should #12：测**调用点**而不是镜像辅助函数本身。
    /// 若把 add_credential 里的 `ensure_master_password(&s, ...)?` 删掉，
    /// 本用例会走到加密 + 落盘并返回 Ok，测试立刻失败。
    #[test]
    fn add_credential_call_site_rejects_wrong_master_password() {
        let store = store_with_master("correct");
        let cred = Credential {
            connection_id: "PRD".to_string(),
            password: "secret".to_string(),
            ..Default::default()
        };
        assert!(matches!(
            add_credential(&store, "wrong", cred),
            Err(CommandError::BadMasterPassword)
        ));
    }

    /// v5 Should #12：update_credential 的验密调用点也必须被钉住。
    #[test]
    fn update_credential_call_site_rejects_wrong_master_password() {
        let store = store_with_master("correct");
        {
            let mut s = store.lock().unwrap();
            s.credentials.push(Credential {
                id: "cred-1".to_string(),
                connection_id: "PRD".to_string(),
                ..Default::default()
            });
        }
        let form = Credential {
            id: "cred-1".to_string(),
            password: "new-secret".to_string(),
            ..Default::default()
        };
        assert!(matches!(
            update_credential(&store, "wrong", form),
            Err(CommandError::BadMasterPassword)
        ));
    }

    /// v5 Should #12：update_credential 必须真的调用 preserve_unmanaged_fields。
    /// 若删掉该调用行，编辑保存会丢掉表单不管理的 5 个字段，本用例失败。
    #[test]
    fn update_credential_call_site_preserves_unmanaged_fields() {
        let store = store_with_master("correct");
        let existing = Credential {
            id: "cred-1".to_string(),
            connection_id: "PRD".to_string(),
            script_path: Some("C:\\scripts\\auto.vbs".to_string()),
            uuid: "service-uuid".to_string(),
            favorite_order: Some(7),
            message_server_port: "3600".to_string(),
            created_at: chrono::DateTime::from_timestamp(1_700_000_000, 0).unwrap(),
            ..Default::default()
        };
        {
            let mut s = store.lock().unwrap();
            s.credentials.push(existing.clone());
        }
        let form = Credential {
            id: "cred-1".to_string(),
            connection_id: "PRD".to_string(),
            password: "secret".to_string(),
            ..Default::default()
        };
        let updated = update_credential(&store, "correct", form).expect("update should succeed");
        assert_eq!(updated.script_path, existing.script_path);
        assert_eq!(updated.uuid, existing.uuid);
        assert_eq!(updated.favorite_order, existing.favorite_order);
        assert_eq!(updated.message_server_port, existing.message_server_port);
        assert_eq!(updated.created_at, existing.created_at);
    }
}
