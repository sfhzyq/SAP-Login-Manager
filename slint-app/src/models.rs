use serde::{Deserialize, Serialize};
use chrono::{DateTime, Utc};

/// 日期时间默认值（用于 serde 反序列化时字段缺失的情况）
fn default_datetime() -> DateTime<Utc> {
    DateTime::from_timestamp(0, 0).unwrap_or_else(|| Utc::now())
}

/// SAP 连接凭据
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Credential {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub connection_id: String,
    #[serde(default)]
    pub client: String,
    #[serde(default)]
    pub username: String,
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
    /// 免密模式（下次启动无需输入主密码）
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
}

fn default_batch_interval() -> u32 { 2 }
fn default_auto_lock() -> u32 { 3 }
fn default_true() -> bool { true }
fn default_clipboard_clear() -> u32 { 20 }

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
            group_by_environment: true,
            compact_mode: false,
            clipboard_clear_seconds: 20,
            default_group: String::new(),
        }
    }
}

/// 导出包
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

/// 存储数据（整体持久化结构）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoreData {
    pub salt: String,
    pub master_password_hash: String,
    pub credentials: Vec<Credential>,
    pub groups: Vec<Group>,
    pub settings: AppSettings,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    /// 密钥派生算法版本: 0 = 旧版(10000轮自定义), 1 = 标准PBKDF2(600000轮)
    #[serde(default)]
    pub key_derivation_version: u32,
    /// 加密算法版本: 0 = AES-256-CBC(无认证), 1 = AES-256-GCM(带认证)
    #[serde(default)]
    pub encryption_version: u32,
    /// 免密模式下用 Windows DPAPI 加密后的主密码（Base64）。空 = 未记忆。
    /// 仅当前 Windows 用户可解密，拷贝文件到其他机器/账户无法还原。
    #[serde(default)]
    pub remembered_master: String,
}
