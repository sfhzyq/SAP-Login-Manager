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
            // "{{" 转义：输出字面 '{'（不消耗参数）
            if i + 1 < bytes.len() && bytes[i + 1] == b'{' {
                out.push('{');
                i += 2;
                continue;
            }
            if let Some(end) = s[i..].find('}') {
                if idx < args.len() {
                    out.push_str(args[idx]);
                }
                idx += 1;
                i += end + 1;
                continue;
            }
        } else if bytes[i] == b'}' {
            // "}}" 转义：输出字面 '}'（与 "{{" 对应，否则 "{{n}}" 会渲染成 "{n}}"）
            if i + 1 < bytes.len() && bytes[i + 1] == b'}' {
                out.push('}');
                i += 2;
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
        "默认" => "Default",
        "全部" => "All",
        "收藏" => "Favorites",
        "收藏视图" => "Favorites view",
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
        "从 SAP 配置导入…" => "Import from SAP Config…",
        // 活动栏按钮 tooltip / 面板标题用不带省略号的 key（与菜单项区分）
        "从 SAP 配置导入" => "Import from SAP Config",
        "导出凭据…" => "Export Credentials…",
        "导出凭据" => "Export Credentials",
        "命令面板 (Ctrl+K)" => "Command Palette (Ctrl+K)",

        // === 面板标题 ===
        "新增凭据" => "New Credential",
        "编辑凭据" => "Edit Credential",
        "分组管理" => "Group Manager",
        "命令面板" => "Command Palette",
        "返回主界面" => "Back to Main",
        "返回主界面 (Esc)" => "Back to Main (Esc)",

        // === 主界面 ===
        "分组筛选" => "Group Filter",
        "系统 / 默认分组" => "System / Default Groups",
        "自定义分组" => "Custom Groups",
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
        "生产" => "Prod",
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
        "Island 布局" => "Island layout",
        "内容区浮为圆角岛，侧栏与顶部贴边，间隙透出窗口底色；关闭恢复经典布局" => {
            "Content area floats as a rounded island with canvas gaps; off restores the classic layout"
        }
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
        "天" => "days",
        "最近使用范围" => "Recent Usage Window",
        "「最近使用 / 最近登录」只显示 N 天内登录过的凭据" => {
            "「Recent」only shows credentials logged in within N days"
        },
        "1 天" => "1 day",
        "3 天" => "3 days",
        "7 天" => "7 days",
        "14 天" => "14 days",
        "30 天" => "30 days",
        "最近 {n} 天没有登录记录" => "No logins in the last {n} days",
        "还没有收藏的凭据\n在凭据卡片上点星标即可收藏" => {
            "No favorites yet\nClick the star on a credential card to add one"
        },
        "主密码丢失无法找回" => "Master password cannot be recovered",
        "主密码是解锁本地数据的唯一凭证，应用只保存验证哈希，不存储明文，无任何找回机制" => {
            "The master password is the only key to your local data. Only a verification hash is stored - there is no recovery mechanism"
        },
        "忘记主密码 = 加密数据永久无法解密" => {
            "Forgetting the master password means the encrypted data can never be decrypted"
        },
        "备份数据目录可防误删，但备份同样需要主密码才能解密" => {
            "Backing up the data folder prevents accidental loss, but backups still require the master password to decrypt"
        },
        "请务必妥善保管主密码，并定期备份数据目录" => {
            "Keep the master password safe and back up the data folder regularly"
        },
        "权责声明" => "Disclaimer",
        "本软件按「现状」提供，作者不对因使用本软件导致的任何数据丢失、凭据泄露承担责任" => {
            "This software is provided \"as is\". The author is not liable for any data loss or credential leakage caused by using it"
        },
        "将公司凭据存入本软件前，请自行确认符合贵司信息安全与合规规定，相关责任由使用者承担" => {
            "Before storing company credentials, confirm compliance with your company's security policies; related liabilities rest with the user"
        },
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
        "清除记忆密码失败：{e}" => "Failed to clear remembered password: {e}",

        // === 托盘菜单 ===
        "显示主窗口" => "Show Main Window",
        "退出" => "Quit",

        // === 表单脏确认 / 批量登录缺失提示 / 删除确认（v1.6.0 复查补录） ===
        "放弃修改？" => "Discard changes?",
        "继续编辑" => "Keep Editing",
        "放弃修改" => "Discard",
        "有未保存的修改，确定放弃并返回主界面吗？" => {
            "You have unsaved changes. Discard and go back?"
        }
        "删除凭据" => "Delete Credential",
        "确定要删除「{name}」吗？\n关联的脚本和登录记录也将被清除。" => {
            "Delete \"{name}\"?\nAssociated scripts and logon history will also be removed."
        }
        "所选凭据均缺少登录信息" => "All selected credentials lack required logon info",
        "已跳过 {n} 个信息缺失的凭据" => "Skipped {n} credential(s) with missing info",

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
        "从备份文件 / BAT 脚本导入…" => "Import from Backup / BAT scripts…",
        "选择 JSON 备份文件…" => "Select a JSON backup file…",
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
        // BAT 批量导入 / 导出（sapshcut 脚本）
        "SAP 配置" => "SAP Config",
        "导出格式" => "Export format",
        "JSON 备份" => "JSON backup",
        "BAT 脚本" => "BAT scripts",
        "BAT 脚本包含明文密码，请妥善保管！" => {
            "BAT scripts contain plaintext passwords. Keep them safe!"
        }
        "将导出 {cred_count} 条凭据为 BAT 脚本（每条一个文件）" => {
            "Export {cred_count} credentials as BAT scripts (one file each)"
        }
        "已导出 {n} 个 BAT 脚本到 {dir}" => "Exported {n} BAT script(s) to {dir}",
        "导入来源" => "Import source",
        "备份 JSON 文件" => "Backup JSON file",
        "解析 sapshcut 命令批量录入；按 系统+集团+用户 查重（已存在默认跳过），明文密码加密入库，无效行标红。" => {
            "Parse sapshcut commands for batch import. Deduplicated by system+client+user (duplicates skipped by default); passwords are encrypted on import; invalid lines are highlighted."
        }
        "选择 BAT 脚本文件" => "Select a BAT script file",
        "未在文件中找到 sapshcut 登录命令" => "No sapshcut login commands found in the file",
        "读取失败：{e}" => "Failed to read: {e}",
        "未选择 BAT 文件" => "No BAT file selected",
        "第 {line} 行 · {summary}" => "Line {line} · {summary}",
        "[{file}] 第 {line} 行 · {summary}" => "[{file}] Line {line} · {summary}",
        "选择 BAT 脚本文件（可多选）" => "Select BAT script file(s) (multi-select)",
        "已选 {files} 个文件 · 共 {total} 条命令，{valid} 条可导入，{matched} 条匹配 SAP GUI" => {
            "{files} file(s) selected · {total} command(s) total, {valid} importable, {matched} matching SAP GUI"
        }
        "部分文件读取失败：{e}" => "Some files failed to read: {e}",
        "{n} 个文件中未找到 sapshcut 命令，已跳过" => {
            "{n} file(s) contained no sapshcut commands and were skipped"
        }
        "{file} · 共 {total} 条命令，{valid} 条可导入，{matched} 条匹配 SAP GUI" => {
            "{file} · {total} command(s) total, {valid} importable, {matched} matching SAP GUI"
        }
        "已匹配 SAP GUI 配置：{name}" => "Matches SAP GUI entry: {name}",
        "部分匹配 SAP GUI 配置：{name}（{reason}）" => {
            "Partial match with SAP GUI entry: {name} ({reason})"
        }
        "未匹配到本机 SAP GUI 配置" => "No matching local SAP GUI entry",
        "系统标识一致，但服务器/实例不一致" => {
            "System ID matches, but server/instance differs"
        }
        "服务器一致，但系统标识不一致" => "Server matches, but system ID differs",
        "未检测到本机 SAP GUI 配置，匹配结果仅供参考" => {
            "Local SAP GUI configuration not detected; match results are informational only"
        }
        "已加载本机 SAP GUI 配置（{n} 条连接）" => {
            "Loaded local SAP GUI configuration ({n} connections)"
        }
        "已存在时覆盖更新" => "Overwrite existing entries",
        "导入到分组" => "Import into group",
        "不分组" => "No group",
        "全部跳过：已存在或无效，共 {skipped} 条" => {
            "All skipped (existing or invalid): {skipped} entr(ies)"
        }
        "导入完成：新增 {added} 条、覆盖 {updated} 条、跳过 {skipped} 条" => {
            "Import finished: {added} added, {updated} updated, {skipped} skipped"
        }

        // === 关于 ===
        "SAP 登录管理器" => "SAP Login Manager",
        "版本" => "Version",
        "凭据本地加密存储，无云端依赖" => "Credentials encrypted locally, no cloud",
        "项目主页" => "Project Homepage",
        "打开" => "Open",
        "在浏览器中打开项目主页" => "Open project homepage in browser",
        "开源项目致谢" => "Open Source Credits",
        "构建信息" => "Build Info",
        "运行时" => "Runtime",
        "口令哈希" => "Password Hash",
        "数据加密" => "Data Encryption",
        "Rust · GPUI 原生" => "Rust · Native GPUI",
        "安全须知" => "Security Notice",
        "主密码是解锁本地数据的唯一凭证，丢失后无法找回。请务必妥善保管主密码，并定期备份数据目录。" => {
            "The master password is the only key to your local data and cannot be recovered. Keep it safe and back up the data folder regularly"
        },
        "复制全部链接" => "Copy All Links",
        "点击复制全部项目链接" => "Click to copy all project links",
        "共 {n} 个开源项目 · 左键打开主页，右键复制链接" => {
            "{n} open source projects - left-click to open homepage, right-click to copy a link"
        },

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
        "Enter 或再点一次登录" => "Enter or click again to Logon",
        "修改…" => "Change…",
        "如 p:CN=ERP, O=Company, C=DE" => "e.g. p:CN=ERP, O=Company, C=DE",
        "无操作指定时间后自动锁定应用" => "Auto-lock after a period of inactivity",
        "验证主密码后，本次运行内锁定无需再次输入；重启后仍需输入主密码。" => {
            "After verification, unlocking within this session needs no password; a restart still requires it."
        }
        "验证后本次运行内锁定免输入（重启后仍需输入主密码）" => {
            "After verification, unlock without a password for this session only (restart still requires it)"
        }

        // === 搜索与列表 ===
        "搜索凭据，Enter 直接登录…" => "Search credentials, Enter to logon…",
        "搜索 名称/系统/用户名/描述/服务器…" => "Search name / SID / user / description / server…",
        "筛选连接名 / 描述 / Workspace" => "Filter connection / description / Workspace",
        "未找到匹配的凭据" => "No matching credentials",
        "该分组暂无凭据" => "No credentials in this group",
        "该环境下暂无凭据" => "No credentials in this environment",
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
        "{shown} / {total} 条" => "{shown} / {total} items",
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
        "设置语言" => "Set Language",
        "设置环境分类" => "Set Environment",
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
        "已更新 {n} 个凭据的语言" => "Language updated for {n} credential(s)",
        "已更新 {n} 个凭据的环境分类" => "Environment updated for {n} credential(s)",
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
        "主密码至少 6 位" => "Master password must be at least 6 characters",
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
        // === 登录后动作（事务码/报表/命令直达） ===
        "登录后动作" => "Post-login action",
        "无" => "None",
        "事务码" => "Transaction",
        "报表" => "Report",
        "命令" => "Command",
        "如 SE38" => "e.g. SE38",
        "登录后直达该事务码" => "Jump to this transaction after login",
        "填写报表程序名" => "Report program name",
        "填写 ok-code（可含空格）" => "ok-code (spaces allowed)",

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
        "开机自启设置失败：{e}" => "Failed to set launch at startup: {e}",
        "设置保存失败：{e}" => "Failed to save settings: {e}",
        // v5 Must #1：启动读取 store.json 失败时必须让用户看见，不能只写日志
        "数据加载失败：{e}。本次会话不会写盘，请排除文件占用/权限后重启应用。" => {
            "Failed to load data: {e}. Writes are disabled for this session; fix file locks or permissions and restart."
        }
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
        "SAP 安装目录或 saplogon.exe 完整路径，留空自动检测" => {
            "SAP install dir or full saplogon.exe path; leave blank to auto-detect"
        }
        "留空则自动检测 saplogon.exe / sapshcut.exe" => {
            "Leave blank to auto-detect saplogon.exe / sapshcut.exe"
        }
        "可填 SAP 安装目录（自动定位 saplogon/sapshcut）或 saplogon.exe 完整路径；留空则自动检测。登录与打开 SAP Logon 均以本设置优先。" => {
            "Enter the SAP install directory (saplogon/sapshcut located automatically) or the full saplogon.exe path; leave blank to auto-detect. Both logon and Open SAP Logon prefer this setting."
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

        // === 脚本库（P4：SAP GUI 脚本管理，按分组+凭据归属） ===
        "脚本库" => "Script Library",
        "未从文件中解析到任何脚本步骤" => "No script steps parsed from the file",
        "从 {file} 导入" => "Imported from {file}",
        "已导入脚本（{n} 个步骤）" => "Imported script ({n} steps)",
        "脚本保存失败：{e}" => "Failed to save script: {e}",
        "解析 .vbs 失败：{e}" => "Failed to parse .vbs: {e}",
        "脚本执行完成" => "Script executed",
        "脚本执行失败：{e}" => "Script execution failed: {e}",
        "已删除脚本 {name}" => "Deleted script {name}",
        "删除脚本失败：{e}" => "Failed to delete script: {e}",
        "共 {n} 个步骤" => "{n} steps",
        "运行脚本" => "Run Script",
        "导入 .vbs…" => "Import .vbs…",
        "打开日志" => "Open Logs",
        "选择分组" => "Select group",
        "未分组" => "Ungrouped",
        "选择凭据" => "Select credential",
        "请先选择凭据" => "Select a credential first",
        "正式环境凭据：脚本将在生产系统执行，请务必确认。" => {
            "Production credential: scripts will run on the production system. Please double-check."
        }
        "自动执行" => "Auto-run",
        "设为自动执行" => "Set as auto-run",
        "取消自动执行" => "Disable auto-run",
        "已设为登录后自动执行" => "Set to auto-run after logon",
        "已取消登录后自动执行" => "Auto-run disabled",
        "该分组下暂无凭据" => "No credentials in this group",
        "该凭据下还没有脚本" => "No scripts for this credential yet",
        "导入 SAP GUI 录制的 .vbs 文件，登录该凭据后将自动执行" => {
            "Import a .vbs file recorded by SAP GUI; it will run automatically after logon"
        }
        "还没有凭据" => "No credentials yet",
        "先在主界面创建或导入凭据，再回到这里管理它的登录脚本" => {
            "Create or import credentials on the main page first, then manage their logon scripts here"
        }
        "切换到其他分组，或在主界面把凭据归入此分组" => {
            "Switch to another group, or assign credentials to this group on the main page"
        }
        "已导入脚本（{n} 个步骤），已设为登录自动执行" => {
            "Imported script ({n} steps), set as auto-run on logon"
        }
        "分组内全部凭据的脚本，按凭据分节展示" => {
            "Scripts of all credentials in the group, shown per credential"
        }
        "该分组包含正式环境凭据：脚本将在生产系统执行，请务必确认。" => {
            "This group contains production credentials: scripts will run on production systems. Please double-check."
        }
        "该凭据暂无脚本，点右上「导入 .vbs…」添加" => {
            "No scripts for this credential yet — click \"Import .vbs…\" above to add one"
        }
        "执行脚本" => "Run Script",
        "尚未选择脚本——点击列表中的脚本行以选中" => {
            "No script selected — click a script row in the list to select it"
        }
        "已登录则直接执行，未登录将自动登录" => {
            "runs in the logged-on session if present, or logs on automatically first"
        }
        "该脚本归属正式环境凭据，将在生产系统执行，请务必确认。" => {
            "This script belongs to a production credential and will run on the production system. Please double-check."
        }
        "正在执行…" => "Running…",
        "脚本按分组与凭据归属存储；登录凭据后自动执行带「自动执行」标记的脚本。" => {
            "Scripts are stored per group and credential; the script marked \"Auto-run\" runs automatically after logon."
        }

        // 脚本库：会话感知执行（v1.5.7）
        "执行" => "Run",
        "登录并执行：已登录则直接在对应会话执行，否则自动登录后执行" => {
            "Logon and run: executes in the matching logged-on session, or logs on automatically first"
        }
        "正在执行脚本，请稍候…" => "Script is running, please wait…",
        "已在已登录的会话中执行脚本" => "Script executed in the existing logged-on session",
        "未检测到对应会话，已自动登录并执行脚本" => {
            "No matching session found — logged on automatically and executed the script"
        }
        "账号" => "User",

        // v4：保存失败必须可见，新增的 3 条错误提示
        "保存设置失败：{e}" => "Failed to save settings: {e}",
        "保存自动执行脚本指针失败：{e}" => "Failed to save the auto-run script pointer: {e}",
        "保存自动执行脚本设置失败：{e}" => "Failed to save the auto-run script setting: {e}",

        // v5 批 2：可见错误通道 / 降级提示 / 批量汇总 / 重试入口
        "全局热键注册失败：{e}。托盘菜单仍可唤回窗口。" => {
            "Failed to register global hotkey: {e}. The tray menu can still restore the window."
        }
        "托盘初始化失败：{e}。关闭窗口将直接退出应用。" => {
            "Tray initialization failed: {e}. Closing the window will quit the app."
        }
        "主密码已修改，请重新解锁" => "Master password changed — please unlock again",
        "保存主题设置失败：{e}" => "Failed to save the theme setting: {e}",
        "清除自动执行脚本指针后保存失败：{e}" => {
            "Failed to save after clearing the auto-run script pointer: {e}"
        }
        "凭据：{e}" => "Credentials: {e}",
        "分组：{e}" => "Groups: {e}",
        "加载失败：{e}" => "Failed to load: {e}",
        "加载失败：{e}（已保留上次成功加载的数据）" => {
            "Failed to load: {e} (showing the last successfully loaded data)"
        }
        "加载失败：{e}（分组计数不可用）" => {
            "Failed to load: {e} (group counts are unavailable)"
        }
        "批量登录中断（部分会话可能已打开）：{e}" => {
            "Batch logon interrupted (some sessions may already be open): {e}"
        }
        "已分享 {ok} 个连接信息到剪贴板（失败 {fail} 个）" => {
            "Copied {ok} connection(s) to the clipboard ({fail} failed)"
        }
        "批量分享失败：{e}" => "Failed to copy connections: {e}",
        "重新读取" => "Retry",
        // v5 批 3：Landscape 部分解析失败可见提示
        "部分配置读取失败，列表可能不完整：{e}" => {
            "Some configuration files could not be read; the list may be incomplete: {e}"
        }

        // 兜底：未收录的 key 原样返回（由调用方回落）
        _ => key,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// i18n key 完整性守护（抽样）：本次修复新增的 key 必须有英文翻译，
    /// 漏收录会让英文界面回落显示中文
    #[test]
    fn en_lookup_translates_new_keys() {
        assert_eq!(en_lookup("返回主界面 (Esc)"), "Back to Main (Esc)");
        assert_eq!(en_lookup("显示主窗口"), "Show Main Window");
        assert_eq!(en_lookup("退出"), "Quit");
        assert_eq!(
            en_lookup("清除记忆密码失败：{e}"),
            "Failed to clear remembered password: {e}"
        );
        assert_eq!(
            en_lookup("删除脚本失败：{e}"),
            "Failed to delete script: {e}"
        );
        assert_eq!(en_lookup("放弃修改？"), "Discard changes?");
        assert_eq!(en_lookup("删除凭据"), "Delete Credential");
        assert_eq!(
            en_lookup("所选凭据均缺少登录信息"),
            "All selected credentials lack required logon info"
        );
        assert_eq!(
            en_lookup("开机自启设置失败：{e}"),
            "Failed to set launch at startup: {e}"
        );
        assert_eq!(
            en_lookup("设置保存失败：{e}"),
            "Failed to save settings: {e}"
        );
        assert_eq!(en_lookup("天"), "days");
        // v4 新增文案：免密语义变更与保存失败提示
        assert_eq!(
            en_lookup("验证后本次运行内锁定免输入（重启后仍需输入主密码）"),
            "After verification, unlock without a password for this session only (restart still requires it)"
        );
        assert_eq!(
            en_lookup("验证主密码后，本次运行内锁定无需再次输入；重启后仍需输入主密码。"),
            "After verification, unlocking within this session needs no password; a restart still requires it."
        );
        assert_eq!(
            en_lookup("保存设置失败：{e}"),
            "Failed to save settings: {e}"
        );
        assert_eq!(
            en_lookup("保存自动执行脚本指针失败：{e}"),
            "Failed to save the auto-run script pointer: {e}"
        );
        assert_eq!(
            en_lookup("保存自动执行脚本设置失败：{e}"),
            "Failed to save the auto-run script setting: {e}"
        );
        // v5 Must #1：启动失败提示
        assert_eq!(
            en_lookup("数据加载失败：{e}。本次会话不会写盘，请排除文件占用/权限后重启应用。"),
            "Failed to load data: {e}. Writes are disabled for this session; fix file locks or permissions and restart."
        );
        // v5 批 2：可见错误通道与降级提示
        assert_eq!(
            en_lookup("托盘初始化失败：{e}。关闭窗口将直接退出应用。"),
            "Tray initialization failed: {e}. Closing the window will quit the app."
        );
        assert_eq!(
            en_lookup("主密码已修改，请重新解锁"),
            "Master password changed — please unlock again"
        );
        assert_eq!(
            en_lookup("清除自动执行脚本指针后保存失败：{e}"),
            "Failed to save after clearing the auto-run script pointer: {e}"
        );
        assert_eq!(
            en_lookup("加载失败：{e}（已保留上次成功加载的数据）"),
            "Failed to load: {e} (showing the last successfully loaded data)"
        );
        assert_eq!(
            en_lookup("批量登录中断（部分会话可能已打开）：{e}"),
            "Batch logon interrupted (some sessions may already be open): {e}"
        );
        assert_eq!(
            en_lookup("已分享 {ok} 个连接信息到剪贴板（失败 {fail} 个）"),
            "Copied {ok} connection(s) to the clipboard ({fail} failed)"
        );
        assert_eq!(en_lookup("重新读取"), "Retry");
        assert_eq!(
            en_lookup("部分配置读取失败，列表可能不完整：{e}"),
            "Some configuration files could not be read; the list may be incomplete: {e}"
        );
    }

    /// {{ }} 转义：输出字面大括号且不消耗占位符参数。
    /// 用默认 zh 模式（key 原样返回）——转义逻辑在 tf 内部，与语言无关，
    /// 避免测试中改全局 LANG 干扰其他并行测试。
    #[test]
    fn tf_escapes_double_braces_without_consuming_args() {
        assert_eq!(tf("共 {{n}} 条", &[]), "共 {n} 条");
        assert_eq!(tf("{a} 和 {{b}}", &["X"]), "X 和 {b}");
        // 转义段在前，不得吞掉真实占位符的参数
        assert_eq!(tf("{{a}} {b}", &["V"]), "{a} V");
        // 孤立右括号原样输出
        assert_eq!(tf("a} b", &[]), "a} b");
    }

    /// 参数不足时占位符静默为空，不得 panic
    #[test]
    fn tf_missing_args_render_empty_without_panic() {
        assert_eq!(tf("{a} {b}", &["only"]), "only ");
    }
}
