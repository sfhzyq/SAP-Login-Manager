use crate::crypto::{decrypt, decrypt_gcm, derive_key};
use crate::models::{Credential, Group, StoreData};
use crate::storage;
use chrono::Utc;
use std::sync::Mutex;
use uuid::Uuid;

pub fn get_credentials(
    store: &Mutex<StoreData>,
) -> Result<Vec<Credential>, String> {
    let s = store.lock().map_err(|e| e.to_string())?;
    let mut creds = s.credentials.clone();
    // 排序：收藏优先 → 登录次数降序 → 最后登录时间降序
    creds.sort_by(|a, b| {
        b.is_favorite.cmp(&a.is_favorite)
            .then_with(|| b.login_count.cmp(&a.login_count))
            .then_with(|| b.last_login_at.cmp(&a.last_login_at))
    });
    Ok(creds)
}

pub fn get_credential(
    store: &Mutex<StoreData>,
    id: String,
) -> Result<Option<Credential>, String> {
    let s = store.lock().map_err(|e| e.to_string())?;
    Ok(s.credentials.iter().find(|c| c.id == id).cloned())
}

pub fn add_credential(
    store: &Mutex<StoreData>,
    master_password: String,
    mut credential: Credential,
) -> Result<Credential, String> {
    let mut s = store.lock().map_err(|e| e.to_string())?;

    credential.id = Uuid::new_v4().to_string();
    credential.encrypted_password = storage::encrypt_credential_password(
        &s, &master_password, &credential.password
    ).map_err(|e| e.to_string())?;
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
            storage::add_credential_to_group(&mut s, &credential.id, gid)
                .map_err(|e| e.to_string())?;
        }
    }

    s.credentials.push(credential.clone());
    storage::save_store(&s).map_err(|e| e.to_string())?;

    Ok(credential)
}

pub fn update_credential(
    store: &Mutex<StoreData>,
    master_password: String,
    mut credential: Credential,
) -> Result<Credential, String> {
    let mut s = store.lock().map_err(|e| e.to_string())?;

    let idx = s.credentials.iter().position(|c| c.id == credential.id)
        .ok_or("凭据不存在")?;

    // 获取旧分组 ID
    let old_group_id = s.credentials[idx].group_id.clone();

    if !credential.password.is_empty() {
        credential.encrypted_password = storage::encrypt_credential_password(
            &s, &master_password, &credential.password
        ).map_err(|e| e.to_string())?;
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

    s.credentials[idx] = credential.clone();

    // 如果分组变更，同步更新分组的 entries 列表
    if old_group_id != new_group_id {
        let cred_id = credential.id.clone();

        // 从所有分组中移除该凭据
        for group in &mut s.groups {
            group.entries.retain(|eid| eid != &cred_id);
        }

        // 添加到新分组
        if let Some(ref gid) = new_group_id {
            if !gid.is_empty() {
                storage::add_credential_to_group(&mut s, &cred_id, gid)
                    .map_err(|e| e.to_string())?;
            }
        }
    }

    storage::save_store(&s).map_err(|e| e.to_string())?;

    Ok(credential)
}

pub fn delete_credential(
    store: &Mutex<StoreData>,
    id: String,
) -> Result<(), String> {
    let mut s = store.lock().map_err(|e| e.to_string())?;
    s.credentials.retain(|c| c.id != id);
    for group in &mut s.groups {
        group.entries.retain(|eid| eid != &id);
    }
    storage::save_store(&s).map_err(|e| format!("保存失败: {}（可能被杀毒软件占用，请稍后重试）", e))
}

/// 撤销删除：原样恢复凭据（保留 id/加密密码/统计/收藏/分组归属）
pub fn restore_credentials(
    store: &Mutex<StoreData>,
    credentials: Vec<Credential>,
) -> Result<usize, String> {
    let mut s = store.lock().map_err(|e| e.to_string())?;
    let mut restored = 0;
    for mut cred in credentials {
        // 已存在同 ID 则跳过，避免重复
        if s.credentials.iter().any(|c| c.id == cred.id) {
            continue;
        }
        cred.password = String::new();
        cred.updated_at = Utc::now();
        // 恢复分组归属（entries）
        if let Some(ref gid) = cred.group_id {
            if !gid.is_empty() {
                let _ = storage::add_credential_to_group(&mut s, &cred.id, gid);
            }
        }
        s.credentials.push(cred);
        restored += 1;
    }
    if restored > 0 {
        storage::save_store(&s).map_err(|e| e.to_string())?;
    }
    Ok(restored)
}

/// 获取解密后的密码（用于复制到剪贴板，前端配合自动清空策略）
pub fn get_decrypted_password(
    store: &Mutex<StoreData>,
    master_password: String,
    credential_id: String,
) -> Result<String, String> {
    let (encrypted, salt, kdv, ev) = {
        let s = store.lock().map_err(|e| e.to_string())?;
        let cred = s.credentials.iter().find(|c| c.id == credential_id)
            .ok_or("凭据不存在")?;
        (cred.encrypted_password.clone(), s.salt.clone(), s.key_derivation_version, s.encryption_version)
    };
    if encrypted.is_empty() {
        return Ok(String::new());
    }
    let key = derive_key(&master_password, &salt, kdv).map_err(|e| e.to_string())?;
    if ev >= 1 {
        decrypt_gcm(&encrypted, &key).map_err(|e| e.to_string())
    } else {
        decrypt(&encrypted, &key).map_err(|e| e.to_string())
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

    // 根据环境类型归入对应系统分组
    match credential.environment.as_str() {
        "production" => Some("group-production".to_string()),
        "test" => Some("group-test".to_string()),
        "development" => Some("group-development".to_string()),
        "configuration" => Some("group-configuration".to_string()),
        _ => storage::get_default_group_id(store),
    }
}

// === Groups ===

pub fn get_groups(
    store: &Mutex<StoreData>,
) -> Result<Vec<Group>, String> {
    let s = store.lock().map_err(|e| e.to_string())?;
    Ok(s.groups.clone())
}

pub fn add_group(
    store: &Mutex<StoreData>,
    mut group: Group,
) -> Result<Group, String> {
    let mut s = store.lock().map_err(|e| e.to_string())?;
    // 用户创建的分组永远不是系统分组或默认分组
    group.id = Uuid::new_v4().to_string();
    group.is_system = false;
    group.is_default = false;
    group.created_at = Utc::now();
    group.updated_at = Utc::now();
    s.groups.push(group.clone());
    storage::save_store(&s).map_err(|e| e.to_string())?;
    Ok(group)
}

pub fn update_group(
    store: &Mutex<StoreData>,
    mut group: Group,
) -> Result<Group, String> {
    let mut s = store.lock().map_err(|e| e.to_string())?;

    let idx = s.groups.iter().position(|g| g.id == group.id)
        .ok_or("组不存在")?;

    // 系统分组和默认分组不可修改名称
    if storage::is_group_protected(&s, &group.id) {
        return Err("系统分组不可修改".to_string());
    }

    group.updated_at = Utc::now();
    s.groups[idx] = group.clone();
    storage::save_store(&s).map_err(|e| e.to_string())?;
    Ok(group)
}

/// 重命名分组
pub fn rename_group(
    store: &Mutex<StoreData>,
    group_id: String,
    new_name: String,
) -> Result<(), String> {
    let mut s = store.lock().map_err(|e| e.to_string())?;

    let idx = s.groups.iter().position(|g| g.id == group_id)
        .ok_or("组不存在")?;

    // 系统分组和默认分组不可修改名称
    if storage::is_group_protected(&s, &group_id) {
        return Err("系统分组不可修改".to_string());
    }

    s.groups[idx].group_name = new_name;
    s.groups[idx].updated_at = Utc::now();
    storage::save_store(&s).map_err(|e| e.to_string())
}

/// 批量移动凭据到指定分组
pub fn batch_move_to_group(
    store: &Mutex<StoreData>,
    credential_ids: Vec<String>,
    group_id: String,
) -> Result<usize, String> {
    let mut s = store.lock().map_err(|e| e.to_string())?;
    let mut moved = 0;

    for id in &credential_ids {
        // 从所有分组中移除该凭据
        for group in &mut s.groups {
            group.entries.retain(|eid| eid != id);
        }
        // 添加到目标分组
        if storage::add_credential_to_group(&mut s, id, &group_id).is_ok() {
            moved += 1;
        }
        // 更新凭据的 group_id
        if let Some(cred) = s.credentials.iter_mut().find(|c| &c.id == id) {
            cred.group_id = Some(group_id.clone());
            cred.updated_at = Utc::now();
        }
    }

    storage::save_store(&s).map_err(|e| e.to_string())?;
    Ok(moved)
}

/// 批量设置收藏（true=收藏，false=取消收藏）
pub fn batch_favorite(
    store: &Mutex<StoreData>,
    credential_ids: Vec<String>,
    favorite: bool,
) -> Result<usize, String> {
    let mut s = store.lock().map_err(|e| e.to_string())?;
    let mut affected = 0;

    // 收藏排序序号：新增收藏时取当前最大值
    let max_order = s.credentials.iter()
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

    storage::save_store(&s).map_err(|e| e.to_string())?;
    Ok(affected)
}

pub fn delete_group(
    store: &Mutex<StoreData>,
    id: String,
) -> Result<(), String> {
    let mut s = store.lock().map_err(|e| e.to_string())?;

    // 系统分组和默认分组不可删除
    if storage::is_group_protected(&s, &id) {
        return Err("系统分组不可删除".to_string());
    }

    // 默认行为：将凭据移入默认分组，避免孤儿数据
    let default_group_id = storage::get_default_group_id(&s)
        .ok_or("默认分组不存在")?;
    for cred in &mut s.credentials {
        if cred.group_id.as_deref() == Some(&id) {
            cred.group_id = Some(default_group_id.clone());
        }
    }
    // 从默认分组的 entries 中无需操作（凭据已通过 group_id 关联）
    s.groups.retain(|g| g.id != id);
    storage::save_store(&s).map_err(|e| e.to_string())
}

/// 删除分组（带选项）
/// mode: "move" = 将凭据移入默认分组后删除分组
///       "delete" = 删除分组内所有凭据后删除分组
pub fn delete_group_with_options(
    store: &Mutex<StoreData>,
    id: String,
    mode: String,
) -> Result<(), String> {
    let mut s = store.lock().map_err(|e| e.to_string())?;

    // 系统分组和默认分组不可删除
    if storage::is_group_protected(&s, &id) {
        return Err("系统分组不可删除".to_string());
    }

    // 获取分组内的凭据 ID 列表
    let entry_ids: Vec<String> = s.groups.iter()
        .find(|g| g.id == id)
        .map(|g| g.entries.clone())
        .unwrap_or_default();

    match mode.as_str() {
        "move" => {
            // 将凭据移入默认分组
            let default_group_id = storage::get_default_group_id(&s)
                .ok_or("默认分组不存在")?;
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
        _ => return Err("无效的删除模式".to_string()),
    }

    // 删除分组本身
    s.groups.retain(|g| g.id != id);
    storage::save_store(&s).map_err(|e| e.to_string())
}

/// 将凭据移动到指定分组
pub fn move_credential_to_group(
    store: &Mutex<StoreData>,
    credential_id: String,
    group_id: String,
) -> Result<(), String> {
    let mut s = store.lock().map_err(|e| e.to_string())?;

    // 从所有分组中移除该凭据
    for group in &mut s.groups {
        group.entries.retain(|eid| eid != &credential_id);
    }

    // 添加到目标分组
    storage::add_credential_to_group(&mut s, &credential_id, &group_id)
        .map_err(|e| e.to_string())?;

    // 更新凭据的 group_id
    if let Some(cred) = s.credentials.iter_mut().find(|c| c.id == credential_id) {
        cred.group_id = Some(group_id);
        cred.updated_at = Utc::now();
    }

    storage::save_store(&s).map_err(|e| e.to_string())
}

// === 收藏 ===

/// 切换凭据收藏状态
pub fn toggle_favorite(
    store: &Mutex<StoreData>,
    credential_id: String,
) -> Result<bool, String> {
    let mut s = store.lock().map_err(|e| e.to_string())?;

    let max_order = s.credentials.iter()
        .filter_map(|c| c.favorite_order)
        .max()
        .unwrap_or(0);

    let cred = s.credentials.iter_mut().find(|c| c.id == credential_id)
        .ok_or("凭据不存在")?;

    cred.is_favorite = !cred.is_favorite;
    if cred.is_favorite {
        // 设置排序序号为当前最大+1
        cred.favorite_order = Some(max_order + 1);
    } else {
        cred.favorite_order = None;
    }
    cred.updated_at = Utc::now();

    let is_fav = cred.is_favorite;
    storage::save_store(&s).map_err(|e| e.to_string())?;
    Ok(is_fav)
}

/// 重排序收藏列表
pub fn reorder_favorites(
    store: &Mutex<StoreData>,
    ordered_ids: Vec<String>,
) -> Result<(), String> {
    let mut s = store.lock().map_err(|e| e.to_string())?;

    for (idx, id) in ordered_ids.iter().enumerate() {
        if let Some(cred) = s.credentials.iter_mut().find(|c| &c.id == id) {
            cred.favorite_order = Some(idx as u32);
            cred.updated_at = Utc::now();
        }
    }

    storage::save_store(&s).map_err(|e| e.to_string())
}

// === 连接分享 ===

/// 分享连接信息（不含凭据）到剪贴板
pub fn share_credential(
    store: &Mutex<StoreData>,
    credential_id: String,
) -> Result<String, String> {
    let s = store.lock().map_err(|e| e.to_string())?;

    let cred = s.credentials.iter().find(|c| c.id == credential_id)
        .ok_or("凭据不存在")?;

    // 生成与 SAP Logon 字段一致的分享文本（不含密码）
    // 系统标识 = SID（3 位，如 PRD）；老数据 system_id 为空时回退 connection_id
    let sid = if cred.system_id.is_empty() { &cred.connection_id } else { &cred.system_id };
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

    let text = lines.join("\n");

    // 复制到剪贴板
    let mut cb = arboard::Clipboard::new().map_err(|e| e.to_string())?;
    cb.set_text(text.clone()).map_err(|e| e.to_string())?;

    Ok(text)
}

// === 批量登录 ===

/// 批量登录多个凭据
/// 不在 sleep 期间持有锁，避免阻塞其他操作
pub fn batch_login(
    store: &Mutex<StoreData>,
    master_password: String,
    credential_ids: Vec<String>,
    interval_secs: u64,
) -> Result<usize, String> {
    // 阶段1：加锁，克隆所需数据，派生密钥（仅一次），释放锁
    let (creds_to_login, key, enc_version) = {
        let s = store.lock().map_err(|e| e.to_string())?;
        let key = derive_key(&master_password, &s.salt, s.key_derivation_version)
            .map_err(|e| e.to_string())?;
        let enc_version = s.encryption_version;
        let creds: Vec<Credential> = credential_ids.iter()
            .filter_map(|id| s.credentials.iter().find(|c| &c.id == id).cloned())
            .filter(|c| !c.username.is_empty())
            .collect();
        (creds, key, enc_version)
    };

    let mut success_count = 0;
    let now = Utc::now();
    let mut success_ids: Vec<String> = Vec::new();

    // 阶段2：不加锁，执行 SAP 登录 + sleep
    for (idx, cred) in creds_to_login.iter().enumerate() {
        if idx > 0 && interval_secs > 0 {
            std::thread::sleep(std::time::Duration::from_secs(interval_secs));
        }

        let password = if enc_version >= 1 {
            decrypt_gcm(&cred.encrypted_password, &key)
                .map_err(|e| e.to_string())?
        } else {
            decrypt(&cred.encrypted_password, &key)
                .map_err(|e| e.to_string())?
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
        };

        match crate::sap_automation::login_to_sap(&params) {
            Ok(()) => {
                success_ids.push(cred.id.clone());
                success_count += 1;
            }
            Err(e) => {
                // 保存已成功的登录计数后再返回错误
                if !success_ids.is_empty() {
                    let mut s = store.lock().map_err(|e| e.to_string())?;
                    for sid in &success_ids {
                        if let Some(c) = s.credentials.iter_mut().find(|c| &c.id == sid) {
                            c.login_count += 1;
                            c.last_login_at = Some(now);
                        }
                    }
                    let _ = storage::save_store(&s);
                }
                return Err(format!("登录 {} 失败: {}", cred.connection_id, e));
            }
        }
    }

    // 阶段3：重新加锁，更新登录次数，保存
    let mut s = store.lock().map_err(|e| e.to_string())?;
    for sid in &success_ids {
        if let Some(c) = s.credentials.iter_mut().find(|c| &c.id == sid) {
            c.login_count += 1;
            c.last_login_at = Some(now);
        }
    }
    storage::save_store(&s).map_err(|e| e.to_string())?;
    Ok(success_count)
}
