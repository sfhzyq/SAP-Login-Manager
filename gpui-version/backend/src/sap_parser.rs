use crate::models::SapConnection;
use std::path::PathBuf;
use std::fs;
use xmltree::Element;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum SapParserError {
    #[error("IO 错误: {0}")]
    Io(#[from] std::io::Error),
    #[error("XML 解析错误: {0}")]
    XmlParse(String),
    #[error("注册表读取错误: {0}")]
    RegistryError(String),
}

/// 从注册表读取 SAP Landscape 文件路径
#[cfg(target_os = "windows")]
fn get_landscape_paths() -> Result<Vec<PathBuf>, SapParserError> {
    use winreg::enums::*;
    use winreg::RegKey;
    use std::collections::HashSet;

    let mut paths: Vec<PathBuf> = Vec::new();
    let mut seen: HashSet<PathBuf> = HashSet::new();
    let mut add = |p: PathBuf| {
        if seen.insert(p.clone()) {
            paths.push(p);
        }
    };

    let landscape_keys = [
        "CoreLandscapeFile",
        "CoreLandscapeFileOnServer",
        "LandscapeFormatXMLFile",
        "LandscapeFileOnServer",
        "UserLandscapeFile",
        "LandscapeFile",
    ];

    // HKCU: SAPLogon 根
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    if let Ok(sap_logon) = hkcu.open_subkey("Software\\SAP\\SAPLogon") {
        // 1) Options 子键
        if let Ok(options) = sap_logon.open_subkey("Options") {
            for key_name in &landscape_keys {
                if let Ok(path) = options.get_value::<String, _>(key_name) {
                    add(PathBuf::from(path));
                }
            }
        }

        // 2) LandscapeFilesLastUsed 子键（最可靠的"最近使用"记录）
        if let Ok(last_used) = sap_logon.open_subkey("LandscapeFilesLastUsed") {
            for key_name in &landscape_keys {
                if let Ok(path) = last_used.get_value::<String, _>(key_name) {
                    add(PathBuf::from(path));
                }
            }
        }

        // 3) LandscapeFilesInUse_* 子键（动态后缀，需枚举）
        //    SAPLogon 运行时会写入 LandscapeFilesInUse_<pid>
        if let Ok(subkeys) = sap_logon.enum_keys().collect::<Result<Vec<_>, _>>() {
            for sk in subkeys {
                if sk.starts_with("LandscapeFilesInUse") {
                    if let Ok(in_use) = sap_logon.open_subkey(&sk) {
                        for key_name in &landscape_keys {
                            if let Ok(path) = in_use.get_value::<String, _>(key_name) {
                                add(PathBuf::from(path));
                            }
                        }
                    }
                }
            }
        }

        // 4) ConfigFilesLastUsed / ConfigFilesInUse_* 子键（旧版 saplogon.ini 等）
        for cfg_sub in ["ConfigFilesLastUsed", "ConfigFilesInUse"] {
            if let Ok(cfg) = sap_logon.open_subkey(cfg_sub) {
                for key_name in ["ConnectionConfigFile", "TreeConfigFile", "ShortcutConfigFile"] {
                    if let Ok(path) = cfg.get_value::<String, _>(key_name) {
                        add(PathBuf::from(path));
                    }
                }
            }
        }
        // ConfigFilesInUse_<pid> 动态后缀
        if let Ok(subkeys) = sap_logon.enum_keys().collect::<Result<Vec<_>, _>>() {
            for sk in subkeys {
                if sk.starts_with("ConfigFilesInUse") {
                    if let Ok(cfg) = sap_logon.open_subkey(&sk) {
                        for key_name in ["ConnectionConfigFile", "TreeConfigFile", "ShortcutConfigFile"] {
                            if let Ok(path) = cfg.get_value::<String, _>(key_name) {
                                add(PathBuf::from(path));
                            }
                        }
                    }
                }
            }
        }
    }

    // HKLM: Options 子键
    let hklm = RegKey::predef(HKEY_LOCAL_MACHINE);
    if let Ok(sap_key) = hklm.open_subkey("Software\\SAP\\SAPLogon\\Options") {
        for key_name in &landscape_keys {
            if let Ok(path) = sap_key.get_value::<String, _>(key_name) {
                add(PathBuf::from(path));
            }
        }
    }

    // 默认路径兜底
    if let Ok(programdata) = std::env::var("PROGRAMDATA") {
        let default_path = PathBuf::from(programdata).join("SAP").join("SAPUILandscapeGlobal.xml");
        if default_path.exists() {
            add(default_path);
        }
    }

    if let Ok(appdata) = std::env::var("APPDATA") {
        let user_path = PathBuf::from(appdata).join("SAP").join("Common").join("SAPUILandscape.xml");
        if user_path.exists() {
            add(user_path);
        }
    }

    Ok(paths)
}

#[cfg(not(target_os = "windows"))]
fn get_landscape_paths() -> Result<Vec<PathBuf>, SapParserError> {
    Ok(Vec::new())
}

/// 解析 SAPUILandscape.xml 文件
///
/// 真实 XML 结构：
/// <Landscape>
///   <Workspaces><Workspace name="Local"><Item serviceid="<uuid>"/></Workspace></Workspaces>
///   <Services><Service uuid="..." name="..." systemid="..." server="host:32NN" 
///            mode="1" msid="..." routerid="..."/></Services>
///   <Routers><Router uuid="..." router="/H/..."/></Routers>
///   <Messageservers><Messageserver uuid="..." name="..." host="..."/></Messageservers>
/// </Landscape>
pub fn parse_landscape_file(path: &PathBuf) -> Result<Vec<SapConnection>, SapParserError> {
    let content = fs::read_to_string(path)?;
    let root = Element::parse(content.as_bytes())
        .map_err(|e| SapParserError::XmlParse(e.to_string()))?;

    // 1) 收集所有 Service 元素，建立 uuid → SapConnection 映射
    let mut service_map: std::collections::HashMap<String, SapConnection> = std::collections::HashMap::new();

    if let Some(services_elem) = find_child(&root, "Services") {
        for child in &services_elem.children {
            if let Some(svc) = child.as_element() {
                if svc.name == "Service" {
                    // #3 改造：仅处理 type="SAPGUI" 的服务
                    let svc_type = svc.attributes.get("type").map(|s| s.as_str()).unwrap_or("");
                    if svc_type != "SAPGUI" {
                        continue;
                    }
                    if let Some(conn) = parse_service_element(svc) {
                        let uuid = svc.attributes.get("uuid").cloned().unwrap_or_default();
                        service_map.insert(uuid, conn);
                    }
                }
            }
        }
    }

    // 2) 收集 Routers，建立 uuid → router 字符串映射
    let mut router_map: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    if let Some(routers_elem) = find_child(&root, "Routers") {
        for child in &routers_elem.children {
            if let Some(r) = child.as_element() {
                if r.name == "Router" {
                    let uuid = r.attributes.get("uuid").cloned().unwrap_or_default();
                    let router = r.attributes.get("router").cloned().unwrap_or_default();
                    if !uuid.is_empty() {
                        router_map.insert(uuid, router);
                    }
                }
            }
        }
    }

    // 3) 收集 Messageservers，建立 uuid → (name, host, port) 映射
    let mut ms_map: std::collections::HashMap<String, (String, String, Option<String>)> = std::collections::HashMap::new();
    if let Some(ms_elem) = find_child(&root, "Messageservers") {
        for child in &ms_elem.children {
            if let Some(ms) = child.as_element() {
                if ms.name == "Messageserver" {
                    let uuid = ms.attributes.get("uuid").cloned().unwrap_or_default();
                    let name = ms.attributes.get("name").cloned().unwrap_or_default();
                    let host = ms.attributes.get("host").cloned().unwrap_or_default();
                    let port = ms.attributes.get("port").cloned().filter(|p| !p.is_empty());
                    if !uuid.is_empty() {
                        ms_map.insert(uuid, (name, host, port));
                    }
                }
            }
        }
    }

    // 4) 为每个 Service 填充 router 和 message_server
    for conn in service_map.values_mut() {
        // routerid 需要从原始 Service 元素获取，这里用 conn.service 存储的 routerid
        if let Some(routerid) = &conn.service.clone() {
            if let Some(router_str) = router_map.get(routerid) {
                conn.saprouter = Some(router_str.clone());
            }
        }
        // 清理 service 字段（不再用作 routerid 临时存储）
        conn.service = None;

        // 对于负载均衡模式，解析 message server
        if conn.connection_type.as_deref() == Some("load_balancing") {
            // message_server 字段暂时存储 msid，需要解析
            if let Some(msid) = conn.message_server.clone() {
                if let Some((name, host, port)) = ms_map.get(&msid) {
                    conn.message_server = Some(host.clone());
                    conn.message_server_port = port.clone();
                    if conn.group.is_none() || conn.group.as_deref() == Some("") {
                        conn.group = Some(name.clone());
                    }
                }
            } else {
                conn.message_server = None;
            }
        }
    }

    // 5) 解析 Workspaces，建立 serviceid → workspace_name 映射
    let mut ws_map: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    if let Some(workspaces_elem) = find_child(&root, "Workspaces") {
        for child in &workspaces_elem.children {
            if let Some(ws) = child.as_element() {
                if ws.name == "Workspace" {
                    let ws_name = ws.attributes.get("name").cloned().unwrap_or_else(|| "默认".to_string());
                    for item_child in &ws.children {
                        if let Some(item) = item_child.as_element() {
                            if item.name == "Item" {
                                if let Some(sid) = item.attributes.get("serviceid") {
                                    ws_map.insert(sid.clone(), ws_name.clone());
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    // 6) 按 Workspace 顺序组装连接列表
    let mut connections = Vec::new();
    let mut added_uuids: std::collections::HashSet<String> = std::collections::HashSet::new();

    // 先添加有 workspace 归属的连接
    if let Some(workspaces_elem) = find_child(&root, "Workspaces") {
        for child in &workspaces_elem.children {
            if let Some(ws) = child.as_element() {
                if ws.name == "Workspace" {
                    let ws_name = ws.attributes.get("name").cloned().unwrap_or_else(|| "默认".to_string());
                    for item_child in &ws.children {
                        if let Some(item) = item_child.as_element() {
                            if item.name == "Item" {
                                if let Some(sid) = item.attributes.get("serviceid") {
                                    if let Some(conn) = service_map.get(sid) {
                                        let mut conn = conn.clone();
                                        conn.workspace_name = Some(ws_name.clone());
                                        connections.push(conn);
                                        added_uuids.insert(sid.clone());
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    // 再添加未被任何 Workspace 引用的连接（兜底）
    for (uuid, conn) in &service_map {
        if !added_uuids.contains(uuid) {
            let mut conn = conn.clone();
            conn.workspace_name = Some("未分组".to_string());
            connections.push(conn);
        }
    }

    Ok(connections)
}

/// 在直接子元素中查找指定名称的元素
fn find_child<'a>(parent: &'a Element, name: &str) -> Option<&'a Element> {
    parent.children.iter()
        .find_map(|c| c.as_element().filter(|e| e.name == name))
}

fn parse_service_element(elem: &Element) -> Option<SapConnection> {
    let name = elem.attributes.get("name").cloned().unwrap_or_default();
    if name.is_empty() {
        return None;
    }

    let system_id = elem.attributes.get("systemid").cloned();
    let description = elem.attributes.get("description").cloned().filter(|s| !s.is_empty());
    let uuid = elem.attributes.get("uuid").cloned().filter(|s| !s.is_empty());
    // #1 改造：读取 sncop（SNC 保护质量），-1 表示未启用
    let sncop = elem.attributes.get("sncop").cloned();
    // 读取 sncname（SNC 名称，如 p:CN=DS4, OU=SAP-HEC, O=SAP SE, C=DE）
    let snc_name = elem.attributes.get("sncname").cloned().filter(|s| !s.is_empty());

    // server 格式: "host:32NN" → 拆分为 app_server 和 system_number
    let server_raw = elem.attributes.get("server").cloned();
    let mut server: Option<String> = None;
    let mut system_number: Option<String> = None;
    let mut message_server: Option<String> = None;
    let mut connection_type: Option<String> = None;
    let mut group: Option<String> = None;

    let mode = elem.attributes.get("mode").map(|s| s.as_str()).unwrap_or("1");

    if let Some(srv) = &server_raw {
        // 负载均衡模式（有 msid，server 存的是 group 名如 "EASP"）
        if let Some(msid) = elem.attributes.get("msid") {
            connection_type = Some("load_balancing".to_string());
            group = Some(srv.clone()); // server 字段在负载均衡模式下存的是 logon group
            message_server = Some(msid.clone()); // 暂存 msid，后续解析为 host
        } else {
            // 直连模式: server="host:32NN"
            connection_type = Some("direct".to_string());
            if let Some(colon_pos) = srv.rfind(':') {
                let (host, port) = srv.split_at(colon_pos);
                let port = &port[1..]; // 去掉冒号
                server = Some(host.to_string());
                // 端口 32NN → 实例编号 NN
                if port.len() == 4 && port.starts_with("32") {
                    system_number = Some(port[2..].to_string());
                } else {
                    system_number = Some(port.to_string());
                }
            } else {
                server = Some(srv.clone());
            }
        }
    } else if mode == "0" {
        connection_type = Some("load_balancing".to_string());
    }

    // routerid 暂存到 service 字段，后续解析
    let routerid = elem.attributes.get("routerid").cloned();

    let conn = SapConnection {
        name,
        description,
        server,
        system_number,
        system_id,
        group: group.clone(),
        service: routerid, // 临时存储，后续解析为 saprouter
        connection_type,
        workspace_name: None,
        saprouter: None,
        message_server,
        message_server_port: None,
        logon_group: group,
        uuid,
        sncop,
        snc_name,
    };

    Some(conn)
}

/// 获取所有 SAP 连接（自动处理 Includes 去重）
///
/// 用户级 SAPUILandscape.xml 通过 <Includes> 引用全局 SAPUILandscapeGlobal.xml，
/// 注册表也会分别返回这两个文件。为避免全局文件的连接被重复导入，
/// 解析时先收集用户级文件的 Includes 引用，跳过已被引用的全局文件。
pub fn get_all_sap_connections() -> Result<Vec<SapConnection>, SapParserError> {
    let paths = get_landscape_paths()?;

    // #4 改造：先扫描所有文件，收集 <Includes> 引用的文件路径
    let mut included_files: std::collections::HashSet<PathBuf> = std::collections::HashSet::new();
    for path in &paths {
        if !path.exists() {
            continue;
        }
        if let Ok(content) = fs::read_to_string(path) {
            if let Ok(root) = Element::parse(content.as_bytes()) {
                if let Some(includes_elem) = find_child(&root, "Includes") {
                    for child in &includes_elem.children {
                        if let Some(inc) = child.as_element() {
                            if inc.name == "Include" {
                                if let Some(url) = inc.attributes.get("url") {
                                    // file:///C:/Users/.../SAPUILandscapeGlobal.xml → 转为路径
                                    let clean = url.strip_prefix("file:///")
                                        .or_else(|| url.strip_prefix("file://"))
                                        .unwrap_or(url);
                                    let clean = clean.replace('/', "\\");
                                    let inc_path = PathBuf::from(clean);
                                    if inc_path.exists() {
                                        included_files.insert(inc_path);
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    // 解析所有文件，但跳过已被 Includes 引用的文件（避免重复）
    let mut all_connections = Vec::new();
    let mut seen_uuids: std::collections::HashSet<String> = std::collections::HashSet::new();

    for path in &paths {
        if !path.exists() {
            continue;
        }
        // 跳过被其他文件 Includes 引用的文件（它的连接会随引用文件一起被解析）
        if included_files.contains(path) {
            continue;
        }
        match parse_landscape_file(path) {
            Ok(conns) => {
                for conn in conns {
                    // 按 UUID 去重（同一连接可能出现在多个文件中）
                    let key = conn.uuid.clone()
                        .unwrap_or_else(|| format!("{}-{}", conn.name, conn.server.as_deref().unwrap_or("")));
                    if seen_uuids.insert(key) {
                        all_connections.push(conn);
                    }
                }
            }
            Err(e) => log::warn!("解析 {} 失败: {}", path.display(), e),
        }
    }

    Ok(all_connections)
}
