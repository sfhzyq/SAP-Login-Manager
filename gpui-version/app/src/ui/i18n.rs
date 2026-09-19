//! 界面多语言（中文 / English）
//!
//! 设计：以中文原文为 key。
//! - `t("创建凭据")`：中文模式原样返回；英文模式查表翻译，查不到则回落中文。
//! - `tf("已选 {count}", &[&count.to_string()])`：带占位符模板，
//!   按占位符出现顺序依序填充。
//! - 语言状态存 `AtomicU8`（0=zh 1=en），启动时从设置载入，
//!   切换后调用 `window.refresh()` 全窗口重绘。

use std::sync::atomic::{AtomicU8, Ordering};

static LANG: AtomicU8 = AtomicU8::new(0);

/// 设置界面语言（"zh" / "en"），供启动载入与设置页切换调用
pub fn set_lang(code: &str) {
    LANG.store(if code == "en" { 1 } else { 0 }, Ordering::Relaxed);
}

/// 当前是否英文界面
pub fn is_en() -> bool {
    LANG.load(Ordering::Relaxed) == 1
}

/// 翻译静态文案（key 为中文原文）
pub fn t(key: &'static str) -> &'static str {
    if !is_en() {
        return key;
    }
    en_lookup(key)
}

/// 翻译带占位符模板：模板中 `{...}` 占位符按出现顺序用 args 依序填充
pub fn tf(key: &'static str, args: &[&str]) -> String {
    let s = t(key).to_string();
    let mut idx = 0;
    let mut out = String::with_capacity(s.len() + 16);
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'{' {
            if let Some(end) = s[i..].find('}') {
                // 跳过转义 {{ }}
                if i + 1 < bytes.len() && bytes[i + 1] == b'{' {
                    out.push('{');
                    i += 2;
                    continue;
                }
                if idx < args.len() {
                    out.push_str(args[idx]);
                }
                idx += 1;
                i += end + 1;
                continue;
            }
        }
        // 多字节 UTF-8 安全推进
        let ch_len = utf8_len(bytes[i]);
        out.push_str(&s[i..i + ch_len]);
        i += ch_len;
    }
    out
}

fn utf8_len(b: u8) -> usize {
    match b {
        0x00..=0x7F => 1,
        0xC0..=0xDF => 2,
        0xE0..=0xEF => 3,
        _ => 4,
    }
}

/// 英文翻译表：中文原文 → 英文
fn en_lookup(key: &'static str) -> &'static str {
    match key {
        // === 通用 ===
        "取消" => "Cancel",
        "保存" => "Save",
        "创建" => "Create",
        "删除" => "Delete",
        "返回" => "Back",
        "默认" => "Default",
        "全部" => "All",
        "收藏" => "Favorites",
        "设置" => "Settings",
        "关于" => "About",
        "重命名" => "Rename",
        "新建" => "New",
        "管理" => "Manage",
        "关闭" => "Close",
        "确定" => "OK",
        "确认" => "Confirm",

        // === 标题栏 ===
        "锁定 (Ctrl+L)" => "Lock (Ctrl+L)",
        "窗口置顶" => "Always on Top",
        "取消置顶" => "Unpin from Top",
        "最小化" => "Minimize",
        "最大化 / 还原" => "Maximize / Restore",

        // === 活动栏 ===
        "新增凭据 (Ctrl+N)" => "New Credential (Ctrl+N)",
        "从 SAP 配置导入" => "Import from SAP Config",
        "导出凭据" => "Export Credentials",
        "命令面板 (Ctrl+K)" => "Command Palette (Ctrl+K)",

        // === 面板标题 ===
        "新增凭据" => "New Credential",
        "编辑凭据" => "Edit Credential",
        "分组管理" => "Group Manager",
        "命令面板" => "Command Palette",
        "返回主界面" => "Back to Main",

        // === 主界面 ===
        "分组筛选" => "Group Filter",
        "系统 / 默认分组" => "System / Default Groups",
        "自定义分组" => "Custom Groups",
        "（暂无自定义分组）" => "(No custom groups)",
        "管理分组…" => "Manage Groups…",
        "排序方式" => "Sort By",
        "默认（置顶恒定最前）" => "Default (pinned first)",
        "常用" => "Frequent",
        "最近登录" => "Recent",
        "名称" => "Name",
        "排序（置顶恒定最前）" => "Sort (pinned always first)",
        "搜索（名称 / 用户名 / 服务器）" => "Search (name / user / server)",

        // === 凭据卡片 ===
        "复制用户名" => "Copy Username",
        "复制密码" => "Copy Password",
        "已复制" => "Copied",
        "登录" => "Logon",
        "编辑" => "Edit",
        "置顶" => "Pin",
        "取消置顶（凭据）" => "Unpin",
        "加入收藏" => "Favorite",
        "取消收藏" => "Unfavorite",

        // === 表单页签与字段 ===
        "连接" => "Connection",
        "凭据" => "Credential",
        "连接名" => "Connection Name",
        "系统标识 (SID)" => "System ID (SID)",
        "应用服务器" => "Application Server",
        "实例编号" => "Instance Number",
        "消息服务器" => "Message Server",
        "系统编号" => "System Number",
        "登录组" => "Logon Group",
        "客户端" => "Client",
        "用户名" => "Username",
        "密码" => "Password",
        "留空保持不变" => "Leave blank to keep unchanged",
        "登录语言" => "Logon Language",
        "环境" => "Environment",
        "生产环境" => "Production",
        "测试环境" => "Test",
        "开发环境" => "Development",
        "配置环境" => "Configuration",
        "正式" => "Prod",
        "测试" => "Test",
        "开发" => "Dev",
        "配置" => "Config",
        "未分类" => "Uncategorized",
        "直连" => "Direct",
        "负载均衡" => "Load Balancing",
        "SNC" => "SNC",
        "启用 SNC" => "Enable SNC",
        "SNC 名称" => "SNC Name",
        "保护质量（QOP）" => "Protection Quality (QOP)",
        "认证" => "Authentication",
        "完整性" => "Integrity",
        "隐私" => "Privacy",
        "最大" => "Maximum",
        "SSO 免密登录" => "SSO Password-free Logon",
        "中文" => "Chinese",
        "英文" => "English",
        "日文" => "Japanese",
        "保存修改" => "Save Changes",
        "创建凭据" => "Create Credential",

        // === 分组管理 ===
        "系统分组" => "System Groups",
        "暂无自定义分组" => "No custom groups yet",
        "新建分组" => "New Group",
        "重命名分组" => "Rename Group",
        "删除分组" => "Delete Group",
        "设为默认分组" => "Set as Default",
        "取消默认（启动时打开全部分组）" => "Unset default (open All on start)",
        "项" => "items",

        // === 设置 ===
        "外观" => "Appearance",
        "主题" => "Theme",
        "浅色" => "Light",
        "深色" => "Dark",
        "跟随系统" => "System",
        "界面语言" => "UI Language",
        "默认登录语言" => "Default Logon Language",
        "凭据默认语言" => "Default Credential Language",
        "安全" => "Security",
        "修改主密码" => "Change Master Password",
        "当前主密码" => "Current Master Password",
        "新主密码（至少 6 位）" => "New Master Password (min 6 chars)",
        "再次输入新主密码" => "Re-enter New Master Password",
        "确认修改" => "Confirm",
        "自动锁定" => "Auto Lock",
        "分钟" => "min",
        "免密模式" => "Password-free Mode",
        "数据" => "Data",
        "SAP Logon 路径" => "SAP Logon Path",
        "批量登录间隔" => "Batch Logon Interval",
        "秒" => "s",
        "剪贴板自动清空" => "Auto-clear Clipboard",
        "列表" => "List",

        // === 提示与通知 ===
        "请输入分组名称" => "Please enter a group name",
        "分组已创建" => "Group created",
        "分组已重命名" => "Group renamed",
        "请填写完整" => "Please fill in all fields",
        "请输入当前主密码" => "Please enter the current master password",
        "新主密码至少 6 位" => "New password must be at least 6 characters",
        "两次输入的新密码不一致" => "Passwords do not match",
        "主密码已修改，所有凭据已重新加密" => "Master password changed; all credentials re-encrypted",
        "凭据已保存" => "Credential saved",
        "请输入主密码" => "Please enter the master password",
        "主密码错误" => "Incorrect master password",
        "解锁" => "Unlock",
        "主密码" => "Master Password",
        "确认密码" => "Confirm Password",
        "设置主密码" => "Set Master Password",

        // === 导入 / 导出 ===
        "读取本机 SAP Logon 连接，按 Workspace 自动分组。导入后请编辑填写账户与密码。" => {
            "Read local SAP Logon connections and group them automatically. Edit accounts and passwords after import."
        }
        "导入" => "Import",
        "导出" => "Export",
        "正在读取本机 SAP 配置…" => "Reading local SAP configuration…",
        // 备份导入 / 导出位置（rfd 原生文件对话框）
        "浏览…" => "Browse…",
        "选择导出位置" => "Choose export location",
        "从备份文件导入" => "Import from Backup",
        "选择 JSON 备份文件" => "Select a JSON backup file",
        "选择此前导出的 JSON 备份，连接将合并导入（同名跳过），密码用当前主密码重新加密。" => {
            "Select a previously exported JSON backup. Connections are merged in (duplicates skipped) and passwords re-encrypted with the current master password."
        }
        "包含 {cred_count} 条连接、{group_count} 个分组 · 备份时间：{time}" => {
            "{cred_count} connections, {group_count} groups · exported at {time}"
        }
        "重新选择" => "Change file",
        "备份文件无效或已损坏" => "Invalid or corrupted backup file",
        "未选择备份文件" => "No backup file selected",
        "验证并导入" => "Verify & Import",
        "已导入 {n} 条连接" => "Imported {n} connection(s)",
        "备份中的连接均已存在，未新增" => "All connections already exist; nothing imported",

        // === 关于 ===
        "SAP 登录管理器" => "SAP Login Manager",
        "版本" => "Version",
        "凭据本地加密存储，无云端依赖" => "Credentials encrypted locally, no cloud",
        "项目主页" => "Project Homepage",
        "打开" => "Open",
        "在浏览器中打开项目主页" => "Open project homepage in browser",
        "开源项目致谢" => "Open Source Credits",

        // === 数字 + 单位 / 时间 ===
        "1 秒" => "1s",
        "2 秒" => "2s",
        "3 秒" => "3s",
        "5 秒" => "5s",
        "10 秒" => "10s",
        "30 秒" => "30s",
        "60 秒" => "60s",
        "1 分钟" => "1 min",
        "3 分钟" => "3 min",
        "10 分钟" => "10 min",
        "30 分钟" => "30 min",
        "不清空" => "Never",
        "无间隔" => "None",
        "不锁定" => "Never",

        // === 标点分隔符 ===
        "、" => ", ",
        "；" => "; ",

        // === 补充 ===
        "20 秒" => "20s",
        "5 分钟" => "5 min",
        "Enter 登录" => "Enter to Logon",
        "修改…" => "Change…",
        "如 p:CN=ERP, O=Company, C=DE" => "e.g. p:CN=ERP, O=Company, C=DE",
        "无操作指定时间后自动锁定应用" => "Auto-lock after a period of inactivity",
        "验证主密码后将用 Windows DPAPI 加密记忆，仅当前 Windows 用户可解密。" => {
            "The master password will be remembered via Windows DPAPI; only the current Windows user can decrypt it."
        }
        "验证后用 Windows DPAPI 记忆主密码，下次启动免输入" => {
            "Remember the master password via Windows DPAPI after verification for password-free startup"
        }

        // === 搜索与列表 ===
        "搜索凭据，Enter 直接登录…" => "Search credentials, Enter to logon…",
        "搜索 名称/系统/用户名/描述/服务器…" => "Search name / SID / user / description / server…",
        "筛选连接名 / 描述 / Workspace" => "Filter connection / description / Workspace",
        "未找到匹配的凭据" => "No matching credentials",
        "该分组暂无凭据" => "No credentials in this group",
        "试试更换搜索关键词，或切换分组筛选" => "Try different keywords, or adjust the group filter",
        "检查 SAP Logon 是否已安装，或在设置中手动指定路径" => {
            "Check that SAP Logon is installed, or set its path in Settings"
        }
        "试试更换或清空搜索关键词" => "Try different keywords, or clear the search",
        "未找到匹配的连接" => "No matching connections",
        "未找到本机 SAP 连接" => "No local SAP connections found",
        "暂无凭据\n按 Ctrl+N 新增，或从 SAP 配置导入" => {
            "No credentials yet\nPress Ctrl+N to add, or import from SAP config"
        }
        "匹配 {} 条" => "{} matched",
        "最近使用" => "Recent",
        "更多操作" => "More",
        "窗口" => "Window",
        "系统" => "System",
        "描述" => "Description",
        "语言" => "Language",
        "服务器" => "Server",
        "分组" => "Group",
        "全选 / 取消全选" => "Select All / None",
        "取消选择" => "Deselect",
        "批量删除" => "Delete Selected",
        "批量收藏" => "Favorite Selected",
        "批量取消收藏" => "Unfavorite Selected",
        "批量登录（逐条执行）" => "Batch Logon (one by one)",
        "移动到分组" => "Move to Group",
        "默认分组" => "Default Group",
        "撤销" => "Undo",
        "复制副本" => "Duplicate",

        // === 凭据卡片 / 操作提示 ===
        "已删除 {} 个凭据" => "Deleted {} credential(s)",
        "已删除「{name}」" => "Deleted \"{name}\"",
        "{failed} 个凭据删除失败" => "{failed} credential(s) failed to delete",
        "已选 {count}" => "{count} selected",
        "已收藏 {n} 个凭据" => "Favorited {n} credential(s)",
        "已取消收藏 {n} 个凭据" => "Unfavorited {n} credential(s)",
        "已移动 {n} 个凭据" => "Moved {n} credential(s)",
        "已恢复该凭据" => "Credential restored",
        "已创建凭据副本" => "Credential duplicated",
        "已发起 SAP 登录" => "SAP logon started",
        "正在登录中，请稍候…" => "Logging on, please wait…",
        "登录中…" => "Logging on…",
        "开始批量登录 {count} 个凭据…" => "Batch logon of {count} credential(s)…",
        "批量登录完成：成功 {n} 个" => "Batch logon done: {n} succeeded",
        "批量登录中断：{e}" => "Batch logon aborted: {e}",
        "该凭据未设置密码" => "This credential has no password",
        "密码已复制到剪贴板" => "Password copied to clipboard",
        "链接已复制到剪贴板" => "Link copied to clipboard",
        "复制链接" => "Copy Link",
        "点击复制项目链接" => "Click to copy project link",
        "复制密码后到时自动清空剪贴板" => "Clipboard auto-clears after copying password",
        "缺少密码" => "Password missing",
        "缺少关键信息：{}" => "Missing required fields: {}",
        "请检查：{}" => "Please check: {}",
        "登录失败：{e}" => "Logon failed: {e}",
        "操作失败：{e}" => "Operation failed: {e}",
        "置顶失败：{e}" => "Pin failed: {e}",
        "恢复失败" => "Restore failed",

        // === 凭据表单字段与校验 ===
        "系统标识" => "System ID",
        "系统标识必填" => "System ID is required",
        "实例编号必填" => "Instance number is required",
        "实例号" => "Instance No.",
        "如 00" => "e.g. 00",
        "如 100" => "e.g. 100",
        "如 PRD" => "e.g. PRD",
        "如 SAP-PRD-100" => "e.g. SAP-PRD-100",
        "应用服务器地址" => "Application server address",
        "应用服务器必填" => "Application server is required",
        "消息服务器地址" => "Message server address",
        "消息服务器必填" => "Message server is required",
        "登录组必填" => "Logon group is required",
        "路由字符串" => "Router string",
        "如 /H/1.2.3.4/H/" => "e.g. /H/1.2.3.4/H/",
        "客户端必填" => "Client is required",
        "语言必填" => "Language is required",
        "SAP 用户名" => "SAP Username",
        "SNC 账户" => "SNC Account",
        "SNC 账户必填" => "SNC account is required",
        "SNC 名称必填" => "SNC name is required",
        "自定义别名（可选）" => "Custom alias (optional)",
        "备注描述" => "Description (optional)",
        "显示名称" => "Display Name",
        "连接类型" => "Connection Type",
        "未启用 SNC，使用密码登录" => "SNC off — logon with password",
        "启用后登录无需密码（使用 SNC 证书）" => "Log on without password via SNC certificate",
        "密码至少 4 个字符" => "Password must be at least 4 characters",
        "保存失败：{e}" => "Save failed: {e}",
        "{base_name} (副本)" => "{base_name} (copy)",
        "两次输入的密码不一致" => "Passwords do not match",
        "请输入主密码以解锁凭据" => "Enter master password to unlock",
        "请输入新主密码" => "Enter the new master password",
        "所有凭据将用新密码重新加密" => "All credentials will be re-encrypted with the new password",
        "主密码用于加密所有凭据，请妥善保管。忘记主密码将无法恢复数据。" => {
            "The master password encrypts all credentials. Without it, data cannot be recovered."
        }
        "设置主密码以保护您的凭据" => "Set a master password to protect your credentials",
        "输入主密码以验证" => "Enter master password to verify",
        "验证并开启" => "Verify & Enable",
        "验证并导出" => "Verify & Export",
        "已开启免密模式" => "Password-free mode enabled",
        "已关闭免密模式" => "Password-free mode disabled",
        "开启免密模式" => "Enable Password-free",
        "开启失败：{e}" => "Enable failed: {e}",
        "快速解锁（免密）" => "Quick Unlock",
        "已闲置超时，自动锁定" => "Auto-locked due to inactivity",
        // === SAP Logon 快捷打开 / 主题切换（标题栏与活动栏） ===
        "打开 SAP Logon" => "Open SAP Logon",
        "SAP Logon 已启动" => "SAP Logon started",
        "SAP Logon 已置于前台" => "SAP Logon brought to front",
        "SAP Logon 正在运行" => "SAP Logon is already running",
        "打开 SAP Logon 失败：{e}" => "Failed to open SAP Logon: {e}",
        "切换深浅模式" => "Toggle light/dark mode",
        "GPUI 原生" => "Native GPUI",

        // === 分组管理 ===
        "分组名称（如：测试环境集合）" => "Group name (e.g. Test environments)",
        "确认删除分组「{gname}」？" => "Delete group \"{gname}\"?",
        "该分组下有 {count} 个凭据，请选择处理方式：" => {
            "{count} credential(s) in this group — choose how to handle them:"
        }
        "移入默认分组（保留 {count} 项凭据）" => "Move to default group (keep {count} credential(s))",
        "同时删除 {count} 项凭据（不可恢复）" => "Also delete {count} credential(s) (irreversible)",
        "分组已删除，凭据已移入默认分组" => "Group deleted; credentials moved to default group",
        "分组及内部凭据已删除" => "Group and its credentials deleted",
        "删除失败：{e}" => "Delete failed: {e}",
        "移动失败：{e}" => "Move failed: {e}",
        "新名称（当前：{current_name}）" => "New name (current: {current_name})",
        "系统分组（{}）" => "System Groups ({})",
        "自定义分组（{}）" => "Custom Groups ({})",
        "全部 ({all_count})" => "All ({all_count})",
        "收藏 ({fav_count})" => "Favorites ({fav_count})",
        "{count} 项" => "{count} items",

        // === SAP 导入 ===
        "请先勾选要导入的连接" => "Select connections to import first",
        "所选连接均已导入" => "Selected connections already imported",
        "已导入 {n} 个连接，请编辑填写账户密码" => {
            "Imported {n} connection(s) — edit to fill in accounts and passwords"
        }
        "导入 {selected_count} 项" => "Import {selected_count}",
        "导入失败：{e}" => "Import failed: {e}",
        "读取 SAP 配置失败：{err}" => "Failed to read SAP config: {err}",
        "{total} 个连接" => "{total} connections",

        // === 导出 ===
        "导出成功：{}" => "Exported: {}",
        "导出失败：{e}" => "Export failed: {e}",
        "导出文件包含明文密码，请妥善保管！" => {
            "The export contains plaintext passwords — keep it safe!"
        }
        "将导出 {cred_count} 条凭据到 JSON 文件" => {
            "Export {cred_count} credential(s) to a JSON file"
        }

        // === 设置 ===
        "主题：{theme_label}（点击切换）" => "Theme: {theme_label} (click to toggle)",
        "切换后立即生效" => "Takes effect immediately",
        "开机自启" => "Launch at Startup",
        "写入 Windows 注册表（当前用户）" => "Writes Windows Registry (current user)",
        "最小化到托盘" => "Minimize to Tray",
        "最小化时隐藏窗口，从托盘左键唤回" => "Hide window on minimize; click tray icon to restore",
        "关闭窗口时最小化到托盘" => "Minimize to Tray on Close",
        "点关闭按钮不退出，从托盘菜单退出可彻底关闭" => {
            "Close button hides to tray; quit from the tray menu"
        }
        "紧凑模式" => "Compact Mode",
        "更小的卡片高度，列表显示更多凭据" => "Shorter cards to fit more credentials",
        "留空自动检测 SAP Logon" => "Leave blank to auto-detect SAP Logon",
        "留空则自动检测 saplogon.exe / sapshcut.exe" => {
            "Leave blank to auto-detect saplogon.exe / sapshcut.exe"
        }
        "批量登录多个凭据时的间隔时间" => "Delay between batch logons of multiple credentials",
        "新增凭据时预填的 SAP 登录语言" => "SAP logon language prefilled for new credentials",
        "默认（按环境归类）" => "Default (grouped by environment)",

        // === 关于 ===
        "GPU 加速 UI 框架（Zed）" => "GPU-accelerated UI framework (Zed)",
        "UI 组件库（按钮/输入/菜单/主题）" => "UI components (button/input/menu/theme)",
        "数据序列化" => "Serialization",
        "日期时间" => "Date & time",
        "凭据 ID 生成" => "Credential ID generation",
        "凭据加密" => "Credential encryption",
        "主密码派生与校验" => "Master password derivation & verification",
        "跨平台剪贴板" => "Cross-platform clipboard",
        "SAP Logon 配置解析" => "SAP Logon config parsing",
        "Windows 注册表读取" => "Windows registry access",
        "Win32 窗口控制/剪贴板" => "Win32 window control / clipboard",
        "Base64 编解码" => "Base64 codec",
        "错误定义" => "Error definitions",
        "日志门面" => "Logging facade",
        "分享连接信息" => "Share connection info",
        "已分享 {ok}/{} 个连接信息到剪贴板" => "Shared {ok}/{} connection info to clipboard",
        "复制失败：{e}" => "Copy failed: {e}",
        "复制密码失败：{e}" => "Copy password failed: {e}",

        // 兜底：未收录的 key 原样返回（由调用方回落）
        _ => key,
    }
}