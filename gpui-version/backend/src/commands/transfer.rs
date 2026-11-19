use crate::bat_parser::{self, BatDraft};
use crate::commands::{CmdResult, CommandError};
use crate::crypto;
use crate::models::{Credential, ExportBundle, Group, StoreData};
use crate::storage;
use chrono::Utc;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;
use uuid::Uuid;
use zeroize::{Zeroize, Zeroizing};

/// 导出凭据到指定文件路径（需要主密码验证，明文密码仅写入文件，不返回前端）
pub fn export_credentials(
    store: &Mutex<StoreData>,
    master_password: &str,
    save_path: String,
) -> CmdResult<()> {
    // 锁内只做验证 + 快照：JSON 序列化与写盘都在锁外，避免大文件导出
    // 期间阻塞其他命令（此前 MutexGuard 跨越 to_string_pretty + fs::write）
    let (mut credentials, groups, salt, kdv, enc_ver) = {
        let s = store
            .lock()
            .map_err(|e| CommandError::Other(e.to_string()))?;
        verify_or_bad_master(&s, master_password)?;
        (
            s.credentials.clone(),
            s.groups.clone(),
            s.salt.clone(),
            s.key_derivation_version,
            s.encryption_version,
        )
    };

    // 密钥一次派生（此前每条凭据各派生一次），解密在锁外进行；
    // 任一条失败即中止，绝不写出不完整备份（v4 Must #2）
    let key = Zeroizing::new(crypto::derive_key(master_password, &salt, kdv)?);
    for cred in &mut credentials {
        cred.password =
            decrypt_export_password(&cred.encrypted_password, &key, enc_ver, &cred.connection_id)?;
        // 导出后清除加密密码（导入时会重新加密）
        cred.encrypted_password.zeroize();
    }

    // v5 Consider #27：整包明文用 Zeroizing 持有，`?` 早退路径同样自动清零
    let mut bundle = Zeroizing::new(ExportBundle {
        app_version: env!("CARGO_PKG_VERSION").to_string(),
        exported_at: Utc::now(),
        group_count: groups.len(),
        connection_count: credentials.len(),
        has_landscape: false,
        source_salt: salt,
        credentials,
        groups,
    });

    // v4 Should #15：导出内容含全部明文密码，序列化副本与 bundle 都要清零，
    // 不能只让 key 是 Zeroizing 而把整包明文留在堆上
    let content = Zeroizing::new(serde_json::to_string_pretty(&*bundle)?);
    fs::write(&save_path, content.as_bytes())?;
    for cred in &mut bundle.credentials {
        cred.password.zeroize();
    }

    Ok(())
}

/// 导出前解密单条密码。任何一条失败都必须中止整批导出，
/// 否则会写出「缺密码字段」的备份，而 UI 仍报导出成功（v4 Must #2）。
fn decrypt_export_password(
    encrypted: &str,
    key: &[u8; 32],
    enc_ver: u32,
    connection_id: &str,
) -> CmdResult<String> {
    if encrypted.is_empty() {
        return Ok(String::new());
    }
    let plain = if enc_ver >= 1 {
        crypto::decrypt_gcm(encrypted, key)
    } else {
        crypto::decrypt(encrypted, key)
    };
    plain.map_err(|e| {
        CommandError::Other(format!(
            "导出中止：凭据 {connection_id} 的密码解密失败，未写出不完整备份: {e}"
        ))
    })
}

/// 导入条目必须自带可用密码。外来密文由源库密钥加密，目标库解不开；
/// 双空条目只可能是损坏的备份（合法的无密码连接会启用 SNC SSO）。
fn ensure_import_password(cred: &Credential) -> CmdResult<()> {
    if cred.password.is_empty() && !cred.snc_sso {
        return Err(CommandError::InvalidInput(format!(
            "备份中凭据 {} 缺少密码，拒绝导入",
            cred.connection_id
        )));
    }
    Ok(())
}

/// 验证主密码，错误时返回 `BadMasterPassword`；
/// 存储尚未设置主密码时映射为专用变体 `MasterPasswordNotSet`
/// （否则会被 `From<StorageError>` 兜进 `Storage(..)`，结构化匹配失效）
fn verify_or_bad_master(s: &StoreData, master_password: &str) -> CmdResult<()> {
    match storage::verify_master_password(s, master_password) {
        Ok(true) => Ok(()),
        Ok(false) => Err(CommandError::BadMasterPassword),
        Err(storage::StorageError::MasterPasswordNotSet) => Err(CommandError::MasterPasswordNotSet),
        Err(e) => Err(e.into()),
    }
}

/// 从导出包导入凭据（跳过已存在的 connection_id）。
///
/// 同时导入 bundle 内的分组（目标库不存在的才插入），并对凭据的
/// `group_id` 做存在性校验回退，避免导入后凭据在分组筛选中不可见。
pub fn import_credentials(
    store: &Mutex<StoreData>,
    master_password: &str,
    mut bundle: Zeroizing<ExportBundle>,
) -> CmdResult<usize> {
    let mut guard = store
        .lock()
        .map_err(|e| CommandError::Other(e.to_string()))?;
    // 工作副本：持久化成功后才提交，避免内存/磁盘分叉
    let mut s = guard.clone();

    // 先验证主密码：导入会用该密码重新加密，密码错误会导致新凭据永远无法解密
    verify_or_bad_master(&s, master_password)?;
    // v4 Should #6：密钥一次派生，避免每条凭据各跑一次 Argon2id（19MiB,t=2）
    let key = Zeroizing::new(crypto::derive_key(
        master_password,
        &s.salt,
        s.key_derivation_version,
    )?);

    // v5 Consider #27：bundle 是 Zeroizing<ExportBundle>，字段要用 take 取出，
    // 取出后的每条凭据仍用 Zeroizing 包裹（`?` 早退 / continue 时自动清零）。
    let groups = std::mem::take(&mut bundle.groups);
    let credentials = std::mem::take(&mut bundle.credentials);

    // 导入 bundle 内的分组：同 id 或同名自建分组复用其 id，
    // 并用 remap 在凭据回退时保持分组归属（只按源 id 判存在会拉平分组）
    let (group_remap, groups_added) = merge_import_groups(&mut s.groups, groups);

    let mut imported = 0;
    for cred in credentials {
        let mut cred = Zeroizing::new(cred);
        // 跳过已存在的同名连接
        let exists = s
            .credentials
            .iter()
            .any(|c| c.connection_id == cred.connection_id);
        if exists {
            continue; // Zeroizing<Credential> 在此析构清零
        }

        // v4 Must #2：损坏备份（缺密码字段）不得静默入库后报「已导入 N 条」
        ensure_import_password(&cred)?;
        // 外来密文由源库密钥加密，目标库主密码解不开，统一丢弃
        cred.encrypted_password = String::new();
        if !cred.password.is_empty() {
            cred.encrypted_password = if s.encryption_version >= 1 {
                crypto::encrypt_gcm(&cred.password, &key)?
            } else {
                crypto::encrypt_legacy_cbc(&cred.password, &key)?
            };
            cred.password.zeroize();
        }
        cred.created_at = Utc::now();
        cred.updated_at = Utc::now();
        // 指定的分组在目标库不存在时回退默认分组，
        // 避免悬挂 group_id 使凭据在 UI 分组筛选中不可见；
        // 源库同名分组已复用目标库 id 时，经 remap 还原归属
        cred.group_id = match cred.group_id.as_deref() {
            Some(gid) if !gid.is_empty() => {
                let mapped = group_remap.get(gid).map(String::as_str).unwrap_or(gid);
                if s.groups.iter().any(|g| g.id == mapped) {
                    Some(mapped.to_string())
                } else {
                    storage::get_default_group_id(&s)
                }
            }
            _ => storage::get_default_group_id(&s),
        };
        // v4 Should #5：备份可能携带与目标库相同的 id（改过连接名后从旧备份恢复），
        // 只按 connection_id 查重会产生重复主键
        if s.credentials.iter().any(|c| c.id == cred.id) {
            cred.id = Uuid::new_v4().to_string();
        }
        // 取出内层 Credential 存入目标库（外层 Zeroizing 壳随后析构，
        // 此时壳里只剩 Default 值，密码字段已被 zeroize 清空）
        s.credentials.push(std::mem::take(&mut *cred));
        imported += 1;
    }

    // 凭据全已存在但分组有新增时也必须提交，否则新分组静默丢失
    if imported > 0 || groups_added > 0 {
        storage::save_store(&s)?;
        *guard = s; // 持久化成功，提交内存
    }
    Ok(imported)
}

/// BAT 批量导入结果统计
#[derive(Debug, Clone, Default)]
pub struct BatImportResult {
    /// 新增条数
    pub added: usize,
    /// 覆盖更新条数
    pub updated: usize,
    /// 跳过条数（已存在且不覆盖 / 批内重复 / 无效行）
    pub skipped: usize,
}

/// 从 sapshcut 命令草稿批量导入凭据
///
/// 查重键：system_id（忽略大小写）+ client + username。
/// `overwrite=false` 时已存在则跳过；`overwrite=true` 时用 bat 中的非空字段覆盖更新。
/// 明文密码用当前主密码重新加密入库；分组使用用户指定的 `group_id`（None = 不分组）。
pub fn import_bat_credentials(
    store: &Mutex<StoreData>,
    master_password: &str,
    drafts: Vec<BatDraft>,
    overwrite: bool,
    group_id: Option<String>,
) -> CmdResult<BatImportResult> {
    let mut guard = store
        .lock()
        .map_err(|e| CommandError::Other(e.to_string()))?;

    // 先验证主密码：导入会用该密码重新加密，密码错误会导致新凭据永远无法解密
    verify_or_bad_master(&guard, master_password)?;
    // v4 Should #6：同样一次派生，避免批内每条草稿各跑一次 Argon2
    // 命名避开循环内批内查重的 key 变量，防止遮蔽
    let import_key = Zeroizing::new(crypto::derive_key(
        master_password,
        &guard.salt,
        guard.key_derivation_version,
    )?);

    let mut s = guard.clone(); // 工作副本：持久化成功后才提交，避免内存/磁盘分叉
    let mut result = BatImportResult::default();
    // 批内查重（同一 bat 里同一账号出现多次只导入第一条）
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();

    for d in drafts.into_iter() {
        // 无效行计入 skipped（与文档注释「跳过条数含无效行」一致）
        if !d.is_valid() {
            result.skipped += 1;
            continue;
        }
        let key = format!("{}|{}|{}", d.system_id.to_uppercase(), d.client, d.username);
        if !seen.insert(key) {
            result.skipped += 1;
            continue;
        }

        // 密码加密要在可变借用前完成（encrypt 需要 &StoreData）
        let enc = if d.password.is_empty() {
            None
        } else if s.encryption_version >= 1 {
            Some(crypto::encrypt_gcm(&d.password, &import_key)?)
        } else {
            Some(crypto::encrypt_legacy_cbc(&d.password, &import_key)?)
        };

        let idx = s.credentials.iter().position(|c| {
            c.system_id.eq_ignore_ascii_case(&d.system_id)
                && c.client == d.client
                && c.username == d.username
        });

        if let Some(idx) = idx {
            if !overwrite {
                result.skipped += 1;
                continue;
            }
            // bat 指定了连接名（-sysname）时覆盖自动生成的连接名——仍需
            // 唯一性保证：原样赋值会在同名 bat 或与现有凭据撞名时产生
            // 重复 connection_id，破坏全库查重键（可变借用前先计算）
            // 可变借用前先计算；排除被覆盖的凭据自身，否则重导入同一份
            // bat 时连接名会逐次漂移 X → X-2 → X-3
            let new_conn = if d.connection_name.is_empty() {
                None
            } else {
                Some(unique_connection_id_base_excluding(
                    &s,
                    &d.connection_name,
                    Some(idx),
                ))
            };
            // 覆盖更新：仅用 bat 中的非空字段覆盖，其余保留现值
            let cred = &mut s.credentials[idx];
            cred.system_id = d.system_id.clone();
            if !d.client.is_empty() {
                cred.client = d.client.clone();
            }
            if !d.language.is_empty() {
                cred.language = d.language.clone();
            }
            if !d.connection_type.is_empty() {
                cred.connection_type = d.connection_type.clone();
            }
            if !d.app_server.is_empty() {
                cred.app_server = d.app_server.clone();
            }
            if !d.system_number.is_empty() {
                cred.system_number = d.system_number.clone();
            }
            if !d.message_server.is_empty() {
                cred.message_server = d.message_server.clone();
            }
            if !d.message_server_port.is_empty() {
                cred.message_server_port = d.message_server_port.clone();
            }
            if !d.logon_group.is_empty() {
                cred.logon_group = d.logon_group.clone();
            }
            if !d.saprouter.is_empty() {
                cred.saprouter = d.saprouter.clone();
            }
            // 与其它字段一致的「非空才覆盖」语义：bat 未指定时不清空现有值
            if !d.post_login_action.is_empty() {
                cred.post_login_action = Some(d.post_login_action.clone());
            }
            if !d.display_name.is_empty() {
                cred.display_name = Some(d.display_name.clone());
            }
            // bat 指定了连接名时才覆盖（new_conn 已含唯一性保证）
            if let Some(n) = new_conn {
                cred.connection_id = n;
            }
            if let Some(enc) = enc {
                cred.encrypted_password = enc;
            }
            cred.updated_at = Utc::now();
            result.updated += 1;
        } else {
            // 新增
            // 连接名：优先 bat 的 -sysname（重复时自动加后缀），无则按 SID-客户端-用户名生成
            let connection_id = if d.connection_name.is_empty() {
                unique_connection_id(&s, &d)
            } else {
                unique_connection_id_base(&s, &d.connection_name)
            };
            // 显示名称：-name 显式指定优先，否则用来源 bat 文件名（不含扩展名）
            let display_name = if !d.display_name.is_empty() {
                Some(d.display_name.clone())
            } else if !d.source.is_empty() {
                std::path::Path::new(&d.source)
                    .file_stem()
                    .map(|s| s.to_string_lossy().into_owned())
                    .filter(|s| !s.is_empty())
            } else {
                None
            };
            let cred = Credential {
                id: Uuid::new_v4().to_string(),
                connection_id,
                system_id: d.system_id.clone(),
                client: d.client.clone(),
                username: d.username.clone(),
                language: d.language.clone(),
                connection_type: d.connection_type.clone(),
                app_server: d.app_server.clone(),
                system_number: d.system_number.clone(),
                message_server: d.message_server.clone(),
                message_server_port: d.message_server_port.clone(),
                logon_group: d.logon_group.clone(),
                saprouter: d.saprouter.clone(),
                post_login_action: if d.post_login_action.is_empty() {
                    None
                } else {
                    Some(d.post_login_action.clone())
                },
                display_name,
                encrypted_password: enc.unwrap_or_default(),
                group_id: group_id.clone(),
                created_at: Utc::now(),
                updated_at: Utc::now(),
                ..Default::default()
            };

            // entries 仅为兼容冗余轨，失败不阻断（归属以 group_id 为事实源）
            if let Some(ref gid) = group_id {
                if !gid.is_empty() {
                    let _ = storage::add_credential_to_group(&mut s, &cred.id, gid);
                }
            }

            s.credentials.push(cred);
            result.added += 1;
        }
    }

    if result.added + result.updated > 0 {
        storage::save_store(&s)?;
        *guard = s; // 持久化成功，提交内存
    }
    Ok(result)
}

/// 把导出包的分组并入目标库。同 id 或同名自建分组不重复创建，
/// 而是记录「源 id → 目标 id」供凭据归属还原；返回 remap 与新增分组数。
fn merge_import_groups(
    target: &mut Vec<Group>,
    groups: Vec<Group>,
) -> (HashMap<String, String>, usize) {
    let mut remap = HashMap::new();
    let mut added = 0;
    // v4 Consider #23：同名判定只查目标库原有分组，不能命中本轮刚 push 的
    // bundle 内其它分组，否则源库里两个同名自建分组会被拉平成一个
    let base_len = target.len();
    for group in groups {
        let existing_id = if target.iter().any(|g| g.id == group.id) {
            Some(group.id.clone())
        } else if !group.is_system && !group.is_default {
            target[..base_len]
                .iter()
                .find(|g| g.group_name == group.group_name && !g.is_system && !g.is_default)
                .map(|g| g.id.clone())
        } else {
            None
        };
        match existing_id {
            Some(id) => {
                remap.insert(group.id.clone(), id);
            }
            None => {
                remap.insert(group.id.clone(), group.id.clone());
                target.push(group);
                added += 1;
            }
        }
    }
    (remap, added)
}

/// 生成不与现有凭据冲突的 connection_id（指定基础名）
fn unique_connection_id_base(s: &StoreData, base: &str) -> String {
    unique_connection_id_base_excluding(s, base, None)
}

/// `skip` 指定一个不计入撞名的索引（覆盖更新时是被覆盖的凭据自身）。
fn unique_connection_id_base_excluding(s: &StoreData, base: &str, skip: Option<usize>) -> String {
    let mut candidate = base.to_string();
    let mut n = 2;
    while s
        .credentials
        .iter()
        .enumerate()
        .any(|(i, c)| Some(i) != skip && c.connection_id == candidate)
    {
        candidate = format!("{base}-{n}");
        n += 1;
    }
    candidate
}

/// 生成不与现有凭据冲突的 connection_id（默认格式 SID-客户端-用户名）
fn unique_connection_id(s: &StoreData, d: &BatDraft) -> String {
    unique_connection_id_base(s, &format!("{}-{}-{}", d.system_id, d.client, d.username))
}

/// 将全部凭据导出为多个 sapshcut BAT 脚本（每条凭据一个 .bat 文件）
///
/// 需要主密码验证后解密密码；明文密码会写入 .bat 文件，调用方需提示用户。
/// 返回导出的文件个数。
pub fn export_bat_files(
    store: &Mutex<StoreData>,
    master_password: &str,
    dir: String,
) -> CmdResult<usize> {
    // 锁内只做验证 + 快照：解密与逐文件写盘都在锁外
    let (credentials, salt, kdv, enc_ver) = {
        let s = store
            .lock()
            .map_err(|e| CommandError::Other(e.to_string()))?;
        verify_or_bad_master(&s, master_password)?;
        if s.credentials.is_empty() {
            return Err(CommandError::Other("没有可导出的凭据".to_string()));
        }
        (
            s.credentials.clone(),
            s.salt.clone(),
            s.key_derivation_version,
            s.encryption_version,
        )
    };

    let dir_path = PathBuf::from(&dir);
    fs::create_dir_all(&dir_path)?;

    // 密钥一次派生，解密在锁外进行
    let key = Zeroizing::new(crypto::derive_key(master_password, &salt, kdv)?);
    // 先解密全部再写盘：任一条失败都中止整批，不产出不带 -pw 的半成品
    let passwords: Vec<String> = credentials
        .iter()
        .map(|cred| {
            decrypt_export_password(&cred.encrypted_password, &key, enc_ver, &cred.connection_id)
        })
        .collect::<CmdResult<Vec<_>>>()?;
    let mut count = 0usize;
    let mut used_names: HashSet<String> = HashSet::new();
    for (cred, password) in credentials.iter().zip(&passwords) {
        let line = bat_parser::credential_to_sapshcut(cred, password);
        let content = format!(
            "@echo off\r\nrem 由 SAP Login Manager 导出（包含明文密码，请妥善保管）\r\n{line}\r\n"
        );

        // 文件名去重（同 display_name 或同名账号会撞名）
        let mut name = bat_parser::bat_file_name(cred);
        let base = name.trim_end_matches(".bat").to_string();
        let mut n = 2;
        while !used_names.insert(name.clone()) {
            name = format!("{base}-{n}.bat");
            n += 1;
        }

        // Windows 下按 GBK 编码写入（cmd 按本地代码页解析 bat，UTF-8 中文会乱码）。
        // v5 Should #7：不可编码字符必须带 connection_id 中止整批，不能静默 best-fit。
        let bytes = bat_parser::encode_bat_bytes(&content).map_err(|e| {
            CommandError::External(format!("导出 {} ({name}) 失败：{e}", cred.connection_id))
        })?;
        fs::write(dir_path.join(&name), bytes)?;
        count += 1;
    }

    Ok(count)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage;

    fn group(id: &str, name: &str) -> Group {
        Group {
            id: id.to_string(),
            group_name: name.to_string(),
            entries: Vec::new(),
            is_system: false,
            is_default: false,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    /// 回归：导出包必须保留明文密码，否则「备份 → 恢复」静默丢失全部密码。
    /// Credential.password 不能用无条件 skip_serializing（ExportBundle 复用该类型）。
    #[test]
    fn export_bundle_keeps_plaintext_password() {
        let cred = Credential {
            connection_id: "PRD-100-TEST".to_string(),
            password: "REDACTED".to_string(),
            ..Default::default()
        };
        let bundle = ExportBundle {
            app_version: "test".to_string(),
            exported_at: Utc::now(),
            group_count: 0,
            connection_count: 1,
            has_landscape: false,
            source_salt: String::new(),
            credentials: vec![cred],
            groups: Vec::new(),
        };
        let json = serde_json::to_string(&bundle).unwrap();
        assert!(
            json.contains("\"password\":\"Secret1\""),
            "导出丢失明文密码: {json}"
        );
        // 空密码仍跳过：store.json 不残留明文字段
        let empty = serde_json::to_string(&Credential::default()).unwrap();
        assert!(!empty.contains("\"password\""), "{empty}");
    }

    /// 回归：覆盖更新时排除自身，重导入同一 bat 不再让连接名逐次漂移
    #[test]
    fn unique_connection_id_excludes_overwritten_self() {
        let mut s = storage::create_empty_store();
        s.credentials.push(Credential {
            connection_id: "X".to_string(),
            ..Default::default()
        });
        s.credentials.push(Credential {
            connection_id: "X-2".to_string(),
            ..Default::default()
        });
        assert_eq!(unique_connection_id_base_excluding(&s, "X", Some(0)), "X");
        assert_eq!(unique_connection_id_base(&s, "X"), "X-3");
    }

    /// 回归：同名自建分组复用目标 id（凭据不落默认分组）+ 新增分组计数
    #[test]
    fn merge_import_groups_reuses_same_named_and_counts_added() {
        let mut target = vec![group("local-id", "我的分组"), group("other-id", "另一个")];
        let incoming = vec![group("src-id", "我的分组"), group("src-new", "新分组")];
        let (remap, added) = merge_import_groups(&mut target, incoming);
        assert_eq!(added, 1);
        assert_eq!(remap["src-id"], "local-id");
        assert_eq!(remap["src-new"], "src-new");
        assert_eq!(target.len(), 3);
        // 再次导入同一分组：幂等，不新增也不重复
        let (remap2, added2) = merge_import_groups(&mut target, vec![group("src-id", "我的分组")]);
        assert_eq!(added2, 0);
        assert_eq!(remap2["src-id"], "local-id");
        assert_eq!(target.len(), 3);
    }

    /// 回归 v4 Consider #23：bundle 自身 id 不同但同名的分组不应被合并
    #[test]
    fn merge_import_groups_keeps_duplicate_names_within_bundle() {
        let mut target = Vec::new();
        let incoming = vec![group("a", "同名"), group("b", "同名")];
        let (remap, added) = merge_import_groups(&mut target, incoming);
        assert_eq!(added, 2);
        assert_eq!(remap["a"], "a");
        assert_eq!(remap["b"], "b");
        assert_eq!(target.len(), 2);
    }

    /// 回归 v4 Must #2：解密失败必须中止整批导出，不得写出缺密码的备份
    #[test]
    fn export_aborts_when_password_cannot_be_decrypted() {
        let key = [0u8; 32];
        let err = decrypt_export_password("not-valid-ciphertext", &key, 1, "PRD-100")
            .expect_err("损坏密文必须中止导出");
        assert!(err.to_string().contains("导出中止"), "{err}");
        // 空密文是合法的无密码凭据，不应报错
        assert_eq!(decrypt_export_password("", &key, 1, "PRD-100").unwrap(), "");
    }

    /// 回归 v4 Must #2：损坏备份（无密码字段）不得静默入库
    #[test]
    fn import_rejects_bundle_without_password() {
        let cred = Credential {
            connection_id: "PRD-100".to_string(),
            ..Default::default()
        };
        assert!(ensure_import_password(&cred).is_err());
        // SNC SSO 免密连接本来就没有密码，应放行
        let sso = Credential {
            snc_sso: true,
            ..cred
        };
        assert!(ensure_import_password(&sso).is_ok());
    }
}
