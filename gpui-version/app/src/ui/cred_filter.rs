//! 凭据列表纯逻辑：排序方式、环境映射、分组命名与近期登录判断。
//!
//! 无 UI / 窗口依赖，仅依赖 backend 模型与 i18n——可独立单元测试。
//! 由 ui/main_page.rs 拆分而来（v1.5.9）。

use super::i18n::t;
use sap_backend::models::{Credential, Group};

/// 列表排序方式（置顶项恒定排最前）
#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum SortBy {
    /// 默认：收藏 → 登录次数 → 最近登录
    #[default]
    Default,
    /// 登录次数降序
    Frequently,
    /// 最近登录降序
    Recent,
    /// 名称升序（不区分大小写）
    Name,
}

impl SortBy {
    pub(crate) fn label(&self) -> &'static str {
        match self {
            SortBy::Default => t("默认"),
            SortBy::Frequently => t("常用"),
            SortBy::Recent => t("最近登录"),
            SortBy::Name => t("名称"),
        }
    }
}

/// 是否在最近 N 天内登录过（「最近使用 / 最近登录」筛选共用）
pub(crate) fn within_recent_days(c: &Credential, days: u32) -> bool {
    c.last_login_at
        .is_some_and(|t| t > chrono::Utc::now() - chrono::Duration::days(days as i64))
}

/// 系统分组 ID → 环境字段值（与 Tauri 版 SYSTEM_GROUP_ENV 一致）
pub(crate) fn system_group_env(group_id: &str) -> Option<&'static str> {
    match group_id {
        "group-production" => Some("production"),
        "group-test" => Some("test"),
        "group-development" => Some("development"),
        "group-configuration" => Some("configuration"),
        _ => None,
    }
}

/// 分组归属判定（唯一实现）：主列表 refilter 与环境 chip 计数共用，
/// 防两处口径漂移（历史上的 chip 失效 bug 正源于此）。
/// 虚拟分组：None=全部、favorites=收藏、recent=最近 N 天内登录过；
/// 默认分组恒放行全部；系统分组按环境字段匹配；自定义分组按 group_id 归属。
pub(crate) fn credential_in_group(
    c: &Credential,
    selected: Option<&str>,
    default_group_id: Option<&str>,
    recent_days: u32,
) -> bool {
    match selected {
        None => true,
        Some("favorites") => c.is_favorite,
        Some("recent") => within_recent_days(c, recent_days),
        Some(gid) if Some(gid) == default_group_id => true,
        Some(gid) => match system_group_env(gid) {
            Some(env) => c.environment == env,
            None => c.group_id.as_deref() == Some(gid),
        },
    }
}

/// 分组显示名：系统/默认分组按 ID 走 i18n（与 Tauri 版 group.name.* 一致）。
/// 这五个分组的名称在存储层不可修改（受保护），ID 恒定，按 ID 映射不会
/// 误伤用户自建分组；自定义分组直接用存储名。
pub(crate) fn group_display_name(g: &Group) -> String {
    match g.id.as_str() {
        "group-default" => t("默认分组").to_string(),
        "group-production" => t("生产环境").to_string(),
        "group-test" => t("测试环境").to_string(),
        "group-development" => t("开发环境").to_string(),
        "group-configuration" => t("配置环境").to_string(),
        _ => g.group_name.clone(),
    }
}

/// 环境字段值 → 默认排序权重：配置→开发→测试→正式→未分组（未知/空归入末组）
pub(crate) fn env_rank(environment: &str) -> u8 {
    match environment {
        "configuration" => 0,
        "development" => 1,
        "test" => 2,
        "production" => 3,
        _ => 4,
    }
}

/// 搜索语法 env: 前缀的别名映射（其余原样匹配 environment 字段值）
pub(crate) fn env_alias(v: &str) -> String {
    match v {
        "生产" | "生产环境" | "Production" | "production" => "production",
        "测试" | "测试环境" | "Test" | "test" => "test",
        "开发" | "开发环境" | "Development" | "development" => "development",
        "配置" | "配置环境" | "Configuration" | "configuration" => "configuration",
        _ => v,
    }
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cred_with(env: &str, days_ago_login: Option<i64>) -> Credential {
        Credential {
            environment: env.to_string(),
            last_login_at: days_ago_login.map(|d| chrono::Utc::now() - chrono::Duration::days(d)),
            ..Default::default()
        }
    }

    fn group(id: &str, name: &str) -> Group {
        Group {
            id: id.to_string(),
            group_name: name.to_string(),
            entries: vec![],
            is_system: id.starts_with("group-") && id != "group-default",
            is_default: id == "group-default",
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        }
    }

    #[test]
    fn env_rank_orders_configuration_first() {
        assert_eq!(env_rank("configuration"), 0);
        assert_eq!(env_rank("development"), 1);
        assert_eq!(env_rank("test"), 2);
        assert_eq!(env_rank("production"), 3);
        // 未知/空环境归入末组，权重高于 production
        assert!(env_rank("") > env_rank("production"));
        assert!(env_rank("staging") > env_rank("production"));
    }

    #[test]
    fn env_alias_maps_cn_and_en_aliases() {
        assert_eq!(env_alias("生产"), "production");
        assert_eq!(env_alias("生产环境"), "production");
        assert_eq!(env_alias("Production"), "production");
        assert_eq!(env_alias("测试"), "test");
        assert_eq!(env_alias("开发环境"), "development");
        assert_eq!(env_alias("配置"), "configuration");
        // 非别名原样返回
        assert_eq!(env_alias("custom-env"), "custom-env");
    }

    #[test]
    fn system_group_env_maps_system_group_ids() {
        assert_eq!(system_group_env("group-production"), Some("production"));
        assert_eq!(system_group_env("group-test"), Some("test"));
        assert_eq!(system_group_env("group-development"), Some("development"));
        assert_eq!(
            system_group_env("group-configuration"),
            Some("configuration")
        );
        // 默认分组与自定义分组不是环境分组
        assert_eq!(system_group_env("group-default"), None);
        assert_eq!(system_group_env("group-my-own"), None);
    }

    #[test]
    fn group_display_name_maps_system_groups_by_id() {
        // 系统分组的存储名被忽略，按 ID 走 i18n 映射（结果不同于存储名）
        let prod = group("group-production", "sentinel");
        assert_ne!(group_display_name(&prod), "sentinel");
        assert!(!group_display_name(&prod).is_empty());
        // 默认分组同理
        let default = group("group-default", "sentinel");
        assert_ne!(group_display_name(&default), "sentinel");
        // 自定义分组直接使用存储名
        let custom = group("group-my-own", "我的分组");
        assert_eq!(group_display_name(&custom), "我的分组");
    }

    #[test]
    fn within_recent_days_filters_by_login_time() {
        // 1 天前登录 → 30 天窗口内
        assert!(within_recent_days(&cred_with("production", Some(1)), 30));
        // 60 天前登录 → 超出 30 天窗口
        assert!(!within_recent_days(&cred_with("production", Some(60)), 30));
        // 恰好卡在窗口边界内（29 天前）
        assert!(within_recent_days(&cred_with("production", Some(29)), 30));
        // 从未登录
        assert!(!within_recent_days(&cred_with("production", None), 30));
    }

    #[test]
    fn sort_by_labels_are_non_empty_and_distinct() {
        let labels = [
            SortBy::Default.label(),
            SortBy::Frequently.label(),
            SortBy::Recent.label(),
            SortBy::Name.label(),
        ];
        for l in labels {
            assert!(!l.is_empty(), "排序方式标签不能为空");
        }
        // 四种排序方式标签互不相同
        let mut sorted = labels;
        sorted.sort_unstable();
        for w in sorted.windows(2) {
            assert_ne!(w[0], w[1], "排序方式标签应互不相同");
        }
    }

    /// 回归 v4 Consider #21：分组口径的唯一实现必须被直接钉住
    #[test]
    fn credential_in_group_covers_all_branches() {
        let c = Credential {
            is_favorite: true,
            environment: "production".to_string(),
            group_id: Some("group-custom".to_string()),
            ..Default::default()
        };
        assert!(credential_in_group(&c, None, None, 30), "全部");
        assert!(credential_in_group(&c, Some("favorites"), None, 30), "收藏");
        assert!(!credential_in_group(
            &Credential {
                is_favorite: false,
                ..Default::default()
            },
            Some("favorites"),
            None,
            30
        ));
        assert!(
            credential_in_group(&c, Some("group-default"), Some("group-default"), 30),
            "默认分组恒放行"
        );
        assert!(
            credential_in_group(&c, Some("group-custom"), None, 30),
            "自定义分组"
        );
        assert!(!credential_in_group(&c, Some("group-other"), None, 30));
        let recent = Credential {
            last_login_at: Some(chrono::Utc::now()),
            ..Default::default()
        };
        assert!(
            credential_in_group(&recent, Some("recent"), None, 30),
            "最近登录"
        );
    }
}
