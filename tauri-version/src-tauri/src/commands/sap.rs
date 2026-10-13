use tauri::State;
use crate::storage;
use crate::sap_parser;
use crate::sap_automation::{self, SapLoginParams};
use crate::models::{SapConnection, StoreData, Credential};
use std::sync::Mutex;
use chrono::Utc;
use uuid::Uuid;

/// 直接打开 SAP Logon（不自动登录）
#[tauri::command]
pub fn open_sap_logon(
    store: State<'_, Mutex<StoreData>>,
) -> Result<(), String> {
    let s = store.lock().map_err(|e| e.to_string())?;
    let custom_path = s.settings.sap_logon_path.as_deref();
    sap_automation::open_sap_logon(custom_path).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn login_to_sap(
    store: State<'_, Mutex<StoreData>>,
    master_password: String,
    credential_id: String,
) -> Result<(), String> {
    let mut s = store.lock().map_err(|e| e.to_string())?;

    let cred = s.credentials.iter().find(|c| c.id == credential_id)
        .ok_or("凭据不存在")?;

    // SNC SSO 模式下无需密码，跳过解密避免空密码解密报错
    let password = if cred.snc_enabled && cred.snc_sso {
        String::new()
    } else {
        storage::decrypt_credential_password(&s, &master_password, &cred.encrypted_password)
            .map_err(|e| e.to_string())?
    };

    let params = SapLoginParams {
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

    sap_automation::login_to_sap(&params).map_err(|e| e.to_string())?;

    // 登录成功后，自增登录次数并更新最后登录时间
    if let Some(cred) = s.credentials.iter_mut().find(|c| c.id == credential_id) {
        cred.login_count += 1;
        cred.last_login_at = Some(Utc::now());
    }
    storage::save_store(&s).map_err(|e| e.to_string())?;

    Ok(())
}

#[tauri::command]
pub fn get_sap_connections() -> Result<Vec<SapConnection>, String> {
    sap_parser::get_all_sap_connections().map_err(|e| e.to_string())
}

/// 将选中的 SAP 连接导入为凭据（按 Workspace 创建自建分组）
#[tauri::command]
pub fn import_sap_connections(
    store: State<'_, Mutex<StoreData>>,
    connections: Vec<SapConnection>,
) -> Result<usize, String> {
    let mut s = store.lock().map_err(|e| e.to_string())?;

    let default_group_id = storage::get_default_group_id(&s)
        .ok_or("默认分组不存在")?;

    let mut imported = 0;
    let now = Utc::now();

    // 按 workspace_name 缓存已创建的分组 ID
    let mut group_cache: std::collections::HashMap<String, String> = std::collections::HashMap::new();

    for conn in connections {
        // 跳过已存在的同名连接
        let exists = s.credentials.iter()
            .any(|c| c.connection_id == conn.name);
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
                    let existing = s.groups.iter()
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
            connection_type: conn.connection_type.clone().unwrap_or_else(|| "direct".to_string()),
            system_id: conn.system_id.clone().unwrap_or_default(),
            app_server: conn.server.clone().unwrap_or_default(),
            system_number: conn.system_number.clone().unwrap_or_default(),
            message_server: conn.message_server.clone().unwrap_or_default(),
            message_server_port: conn.message_server_port.clone().unwrap_or_default(),
            logon_group: conn.logon_group.clone().or(conn.group.clone()).unwrap_or_default(),
            saprouter: conn.saprouter.clone().unwrap_or_default(),
            description: conn.description.clone().unwrap_or_default(),
            post_login_action: None,
            post_login_action_type: None,
            group_id: Some(group_id.clone()),
            environment: String::new(),
            login_count: 0,
            last_login_at: None,
            color_tag: String::new(),
            is_favorite: false,
            favorite_order: None,
            display_name: Some(conn.name.clone()),
            uuid: conn.uuid.clone().unwrap_or_default(),
            // #1 改造：sncop 映射 SNC 配置（-1 表示未启用）
            snc_enabled: conn.sncop.as_deref().map(|s| s != "-1" && !s.is_empty()).unwrap_or(false),
            snc_name: conn.snc_name.clone().unwrap_or_default(),
            snc_qop: conn.sncop.clone().filter(|s| s != "-1").unwrap_or_default(),
            snc_sso: false,
            created_at: now,
            updated_at: now,
        };

        // 将凭据 ID 加入目标分组
        storage::add_credential_to_group(&mut s, &cred.id, &group_id)
            .map_err(|e| e.to_string())?;

        s.credentials.push(cred);
        imported += 1;
    }

    if imported > 0 {
        storage::save_store(&s).map_err(|e| e.to_string())?;
    }

    Ok(imported)
}
