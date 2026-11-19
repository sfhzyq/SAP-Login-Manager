use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// 日期时间默认值（用于 serde 反序列化时字段缺失的情况）
fn default_datetime() -> DateTime<Utc> {
    DateTime::from_timestamp(0, 0).unwrap_or_else(Utc::now)
}

/// SAP 连接凭据
#[derive(Clone, Default, Serialize, Deserialize)]
pub struct Credential {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub connection_id: String,
    #[serde(default)]
    pub client: String,
    #[serde(default)]
    pub username: String,
    // 仅为空时跳过：本字段是「传输用临时明文」（导入/表单提交），
    // 所有 store.json 写入方在 save_store 前都会清空，故落盘永远为空。
    // 不能用无条件 skip_serializing —— ExportBundle 直接复用 Credential，
    // 无条件跳过会让「导出备份 → 导入恢复」静默丢失全部密码
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub password: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub encrypted_password: String,
    #[serde(default)]
    pub language: String,
    /// 连接类型: "direct" | "load_balancing"
    #[serde(default)]
    pub connection_type: String,
    /// 系统标识（如 PRD）
    #[serde(default)]
    pub system_id: String,
    /// 应用服务器（直连）
    #[serde(default)]
    pub app_server: String,
    /// 实例编号（直连）
    #[serde(default)]
    pub system_number: String,
    /// 消息服务器（负载均衡）
    #[serde(default)]
    pub message_server: String,
    /// 消息服务器端口（负载均衡）
    #[serde(default)]
    pub message_server_port: String,
    /// 登录组（负载均衡）
    #[serde(default)]
    pub logon_group: String,
    /// 路由字符串
    #[serde(default)]
    pub saprouter: String,
    /// 描述/备注
    #[serde(default)]
    pub description: String,
    pub post_login_action: Option<String>,
    pub post_login_action_type: Option<String>,
    /// 挂载的脚本文件路径（登录后自动执行）；为空则不执行脚本
    #[serde(default)]
    pub script_path: Option<String>,
    pub group_id: Option<String>,
    /// 环境类型: "production" | "test" | "development" | ""(未分类)
    #[serde(default)]
    pub environment: String,
    /// 登录次数，用于排序（经常使用的排前面）
    #[serde(default)]
    pub login_count: u32,
    /// 最后登录时间
    #[serde(default)]
    pub last_login_at: Option<DateTime<Utc>>,
    /// 颜色标签: "red" | "orange" | "yellow" | "green" | "blue" | "purple" | ""
    #[serde(default)]
    pub color_tag: String,
    /// 是否收藏
    #[serde(default)]
    pub is_favorite: bool,
    /// 收藏排序序号（越小越靠前）
    #[serde(default)]
    pub favorite_order: Option<u32>,
    /// 显示名称（自定义别名，为空时使用 connection_id）
    #[serde(default)]
    pub display_name: Option<String>,
    /// Service UUID（SAP GUI 8.10 的 -uuid 参数）
    #[serde(default)]
    pub uuid: String,
    /// SNC 是否启用
    #[serde(default)]
    pub snc_enabled: bool,
    /// SNC 名称（如 p:CN=ERP, O=Company, C=DE）
    #[serde(default)]
    pub snc_name: String,
    /// SNC 保护质量级别（1=认证 2=完整性 3=隐私 8=最大 9=默认）
    #[serde(default)]
    pub snc_qop: String,
    /// SNC 是否使用 SSO（true=SSO免密登录，false=需输入密码）
    #[serde(default)]
    pub snc_sso: bool,
    #[serde(default = "default_datetime")]
    pub created_at: DateTime<Utc>,
    #[serde(default = "default_datetime")]
    pub updated_at: DateTime<Utc>,
}

/// 手写 Debug：密码字段一律 redact，避免任何 `{:?}` / 崩溃转储泄漏明文
///（M-PUBLIC-DEBUG，v4 Consider #18）。
impl std::fmt::Debug for Credential {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Credential")
            .field("id", &self.id)
            .field("connection_id", &self.connection_id)
            .field("password", &"<redacted>")
            .field("encrypted_password", &"<redacted>")
            .finish_non_exhaustive()
    }
}

/// 凭据组
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Group {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub group_name: String,
    #[serde(default)]
    pub entries: Vec<String>,
    /// 系统内置分组（生产/测试/开发），不可删除不可重命名
    #[serde(default)]
    pub is_system: bool,
    /// 默认分组，SAP 配置导入的目标分组，不可删除
    #[serde(default)]
    pub is_default: bool,
    #[serde(default = "default_datetime")]
    pub created_at: DateTime<Utc>,
    #[serde(default = "default_datetime")]
    pub updated_at: DateTime<Utc>,
}

/// SAP Landscape 连接条目
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SapConnection {
    pub name: String,
    pub description: Option<String>,
    pub server: Option<String>,
    pub system_number: Option<String>,
    pub system_id: Option<String>,
    pub group: Option<String>,
    pub service: Option<String>,
    pub connection_type: Option<String>,
    /// 所属 Workspace 名称（用于导入时创建自建分组）
    pub workspace_name: Option<String>,
    /// SAP 路由字符串（已解析，如 /H/1.2.3.4/H/）
    pub saprouter: Option<String>,
    /// 消息服务器（负载均衡模式）
    pub message_server: Option<String>,
    /// 消息服务器端口（负载均衡模式）
    pub message_server_port: Option<String>,
    /// 登录组（负载均衡模式）
    pub logon_group: Option<String>,
    /// Service UUID（SAP GUI 8.10 推荐的 -uuid 参数）
    pub uuid: Option<String>,
    /// SNC 保护质量（-1=未启用，1=认证 2=完整性 3=隐私 8=最大 9=默认）
    pub sncop: Option<String>,
    /// SNC 名称（如 p:CN=DS4, OU=SAP-HEC, O=SAP SE, C=DE）
    pub snc_name: Option<String>,
}

/// 应用设置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppSettings {
    pub auto_start: bool,
    pub minimize_to_tray: bool,
    pub close_to_tray: bool,
    pub default_language: String,
    pub sap_logon_path: Option<String>,
    pub theme: String,
    /// 免密模式（仅本次运行内有效；重启后仍需输入主密码，主密码不落盘）
    #[serde(default)]
    pub password_free: bool,
    /// 批量登录间隔（秒）
    #[serde(default = "default_batch_interval")]
    pub batch_login_interval: u32,
    /// 窗口置顶
    #[serde(default)]
    pub always_on_top: bool,
    /// 自动锁定时间（分钟），0 表示不自动锁定
    #[serde(default = "default_auto_lock")]
    pub auto_lock_minutes: u32,
    /// 界面字体（空表示使用系统默认字体栈）
    #[serde(default)]
    pub font_family: String,
    /// 界面语言（"zh" 中文 / "en" English）
    #[serde(default = "default_ui_language")]
    pub ui_language: String,
    /// 凭据列表按系统分组（环境）折叠展示
    #[serde(default = "default_true")]
    pub group_by_environment: bool,
    /// 紧凑密度（更小的卡片间距与内边距）
    #[serde(default)]
    pub compact_mode: bool,
    /// 剪贴板自动清空秒数（0 = 不清空）
    #[serde(default = "default_clipboard_clear")]
    pub clipboard_clear_seconds: u32,
    /// 默认打开的分组：""=全部；"favorites"=收藏；或系统分组ID/自定义分组ID
    #[serde(default)]
    pub default_group: String,
    /// 置顶的凭据 ID 列表（列表中排在最前）
    #[serde(default)]
    pub pinned_credential_ids: Vec<String>,
    /// 「最近登录」筛选窗口（天）：仅显示 N 天内登录过的凭据
    #[serde(default = "default_recent_days")]
    pub recent_days: u32,
    /// Island 浮动布局（v1.5.7 方案 B）：主界面活动栏/内容区以圆角浮岛呈现，
    /// 关闭时为经典边到边布局
    #[serde(default)]
    pub island_layout: bool,
}

fn default_batch_interval() -> u32 {
    2
}
fn default_auto_lock() -> u32 {
    3
}
fn default_true() -> bool {
    true
}
fn default_clipboard_clear() -> u32 {
    20
}
fn default_ui_language() -> String {
    "zh".to_string()
}
fn default_recent_days() -> u32 {
    7
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            auto_start: false,
            minimize_to_tray: false,
            close_to_tray: true,
            default_language: "ZH".to_string(),
            sap_logon_path: None,
            theme: "light".to_string(),
            password_free: false,
            batch_login_interval: 2,
            always_on_top: false,
            auto_lock_minutes: 3,
            font_family: "'Microsoft YaHei', '微软雅黑'".to_string(),
            ui_language: "zh".to_string(),
            group_by_environment: true,
            compact_mode: false,
            clipboard_clear_seconds: 20,
            default_group: String::new(),
            pinned_credential_ids: Vec::new(),
            recent_days: 7,
            island_layout: false,
        }
    }
}

/// 导出包（注意：序列化后是**明文**，含所有凭据密码）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExportBundle {
    pub app_version: String,
    pub exported_at: DateTime<Utc>,
    pub group_count: usize,
    pub connection_count: usize,
    pub has_landscape: bool,
    pub source_salt: String,
    pub credentials: Vec<Credential>,
    pub groups: Vec<Group>,
}

/// v5 Consider #27：给明文凭据提供清零能力，供 `Zeroizing<...>` 包裹。
/// 只清敏感字段与容器，不动时间戳等无敏感信息。
impl zeroize::Zeroize for Credential {
    fn zeroize(&mut self) {
        self.password.zeroize();
        self.encrypted_password.zeroize();
    }
}

impl zeroize::Zeroize for ExportBundle {
    fn zeroize(&mut self) {
        self.source_salt.zeroize();
        self.app_version.zeroize();
        for cred in &mut self.credentials {
            cred.zeroize();
        }
        self.credentials.clear();
        self.groups.clear();
    }
}

/// 存储数据（整体持久化结构）
#[derive(Clone, Serialize, Deserialize)]
pub struct StoreData {
    pub salt: String,
    pub master_password_hash: String,
    pub credentials: Vec<Credential>,
    pub groups: Vec<Group>,
    pub settings: AppSettings,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    /// 密钥派生算法版本: 0 = 旧版(10000轮自定义), 1 = 标准PBKDF2(600000轮), 2 = Argon2id
    #[serde(default)]
    pub key_derivation_version: u32,
    /// 加密算法版本: 0 = AES-256-CBC(无认证), 1 = AES-256-GCM(带认证)
    #[serde(default)]
    pub encryption_version: u32,
    /// 历史遗留字段：旧版本曾写入 DPAPI 加密的主密码，但应用从未读取。
    /// 现在开启免密会顺带清空该字段；保留字段仅为兼容旧 store.json。
    #[serde(default)]
    pub remembered_master: String,
}

/// 手写 Debug：主密码哈希与历史 DPAPI 记忆不出现在任何调试输出中
///（v4 Consider #18）。
impl std::fmt::Debug for StoreData {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StoreData")
            .field("credentials", &self.credentials.len())
            .field("groups", &self.groups.len())
            .field("master_password_hash", &"<redacted>")
            .field("remembered_master", &"<redacted>")
            .finish_non_exhaustive()
    }
}
