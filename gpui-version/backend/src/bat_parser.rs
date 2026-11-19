//! sapshcut 批量脚本（.bat）解析与生成 —— BAT 批量录入功能
//!
//! 用户习惯用 `sapshcut.exe` 命令行登录 SAP，本模块负责：
//! - 解析：把 bat 文本中的 sapshcut 命令行还原为凭据草稿（[`BatDraft`]），
//!   容错等号/空格两种传参形式、引号包裹、环境变量展开、GBK/UTF-8 编码嗅探
//! - 生成：把 [`Credential`] 反向生成为单行 sapshcut 命令（导出为多个 .bat 文件）
//!
//! sapshcut 参数 → 字段映射：
//! `-system`→system_id、`-client`→client、`-user`→username、`-pw`→password、
//! `-language`→language、`-hostname`+`-systemnumber`/`-serviceno`→直连、
//! `-msghost`+`-msservno`+`-group`→负载均衡、`-saprouter`→saprouter、
//! `-command`→post_login_action、`-name`→display_name

use crate::models::{Credential, SapConnection};

/// 单条 sapshcut 命令解析出的凭据草稿
#[derive(Debug, Clone, Default)]
pub struct BatDraft {
    /// bat 文件中的行号（从 1 起，指 sapshcut 命令所在行）
    pub line_no: usize,
    /// 来源文件名（多文件导入时由调用方填入，parse_bat 不填）
    pub source: String,
    /// 连接名（-sysname：SAP GUI 快捷方式的系统名称，导入时映射为 connection_id）
    pub connection_name: String,
    pub display_name: String,
    pub system_id: String,
    pub client: String,
    pub username: String,
    pub password: String,
    pub language: String,
    /// "direct" | "load_balancing"（由 hostname/msghost 推断）
    pub connection_type: String,
    /// 应用服务器（直连，-hostname）
    pub app_server: String,
    /// 实例编号（直连，-systemnumber 或由 -serviceno 换算）
    pub system_number: String,
    /// 消息服务器（负载均衡，-msghost）
    pub message_server: String,
    /// 消息服务器端口（-msservno / -msserv）
    pub message_server_port: String,
    /// 登录组（负载均衡，-group）
    pub logon_group: String,
    /// 路由字符串（-saprouter，如 /H/proxy/H/）
    pub saprouter: String,
    /// 登录后事务码（-command）
    pub post_login_action: String,
    /// 致命错误（导入时跳过该条）
    pub errors: Vec<String>,
    /// 警告（仍可导入）
    pub warnings: Vec<String>,
}

impl BatDraft {
    pub fn is_valid(&self) -> bool {
        self.errors.is_empty()
    }

    /// 预览摘要（如 `PRD 客户端 100 · ZHANGSAN · 直连 host:00`）
    pub fn summary(&self) -> String {
        let mut parts: Vec<String> = Vec::new();
        if !self.system_id.is_empty() {
            parts.push(self.system_id.clone());
        }
        if !self.client.is_empty() {
            parts.push(format!("客户端 {}", self.client));
        }
        if !self.username.is_empty() {
            parts.push(self.username.clone());
        }
        if self.connection_type == "load_balancing" {
            let mut lb = "负载均衡".to_string();
            if !self.message_server.is_empty() {
                lb.push_str(&format!(" {}", self.message_server));
            }
            if !self.logon_group.is_empty() {
                lb.push_str(&format!(" / {}", self.logon_group));
            }
            parts.push(lb);
        } else if self.connection_type == "direct" {
            let mut d = "直连".to_string();
            if !self.app_server.is_empty() {
                d.push_str(&format!(" {}", self.app_server));
            }
            if !self.system_number.is_empty() {
                d.push_str(&format!(":{}", self.system_number));
            }
            parts.push(d);
        }
        if !self.display_name.is_empty() {
            parts.push(format!("（{}）", self.display_name));
        }
        if parts.is_empty() {
            "(空)".to_string()
        } else {
            parts.join(" · ")
        }
    }
}

/// 解析 bat 文本中所有 sapshcut 命令行
///
/// 非 sapshcut 行（@echo off、rem 注释、其他命令）静默跳过；
/// 每条 sapshcut 命令产出一个 [`BatDraft`]，字段缺失/冲突记入 errors / warnings。
pub fn parse_bat(content: &str) -> Vec<BatDraft> {
    let mut drafts = Vec::new();
    for (i, raw) in content.lines().enumerate() {
        let line = expand_env_vars(raw.trim());
        let trimmed = line.trim();
        // 跳过注释/样板行
        if trimmed.is_empty()
            || trimmed.starts_with("::")
            || trimmed.starts_with('@')
            || trimmed.eq_ignore_ascii_case("echo off")
            || is_comment_line(trimmed)
        {
            continue;
        }
        // 只处理包含 sapshcut 的行（不区分大小写）
        if !trimmed.to_ascii_lowercase().contains("sapshcut") {
            continue;
        }
        drafts.push(parse_sapshcut_line(i + 1, trimmed));
    }
    drafts
}

/// rem 注释行（rem 后跟空格，不区分大小写）
fn is_comment_line(s: &str) -> bool {
    let lower = s.to_ascii_lowercase();
    lower.starts_with("rem ") || lower == "rem"
}

/// 尽力展开 %VAR% 环境变量（未定义的保留原样）
fn expand_env_vars(s: &str) -> String {
    if !s.contains('%') {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(start) = rest.find('%') {
        out.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        match after.find('%') {
            Some(end) if end > 0 => {
                let name = &after[..end];
                match std::env::var(name) {
                    Ok(v) => out.push_str(&v),
                    Err(_) => out.push_str(&format!("%{name}%")),
                }
                rest = &after[end + 1..];
            }
            Some(0) => {
                // `%%` 是 bat 里字面 % 的转义（与 quote_value 的 %% 互逆）；
                // 折叠成单个 % 后继续处理后续内容
                out.push('%');
                rest = &after[1..];
            }
            _ => {
                // 没有配对的 %，原样保留剩余部分
                out.push('%');
                out.push_str(after);
                rest = "";
                break;
            }
        }
    }
    out.push_str(rest);
    out
}

/// 按双引号分词（引号内空格不切分），并逆转 quote_value 的转义。
///
/// `\\` → 字面 `\`；`\"` → 字面 `"`（且不翻转引号态）。这样
/// `credential_to_sapshcut` 导出的 `-pw="ab\\\"cd"` 再导入能还原成原值，
/// 而不是丢字符或把后续 token 并进同一个参数。
fn tokenize(line: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut cur = String::new();
    let mut in_quotes = false;
    let mut chars = line.chars().peekable();
    while let Some(ch) = chars.next() {
        match ch {
            '\\' => match chars.peek() {
                Some('\\') => {
                    cur.push('\\');
                    chars.next();
                }
                Some('"') => {
                    cur.push('"');
                    chars.next();
                }
                _ => cur.push('\\'),
            },
            '"' => {
                in_quotes = !in_quotes;
            }
            ' ' | '\t' if !in_quotes => {
                if !cur.is_empty() {
                    tokens.push(std::mem::take(&mut cur));
                }
            }
            _ => cur.push(ch),
        }
    }
    if !cur.is_empty() {
        tokens.push(cur);
    }
    tokens
}

/// sapshcut 支持的已知但与本功能无关的参数（静默忽略）
const IGNORED_FLAGS: &[&str] = &[
    "type", "uuid", "gui", "max", "low", "reuse", "ini", "lwo", "e", "sncname", "sncqop", "k",
    "maxgui",
];

/// 解析单条 sapshcut 命令行
fn parse_sapshcut_line(line_no: usize, line: &str) -> BatDraft {
    let mut d = BatDraft {
        line_no,
        ..Default::default()
    };

    let tokens = tokenize(line);
    let mut pairs: Vec<(String, Option<String>)> = Vec::new();
    let mut i = 0;
    while i < tokens.len() {
        let tok = &tokens[i];
        let lower = tok.to_ascii_lowercase();
        // 跳过可执行文件路径与 start 命令
        if lower == "start"
            || lower.contains("sapshcut")
            || lower.ends_with(".exe")
            || lower == "call"
            || lower == "/wait"
            || lower == "-wait"
        {
            i += 1;
            continue;
        }
        // 参数形式：-key=value / -key:value / -key value（也容忍 /key）
        let stripped = tok
            .strip_prefix('-')
            .or_else(|| tok.strip_prefix('/'))
            .map(|s| s.to_string());
        if let Some(rest) = stripped {
            let mut key = rest.clone();
            let mut val: Option<String> = None;
            if let Some(pos) = rest.find(['=', ':']) {
                key = rest[..pos].to_string();
                val = Some(rest[pos + 1..].to_string());
            }
            let lower_key = key.to_ascii_lowercase();
            // 已知但无关的开关参数（-max -gui 等常为无值形式）静默忽略
            if IGNORED_FLAGS.contains(&lower_key.as_str()) {
                i += 1;
                continue;
            }
            if val.is_none() {
                // 空格传参：取下一个 token
                if i + 1 < tokens.len()
                    && !tokens[i + 1].starts_with('-')
                    && !tokens[i + 1].starts_with('/')
                {
                    val = Some(tokens[i + 1].clone());
                    i += 1;
                } else {
                    d.warnings
                        .push(format!("参数 -{} 缺少取值，已忽略", lower_key));
                }
            }
            pairs.push((lower_key, val));
        }
        // 其余裸 token（无 - 前缀的值）忽略
        i += 1;
    }

    for (key, val) in pairs {
        let v = val.unwrap_or_default();
        match key.as_str() {
            "system" | "sid" => d.system_id = v.to_ascii_uppercase(),
            "client" | "mandant" => d.client = v,
            "user" | "u" => d.username = v,
            "pw" | "password" => d.password = v,
            "language" | "l" => d.language = lang_key_to_iso(&v),
            "hostname" | "host" => d.app_server = v,
            "systemnumber" | "sysnr" => d.system_number = normalize_instance_no(&v),
            "serviceno" | "service" => {
                d.system_number = service_no_to_instance(&v, &mut d.warnings);
            }
            "msghost" => d.message_server = v,
            "msservno" | "msserv" => {
                d.message_server_port = service_no_to_port(&v, &mut d.warnings)
            }
            "group" => d.logon_group = v,
            "saprouter" | "router" => d.saprouter = v,
            "command" => d.post_login_action = v,
            "name" => d.display_name = v,
            // -sysname：SAP GUI 快捷方式的系统名称 → 连接名（-name 显式指定的别名仍归显示名称）
            "sysname" => d.connection_name = v,
            f if IGNORED_FLAGS.contains(&f) => {}
            "" => {}
            other => d.warnings.push(format!("未知参数 -{other}，已忽略")),
        }
    }

    // 推断连接类型 + 必填校验
    if !d.message_server.is_empty() {
        d.connection_type = "load_balancing".to_string();
        if !d.logon_group.is_empty() {
            // 合法
        }
    } else if !d.app_server.is_empty() {
        d.connection_type = "direct".to_string();
    }

    if d.system_id.is_empty() {
        d.errors.push("缺少 -system（系统标识）".to_string());
    }
    if d.client.is_empty() {
        d.errors.push("缺少 -client（集团编号）".to_string());
    }
    if d.username.is_empty() {
        d.errors.push("缺少 -user（用户名）".to_string());
    }
    if d.connection_type.is_empty() {
        // SAP GUI 原生快捷方式常为纯 SID 引用型（-SYSTEM -CLIENT，无服务器地址），
        // 依赖本机 SAP GUI 配置解析服务器——不判致命，由 resolve_from_landscape 补全
        d.warnings.push(
            "未指定服务器地址（-hostname/-msghost），将尝试从本机 SAP GUI 配置按系统标识补全"
                .to_string(),
        );
    }
    if d.system_id.len() > 3 {
        d.warnings.push(format!(
            "-system 值 \"{}\" 超过 3 位，请确认是否为系统标识",
            d.system_id
        ));
    }
    if !d.client.is_empty()
        && (d.client.len() != 3 || !d.client.chars().all(|c| c.is_ascii_digit()))
    {
        d.warnings
            .push(format!("-client 值 \"{}\" 不是 3 位数字，请确认", d.client));
    }
    if !d.system_number.is_empty() && d.system_number.len() != 2 {
        d.warnings.push(format!(
            "实例编号 \"{}\" 不是 2 位，请确认",
            d.system_number
        ));
    }
    if d.saprouter.is_empty() && d.app_server.contains("/H/") {
        d.warnings
            .push("-hostname 中含 /H/ 路由串，建议改用 -saprouter 指定".to_string());
    }

    d
}

/// 实例编号规整：去掉非数字前缀，保留数字部分
fn normalize_instance_no(v: &str) -> String {
    let digits: String = v.chars().filter(|c| c.is_ascii_digit()).collect();
    if digits.is_empty() {
        v.to_string()
    } else {
        digits
    }
}

/// -serviceno 端口（32NN）→ 实例编号（NN）
fn service_no_to_instance(v: &str, warnings: &mut Vec<String>) -> String {
    let v = v.trim();
    if v.len() == 4 && v.starts_with("32") && v[2..].chars().all(|c| c.is_ascii_digit()) {
        v[2..].to_string()
    } else if v.len() <= 2 && v.chars().all(|c| c.is_ascii_digit()) {
        v.to_string()
    } else {
        warnings.push(format!(
            "-serviceno 值 \"{v}\" 不是 32NN 形式的端口，已原样保留为实例编号"
        ));
        v.to_string()
    }
}

/// 消息服务器端口（36NN 或直接端口号）规整
fn service_no_to_port(v: &str, warnings: &mut Vec<String>) -> String {
    let v = v.trim();
    if v.len() == 4 && v.starts_with("36") && v[2..].chars().all(|c| c.is_ascii_digit()) {
        v[2..].to_string()
    } else {
        // 直接端口号（如 3601）也容忍
        if v.chars().all(|c| c.is_ascii_digit()) {
            v.to_string()
        } else {
            warnings.push(format!("-msservno 值 \"{v}\" 不是数字端口，已忽略"));
            String::new()
        }
    }
}

/// 读取 bat 文件内容：按 BOM 支持 UTF-16LE / UTF-16BE / UTF-8，
/// 无 BOM 时 UTF-8 优先、失败回退 GBK（Windows）。
///
/// v5 Consider #22：此前 `FE FF`（UTF-16BE BOM）会被当成普通字节送去 GBK
/// 解码，解出含 `\0` 的乱码；且该公共入口已被 `sap_parser` / `sap_scripting`
/// 复用，能力缺口会扩散到 Landscape 与 VBS 导入。
pub fn read_bat_file(path: &std::path::Path) -> Result<String, String> {
    // 错误消息带文件路径：多文件批量导入时用户需要定位是哪个文件失败
    let raw = std::fs::read(path).map_err(|e| format!("读取 {} 失败: {e}", path.display()))?;
    let ctx = |detail: String| format!("读取 {} 失败: {detail}", path.display());

    // UTF-16 BOM 必须优先于 UTF-8 BOM 判定（FE FF 不是合法 UTF-8 前缀）
    if raw.starts_with(&[0xFE, 0xFF]) {
        return utf16_decode(&raw[2..], true).map_err(ctx);
    }
    if raw.starts_with(&[0xFF, 0xFE]) {
        return utf16_decode(&raw[2..], false).map_err(ctx);
    }
    let bytes: &[u8] = if raw.starts_with(&[0xEF, 0xBB, 0xBF]) {
        &raw[3..]
    } else {
        &raw[..]
    };
    if let Ok(s) = std::str::from_utf8(bytes) {
        return Ok(s.to_string());
    }
    #[cfg(target_os = "windows")]
    {
        decode_gbk(bytes).ok_or_else(|| {
            format!(
                "读取 {} 失败: 文件编码无法识别（尝试过 UTF-16 / UTF-8 / GBK），请用记事本另存为 ANSI 或 UTF-8",
                path.display()
            )
        })
    }
    #[cfg(not(target_os = "windows"))]
    {
        Ok(String::from_utf8_lossy(bytes).into_owned())
    }
}

/// UTF-16 解码（be=true 为大端）。
/// v5 Consider #22：奇数尾字节与非法代理对不再静默丢弃 / 替换成 U+FFFD，
/// 而是返回带原因的 Err，避免「导入成功但内容残缺」。
fn utf16_decode(bytes: &[u8], be: bool) -> Result<String, String> {
    if !bytes.len().is_multiple_of(2) {
        return Err(format!(
            "UTF-16 数据长度为奇数（{} 字节），最后一个字节疑似被截断",
            bytes.len()
        ));
    }
    // 长度已校验为偶数，按两个字节一组步进；不用 chunks_exact 是因为
    // 常量分块会被当前 clippy 建议改用较新的切片分块 API，这里保持
    // 与 MSRV 无关的最朴素稳定写法。
    let units: Vec<u16> = (0..bytes.len())
        .step_by(2)
        .map(|i| {
            let pair = [bytes[i], bytes[i + 1]];
            if be {
                u16::from_be_bytes(pair)
            } else {
                u16::from_le_bytes(pair)
            }
        })
        .collect();
    String::from_utf16(&units).map_err(|e| format!("UTF-16 含非法代理对: {e}"))
}

/// GBK（代码页 936）解码，严格模式失败返回 None
#[cfg(target_os = "windows")]
fn decode_gbk(bytes: &[u8]) -> Option<String> {
    use windows_sys::Win32::Globalization::{MultiByteToWideChar, MB_ERR_INVALID_CHARS};
    const CP_GBK: u32 = 936;
    if bytes.is_empty() {
        return Some(String::new());
    }
    // SAFETY: bytes 为非空只读缓冲区；两阶段调用（cchWideChar=0 先量所需
    // 长度，再实际转换）是 MultiByteToWideChar 的文档化用法。
    // MB_ERR_INVALID_CHARS 严格模式下非法序列返回 0，由上方判负返回 None。
    // len as i32 要求输入 < 2GiB——bat 文件恒满足该前提。
    unsafe {
        let need = MultiByteToWideChar(
            CP_GBK,
            MB_ERR_INVALID_CHARS,
            bytes.as_ptr(),
            bytes.len() as i32,
            std::ptr::null_mut(),
            0,
        );
        if need <= 0 {
            return None;
        }
        let mut buf = vec![0u16; need as usize];
        let written = MultiByteToWideChar(
            CP_GBK,
            MB_ERR_INVALID_CHARS,
            bytes.as_ptr(),
            bytes.len() as i32,
            buf.as_mut_ptr(),
            need,
        );
        if written <= 0 {
            return None;
        }
        buf.truncate(written as usize);
        String::from_utf16(&buf).ok()
    }
}

/// 值含 cmd 特殊字符时加双引号包裹（防止 `<` 被当作重定向、`&` 被当作命令分隔符等）。
///
/// bat/cmd 语境的三条转义规则（与 Windows CRT 参数解析不同）：
/// - `%` 在双引号内仍会展开 `%VAR%`，bat 内必须写成 `%%`；
/// - 双引号内的 `"` 须转义为 `\"`（写成 `""` 会被命令行解析吞掉引号）；
/// - 换行会直接截断 bat 行，剩余内容被当作下一条命令执行——替换为空格。
fn quote_value(v: &str) -> String {
    if v.is_empty() {
        return String::new();
    }
    // 先做 bat/CRT 语义转义，再决定是否加引号。
    // 反斜杠必须双写：值 `ab\` 若写成 `"ab\"`，CRT 会把 `\"` 读成字面引号，
    // 引号不闭合，其后所有参数（-maxgui/-command）都会被吞掉
    let v = v
        .replace('\\', "\\\\")
        .replace('%', "%%")
        .replace('"', "\\\"");
    let v = v.replace(['\r', '\n'], " ");
    const CMD_SPECIAL: &[char] = &[
        ' ', '^', '&', '<', '>', '|', '(', ')', ';', ',', '=', '!', '%',
    ];
    if v.chars().any(|c| CMD_SPECIAL.contains(&c)) || v.contains('\\') {
        format!("\"{v}\"")
    } else {
        v
    }
}

/// T002 标准：1 位 SAP 语言键 → 2 位 ISO 码（导入归一化，模型/表单统一用 2 位）
///
/// 依据 SAP T002 表（SPRAS ↔ LAISO，官方转换例程 CONVERSION_EXIT_ISOLA_*）：
/// 1=ZH 简体中文、E=EN 英文、D=DE、J=JA、M=ZF 繁体……未映射的值原样大写返回
///（兼容 2 位码直接出现在 bat 里的情况，如 -language=ZH）
fn lang_key_to_iso(v: &str) -> String {
    // 小写专有键先精确匹配（大写化会撞车：a=AF ≠ A=AR）
    match v {
        "a" => return "AF".to_string(),
        "b" => return "IS".to_string(),
        "c" => return "CA".to_string(),
        "d" => return "SH".to_string(),
        "i" => return "ID".to_string(),
        _ => {}
    }
    let up = v.to_ascii_uppercase();
    let mapped = match up.as_str() {
        "0" => "SR",
        "1" => "ZH",
        "2" => "TH",
        "3" => "KO",
        "4" => "RO",
        "5" => "SL",
        "6" => "HR",
        "7" => "MS",
        "8" => "UK",
        "9" => "ET",
        "A" => "AR",
        "B" => "HE",
        "C" => "CS",
        "D" => "DE",
        "E" => "EN",
        "F" => "FR",
        "G" => "EL",
        "H" => "HU",
        "I" => "IT",
        "J" => "JA",
        "K" => "DA",
        "L" => "PL",
        "M" => "ZF",
        "N" => "NL",
        "O" => "NO",
        "P" => "PT",
        "Q" => "SK",
        "R" => "RU",
        "S" => "ES",
        "T" => "TR",
        "U" => "FI",
        "V" => "SV",
        "W" => "BG",
        "X" => "LT",
        "Y" => "LV",
        "Z" => "Z1",
        _ => return up,
    };
    mapped.to_string()
}

/// 2 位 ISO 码 → 1 位 SAP 语言键（导出还原原生 sapshcut 格式：ZH→1、EN→E）
///
/// 未映射的值原样返回（含历史数据里已存的 1 位码，如 "1"/"E"，保持幂等）
fn iso_to_lang_key(v: &str) -> String {
    let mapped = match v {
        "ZH" => "1",
        "EN" => "E",
        "DE" => "D",
        "FR" => "F",
        "JA" => "J",
        "ZF" => "M",
        "ES" => "S",
        "RU" => "R",
        "DA" => "K",
        "IT" => "I",
        "NL" => "N",
        "PT" => "P",
        "TR" => "T",
        "EL" => "G",
        "HU" => "H",
        "PL" => "L",
        "NO" => "O",
        "SK" => "Q",
        "FI" => "U",
        "SV" => "V",
        "BG" => "W",
        "LT" => "X",
        "LV" => "Y",
        "SR" => "0",
        "TH" => "2",
        "KO" => "3",
        "RO" => "4",
        "SL" => "5",
        "HR" => "6",
        "MS" => "7",
        "UK" => "8",
        "ET" => "9",
        "AR" => "A",
        "HE" => "B",
        "CS" => "C",
        "AF" => "a",
        "IS" => "b",
        "CA" => "c",
        "SH" => "d",
        "ID" => "i",
        _ => return v.to_string(),
    };
    mapped.to_string()
}

/// 凭据 → 单行 sapshcut 命令（导出 BAT 用；password 为已解密的明文）
///
/// 格式与 SAP GUI 原生快捷方式一致：
/// `"完整路径\sapshcut.exe" -user=X -pw=Y -language=1 -SYSTEM=PRD -CLIENT=100
///  -hostname=h -serviceno=3200 -sysname=显示名 -maxgui`
pub fn credential_to_sapshcut(cred: &Credential, password: &str) -> String {
    // sapshcut 不一定在 PATH 中，写完整路径（含空格时引号包裹）
    let exe = detect_sapshcut_path();
    let mut parts: Vec<String> = vec![quote_value(&exe)];
    let mut push = |flag: &str, v: &str| {
        if !v.is_empty() {
            parts.push(format!("-{}={}", flag, quote_value(v)));
        }
    };
    // 参数顺序与原生快捷方式一致：user → pw → language → system → client → 服务器信息
    push("user", &cred.username);
    push("pw", password);
    // 语言用 1 位 SAP 键（T002 SPRAS，如 1=中文、E=英文），与原生快捷方式一致
    push("language", &iso_to_lang_key(&cred.language));
    push("system", &cred.system_id);
    push("client", &cred.client);
    if cred.connection_type == "load_balancing" {
        push("msghost", &cred.message_server);
        push("msservno", &cred.message_server_port);
        push("group", &cred.logon_group);
    } else {
        push("hostname", &cred.app_server);
        if !cred.system_number.is_empty() {
            push("serviceno", &format!("32{}", cred.system_number));
        }
    }
    push("saprouter", &cred.saprouter);
    if let Some(action) = cred.post_login_action.as_deref() {
        push("command", action);
    }
    // 连接名用 -sysname（与导入解析互逆：sysname ↔ connection_id）
    push("sysname", &cred.connection_id);
    // v4 Should #11：导入侧 parse_sapshcut_line 认 -name → display_name，
    // 导出侧必须带上，否则往返只能靠有损的 bat 文件名回退
    if let Some(name) = cred.display_name.as_deref() {
        push("name", name);
    }
    parts.push("-maxgui".to_string());
    parts.join(" ")
}

/// Windows 保留设备名（含带扩展名形式），任何目录都不可作文件名。
/// bat 导出与脚本库文件名净化共用，避免两处口径漂移（v4 Consider #20）。
pub(crate) const WINDOWS_RESERVED_NAMES: [&str; 22] = [
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
    "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

/// 凭据 → BAT 文件名（清理 Windows 非法字符、保留设备名与结尾点）
pub fn bat_file_name(cred: &Credential) -> String {
    let base = cred
        .display_name
        .clone()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| format!("{}-{}-{}", cred.system_id, cred.client, cred.username));
    let cleaned: String = base
        .chars()
        .map(|c| match c {
            '\\' | '/' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            // v4 Consider #20：控制字符（含 \n/\r）同样不可用于 Windows 文件名，
            // 否则 display_name 含换行会让整批 bat 导出在 fs::write 处中断
            c if c.is_control() => '_',
            _ => c,
        })
        .collect();
    let trimmed = cleaned.trim();
    let stem = trimmed.trim_end_matches('.');
    // Windows 对保留设备名的任意扩展名形式（CON.txt 等）都保留——
    // 按第一个点前的 token 判定，不能整串比对（与注释声明一致）
    let base = stem.split('.').next().unwrap_or(stem);
    let is_reserved = WINDOWS_RESERVED_NAMES
        .iter()
        .any(|r| base.eq_ignore_ascii_case(r));
    if trimmed.is_empty() || is_reserved {
        "connection.bat".to_string()
    } else {
        // 结尾的点/空格在 Windows 文件系统中会被静默丢弃或导致异常，统一剥掉
        format!("{}.bat", trimmed.trim_end_matches(['.', ' ']))
    }
}

/// 单条 bat 草稿与本机 SAP GUI Landscape 的匹配结果
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LandscapeMatch {
    /// 完全匹配（系统标识 + 服务器/实例 或 消息服务器/登录组 全部一致），含 Landscape 条目名
    Matched(String),
    /// 部分匹配（仅系统标识或仅服务器信息一致），含条目名与差异说明
    Partial(String, String),
    /// 未匹配到任何 Landscape 条目
    NotFound,
}

/// 将 bat 草稿与本机 SAP GUI Landscape 连接逐条比对
///
/// 匹配键（宽松容忍大小写差异）：
/// - 直连草稿：system_id + app_server + system_number
/// - 负载均衡草稿：system_id + message_server + logon_group
///
/// 仅 system_id 命中或仅服务器信息命中记为部分匹配（用于发现 bat 手写错误）。
pub fn match_landscape(d: &BatDraft, conns: &[SapConnection]) -> LandscapeMatch {
    let mut partial: Option<(String, String)> = None;
    let sid_upper = d.system_id.to_ascii_uppercase();

    for c in conns {
        let conn_sid = c.system_id.as_deref().unwrap_or("").to_ascii_uppercase();
        let sid_hit = !sid_upper.is_empty() && conn_sid == sid_upper;

        // 直连比对：host + 实例编号
        let conn_host = c.server.as_deref().unwrap_or("").to_ascii_lowercase();
        let host_hit = !d.app_server.is_empty()
            && conn_host == d.app_server.to_ascii_lowercase()
            && c.system_number.as_deref().unwrap_or("") == d.system_number;

        // 负载均衡比对：消息服务器 + 登录组
        let conn_ms = c
            .message_server
            .as_deref()
            .unwrap_or("")
            .to_ascii_lowercase();
        let lb_hit = !d.message_server.is_empty()
            && conn_ms == d.message_server.to_ascii_lowercase()
            && c.logon_group.as_deref().unwrap_or("") == d.logon_group;

        if sid_hit && (host_hit || lb_hit) {
            return LandscapeMatch::Matched(c.name.clone());
        }
        if (sid_hit || host_hit || lb_hit) && partial.is_none() {
            let why = if sid_hit {
                "系统标识一致，但服务器/实例不一致"
            } else {
                "服务器一致，但系统标识不一致"
            };
            partial = Some((c.name.clone(), why.to_string()));
        }
    }

    match partial {
        Some((name, reason)) => LandscapeMatch::Partial(name, reason),
        None => LandscapeMatch::NotFound,
    }
}

/// 纯 SID 引用型草稿（bat 未含服务器地址）从本机 Landscape 反查补全
///
/// 按 system_id 匹配第一条 Landscape 条目，回填连接类型与服务器信息，
/// 并移除「未指定服务器地址」警告。返回 false 表示本机配置中也找不到，
/// 调用方可据此标记该条无法导入。
pub fn resolve_from_landscape(d: &mut BatDraft, conns: &[SapConnection]) -> bool {
    if !d.connection_type.is_empty() {
        return true; // 已有连接信息，无需补全
    }
    let sid_upper = d.system_id.to_ascii_uppercase();
    for c in conns {
        if c.system_id.as_deref().unwrap_or("").to_ascii_uppercase() != sid_upper {
            continue;
        }
        // 直连优先（Landscape 条目有 host + 实例编号）
        if let Some(srv) = c.server.as_deref().filter(|s| !s.is_empty()) {
            d.connection_type = "direct".to_string();
            d.app_server = srv.to_string();
            d.system_number = c.system_number.clone().unwrap_or_default();
            d.warnings.retain(|w| !w.contains("未指定服务器地址"));
            return true;
        }
        // 负载均衡（消息服务器 + 登录组）
        if let Some(ms) = c.message_server.as_deref().filter(|s| !s.is_empty()) {
            d.connection_type = "load_balancing".to_string();
            d.message_server = ms.to_string();
            d.message_server_port = c.message_server_port.clone().unwrap_or_default();
            d.logon_group = c.logon_group.clone().unwrap_or_default();
            d.warnings.retain(|w| !w.contains("未指定服务器地址"));
            return true;
        }
    }
    false
}

/// 探测本机 sapshcut.exe 完整路径（找不到则返回裸命令名，依赖 PATH）
///
/// 探测顺序：常见安装路径 → App Paths 注册表（仅 Windows）。
pub fn detect_sapshcut_path() -> String {
    // 实测样例：C:\Program Files\SAP\FrontEnd\SapGui\sapshcut.exe
    const CANDIDATES: &[&str] = &[
        r"C:\Program Files\SAP\FrontEnd\SapGui\sapshcut.exe",
        r"C:\Program Files\SAP\FrontEnd\SAPGUI\sapshcut.exe",
        r"C:\Program Files (x86)\SAP\FrontEnd\SAPGUI\sapshcut.exe",
        r"C:\Program Files (x86)\SAP\FrontEnd\SapGui\sapshcut.exe",
    ];
    for c in CANDIDATES {
        if std::path::Path::new(c).exists() {
            return c.to_string();
        }
    }
    #[cfg(target_os = "windows")]
    {
        use winreg::enums::*;
        use winreg::RegKey;
        if let Ok(key) = RegKey::predef(HKEY_LOCAL_MACHINE)
            .open_subkey(r"SOFTWARE\Microsoft\Windows\CurrentVersion\App Paths\sapshcut.exe")
        {
            if let Ok(path) = key.get_value::<String, _>("") {
                if std::path::Path::new(&path).exists() {
                    return path;
                }
            }
        }
    }
    "sapshcut.exe".to_string()
}

/// 将 bat 内容编码为字节（Windows 下转 GBK/ANSI，避免 cmd 按本地代码页读取时中文乱码）。
///
/// v5 Should #7：GBK 不可映射字符（emoji / 特殊符号）过去被静默 best-fit
/// 成 `?`，导出仍报成功但 `-pw` / `-user` 已被改写。现在返回 Err 中止整批，
/// 绝不把 UTF-8 字节当 GBK 写出去（那会让 sapshcut 读出乱码）。
pub fn encode_bat_bytes(content: &str) -> Result<Vec<u8>, String> {
    #[cfg(target_os = "windows")]
    {
        encode_gbk(content).ok_or_else(|| {
            "内容含 GBK 无法表示的字符，已中止导出以免密码/用户名被静默改写；请改用 ASCII 可表示的值或用记事本另存为 ANSI".to_string()
        })
    }
    #[cfg(not(target_os = "windows"))]
    {
        Ok(content.as_bytes().to_vec())
    }
}

/// UTF-16 → GBK（代码页 936）编码，含不可映射字符时返回 None。
///
/// v5 Should #7：与 `sap_automation::utf8_to_ansi_bytes` 同口径 ——
/// `WC_NO_BEST_FIT_CHARS` + `lpUsedDefaultChar` 两阶段探测；`flags=0` 时
/// WideCharToMultiByte 会把 emoji 静默换成 `?` / best-fit 字符而不报错。
#[cfg(target_os = "windows")]
fn encode_gbk(s: &str) -> Option<Vec<u8>> {
    use windows_sys::Win32::Globalization::{WideCharToMultiByte, WC_NO_BEST_FIT_CHARS};
    const CP_GBK: u32 = 936;
    let wide: Vec<u16> = s.encode_utf16().collect();
    if wide.is_empty() {
        return Some(Vec::new());
    }
    // SAFETY: wide 为非空合法 UTF-16 缓冲区；两阶段调用（cbMultiByte=0 先
    // 量所需长度再实际转换）是 WideCharToMultiByte 的文档化用法。
    // len as i32 要求输入 < 2GiB——bat 内容恒满足该前提。
    unsafe {
        let mut used_default: i32 = 0;
        let need = WideCharToMultiByte(
            CP_GBK,
            WC_NO_BEST_FIT_CHARS,
            wide.as_ptr(),
            wide.len() as i32,
            std::ptr::null_mut(),
            0,
            std::ptr::null(),
            &mut used_default,
        );
        if need <= 0 || used_default != 0 {
            return None;
        }
        let mut buf = vec![0u8; need as usize];
        let written = WideCharToMultiByte(
            CP_GBK,
            WC_NO_BEST_FIT_CHARS,
            wide.as_ptr(),
            wide.len() as i32,
            buf.as_mut_ptr(),
            need,
            std::ptr::null(),
            &mut used_default,
        );
        if written <= 0 || used_default != 0 {
            return None;
        }
        buf.truncate(written as usize);
        Some(buf)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_direct_with_equals() {
        let drafts = parse_bat("sapshcut.exe -system=PRD -client=100 -user=ZHANG -pw=Secret1 -language=ZH -hostname=erp.example.com -serviceno=3200");
        assert_eq!(drafts.len(), 1);
        let d = &drafts[0];
        assert!(d.is_valid(), "errors: {:?}", d.errors);
        assert_eq!(d.system_id, "PRD");
        assert_eq!(d.client, "100");
        assert_eq!(d.username, "ZHANG");
        assert_eq!(d.password, "Secret1");
        assert_eq!(d.language, "ZH");
        assert_eq!(d.connection_type, "direct");
        assert_eq!(d.app_server, "erp.example.com");
        assert_eq!(d.system_number, "00");
    }

    #[test]
    fn test_space_form_and_quotes() {
        let drafts = parse_bat(
            r#"start "" sapshcut -system QAS -client 200 -user "LI SI" -pw "my pass word" -hostname 198.51.100.4 -systemnumber 01"#,
        );
        assert_eq!(drafts.len(), 1);
        let d = &drafts[0];
        assert!(d.is_valid(), "errors: {:?}", d.errors);
        assert_eq!(d.system_id, "QAS");
        assert_eq!(d.client, "200");
        assert_eq!(d.username, "LI SI");
        assert_eq!(d.password, "my pass word");
        assert_eq!(d.system_number, "01");
    }

    #[test]
    fn test_load_balancing() {
        let drafts = parse_bat(
            "sapshcut -system=PRD -client=100 -user=A -msghost=ms.sap.local -msservno=3600 -group=EASP",
        );
        assert_eq!(drafts.len(), 1);
        let d = &drafts[0];
        assert!(d.is_valid(), "errors: {:?}", d.errors);
        assert_eq!(d.connection_type, "load_balancing");
        assert_eq!(d.message_server, "ms.sap.local");
        assert_eq!(d.message_server_port, "00");
        assert_eq!(d.logon_group, "EASP");
    }

    #[test]
    fn test_missing_required_fields() {
        let drafts = parse_bat("sapshcut -system=PRD -client=100");
        assert_eq!(drafts.len(), 1);
        let d = &drafts[0];
        assert!(!d.is_valid());
        assert!(d.errors.iter().any(|e| e.contains("-user")));
        // 无服务器不再是致命错误（纯 SID 引用型可由本机配置补全），应为警告
        assert!(!d.errors.iter().any(|e| e.contains("-hostname")));
        assert!(d.warnings.iter().any(|w| w.contains("未指定服务器地址")));
    }

    #[test]
    fn test_skips_boilerplate_and_comments() {
        let content = "\
@echo off\r\n\
rem 登录生产系统\r\n\
echo Starting...\r\n\
sapshcut -system=PRD -client=100 -user=A -hostname=h1 -systemnumber=00\r\n\
:: 注释\r\n\
\r\n\
sapshcut -system=QAS -client=200 -user=B -msghost=ms -group=TEST\r\n";
        let drafts = parse_bat(content);
        assert_eq!(drafts.len(), 2);
        assert_eq!(drafts[0].system_id, "PRD");
        assert_eq!(drafts[0].line_no, 4);
        assert_eq!(drafts[1].system_id, "QAS");
        assert_eq!(drafts[1].line_no, 7);
    }

    #[test]
    fn test_unknown_flag_warns() {
        let drafts = parse_bat(
            "sapshcut -system=PRD -client=100 -user=A -hostname=h1 -systemnumber=00 -foo=bar",
        );
        assert!(drafts[0].is_valid());
        assert!(drafts[0].warnings.iter().any(|w| w.contains("-foo")));
    }

    #[test]
    fn test_ignored_flags_silent() {
        let drafts = parse_bat(
            "sapshcut -system=PRD -client=100 -user=A -hostname=h1 -systemnumber=00 -max -gui -maxgui",
        );
        assert!(drafts[0].is_valid());
        assert!(
            drafts[0].warnings.is_empty(),
            "warnings: {:?}",
            drafts[0].warnings
        );
    }

    #[test]
    fn test_native_shortcut_format() {
        // SAP GUI 原生快捷方式格式（用户实测样例）：完整路径 + 大写 flag + sysname + maxgui
        let drafts = parse_bat(
            r#""C:\Program Files\SAP\FrontEnd\SapGui\sapshcut.exe" -user=DS4_ADMIN -pw="v~2fJD5F$qZsgn" -language=E -SYSTEM=DS4 -CLIENT=200 -sysname=RS测试系统 -maxgui"#,
        );
        assert_eq!(drafts.len(), 1);
        let d = &drafts[0];
        assert!(
            d.is_valid(),
            "原生快捷方式应可直接导入，errors: {:?}",
            d.errors
        );
        assert_eq!(d.system_id, "DS4");
        assert_eq!(d.client, "200");
        assert_eq!(d.username, "DS4_ADMIN");
        assert_eq!(d.password, "v~2fJD5F$qZsgn");
        // 1 位 SAP 语言键归一化为 2 位 ISO（E → EN，模型/表单统一 2 位）
        assert_eq!(d.language, "EN");
        // sysname → 连接名（display_name 仅来自 -name）
        assert_eq!(d.connection_name, "RS测试系统");
        assert!(d.display_name.is_empty());
        // 纯 SID 引用型：无服务器地址 → 警告而非错误
        assert!(d.warnings.iter().any(|w| w.contains("未指定服务器地址")));
        assert!(d.connection_type.is_empty());
    }

    #[test]
    fn test_resolve_from_landscape() {
        let mut d = BatDraft {
            system_id: "DS4".to_string(),
            client: "200".to_string(),
            username: "A".to_string(),
            warnings: vec![
                "未指定服务器地址（-hostname/-msghost），将尝试从本机 SAP GUI 配置按系统标识补全"
                    .to_string(),
            ],
            ..Default::default()
        };
        let conns = vec![lc(
            "测试",
            Some("DS4"),
            Some("erp.local"),
            Some("00"),
            None,
            None,
        )];
        assert!(bat_resolve(&mut d, &conns));
        assert_eq!(d.connection_type, "direct");
        assert_eq!(d.app_server, "erp.local");
        assert_eq!(d.system_number, "00");
        assert!(d.warnings.is_empty());

        // 本机配置中无该系统 → 失败
        let mut d2 = BatDraft {
            system_id: "XXX".to_string(),
            ..Default::default()
        };
        assert!(!bat_resolve(&mut d2, &conns));
    }

    /// 绕过私有边界的薄封装（测试用）
    fn bat_resolve(d: &mut BatDraft, conns: &[SapConnection]) -> bool {
        resolve_from_landscape(d, conns)
    }

    #[test]
    fn test_sapshcut_path_quoted() {
        // 完整路径含空格时应被引号包裹；连接名写入 -sysname
        let cred = Credential {
            connection_id: "DS4-100-ST ABAP1".to_string(),
            system_id: "PRD".to_string(),
            client: "100".to_string(),
            username: "A".to_string(),
            connection_type: "direct".to_string(),
            app_server: "h1".to_string(),
            system_number: "00".to_string(),
            ..Default::default()
        };
        let line = credential_to_sapshcut(&cred, "pw1");
        assert!(line.starts_with('"'), "exe 路径应加引号: {line}");
        assert!(line.contains("-maxgui"));
        assert!(line.contains("-sysname="));
        // 含空格的连接名应加引号
        assert!(line.contains("-sysname=\"DS4-100-ST ABAP1\""));
    }

    #[test]
    fn test_quote_value_special_chars() {
        // 含 cmd 特殊字符（<、&、=）的值应加引号
        assert_eq!(quote_value("a<b"), "\"a<b\"");
        assert_eq!(quote_value("a&b"), "\"a&b\"");
        assert_eq!(quote_value("a=b"), "\"a=b\"");
        assert_eq!(quote_value("plain"), "plain");
    }

    /// 值尾反斜杠必须双写：否则 CRT 把 `"ab\"` 的 `\"` 读成字面引号，
    /// 引号不闭合会吞掉其后所有参数
    #[test]
    fn test_quote_value_doubles_trailing_backslash() {
        assert_eq!(quote_value("ab\\"), "\"ab\\\\\"");
    }

    /// 解析侧必须逆转导出侧的 \\ / \" / %% 转义
    #[test]
    fn test_tokenize_and_expand_reverse_export_escapes() {
        assert_eq!(
            tokenize(r#"-pw="ab\\\"cd" -maxgui"#),
            vec!["-pw=ab\\\"cd", "-maxgui"]
        );
        assert_eq!(expand_env_vars("a%%b"), "a%b");
        // 未定义的环境变量仍原样保留
        assert_eq!(
            expand_env_vars("%SAP_LOGIN_MANAGER_UNDEFINED_VAR_XYZ%"),
            "%SAP_LOGIN_MANAGER_UNDEFINED_VAR_XYZ%"
        );
    }

    /// 导出 → 导入往返：% / \ / " / 空格 / & 全部无损
    #[test]
    fn test_sapshcut_roundtrip_with_special_chars() {
        let cred = Credential {
            system_id: "PRD".to_string(),
            client: "100".to_string(),
            username: "ZHANG".to_string(),
            language: "ZH".to_string(),
            connection_type: "direct".to_string(),
            app_server: "erp.example.com".to_string(),
            system_number: "00".to_string(),
            ..Default::default()
        };
        for pw in ["a%b", "ab\\", "a\"b", "a b\\\"c", "p@ss&word", "100%%\\\""] {
            let line = credential_to_sapshcut(&cred, pw);
            let drafts = parse_bat(&line);
            assert_eq!(drafts.len(), 1, "line: {line}");
            assert_eq!(drafts[0].password, pw, "往返丢失字符: {line}");
        }
    }

    #[test]
    fn test_saprouter() {
        let drafts = parse_bat(
            "sapshcut -system=PRD -client=100 -user=A -hostname=h1 -systemnumber=00 -saprouter=/H/proxy/H/",
        );
        assert_eq!(drafts[0].saprouter, "/H/proxy/H/");
    }

    #[test]
    fn test_client_warning() {
        let drafts =
            parse_bat("sapshcut -system=PRD -client=9999 -user=A -hostname=h1 -systemnumber=00");
        assert!(drafts[0].is_valid());
        assert!(drafts[0].warnings.iter().any(|w| w.contains("-client")));
    }

    #[test]
    fn test_to_sapshcut_roundtrip() {
        let cred = Credential {
            system_id: "PRD".to_string(),
            client: "100".to_string(),
            username: "ZHANG".to_string(),
            language: "ZH".to_string(),
            connection_type: "direct".to_string(),
            app_server: "erp.example.com".to_string(),
            system_number: "00".to_string(),
            ..Default::default()
        };
        let line = credential_to_sapshcut(&cred, "Secret1");
        // 导出用 1 位 SAP 语言键（ZH → 1，与原生快捷方式一致）
        assert!(line.contains("-language=1"), "line: {line}");
        let drafts = parse_bat(&line);
        assert_eq!(drafts.len(), 1);
        let d = &drafts[0];
        assert!(d.is_valid(), "errors: {:?}", d.errors);
        assert_eq!(d.system_id, "PRD");
        assert_eq!(d.system_number, "00");
        assert_eq!(d.password, "Secret1");
        // 导入归一化回 2 位：1 → ZH（往返一致）
        assert_eq!(d.language, "ZH");
    }

    #[test]
    fn test_language_key_mapping() {
        // 导入归一化：1 位 T002 键 → 2 位 ISO
        let drafts = parse_bat(
            "sapshcut -system=PRD -client=100 -user=A -hostname=h1 -systemnumber=00 -language=1",
        );
        assert_eq!(drafts[0].language, "ZH");
        let drafts = parse_bat(
            "sapshcut -system=PRD -client=100 -user=A -hostname=h1 -systemnumber=00 -language=E",
        );
        assert_eq!(drafts[0].language, "EN");
        let drafts = parse_bat(
            "sapshcut -system=PRD -client=100 -user=A -hostname=h1 -systemnumber=00 -language=J",
        );
        assert_eq!(drafts[0].language, "JA");
        // 2 位码直接出现在 bat 里：原样保留
        let drafts = parse_bat(
            "sapshcut -system=PRD -client=100 -user=A -hostname=h1 -systemnumber=00 -language=ZH",
        );
        assert_eq!(drafts[0].language, "ZH");
        // 导出还原：EN → E、JA → J；未映射的 1 位码（历史数据）幂等原样
        assert_eq!(iso_to_lang_key("EN"), "E");
        assert_eq!(iso_to_lang_key("JA"), "J");
        assert_eq!(iso_to_lang_key("1"), "1");
        assert_eq!(iso_to_lang_key(""), "");
        // 导出 JA 凭据 → -language=J
        let cred = Credential {
            system_id: "PRD".to_string(),
            client: "100".to_string(),
            username: "A".to_string(),
            language: "JA".to_string(),
            connection_type: "direct".to_string(),
            app_server: "h1".to_string(),
            system_number: "00".to_string(),
            ..Default::default()
        };
        let line = credential_to_sapshcut(&cred, "pw");
        assert!(line.contains("-language=J"), "line: {line}");
    }

    #[test]
    fn test_to_sapshcut_lb() {
        let cred = Credential {
            system_id: "PRD".to_string(),
            client: "100".to_string(),
            username: "A".to_string(),
            connection_type: "load_balancing".to_string(),
            message_server: "ms.sap.local".to_string(),
            message_server_port: "00".to_string(),
            logon_group: "EASP".to_string(),
            ..Default::default()
        };
        let line = credential_to_sapshcut(&cred, "");
        assert!(line.contains("-msghost=ms.sap.local"));
        assert!(line.contains("-msservno=00"));
        assert!(line.contains("-group=EASP"));
        assert!(!line.contains("-pw="));
    }

    #[test]
    fn test_bat_file_name_sanitizes() {
        let named = Credential {
            display_name: Some("生产/PRD*100".to_string()),
            ..Default::default()
        };
        assert_eq!(bat_file_name(&named), "生产_PRD_100.bat");
        // display_name 为空时回退 系统-客户端-用户名（含空格保留）
        let fallback = Credential {
            system_id: "PRD".to_string(),
            client: "100".to_string(),
            username: "A B".to_string(),
            ..Default::default()
        };
        assert_eq!(bat_file_name(&fallback), "PRD-100-A B.bat");
    }

    // ===== Landscape 比对测试 =====

    /// 构造 Landscape 连接条目（无 Default，需全字段）
    fn lc(
        name: &str,
        system_id: Option<&str>,
        server: Option<&str>,
        system_number: Option<&str>,
        message_server: Option<&str>,
        logon_group: Option<&str>,
    ) -> SapConnection {
        SapConnection {
            name: name.to_string(),
            description: None,
            server: server.map(|s| s.to_string()),
            system_number: system_number.map(|s| s.to_string()),
            system_id: system_id.map(|s| s.to_string()),
            group: logon_group.map(|s| s.to_string()),
            service: None,
            connection_type: None,
            workspace_name: None,
            saprouter: None,
            message_server: message_server.map(|s| s.to_string()),
            message_server_port: None,
            logon_group: logon_group.map(|s| s.to_string()),
            uuid: None,
            sncop: None,
            snc_name: None,
        }
    }

    #[test]
    fn test_landscape_matched_direct() {
        let conns = vec![lc(
            "生产-ERP",
            Some("PRD"),
            Some("erp.example.com"),
            Some("00"),
            None,
            None,
        )];
        let drafts = parse_bat(
            "sapshcut -system=PRD -client=100 -user=A -hostname=ERP.EXAMPLE.COM -systemnumber=00",
        );
        assert_eq!(
            match_landscape(&drafts[0], &conns),
            LandscapeMatch::Matched("生产-ERP".to_string())
        );
    }

    #[test]
    fn test_landscape_matched_load_balancing() {
        let conns = vec![lc(
            "生产-组",
            Some("PRD"),
            Some("EASP"),
            None,
            Some("ms.sap.local"),
            Some("EASP"),
        )];
        let drafts =
            parse_bat("sapshcut -system=PRD -client=100 -user=A -msghost=ms.sap.local -group=EASP");
        assert_eq!(
            match_landscape(&drafts[0], &conns),
            LandscapeMatch::Matched("生产-组".to_string())
        );
    }

    #[test]
    fn test_landscape_partial_sid_only() {
        let conns = vec![lc(
            "生产-ERP",
            Some("PRD"),
            Some("erp.example.com"),
            Some("00"),
            None,
            None,
        )];
        // system_id 一致但实例号不同 → 部分匹配（发现 bat 手写错误）
        let drafts = parse_bat(
            "sapshcut -system=PRD -client=100 -user=A -hostname=erp.example.com -systemnumber=01",
        );
        match match_landscape(&drafts[0], &conns) {
            LandscapeMatch::Partial(name, reason) => {
                assert_eq!(name, "生产-ERP");
                assert!(reason.contains("服务器/实例"));
            }
            other => panic!("expected Partial, got {other:?}"),
        }
    }

    #[test]
    fn test_landscape_partial_host_only() {
        let conns = vec![lc(
            "生产-ERP",
            Some("PRD"),
            Some("erp.example.com"),
            Some("00"),
            None,
            None,
        )];
        // host 一致但 system_id 不同 → 部分匹配
        let drafts = parse_bat(
            "sapshcut -system=QAS -client=100 -user=A -hostname=erp.example.com -systemnumber=00",
        );
        match match_landscape(&drafts[0], &conns) {
            LandscapeMatch::Partial(name, reason) => {
                assert_eq!(name, "生产-ERP");
                assert!(reason.contains("系统标识"));
            }
            other => panic!("expected Partial, got {other:?}"),
        }
    }

    #[test]
    fn test_landscape_not_found() {
        let conns = vec![lc(
            "生产-ERP",
            Some("PRD"),
            Some("erp.example.com"),
            Some("00"),
            None,
            None,
        )];
        let drafts = parse_bat(
            "sapshcut -system=DEV -client=100 -user=A -hostname=dev.local -systemnumber=00",
        );
        assert_eq!(
            match_landscape(&drafts[0], &conns),
            LandscapeMatch::NotFound
        );
        // 空列表
        assert_eq!(match_landscape(&drafts[0], &[]), LandscapeMatch::NotFound);
    }

    #[test]
    fn test_landscape_priority_strict_over_partial() {
        // 第一条部分匹配、第二条完全匹配 → 应返回完全匹配
        let conns = vec![
            lc(
                "旧-ERP",
                Some("PRD"),
                Some("old.example.com"),
                Some("00"),
                None,
                None,
            ),
            lc(
                "新-ERP",
                Some("PRD"),
                Some("erp.example.com"),
                Some("00"),
                None,
                None,
            ),
        ];
        let drafts = parse_bat(
            "sapshcut -system=PRD -client=100 -user=A -hostname=erp.example.com -systemnumber=00",
        );
        assert_eq!(
            match_landscape(&drafts[0], &conns),
            LandscapeMatch::Matched("新-ERP".to_string())
        );
    }

    /// 回归 v4 Should #11：-name 必须参与导出，往返不丢显示名
    #[test]
    fn sapshcut_roundtrip_preserves_display_name() {
        let cred = Credential {
            system_id: "PRD".to_string(),
            client: "100".to_string(),
            username: "ZHANG".to_string(),
            display_name: Some("生产系统".to_string()),
            ..Default::default()
        };
        let line = credential_to_sapshcut(&cred, "secret");
        let drafts = parse_bat(&line);
        assert_eq!(drafts.len(), 1, "line: {line}");
        assert_eq!(
            drafts[0].display_name, "生产系统",
            "往返丢失 display_name: {line}"
        );
    }

    /// 回归 v4 Consider #20：文件名净化必须过滤控制字符
    #[test]
    fn bat_file_name_replaces_control_characters() {
        let cred = Credential {
            display_name: Some("a\nb\u{0001}c".to_string()),
            ..Default::default()
        };
        let name = bat_file_name(&cred);
        assert!(name.ends_with(".bat"), "{name:?}");
        assert!(!name.chars().any(|c| c.is_control()), "{name:?}");
    }

    /// v5 Consider #22：UTF-16BE BOM 必须被识别，不能被当成 GBK 乱码。
    #[test]
    fn read_bat_file_decodes_utf16be_bom() {
        use std::io::Write as _;
        let dir = std::env::temp_dir().join(format!(
            "slm-bat-utf16be-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_millis()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("be.bat");
        let mut f = std::fs::File::create(&path).unwrap();
        f.write_all(&[0xFE, 0xFF]).unwrap();
        for u in "sapshcut.exe -system=PRD".encode_utf16() {
            f.write_all(&u.to_be_bytes()).unwrap();
        }
        drop(f);
        let text = read_bat_file(&path).unwrap();
        assert_eq!(text, "sapshcut.exe -system=PRD");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// v5 Consider #22：奇数尾字节 / 非法代理对必须返回 Err，不再静默丢字节。
    #[test]
    fn utf16_decode_rejects_truncated_and_invalid_data() {
        assert!(utf16_decode(&[0x41, 0x00, 0x42], false).is_err());
        // 孤立高代理项
        assert!(utf16_decode(&[0x00, 0xD8], false).is_err());
        assert_eq!(utf16_decode(&[0x41, 0x00], false).unwrap(), "A");
        assert_eq!(utf16_decode(&[0x00, 0x41], true).unwrap(), "A");
    }

    /// v5 Should #7：GBK 不可映射字符必须中止导出，不能静默 best-fit 成 '?'。
    #[test]
    #[cfg(target_os = "windows")]
    fn encode_bat_bytes_rejects_unmappable_characters() {
        assert!(encode_bat_bytes("sapshcut.exe -user=张三").is_ok());
        let res = encode_bat_bytes("sapshcut.exe -pw=abc😀");
        assert!(res.is_err(), "emoji 必须被拒绝，实际: {res:?}");
    }
}
