use crate::commands::{CmdResult, CommandError};
use crate::models::{Credential, SapConnection, StoreData};
use crate::sap_automation::{self, SapLoginParams};
use crate::sap_parser;
use crate::storage;
use chrono::Utc;
use std::sync::Mutex;
use uuid::Uuid;

/// 锁中毒（持锁线程 panic 后）转为命令错误的统一写法
fn poisoned(e: impl std::fmt::Display) -> CommandError {
    CommandError::Other(format!("存储锁不可用: {e}"))
}

/// 直接打开 SAP Logon（不自动登录）
pub fn open_sap_logon(store: &Mutex<StoreData>) -> CmdResult<()> {
    // 只在读取路径时持锁，启动外部进程不持锁
    let custom_path = {
        let s = store.lock().map_err(poisoned)?;
        s.settings.sap_logon_path.clone()
    };
    sap_automation::open_sap_logon(custom_path.as_deref())
        .map_err(|e| CommandError::External(e.to_string()))
}

/// 登录核心：解密参数 → 执行 sapshcut 登录 →（可选）挂载脚本 → 计数落盘。
///
/// 主界面登录（执行挂载脚本）与脚本库「登录并执行」（不执行挂载脚本，
/// 由调用方在登录后定向执行所选脚本）共用本函数。
fn login_core(
    store: &Mutex<StoreData>,
    master_password: &str,
    credential_id: String,
    run_attached_script: bool,
) -> CmdResult<()> {
    // 阶段1：持锁取数据并解密后立即释放——SAP 自动化耗时可达秒级~分钟级，
    // 全程持锁会阻塞所有其他命令（与 batch_login 同姿势）
    let params = {
        let s = store.lock().map_err(poisoned)?;

        let cred = s
            .credentials
            .iter()
            .find(|c| c.id == credential_id)
            .ok_or_else(|| CommandError::NotFound {
                entity: "凭据",
                id: credential_id.clone(),
            })?;

        // SNC SSO 模式下无需密码，跳过解密避免空密码解密报错
        let password = if cred.snc_enabled && cred.snc_sso {
            String::new()
        } else {
            storage::decrypt_credential_password(&s, master_password, &cred.encrypted_password)
                .map_err(CommandError::from)?
        };

        SapLoginParams {
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
            custom_sap_path: s.settings.sap_logon_path.clone(),
        }
    };

    // 读取挂载脚本路径 + 凭据显示名（持锁，随后立即释放，避免阻塞登录自动化）
    let (attached_script, attached_cred_label) = if run_attached_script {
        let s = store.lock().map_err(poisoned)?;
        let cred = s.credentials.iter().find(|c| c.id == credential_id);
        let label = cred
            .map(|c| {
                c.display_name
                    .clone()
                    .filter(|n| !n.is_empty())
                    .unwrap_or_else(|| c.connection_id.clone())
            })
            .unwrap_or_default();
        (cred.and_then(|c| c.script_path.clone()), label)
    } else {
        (None, String::new())
    };

    // 阶段2：不加锁执行 SAP 登录自动化
    sap_automation::login_to_sap(&params).map_err(|e| CommandError::External(e.to_string()))?;

    // 登录后：异步执行挂载脚本（尽力而为，失败仅记日志，不阻塞也不失败登录）
    if let Some(script_path) = attached_script {
        let script_name = std::path::Path::new(&script_path)
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        std::thread::spawn(move || {
            // 执行日志：逐动作落 <exe_dir>/data/logs/script-YYYYMMDD.log
            let log = crate::sap_scripting::RunLog::begin(
                "登录挂载",
                &attached_cred_label,
                &script_name,
                &script_path,
            );
            log.note("附着 SAP GUI 首会话 attach(0,0)（登录挂载固定行为）");
            // 先等界面加载完成（默认 3 秒，可由脚本 default_delay_ms 控制）
            std::thread::sleep(std::time::Duration::from_secs(3));
            let result = crate::sap_scripting::run_script_file(
                std::path::Path::new(&script_path),
                Some(&log),
            );
            log.finish(&result);
            match result {
                Ok(()) => log::info!("登录后脚本执行成功: {}", script_path),
                Err(e) => log::error!("登录后脚本执行失败 ({}): {}", script_path, e),
            }
        });
    }

    // 阶段3：重新加锁，自增登录次数并更新最后登录时间。
    // 记账属尽力而为：SAP 会话已真实打开，落盘失败不应把已成功的登录
    // 报成错误（否则用户重试会开出重复会话）；内存优先提交，落盘失败
    // 留日志（与 batch_login 的 apply_success_accounting 同一哲学）
    // v5 Should #13：SAP 会话已真实打开，毒锁时不得用 ? 把成功登录报成
    // 错误（用户重试会开出重复会话）。与 batch_login 的
    // apply_success_accounting 同一哲学：记账尽力而为，不计账也不报失败。
    let mut guard = match store.lock() {
        Ok(g) => g,
        Err(e) => {
            log::warn!("登录记账跳过（存储锁不可用）: {e}");
            return Ok(());
        }
    };
    if let Some(cred) = guard.credentials.iter_mut().find(|c| c.id == credential_id) {
        cred.login_count += 1;
        cred.last_login_at = Some(Utc::now());
    }
    if let Err(e) = storage::save_store(&guard) {
        log::warn!("登录记账落盘失败（内存已更新）: {e}");
    }

    Ok(())
}

pub fn login_to_sap(
    store: &Mutex<StoreData>,
    master_password: &str,
    credential_id: String,
) -> CmdResult<()> {
    login_core(store, master_password, credential_id, true)
}

/// 仅登录，不执行凭据上挂载的自动脚本（脚本库「登录并执行」专用：
/// 避免用户点了脚本 B，却把凭据上挂的脚本 A 也跑一遍）。
pub fn login_to_sap_no_script(
    store: &Mutex<StoreData>,
    master_password: &str,
    credential_id: String,
) -> CmdResult<()> {
    login_core(store, master_password, credential_id, false)
}

/// 脚本库「执行」结果：是否复用了已登录会话（UI 据此提示）。
#[derive(Debug)]
pub struct ScriptRunResult {
    pub used_existing_session: bool,
}

/// 凭据与会话是否匹配：system / client 已知字段须一致，
/// 两者都拿不到匹配依据则视为不匹配（避免乱附着到无关会话）。
fn session_matches(system_id: &str, client: &str, s: &crate::sap_scripting::SessionRef) -> bool {
    let sys_known = !system_id.is_empty() && !s.system.is_empty();
    let cli_known = !client.is_empty() && !s.client.is_empty();
    if !sys_known && !cli_known {
        return false;
    }
    let sys_ok = !sys_known || system_id.eq_ignore_ascii_case(&s.system);
    let cli_ok = !cli_known || client.trim().eq_ignore_ascii_case(s.client.trim());
    sys_ok && cli_ok
}

/// 脚本库会话感知执行：
/// 1) 已有匹配 (system_id+client) 的登录会话 → 直接在该会话执行；
/// 2) 未检测到 → 自动登录（不执行挂载脚本）→ 轮询等新会话出现 → 定向执行；
/// 3) 轮询超时兜底：附着首会话执行（与旧行为一致）。
pub fn run_script_for_credential(
    store: &Mutex<StoreData>,
    master_password: &str,
    credential_id: String,
    script_path: String,
) -> CmdResult<ScriptRunResult> {
    let (system_id, client, cred_label) = {
        let s = store.lock().map_err(poisoned)?;
        let cred = s
            .credentials
            .iter()
            .find(|c| c.id == credential_id)
            .ok_or_else(|| CommandError::NotFound {
                entity: "凭据",
                id: credential_id.clone(),
            })?;
        let label = cred
            .display_name
            .clone()
            .filter(|n| !n.is_empty())
            .unwrap_or_else(|| cred.connection_id.clone());
        (cred.system_id.clone(), cred.client.clone(), label)
    };
    let script_name = std::path::Path::new(&script_path)
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    // 执行日志：逐动作落 <exe_dir>/data/logs/script-YYYYMMDD.log
    let log =
        crate::sap_scripting::RunLog::begin("脚本库执行", &cred_label, &script_name, &script_path);

    // 1) 已登录 → 复用会话。
    // v5 Consider #20：COM 枚举失败不等于「无会话」。旧代码 `if let Ok(..)`
    // 无 else、连日志都没有，随后自动重新登录，用户明明已有会话却可能开出
    // 重复会话且排障看不到原因。现在写入 RunLog 并返回带原因的 Err，由 UI
    // 明确告知，让用户决定是否重试。
    let sessions = match crate::sap_scripting::list_open_sessions() {
        Ok(s) => s,
        Err(e) => {
            let msg = format!(
                "枚举 SAP GUI 已登录会话失败（COM）：{e}；无法判断是否已有会话，已中止以免开出重复会话"
            );
            log::warn!("{msg}");
            log.note(&msg);
            log.finish(&Err(crate::sap_scripting::ScriptError::Com(msg.clone())));
            return Err(CommandError::External(msg));
        }
    };
    if let Some((c, s)) = sessions
        .iter()
        .find(|x| session_matches(&system_id, &client, x))
        .map(|x| (x.connection, x.session))
    {
        log.note(&format!(
            "复用已登录会话 connection#{c} session#{s}（按 system/client 匹配）"
        ));
        let result = crate::sap_scripting::run_script_file_in_session(
            std::path::Path::new(&script_path),
            c,
            s,
            Some(&log),
        );
        log.finish(&result);
        result.map_err(|e| CommandError::External(e.to_string()))?;
        return Ok(ScriptRunResult {
            used_existing_session: true,
        });
    }

    // 2) 未检测到 → 登录后轮询匹配会话（GUI 启动需数秒）
    log.note("未检测到匹配会话，自动登录（不跑挂载脚本）…");
    if let Err(e) = login_to_sap_no_script(store, master_password, credential_id) {
        log.finish(&Err(crate::sap_scripting::ScriptError::Com(e.to_string())));
        return Err(e);
    }
    let mut target: Option<(usize, usize)> = None;
    for _ in 0..60 {
        std::thread::sleep(std::time::Duration::from_millis(500));
        if let Ok(sessions) = crate::sap_scripting::list_open_sessions() {
            if let Some(f) = sessions
                .iter()
                .find(|x| session_matches(&system_id, &client, x))
                .map(|x| (x.connection, x.session))
            {
                log.note(&format!(
                    "自动登录完成，检测到新会话 connection#{} session#{}",
                    f.0, f.1
                ));
                target = Some(f);
                break;
            }
        }
    }

    // 3) 执行：定向会话优先，超时兜底首会话
    match target {
        Some((c, s)) => {
            let result = crate::sap_scripting::run_script_file_in_session(
                std::path::Path::new(&script_path),
                c,
                s,
                Some(&log),
            );
            log.finish(&result);
            result.map_err(|e| CommandError::External(e.to_string()))?;
        }
        None => {
            log.note("轮询超时（30s），兜底附着首会话 attach(0,0)");
            let result = crate::sap_scripting::run_script_file(
                std::path::Path::new(&script_path),
                Some(&log),
            );
            log.finish(&result);
            result.map_err(|e| CommandError::External(e.to_string()))?;
        }
    }
    Ok(ScriptRunResult {
        used_existing_session: false,
    })
}

pub fn get_sap_connections() -> CmdResult<Vec<SapConnection>> {
    let (conns, errors) = sap_parser::get_all_sap_connections_with_errors();
    if conns.is_empty() && !errors.is_empty() {
        return Err(CommandError::External(errors.join("; ")));
    }
    Ok(conns)
}

/// v5 Should #8：把「部分 Landscape 文件解析失败」一并返回给 UI。
/// 旧行为只要有一个文件成功就丢弃全部错误，用户看到「导入成功」却少了一批连接。
pub fn get_sap_connections_with_errors() -> CmdResult<(Vec<SapConnection>, Vec<String>)> {
    Ok(sap_parser::get_all_sap_connections_with_errors())
}

/// 将选中的 SAP 连接导入为凭据（按 Workspace 创建自建分组）
pub fn import_sap_connections(
    store: &Mutex<StoreData>,
    connections: Vec<SapConnection>,
) -> CmdResult<usize> {
    let mut guard = store.lock().map_err(poisoned)?;
    let mut s = guard.clone(); // 工作副本：持久化成功后才提交，避免内存/磁盘分叉

    let default_group_id =
        storage::get_default_group_id(&s).ok_or_else(|| CommandError::NotFound {
            entity: "默认分组",
            id: "group-default".to_string(),
        })?;

    let mut imported = 0;
    let now = Utc::now();

    // 按 workspace_name 缓存已创建的分组 ID
    let mut group_cache: std::collections::HashMap<String, String> =
        std::collections::HashMap::new();

    for conn in connections {
        // 跳过已存在的同名连接
        let exists = s.credentials.iter().any(|c| c.connection_id == conn.name);
        if exists {
            continue;
        }

        // 确定目标分组：有 workspace_name 则创建/复用自建分组，否则用默认分组
        let group_id = if let Some(ws_name) = &conn.workspace_name {
            if ws_name.is_empty() || ws_name == "未分组" {
                default_group_id.clone()
            } else {
                // 查找缓存
                if let Some(gid) = group_cache.get(ws_name) {
                    gid.clone()
                } else {
                    // 查找已有同名自建分组
                    let existing = s
                        .groups
                        .iter()
                        .find(|g| g.group_name == *ws_name && !g.is_system && !g.is_default)
                        .map(|g| g.id.clone());
                    let gid = if let Some(id) = existing {
                        id
                    } else {
                        // 创建新分组
                        let new_group = crate::models::Group {
                            id: Uuid::new_v4().to_string(),
                            group_name: ws_name.clone(),
                            entries: Vec::new(),
                            is_system: false,
                            is_default: false,
                            created_at: now,
                            updated_at: now,
                        };
                        let new_id = new_group.id.clone();
                        s.groups.push(new_group);
                        new_id
                    };
                    group_cache.insert(ws_name.clone(), gid.clone());
                    gid
                }
            }
        } else {
            default_group_id.clone()
        };

        let cred = Credential {
            id: Uuid::new_v4().to_string(),
            connection_id: conn.name.clone(),
            client: String::new(),
            username: String::new(),
            password: String::new(),
            encrypted_password: String::new(),
            language: "ZH".to_string(),
            connection_type: conn
                .connection_type
                .clone()
                .unwrap_or_else(|| "direct".to_string()),
            system_id: conn.system_id.clone().unwrap_or_default(),
            app_server: conn.server.clone().unwrap_or_default(),
            system_number: conn.system_number.clone().unwrap_or_default(),
            message_server: conn.message_server.clone().unwrap_or_default(),
            message_server_port: conn.message_server_port.clone().unwrap_or_default(),
            logon_group: conn
                .logon_group
                .clone()
                .or(conn.group.clone())
                .unwrap_or_default(),
            saprouter: conn.saprouter.clone().unwrap_or_default(),
            description: conn.description.clone().unwrap_or_default(),
            post_login_action: None,
            post_login_action_type: None,
            script_path: None,
            group_id: Some(group_id.clone()),
            environment: String::new(),
            login_count: 0,
            last_login_at: None,
            color_tag: String::new(),
            is_favorite: false,
            favorite_order: None,
            display_name: Some(conn.name.clone()),
            uuid: conn.uuid.clone().unwrap_or_default(),
            // sncop 映射 SNC 配置（-1 表示未启用）
            snc_enabled: conn
                .sncop
                .as_deref()
                .map(|s| s != "-1" && !s.is_empty())
                .unwrap_or(false),
            snc_name: conn.snc_name.clone().unwrap_or_default(),
            snc_qop: conn.sncop.clone().filter(|s| s != "-1").unwrap_or_default(),
            snc_sso: false,
            created_at: now,
            updated_at: now,
        };

        // 将凭据 ID 加入目标分组
        storage::add_credential_to_group(&mut s, &cred.id, &group_id)
            .map_err(CommandError::from)?;

        s.credentials.push(cred);
        imported += 1;
    }

    if imported > 0 {
        storage::save_store(&s)?;
        *guard = s; // 持久化成功，提交内存
    }

    Ok(imported)
}
