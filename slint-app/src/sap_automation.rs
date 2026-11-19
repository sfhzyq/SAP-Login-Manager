use std::process::Command;
use thiserror::Error;

/// 将 UTF-8 字符串转换为系统 ANSI 代码页（CP_ACP，中文环境为 GBK/936）字节序列。
///
/// sapshcut.exe 读取 .bat 文件时按系统 ANSI 代码页解析；若 -sysname 中含中文，
/// 必须以 ANSI 写入，否则中文会被当成多字节 UTF-8 导致乱码/命令行超长。
/// 转换失败（含无法映射的字符）时回退为原始 UTF-8 字节。
#[cfg(target_os = "windows")]
fn utf8_to_ansi_bytes(s: &str) -> Vec<u8> {
    use windows_sys::Win32::Globalization::{WideCharToMultiByte, CP_ACP};
    // UTF-8 -> UTF-16
    let wide: Vec<u16> = s.encode_utf16().collect();
    if wide.is_empty() {
        return Vec::new();
    }
    unsafe {
        // 先取所需长度
        let len = WideCharToMultiByte(
            CP_ACP, 0, wide.as_ptr(), wide.len() as i32,
            std::ptr::null_mut(), 0, std::ptr::null(), std::ptr::null_mut(),
        );
        if len <= 0 {
            return s.as_bytes().to_vec();
        }
        let mut buf = vec![0u8; len as usize];
        let written = WideCharToMultiByte(
            CP_ACP, 0, wide.as_ptr(), wide.len() as i32,
            buf.as_mut_ptr(), len, std::ptr::null(), std::ptr::null_mut(),
        );
        if written <= 0 {
            return s.as_bytes().to_vec();
        }
        buf.truncate(written as usize);
        buf
    }
}

#[cfg(not(target_os = "windows"))]
fn utf8_to_ansi_bytes(s: &str) -> Vec<u8> {
    s.as_bytes().to_vec()
}

#[derive(Error, Debug)]
pub enum SapAutomationError {
    #[error("启动 SAP GUI 失败: {0}")]
    LaunchFailed(String),
    #[error("SAP GUI 未找到")]
    NotFound,
}

/// SAP 自动登录参数
pub struct SapLoginParams {
    pub connection_id: String,
    pub client: String,
    pub username: String,
    pub password: String,
    pub language: String,
    /// 连接类型: "direct" | "load_balancing"
    pub connection_type: String,
    /// 系统标识（SID）
    pub system_id: String,
    /// 应用服务器（直连）
    pub app_server: String,
    /// 实例编号（直连）
    pub system_number: String,
    /// 消息服务器（负载均衡）
    pub message_server: String,
    /// 消息服务器端口（负载均衡）
    pub message_server_port: String,
    /// 登录组（负载均衡）
    pub logon_group: String,
    /// 路由字符串
    pub saprouter: String,
    /// Service UUID（SAP GUI 8.10 的 -uuid 参数）
    pub uuid: String,
    /// SNC 是否启用
    pub snc_enabled: bool,
    /// SNC 名称
    pub snc_name: String,
    /// SNC 保护质量
    pub snc_qop: String,
    /// SNC 是否使用 SSO
    pub snc_sso: bool,
}

/// 直接打开 SAP Logon（不带登录参数）
pub fn open_sap_logon(custom_path: Option<&str>) -> Result<(), SapAutomationError> {
    let saplogon_path = if let Some(p) = custom_path.filter(|p| !p.is_empty()) {
        p.to_string()
    } else {
        find_saplogon()?
    };

    Command::new(&saplogon_path)
        .spawn()
        .map_err(|e| SapAutomationError::LaunchFailed(e.to_string()))?;

    Ok(())
}

/// 通过生成并执行 .bat 文件启动 sapshcut.exe 自动登录。
///
/// 用户提供的可用脚本格式为：
///   "C:\...\sapshcut.exe" -user=<user> -pw=<pwd> -language=<lang> -SYSTEM=<SID> -CLIENT=<clnt> -sysname=<连接名>
///
/// 关键点：
/// - 使用 `-system=<SID>` 而非 `-sid=`（前者才是 sapshcut 识别的参数）。
/// - 不默认追加 `-maxgui`、不默认追加 `-gui=`，与用户可用脚本保持一致；
///   仅当存在显式服务器信息（直连/负载均衡/SNC）时才补充 `-gui=`。
/// - 生成临时 .bat 文件后用 `cmd /C` 执行，行为与用户手动双击 BAT 完全一致，
///   避免直接 spawn 时 SAP GUI 对参数/环境解析差异导致的登录失败。
pub fn login_to_sap(params: &SapLoginParams) -> Result<(), SapAutomationError> {
    let sapshcut_path = find_sapshcut()?;
    log::info!("sapshcut 路径: {}", sapshcut_path);
    log::info!("连接 ID: {}, 客户端: {}, 用户: {}", params.connection_id, params.client, params.username);

    // 收集独立参数（对齐用户可用 BAT 脚本的顺序与格式）
    let mut args: Vec<String> = Vec::new();

    // -user：用户名
    if !params.username.is_empty() {
        args.push(format!("-user={}", params.username));
    }

    // -pw：密码（SNC SSO 模式下不传密码，由 SNC 完成认证）
    if !params.snc_enabled || !params.snc_sso {
        args.push(format!("-pw={}", params.password));
    }

    // -language：登录语言
    if !params.language.is_empty() {
        args.push(format!("-language={}", params.language));
    }

    // -system：系统标识（SID）——注意是 -system= 而不是 -sid=
    if !params.system_id.is_empty() {
        args.push(format!("-system={}", params.system_id));
    }

    // -client：集团
    if !params.client.is_empty() {
        args.push(format!("-client={}", params.client));
    }

    // -sysname：SAP Logon 中保存的连接名称
    if !params.connection_id.is_empty() {
        args.push(format!("-sysname={}", params.connection_id));
    }

    // -gui：仅当没有 -sysname（无法从已保存连接解析路由）时才补充显式路由。
    // 若已有 -sysname，则不传 -gui，与用户可用脚本保持一致，
    // 同时避免超长路由字符串导致命令行超过 4000 字符上限。
    if params.connection_id.is_empty() {
        if let Some(gui_value) = build_gui_route(params) {
            args.push(format!("-gui={}", gui_value));
        }
    }

    // SNC 参数：SSO 模式下不传 -snc_name/-snc_qop（SNC 信息从 SAP Logon 配置通过 -sysname 读取）
    // 仅在 SNC 启用且非 SSO 模式下显式传 SNC 参数
    if params.snc_enabled && !params.snc_sso && !params.snc_name.is_empty() {
        args.push(format!("-snc_name={}", params.snc_name));
        if !params.snc_qop.is_empty() {
            args.push(format!("-snc_qop={}", params.snc_qop));
        }
    }

    // 脱敏日志（不打印密码）
    let safe_args: Vec<String> = args.iter()
        .map(|a| if a.starts_with("-pw=") { "-pw=***".to_string() } else { a.clone() })
        .collect();
    log::info!("登录参数: {}", safe_args.join(" "));

    // 命令行长度保护：sapshcut 上限约 4000 字符。按 ANSI 字节数估算，
    // 因为 sapshcut 读取的是 ANSI 编码内容（中文按 GBK 为 2 字节）。
    let full_cmd_line = format!("\"{}\" {}", sapshcut_path, args.join(" "));
    let ansi_len = utf8_to_ansi_bytes(&full_cmd_line).len();
    if ansi_len > 3800 {
        log::error!("命令行过长: {} 字节（上限约 4000）", ansi_len);
        return Err(SapAutomationError::LaunchFailed(format!(
            "登录命令行过长（{} 字节，超过 SAP 上限 4000）。请检查该连接的服务器/路由等字段是否异常。",
            ansi_len
        )));
    }

    // 生成 .bat 文件内容。
    // 关键：sapshcut 的参数必须是「不加引号」的 -key=value 形式（与用户可用脚本一致）。
    // 若给每个 -key=value 加双引号，sapshcut 会把带引号的 token 误当成「要打开的快捷方式文件名」，
    // 报错「无法打开 SAP 快捷方式文件」。因此仅对 sapshcut.exe 路径加引号，参数保持原样。
    // bat 中 % 需转义为 %%，避免被 cmd 当作变量展开。
    let escaped_args: Vec<String> = args.iter().map(|a| a.replace('%', "%%")).collect();
    let bat_content = format!(
        "@echo off\r\n\"{}\" {}\r\n",
        sapshcut_path,
        escaped_args.join(" ")
    );

    // 写入临时 .bat 文件
    let bat_path = std::env::temp_dir().join(format!(
        "sap_login_{}.bat",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0)
    ));
    std::fs::write(&bat_path, utf8_to_ansi_bytes(&bat_content))
        .map_err(|e| SapAutomationError::LaunchFailed(format!("写入 bat 文件失败: {}", e)))?;
    log::info!("已生成登录脚本(ANSI): {:?}", bat_path);

    // 工作目录设为 sapshcut.exe 所在目录（SAP GUI 需加载同目录 DLL）
    let working_dir = std::path::Path::new(&sapshcut_path)
        .parent()
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_default());
    log::info!("工作目录: {:?}", working_dir);

    // 用 cmd /C 执行 bat 文件（隐藏命令行窗口）
    let mut cmd = Command::new("cmd");
    cmd.args(["/C", &bat_path.to_string_lossy()])
        .current_dir(&working_dir);
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        // CREATE_NO_WINDOW: 不弹出黑色命令行窗口
        cmd.creation_flags(0x0800_0000);
    }
    match cmd.spawn()
    {
        Ok(child) => {
            log::info!("登录脚本已执行, PID: {}", child.id());
            std::mem::forget(child);
            // 延迟清理临时 bat（给 sapshcut 足够时间读取）
            let cleanup_path = bat_path.clone();
            std::thread::spawn(move || {
                std::thread::sleep(std::time::Duration::from_secs(15));
                let _ = std::fs::remove_file(&cleanup_path);
            });
            Ok(())
        }
        Err(e) => {
            log::error!("登录脚本执行失败: {}", e);
            let _ = std::fs::remove_file(&bat_path);
            Err(SapAutomationError::LaunchFailed(e.to_string()))
        }
    }
}

/// 构建 -gui 路由字符串（直连 / 负载均衡 / SAProuter / SNC 无 SSO）。
/// 返回 None 表示无显式服务器信息，此时依赖 -sysname 解析已保存的连接。
fn build_gui_route(params: &SapLoginParams) -> Option<String> {
    let mut gui_value = String::new();

    // SAProuter 前缀
    if !params.saprouter.is_empty() {
        gui_value.push_str(&params.saprouter);
        gui_value.push(' ');
    }

    let conn_type = params.connection_type.as_str();
    let mut has_route = false;

    if conn_type == "load_balancing" && !params.message_server.is_empty() {
        // 负载均衡端口推断：
        // 优先 /M/<ms>/S/<port>/G/<group>（有端口）
        // 其次 /R/<SID>/M/<ms>/G/<group>（有 SID）
        // 最后 /M/<ms>/G/<group>（无端口无 SID）
        if !params.message_server_port.is_empty() {
            gui_value.push_str(&format!(
                "/M/{}/S/{}/G/{}",
                params.message_server, params.message_server_port, params.logon_group
            ));
        } else if !params.system_id.is_empty() {
            gui_value.push_str(&format!(
                "/R/{}/M/{}/G/{}",
                params.system_id, params.message_server, params.logon_group
            ));
        } else {
            gui_value.push_str(&format!("/M/{}/G/{}", params.message_server, params.logon_group));
        }
        has_route = true;
    } else if conn_type == "load_balancing" && !params.system_id.is_empty() {
        // 负载均衡但无消息服务器，用 SID + group
        gui_value.push_str(&format!("/R/{}/G/{}", params.system_id, params.logon_group));
        has_route = true;
    } else if !params.app_server.is_empty() {
        // 直连: <app_server> <instance>
        gui_value.push_str(&params.app_server);
        if !params.system_number.is_empty() {
            gui_value.push(' ');
            gui_value.push_str(&params.system_number);
        }
        has_route = true;
    }

    if !has_route {
        return None;
    }

    // SNC 无 SSO: 追加 /SUPPORTBIT_ON=NEED_STDDYNPRO
    if params.snc_enabled && !params.snc_sso {
        gui_value.push_str(" /SUPPORTBIT_ON=NEED_STDDYNPRO");
    }

    Some(gui_value)
}

/// 查找 SAP 可执行文件（公共逻辑）
#[cfg(target_os = "windows")]
fn find_sap_executable(exe_name: &str) -> Result<String, SapAutomationError> {
    use winreg::enums::*;
    use winreg::RegKey;
    use std::path::PathBuf;

    // 1. 尝试从注册表查找 SAP 安装路径（多个可能的注册表位置）
    let hklm = RegKey::predef(HKEY_LOCAL_MACHINE);
    let reg_paths = [
        r"Software\SAP\SAPLogon",
        r"SOFTWARE\WOW6432Node\SAP\SAPLogon",
        r"Software\SAP\SAPGUI",
        r"SOFTWARE\WOW6432Node\SAP\SAPGUI",
        r"Software\SAP\SAP Frontend",
    ];

    for reg_path in &reg_paths {
        if let Ok(sap_key) = hklm.open_subkey(reg_path) {
            // 尝试多个可能的路径字段名
            for field in &["Path", "InstallPath", "SapGuiPath", "ExePath"] {
                if let Ok(path) = sap_key.get_value::<String, _>(field) {
                    let exe = PathBuf::from(&path).join(exe_name);
                    if exe.exists() {
                        return Ok(exe.to_string_lossy().to_string());
                    }
                }
            }
        }
    }

    // 2. 常见安装路径（扩展搜索范围）
    let common_paths = [
        r"C:\Program Files\SAP\FrontEnd\SAPgui",
        r"C:\Program Files (x86)\SAP\FrontEnd\SAPgui",
        r"C:\Program Files\SAP\SAPGUI",
        r"C:\Program Files (x86)\SAP\SAPGUI",
        r"C:\Program Files\SAP\FrontEnd",
        r"C:\Program Files (x86)\SAP\FrontEnd",
        // 用户安装路径
        r"C:\Users\Public\SAP\FrontEnd\SAPgui",
    ];

    for dir in &common_paths {
        let exe = PathBuf::from(dir).join(exe_name);
        if exe.exists() {
            return Ok(exe.to_string_lossy().to_string());
        }
    }

    // 3. 使用 WHERE 命令搜索 PATH 环境变量
    if let Ok(output) = std::process::Command::new("where")
        .arg(exe_name)
        .output()
    {
        if output.status.success() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            if let Some(first_line) = stdout.lines().next() {
                let path = first_line.trim();
                if !path.is_empty() && std::path::Path::new(path).exists() {
                    return Ok(path.to_string());
                }
            }
        }
    }

    Err(SapAutomationError::NotFound)
}

/// 查找 SAP Logon 可执行文件
#[cfg(target_os = "windows")]
fn find_saplogon() -> Result<String, SapAutomationError> {
    find_sap_executable("saplogon.exe")
}

/// 查找 sapshcut.exe（SAP 命令行自动登录工具）
#[cfg(target_os = "windows")]
fn find_sapshcut() -> Result<String, SapAutomationError> {
    find_sap_executable("sapshcut.exe")
}

#[cfg(not(target_os = "windows"))]
fn find_saplogon() -> Result<String, SapAutomationError> {
    Err(SapAutomationError::NotFound)
}

#[cfg(not(target_os = "windows"))]
fn find_sapshcut() -> Result<String, SapAutomationError> {
    Err(SapAutomationError::NotFound)
}
