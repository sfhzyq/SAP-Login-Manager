use std::process::Command;
use thiserror::Error;
// v5 Should #15：登录链路里含明文密码的中间缓冲，析构时自动清零
use zeroize::Zeroizing;

/// CREATE_NO_WINDOW（0x08000000）：派生控制台程序时不弹黑色命令行窗口。
/// 未启用 windows-sys 的 Win32_System_Threading feature，以本地具名常量替代魔数。
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// 将 UTF-8 字符串转换为系统 ANSI 代码页（CP_ACP，中文环境为 GBK/936）字节序列。
///
/// sapshcut.exe 读取 .bat 文件时按系统 ANSI 代码页解析；若 -sysname 中含中文，
/// 必须以 ANSI 写入，否则中文会被当成多字节 UTF-8 导致乱码/命令行超长。
/// 转换失败或存在无法映射的字符（best-fit 替换）时回退为原始 UTF-8 字节。
#[cfg(target_os = "windows")]
fn utf8_to_ansi_bytes(s: &str) -> Vec<u8> {
    use windows_sys::Win32::Globalization::{WideCharToMultiByte, CP_ACP, WC_NO_BEST_FIT_CHARS};
    // UTF-8 -> UTF-16
    let wide: Vec<u16> = s.encode_utf16().collect();
    if wide.is_empty() {
        return Vec::new();
    }
    // SAFETY: wide 为非空 UTF-16 缓冲区；两阶段调用（cbMultiByte=0 先量
    // 所需长度，再实际转换）是 WideCharToMultiByte 的文档化用法。
    // WC_NO_BEST_FIT_CHARS + used_default 探测：不可映射字符必须报出来，
    // 否则会被静默 best-fit 改写（如 € → x），与文档承诺的「回退 UTF-8」不符
    unsafe {
        let mut used_default: i32 = 0;
        // 先取所需长度
        let len = WideCharToMultiByte(
            CP_ACP,
            WC_NO_BEST_FIT_CHARS,
            wide.as_ptr(),
            wide.len() as i32,
            std::ptr::null_mut(),
            0,
            std::ptr::null(),
            &mut used_default,
        );
        if len <= 0 || used_default != 0 {
            return s.as_bytes().to_vec();
        }
        let mut buf = vec![0u8; len as usize];
        let written = WideCharToMultiByte(
            CP_ACP,
            WC_NO_BEST_FIT_CHARS,
            wide.as_ptr(),
            wide.len() as i32,
            buf.as_mut_ptr(),
            len,
            std::ptr::null(),
            &mut used_default,
        );
        if written <= 0 || used_default != 0 {
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
    /// 登录后动作内容（事务码 / 报表名 / ok-code）
    pub post_login_action: Option<String>,
    /// 登录后动作类型: "transaction" | "report" | "command"
    pub post_login_action_type: Option<String>,
    /// 用户设置中的 SAP 路径（可为安装目录或 saplogon.exe 完整路径）；
    /// None/空/无效则回落自动检测。优先级：用户设置 > 自动检测
    pub custom_sap_path: Option<String>,
}

/// 归一化用户设置路径：填目录则拼 exe 名，填 exe 直接用；只有文件实际存在才返回 Some。
///
/// 修复「填了目录不生效」：设置值不再要求必须是完整 exe 路径。
pub fn resolve_custom_path(custom: Option<&str>, exe_name: &str) -> Option<String> {
    let p = custom?.trim();
    if p.is_empty() {
        return None;
    }
    let pb = std::path::PathBuf::from(p);
    if pb.is_file() {
        return Some(pb.to_string_lossy().to_string());
    }
    if pb.is_dir() {
        let exe = pb.join(exe_name);
        if exe.is_file() {
            return Some(exe.to_string_lossy().to_string());
        }
    }
    None
}

/// 直接打开 SAP Logon（不带登录参数）
pub fn open_sap_logon(custom_path: Option<&str>) -> Result<(), SapAutomationError> {
    // 优先级：用户设置（目录或 exe 均可）> 自动检测
    let saplogon_path = match resolve_custom_path(custom_path, "saplogon.exe") {
        Some(p) => p,
        None => find_saplogon()?,
    };

    Command::new(&saplogon_path)
        .spawn()
        .map_err(|e| SapAutomationError::LaunchFailed(e.to_string()))?;

    Ok(())
}

/// 直接启动 sapshcut.exe 完成自动登录。
///
/// 用户提供的可用脚本格式为：
///   "C:\...\sapshcut.exe" `-user=<user> -pw=<pwd> -language=<lang> -SYSTEM=<SID> -CLIENT=<clnt> -sysname=<连接名>`
///
/// 关键点：
/// - 使用 `-system=<SID>` 而非 `-sid=`（前者才是 sapshcut 识别的参数）。
/// - 不默认追加 `-maxgui`、不默认追加 `-gui=`，与用户可用脚本保持一致；
///   仅当存在显式服务器信息（直连/负载均衡/SNC）时才补充 `-gui=`。
/// - 通过 CreateProcessW 直接传参（raw_arg 不做自动引号包裹），
///   不再生成 .bat 临时文件，消除命令注入风险和明文密码磁盘残留。
pub fn login_to_sap(params: &SapLoginParams) -> Result<(), SapAutomationError> {
    // 优先级：用户设置（目录或 exe 均可）> 自动检测。
    // 修复：此前登录链路完全不读设置中的 SAP Logon 路径，D 盘安装等
    // 自动检测失败的场景即使填了设置也不生效。
    let sapshcut_path = match resolve_custom_path(params.custom_sap_path.as_deref(), "sapshcut.exe")
    {
        Some(p) => p,
        None => find_sapshcut()?,
    };
    log::info!("sapshcut 路径: {}", sapshcut_path);
    log::info!(
        "连接 ID: {}, 客户端: {}, 用户: {}",
        params.connection_id,
        params.client,
        params.username
    );

    // 收集独立参数（对齐用户可用 BAT 脚本的顺序与格式）。
    // v5 Should #15：args 里含 `-pw=` 明文，用 Zeroizing 持有，函数返回时
    // 连同所有中间副本一起清零（此前只 drop，明文残留在已释放堆内存）。
    let mut args: Vec<Zeroizing<String>> = Vec::new();

    // -user：用户名
    if !params.username.is_empty() {
        args.push(Zeroizing::new(format!("-user={}", params.username)));
    }

    // -pw：密码（SNC SSO 模式下不传密码，由 SNC 完成认证）
    if !params.snc_enabled || !params.snc_sso {
        args.push(Zeroizing::new(format!("-pw={}", params.password)));
    }

    // -language：登录语言
    if !params.language.is_empty() {
        args.push(Zeroizing::new(format!("-language={}", params.language)));
    }

    // -system：系统标识（SID）——注意是 -system= 而不是 -sid=
    if !params.system_id.is_empty() {
        args.push(Zeroizing::new(format!("-system={}", params.system_id)));
    }

    // -client：集团
    if !params.client.is_empty() {
        args.push(Zeroizing::new(format!("-client={}", params.client)));
    }

    // -sysname：SAP Logon 中保存的连接名称
    if !params.connection_id.is_empty() {
        args.push(Zeroizing::new(format!("-sysname={}", params.connection_id)));
    }

    // -gui：仅当没有 -sysname（无法从已保存连接解析路由）时才补充显式路由。
    // 若已有 -sysname，则不传 -gui，与用户可用脚本保持一致，
    // 同时避免超长路由字符串导致命令行超过 4000 字符上限。
    if params.connection_id.is_empty() {
        if let Some(gui_value) = build_gui_route(params) {
            args.push(Zeroizing::new(format!("-gui={}", gui_value)));
        }
    }

    // SNC 参数：SSO 模式下不传 -snc_name/-snc_qop（SNC 信息从 SAP Logon 配置通过 -sysname 读取）
    // 仅在 SNC 启用且非 SSO 模式下显式传 SNC 参数
    if params.snc_enabled && !params.snc_sso && !params.snc_name.is_empty() {
        args.push(Zeroizing::new(format!("-snc_name={}", params.snc_name)));
        if !params.snc_qop.is_empty() {
            args.push(Zeroizing::new(format!("-snc_qop={}", params.snc_qop)));
        }
    }

    // 登录后动作：登录成功后直接执行（SAP Note 103019 的 -type/-command 组合）
    // -type=Transaction | Report | Command；-command 为不带 /n 前缀的 ok-code，
    // 含空格时需用双引号包裹（Windows 参数解析层面）。
    if let (Some(action_type), Some(action)) =
        (&params.post_login_action_type, &params.post_login_action)
    {
        if !action.trim().is_empty() {
            let sap_type = match action_type.as_str() {
                "report" => "Report",
                "command" => "Command",
                _ => "Transaction",
            };
            args.push(Zeroizing::new(format!("-type={}", sap_type)));
            // 引号包裹统一由启动循环里的 quote_value_part 处理——此处若预包裹，
            // 循环会二次包裹（内部引号再转义为 \"），sapshcut 收到带字面引号
            // 的值导致登录后动作静默失效
            args.push(Zeroizing::new(format!("-command={}", action.trim())));
        }
    }

    // 脱敏日志（不打印密码）；脱敏副本里不含明文，可安全 clone
    let safe_args: Vec<String> = args
        .iter()
        .map(|a| {
            if a.starts_with("-pw=") {
                "-pw=***".to_string()
            } else {
                a.as_str().to_string()
            }
        })
        .collect();
    log::info!("登录参数: {}", safe_args.join(" "));

    // 命令行长度保护：sapshcut 上限约 4000 字符。按 ANSI 字节数估算，
    // 因为 sapshcut 读取的是 ANSI 编码内容（中文按 GBK 为 2 字节）。
    // v5 Should #15：不再把含密码的完整命令行拼成一个大 String —— 改为
    // 逐参数累加字节数，转换缓冲也用 Zeroizing 持有，避免明文散落堆上。
    let mut ansi_len = Zeroizing::new(utf8_to_ansi_bytes(&format!("\"{}\" ", sapshcut_path))).len();
    for arg in &args {
        let bytes = Zeroizing::new(utf8_to_ansi_bytes(arg.as_str()));
        ansi_len += bytes.len() + 1; // 参数间的空格
    }
    if ansi_len > 3800 {
        log::error!("命令行过长: {} 字节（上限约 4000）", ansi_len);
        return Err(SapAutomationError::LaunchFailed(format!(
            "登录命令行过长（{} 字节，超过 SAP 上限 4000）。请检查该连接的服务器/路由等字段是否异常。",
            ansi_len
        )));
    }

    // 工作目录设为 sapshcut.exe 所在目录（SAP GUI 需加载同目录 DLL）
    let working_dir = std::path::Path::new(&sapshcut_path)
        .parent()
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_default());
    log::info!("工作目录: {:?}", working_dir);

    // 直接启动 sapshcut.exe，通过 CreateProcessW 传递 Unicode 命令行参数。
    // raw_arg 跳过 std 的自动转义；含空格的值手动用引号包住值部分
    // （`-pw="ab cd"` 经 CRT 解析为单参数 `-pw=ab cd`），否则值会在空格处
    // 裂开、sapshcut 收到错误密码——登录静默失败。值内部引号按 Windows
    // CRT 规则转义为 \"。不再生成 .bat 临时文件，消除命令注入风险和明文
    // 密码磁盘残留。
    let mut cmd = Command::new(&sapshcut_path);
    cmd.current_dir(&working_dir);
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(CREATE_NO_WINDOW);
        for arg in &args {
            // quote_value_part 的返回值同样含明文密码，Zeroizing 包裹后
            // 每轮循环结束即清零（Command 内部副本由 OS 进程边界持有）
            let quoted = Zeroizing::new(quote_value_part(arg.as_str()));
            cmd.raw_arg(quoted.as_str());
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        for arg in &args {
            cmd.arg(arg.as_str());
        }
    }
    match cmd.spawn() {
        Ok(child) => {
            log::info!("sapshcut 已启动, PID: {}", child.id());
            // Child 在 drop 时不会等待进程退出，直接丢弃句柄即可
            drop(child);
            Ok(())
        }
        Err(e) => {
            log::error!("sapshcut 启动失败: {}", e);
            Err(SapAutomationError::LaunchFailed(e.to_string()))
        }
    }
}

/// 对 `-key=value` 形式的参数按 Windows CRT 规则包引号：
/// 值含空格/引号/制表符时，仅给值部分加引号（键保持在引号外，
/// 保证 sapshcut 仍按 `-key=` 前缀识别参数），值内部反斜杠双写、
/// 引号转义为 \"。反斜杠双写是 CRT 语义要求：值尾单个 `\` 紧邻闭引号
/// 会被解析为字面引号（`\"`），引号不闭合吞掉后续所有参数。
fn quote_value_part(arg: &str) -> String {
    let esc = |v: &str| v.replace('\\', "\\\\").replace('"', "\\\"");
    let needs_quote = arg.chars().any(|c| c == ' ' || c == '\t' || c == '"');
    if !needs_quote {
        // 不加引号的裸值中 `\` 与 `"` 均无特殊语义，原样保留
        return arg.to_string();
    }
    match arg.split_once('=') {
        Some((key, value)) => format!("{}=\"{}\"", key, esc(value)),
        None => format!("\"{}\"", esc(arg)),
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
            gui_value.push_str(&format!(
                "/M/{}/G/{}",
                params.message_server, params.logon_group
            ));
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
    use std::path::PathBuf;
    use winreg::enums::*;
    use winreg::RegKey;

    // 1. 尝试从注册表查找 SAP 安装路径（多个可能的注册表位置）
    let hklm = RegKey::predef(HKEY_LOCAL_MACHINE);
    let reg_paths = [
        r"Software\SAP\SAPLogon",
        r"SOFTWARE\WOW6432Node\SAP\SAPLogon",
        r"Software\SAP\SAPGUI",
        r"SOFTWARE\WOW6432Node\SAP\SAPGUI",
        r"Software\SAP\SAP Frontend",
        // SAP GUI 7.50+/8.x 标准键：SAPGUIReleaseDirectory 指向安装目录
        //（无论装在 C 盘还是 D 盘自定义路径，安装器都会写这里）
        r"Software\SAP\SAP Shared",
        r"SOFTWARE\WOW6432Node\SAP\SAP Shared",
    ];

    for reg_path in &reg_paths {
        if let Ok(sap_key) = hklm.open_subkey(reg_path) {
            // 尝试多个可能的路径字段名
            for field in &[
                "Path",
                "InstallPath",
                "SapGuiPath",
                "ExePath",
                "SAPGUIReleaseDirectory",
            ] {
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
    // CREATE_NO_WINDOW：GUI 进程派生控制台程序（where.exe）默认会分配新控制台，
    // 导致「登录时弹出黑色 cmd 窗口一闪而过」，必须显式抑制。
    if let Ok(output) = {
        let mut cmd = std::process::Command::new("where");
        cmd.arg(exe_name);
        #[cfg(target_os = "windows")]
        {
            use std::os::windows::process::CommandExt;
            cmd.creation_flags(CREATE_NO_WINDOW);
        }
        cmd.output()
    } {
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

#[cfg(test)]
mod tests {
    use super::quote_value_part;

    #[test]
    fn wraps_only_value_part_when_spaces_present() {
        // 键留在引号外，sapshcut 才能按 -key= 前缀识别参数
        assert_eq!(
            quote_value_part("-command=ZMM_TEST REPORT"),
            "-command=\"ZMM_TEST REPORT\""
        );
    }

    #[test]
    fn escapes_quotes_and_backslashes_in_value() {
        // CRT 语义：\" → 字面引号；\\ → 字面反斜杠；值尾 \ 必须双写，
        // 否则 \" 被解析为字面引号导致引号不闭合、吞掉后续参数
        assert_eq!(quote_value_part("-pw=a\"b"), "-pw=\"a\\\"b\"");
        assert_eq!(quote_value_part("-pw=ab c\\"), "-pw=\"ab c\\\\\"");
        assert_eq!(quote_value_part("-pw=a\\\"b"), "-pw=\"a\\\\\\\"b\"");
    }

    #[test]
    fn leaves_plain_values_untouched() {
        assert_eq!(quote_value_part("-client=100"), "-client=100");
        assert_eq!(quote_value_part("-pw=s3cret"), "-pw=s3cret");
    }
}
