//! 主界面：搜索 + 分组筛选 + 凭据卡片列表（多选 / 置顶 / 批量操作）
//!
//! 对应 Tauri 版 App.tsx 主列表：
//! - 搜索匹配：显示名/连接名/系统/客户端/用户名/描述/服务器/路由/登录组/SNC 名/环境/分组名
//! - 分组筛选：全部（默认分组=全部）→ 收藏 → 系统分组（按环境）→ 自定义分组（按 group_id）
//! - 卡片式列表（uniform_list 虚拟滚动），后台线程执行 SAP 登录
//! - 多选：勾选框 / Ctrl、Shift 点选；底部批量操作栏

use super::i18n::{t, tf};
use std::collections::HashSet;

use gpui_kit::assets::IconName;
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::notification::Notification;
use gpui_kit::component::popover::Popover;
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::{
    ActiveTheme as _, Colorize as _, Disableable as _, Icon, Sizable as _, WindowExt as _,
    button::{Button, ButtonVariants as _},
    h_flex, menu::{DropdownMenu as _, PopupMenuItem}, v_flex,
};
use gpui_kit::base::ElementExt as _;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use sap_backend::commands::{credentials as cred_cmd, settings as settings_cmd, sap as sap_cmd};
use sap_backend::models::{Credential, Group};

use crate::state::AppState;

use super::credential_card;

/// 列表排序方式（置顶项恒定排最前）
#[derive(Clone, Copy, PartialEq, Eq, Default)]
enum SortBy {
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
    fn label(&self) -> &'static str {
        match self {
            SortBy::Default => t("默认"),
            SortBy::Frequently => t("常用"),
            SortBy::Recent => t("最近登录"),
            SortBy::Name => t("名称"),
        }
    }
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
fn env_rank(environment: &str) -> u8 {
    match environment {
        "configuration" => 0,
        "development" => 1,
        "test" => 2,
        "production" => 3,
        _ => 4,
    }
}

/// 搜索语法 env: 前缀的别名映射（其余原样匹配 environment 字段值）
fn env_alias(v: &str) -> String {
    match v {
        "生产" | "生产环境" | "Production" | "production" => "production",
        "测试" | "测试环境" | "Test" | "test" => "test",
        "开发" | "开发环境" | "Development" | "development" => "development",
        "配置" | "配置环境" | "Configuration" | "configuration" => "configuration",
        _ => v,
    }
    .to_string()
}

/// 分组下拉弹层：菜单行（勾选标记 + 名称 + 数量徽标）。主题色由调用方传入。
fn group_menu_row(
    id: String,
    label: String,
    count: usize,
    checked: bool,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    hover_bg: gpui_kit::Hsla,
    muted: gpui_kit::Hsla,
    primary: gpui_kit::Hsla,
) -> AnyElement {
    h_flex()
        .id(id)
        .items_center()
        .gap_2()
        .px_2()
        .py(px(6.))
        .rounded(px(6.))
        .cursor_pointer()
        .hover(move |s| s.bg(hover_bg))
        // 按压反馈：比 hover 再深一档（secondary 中间调在明暗主题下压暗都可辨）
        .active(move |s| s.bg(hover_bg.darken(0.08)))
        .on_click(on_click)
        .child(
            div().w(px(14.)).flex_none().children(checked.then(|| {
                Icon::new(IconName::Check).size_3().text_color(primary)
            })),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .text_size(px(super::tokens::TS_SM))
                .child(label),
        )
        .child(
            div()
                .flex_none()
                .text_size(px(super::tokens::TS_XS))
                .text_color(muted)
                .child(count.to_string()),
        )
        .into_any_element()
}

/// 分组下拉弹层：分区头（chevron + 标题 + 计数，点击折叠/展开）。主题色由调用方传入。
fn group_menu_section(
    id: &'static str,
    title: String,
    count: usize,
    collapsed: bool,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    hover_bg: gpui_kit::Hsla,
    muted: gpui_kit::Hsla,
) -> AnyElement {
    h_flex()
        .id(id)
        .items_center()
        .gap_1()
        .mt_1()
        .px_2()
        .py(px(4.))
        .rounded(px(6.))
        .cursor_pointer()
        .hover(move |s| s.bg(hover_bg))
        .active(move |s| s.bg(hover_bg.darken(0.08)))
        .on_click(on_click)
        .child(
            Icon::new(if collapsed {
                IconName::ChevronRight
            } else {
                IconName::ChevronDown
            })
            .size_3()
            .flex_none()
            .text_color(muted),
        )
        .child(
            div()
                .text_size(px(super::tokens::TS_XS))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(muted)
                .child(title),
        )
        .child(div().flex_1())
        .child(
            div()
                .flex_none()
                .text_size(px(super::tokens::TS_XS))
                .text_color(muted)
                .child(count.to_string()),
        )
        .into_any_element()
}

pub struct MainPage {
    pub(crate) focus_handle: FocusHandle,
    /// 主密码（解锁后由 SapApp 传入；Drop 时内存清零）
    master_password: String,
    /// 搜索输入
    search_input: Entity<InputState>,
    search_query: String,
    /// 当前分组筛选：None=全部，Some("favorites")=收藏，Some(group_id)=指定分组
    selected_group: Option<String>,
    /// 全量凭证（已排序：置顶 → 收藏 → 登录次数 → 最后登录）
    credentials: Vec<Credential>,
    /// 分组
    groups: Vec<Group>,
    /// 默认分组 ID（选中时显示全部，与 Tauri 版一致）
    default_group_id: Option<String>,
    /// 筛选结果（数据/搜索/分组变化时重算）
    filtered: Vec<Credential>,
    /// 置顶的凭据 ID
    pinned: HashSet<String>,
    /// 当前排序方式
    sort_by: SortBy,
    /// 多选中的凭据 ID
    pub(crate) selected: HashSet<String>,
    /// 最近一次点选的卡片序号（Shift 范围选择基准）
    pub(crate) last_selected_ix: Option<usize>,
    /// 登录进行中（防重复触发）
    logging_in: bool,
    /// 正在登录的凭据 ID（单条登录时卡片按钮显示「登录中…」并禁用）
    logging_in_id: Option<String>,
    /// 刚复制密码成功的凭据 ID（卡片显示 ✓，1.5s 后清除）
    copied_id: Option<String>,
    /// 当前悬停的卡片序号（控制置顶/收藏按钮悬停显示）
    pub(crate) hovered_card: Option<usize>,
    /// 分组下拉弹层：系统分组分区折叠状态
    collapsed_system: bool,
    /// 分组下拉弹层：自定义分组分区折叠状态
    collapsed_custom: bool,
    /// 分组下拉触发器实测宽度（on_prepaint 每帧记录，弹层与触发器同宽）
    group_pop_width: f32,
    /// 卡片列表滚动 handle（uniform_list 每帧复用，供滚动条 overlay 读取位置）
    list_scroll: UniformListScrollHandle,
    /// 分组下拉弹层滚动 handle
    group_menu_scroll: ScrollHandle,
    /// 批量移动弹层滚动 handle
    batch_move_scroll: ScrollHandle,
    _search_subscription: Subscription,
}

/// 销毁时把主密码内存清零（锁定重建/退出时防明文残留堆内存）
impl Drop for MainPage {
    fn drop(&mut self) {
        use zeroize::Zeroize as _;
        self.master_password.zeroize();
    }
}

impl MainPage {
    pub fn new(
        master_password: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let store = AppState::global(cx).store.clone();
        let credentials = cred_cmd::get_credentials(&store).unwrap_or_default();
        let groups = cred_cmd::get_groups(&store).unwrap_or_default();
        let (default_group_setting, default_group_id) = {
            let s = store.lock();
            (
                s.settings.default_group.clone(),
                sap_backend::storage::get_default_group_id(&s),
            )
        };

        // 初始分组：设置中的默认分组（校验存在）
        let selected_group = match default_group_setting.as_str() {
            "" => None,
            "favorites" => Some("favorites".to_string()),
            gid => {
                if groups.iter().any(|g| g.id == gid) {
                    Some(gid.to_string())
                } else {
                    None
                }
            }
        };

        let search_input = cx.new(|cx| {
            InputState::new(window, cx).placeholder(t("搜索 名称/系统/用户名/描述/服务器…"))
        });
        let _search_subscription =
            cx.subscribe_in(&search_input, window, |this, _, event, _, cx| {
                if matches!(event, InputEvent::Change) {
                    this.search_query = this.search_input.read(cx).value().to_string();
                    this.refilter();
                    cx.notify();
                }
            });

        let pinned: HashSet<String> = {
            let s = store.lock();
            s.settings.pinned_credential_ids.iter().cloned().collect()
        };

        let mut page = Self {
            focus_handle: cx.focus_handle(),
            master_password,
            search_input,
            search_query: String::new(),
            selected_group,
            credentials,
            groups,
            default_group_id,
            filtered: Vec::new(),
            pinned,
            sort_by: SortBy::Default,
            selected: HashSet::new(),
            last_selected_ix: None,
            logging_in: false,
            logging_in_id: None,
            copied_id: None,
            hovered_card: None,
            collapsed_system: false,
            collapsed_custom: false,
            group_pop_width: 280.0,
            list_scroll: UniformListScrollHandle::new(),
            group_menu_scroll: ScrollHandle::new(),
            batch_move_scroll: ScrollHandle::new(),
            _search_subscription,
        };
        page.refilter();
        page
    }

    /// 重新加载存储数据（凭证/分组）并重算筛选
    pub fn refresh(&mut self, cx: &mut Context<Self>) {
        let store = AppState::global(cx).store.clone();
        self.credentials = cred_cmd::get_credentials(&store).unwrap_or_default();
        self.groups = cred_cmd::get_groups(&store).unwrap_or_default();
        {
            let s = store.lock();
            self.default_group_id = sap_backend::storage::get_default_group_id(&s);
            self.pinned = s.settings.pinned_credential_ids.iter().cloned().collect();
        }
        // 分组被删除时回退到全部
        if let Some(ref gid) = self.selected_group {
            if gid != "favorites" && !self.groups.iter().any(|g| &g.id == gid) {
                self.selected_group = None;
            }
        }
        self.refilter();
        cx.notify();
    }

    /// 重算筛选结果（搜索 + 分组）
    fn refilter(&mut self) {
        let q = self.search_query.to_lowercase();
        let sel = self.selected_group.clone();

        // 结构化搜索语法：env:生产（中文别名或 production 等原值）、grp:分组名，
        // 可多个叠加（AND）；其余词合并后保持原「整串 contains」行为
        let mut env_filters: Vec<String> = Vec::new();
        let mut grp_filters: Vec<String> = Vec::new();
        let mut plain_terms: Vec<&str> = Vec::new();
        for tok in q.split_whitespace() {
            if let Some(v) = tok.strip_prefix("env:") {
                if !v.is_empty() {
                    env_filters.push(env_alias(v));
                }
            } else if let Some(v) = tok.strip_prefix("grp:") {
                if !v.is_empty() {
                    grp_filters.push(v.to_string());
                }
            } else {
                plain_terms.push(tok);
            }
        }
        let plain_q = plain_terms.join(" ");
        let structured = !env_filters.is_empty() || !grp_filters.is_empty();

        self.filtered = self
            .credentials
            .iter()
            .filter(|c| {
                // 搜索匹配（与 Tauri 版字段一致）
                let ms = if q.is_empty() {
                    true
                } else {
                    let display = c.display_name.clone().unwrap_or_default();
                    let group_name = c
                        .group_id
                        .as_ref()
                        .and_then(|gid| self.groups.iter().find(|g| &g.id == gid))
                        .map(|g| g.group_name.clone())
                        .unwrap_or_default();
                    // 译名参与匹配：英文界面搜 "Production" 也能命中系统分组
                    let group_display = c
                        .group_id
                        .as_ref()
                        .and_then(|gid| self.groups.iter().find(|g| &g.id == gid))
                        .map(group_display_name)
                        .unwrap_or_default();
                    // 结构化条件：全部满足（AND）
                    if !env_filters.iter().all(|e| c.environment == *e) {
                        return false;
                    }
                    if !grp_filters.iter().all(|g| {
                        group_name.to_lowercase().contains(g)
                            || group_display.to_lowercase().contains(g)
                    }) {
                        return false;
                    }
                    // 仅有结构化条件时直接通过；否则剩余词走原字段整串匹配
                    if structured && plain_q.is_empty() {
                        true
                    } else {
                        [
                            display,
                            c.connection_id.clone(),
                            c.system_id.clone(),
                            c.username.clone(),
                            c.app_server.clone(),
                            c.message_server.clone(),
                            c.system_number.clone(),
                            c.client.clone(),
                            c.description.clone(),
                            c.saprouter.clone(),
                            c.logon_group.clone(),
                            c.snc_name.clone(),
                            c.environment.clone(),
                            group_name,
                            group_display,
                        ]
                        .iter()
                        .any(|f| !f.is_empty() && f.to_lowercase().contains(&plain_q))
                    }
                };
                if !ms {
                    return false;
                }
                // 分组匹配（与 Tauri 版逻辑一致）
                match sel.as_deref() {
                    None | Some("favorites") => true,
                    Some(gid) if Some(gid) == self.default_group_id.as_deref() => true,
                    Some(gid) => {
                        if let Some(env) = system_group_env(gid) {
                            c.environment == env
                        } else {
                            c.group_id.as_deref() == Some(gid)
                        }
                    }
                }
            })
            .filter(|c| self.selected_group.as_deref() != Some("favorites") || c.is_favorite)
            .cloned()
            .collect();

        // 排序（置顶项稳定排最前）
        match self.sort_by {
            SortBy::Default => {
                // 默认排序：
                // 1) 分组内有登录记录 → 登录次数从高到低（同次数按环境分组序，再按最近登录）
                // 2) 全都没登录过 → 系统分组顺序：配置→开发→测试→正式→未分组
                // 3) 全为未分组 → 按名称从上到下
                let any_login = self.filtered.iter().any(|c| c.login_count > 0);
                let all_ungrouped = self
                    .filtered
                    .iter()
                    .all(|c| c.group_id.as_deref().is_none_or(str::is_empty));
                if all_ungrouped {
                    let key = |c: &Credential| {
                        c.display_name
                            .clone()
                            .filter(|s| !s.is_empty())
                            .unwrap_or_else(|| c.connection_id.clone())
                            .to_lowercase()
                    };
                    self.filtered.sort_by_key(|c| key(c));
                } else if any_login {
                    self.filtered.sort_by(|a, b| {
                        b.login_count
                            .cmp(&a.login_count)
                            .then_with(|| env_rank(&a.environment).cmp(&env_rank(&b.environment)))
                            .then_with(|| b.last_login_at.cmp(&a.last_login_at))
                    });
                } else {
                    self.filtered.sort_by_key(|c| env_rank(&c.environment));
                }
            }
            SortBy::Frequently => self
                .filtered
                .sort_by_key(|c| std::cmp::Reverse(c.login_count)),
            SortBy::Recent => self
                .filtered
                .sort_by_key(|c| std::cmp::Reverse(c.last_login_at)),
            SortBy::Name => self.filtered.sort_by(|a, b| {
                let key = |c: &Credential| {
                    c.display_name
                        .clone()
                        .filter(|s| !s.is_empty())
                        .unwrap_or_else(|| c.connection_id.clone())
                        .to_lowercase()
                };
                key(a).cmp(&key(b))
            }),
        }
        // 置顶项恒定排最前——默认排序除外：
        // 默认排序严格按既定规则（登录次数降序等），不被置顶卡打断
        if self.sort_by != SortBy::Default {
            let pinned = &self.pinned;
            self.filtered.sort_by_key(|c| !pinned.contains(&c.id));
        }

        // 清理已删除凭据的选中态
        let ids: HashSet<&String> = self.filtered.iter().map(|c| &c.id).collect();
        self.selected.retain(|id| ids.contains(id));
    }

    /// 当前选中的凭据 ID 列表
    fn selected_ids(&self) -> Vec<String> {
        self.selected.iter().cloned().collect()
    }

    /// 是否有登录任务进行中（自动锁定据此跳过，避免打断批量登录）
    pub(crate) fn is_logging_in(&self) -> bool {
        self.logging_in
    }

    /// 清空多选
    fn clear_selection(&mut self, cx: &mut Context<Self>) {
        self.selected.clear();
        self.last_selected_ix = None;
        cx.notify();
    }

    /// 全选 / 取消全选（当前展示的全部凭据）
    fn toggle_select_all(&mut self, cx: &mut Context<Self>) {
        if !self.filtered.is_empty()
            && self.selected.len() == self.filtered.len()
        {
            self.selected.clear();
        } else {
            self.selected = self.filtered.iter().map(|c| c.id.clone()).collect();
        }
        cx.notify();
    }

    /// Ctrl 点选：切换单个凭据选中态
    pub(crate) fn toggle_select(&mut self, cred_id: String, ix: usize, cx: &mut Context<Self>) {
        if !self.selected.remove(&cred_id) {
            self.selected.insert(cred_id);
        }
        self.last_selected_ix = Some(ix);
        cx.notify();
    }

    /// Shift 范围选择（从上次点选卡片到当前卡片）
    pub(crate) fn select_range(&mut self, ix: usize, cx: &mut Context<Self>) {
        let Some(start) = self.last_selected_ix else {
            self.selected.insert(
                self.filtered.get(ix).map(|c| c.id.clone()).unwrap_or_default(),
            );
            self.last_selected_ix = Some(ix);
            cx.notify();
            return;
        };
        let (lo, hi) = (start.min(ix), start.max(ix));
        if let Some(slice) = self.filtered.get(lo..=hi.min(self.filtered.len().saturating_sub(1))) {
            for c in slice {
                self.selected.insert(c.id.clone());
            }
        }
        cx.notify();
    }

    // === 业务操作（供卡片 / 批量操作栏调用） ===

    /// 登录 SAP（后台线程执行 sapshcut）
    pub fn login(&mut self, cred_id: String, window: &mut Window, cx: &mut Context<Self>) {
        if self.logging_in {
            window.push_notification(t("正在登录中，请稍候…"), cx);
            return;
        }
        self.logging_in = true;
        self.logging_in_id = Some(cred_id.clone());
        cx.notify();

        let store = AppState::global(cx).store.clone();
        let mp = self.master_password.clone();

        cx.spawn_in(window, async move |this, window| {
            let result = window
                .background_executor()
                .spawn(async move { sap_cmd::login_to_sap(&store, mp, cred_id) })
                .await;

            _ = this.update_in(window, |page: &mut Self, window, cx| {
                page.logging_in = false;
                page.logging_in_id = None;
                match result {
                    Ok(()) => window.push_notification(t("已发起 SAP 登录"), cx),
                    Err(e) => window.push_notification(tf("登录失败：{e}", &[&e.to_string()]), cx),
                }
                page.refresh(cx);
            });
        })
        .detach();
    }

    /// 切换收藏
    pub fn toggle_favorite(
        &mut self,
        cred_id: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let store = AppState::global(cx).store.clone();
        match cred_cmd::toggle_favorite(&store, cred_id) {
            Ok(_) => self.refresh(cx),
            Err(e) => window.push_notification(tf("操作失败：{e}", &[&e.to_string()]), cx),
        }
    }

    /// 切换置顶（持久化到设置）
    pub fn toggle_pin(&mut self, cred_id: String, window: &mut Window, cx: &mut Context<Self>) {
        let store = AppState::global(cx).store.clone();
        let settings = {
            let mut s = store.lock();
            let pinned = &mut s.settings.pinned_credential_ids;
            match pinned.iter().position(|i| i == &cred_id) {
                Some(ix) => {
                    pinned.remove(ix);
                }
                None => pinned.push(cred_id),
            }
            s.settings.clone()
        };
        if let Err(e) = settings_cmd::save_settings(&store, settings) {
            window.push_notification(tf("置顶失败：{e}", &[&e.to_string()]), cx);
        }
        self.refresh(cx);
    }

    /// 打开编辑表单（右侧面板，内部滚动，小窗口不丢底栏）
    pub fn open_edit(&mut self, cred_id: String, window: &mut Window, cx: &mut Context<Self>) {
        let mp = self.master_password.clone();
        crate::ui::open_credential_panel(Some(cred_id), mp, window, cx);
    }

    /// 复制凭据（副本）
    pub fn duplicate(&mut self, cred_id: String, window: &mut Window, cx: &mut Context<Self>) {
        let store = AppState::global(cx).store.clone();
        let mp = self.master_password.clone();

        let cred = {
            let s = store.lock();
            s.credentials.iter().find(|c| c.id == cred_id).cloned()
        };
        let Some(mut c) = cred else {
            return;
        };

        // 显示名加"副本"后缀
        let base_name = c
            .display_name
            .clone()
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| c.connection_id.clone());
        c.display_name = Some(tf("{base_name} (副本)", &[&base_name.to_string()]));
        c.id = String::new();
        // 解密原密码为明文，交由 add_credential 重新加密
        if !c.encrypted_password.is_empty() {
            match sap_backend::storage::decrypt_credential_password(
                &store.lock(),
                &mp,
                &c.encrypted_password,
            ) {
                Ok(pw) => {
                    c.password = pw;
                    c.encrypted_password = String::new();
                }
                Err(e) => {
                    window.push_notification(tf("复制失败：{e}", &[&e.to_string()]), cx);
                    return;
                }
            }
        }

        match cred_cmd::add_credential(&store, mp, c) {
            Ok(_) => {
                self.refresh(cx);
                window.push_notification(t("已创建凭据副本"), cx);
            }
            Err(e) => window.push_notification(tf("复制失败：{e}", &[&e.to_string()]), cx),
        }
    }

    /// 复制解密后的密码到剪贴板（按设置自动清空；成功后卡片显示 ✓ 1.5 秒）
    pub fn copy_password(&mut self, cred_id: String, window: &mut Window, cx: &mut Context<Self>) {
        let store = AppState::global(cx).store.clone();
        let mp = self.master_password.clone();
        let seconds = {
            let s = store.lock();
            s.settings.clipboard_clear_seconds
        };
        let copied_cred_id = cred_id.clone();

        cx.spawn_in(window, async move |this, window| {
            let result = window
                .background_executor()
                .spawn(async move { cred_cmd::get_decrypted_password(&store, mp, cred_id) })
                .await;

            match result {
                Ok(pw) if !pw.is_empty() => {
                    if let Ok(mut cb) = arboard::Clipboard::new() {
                        let _ = cb.set_text(pw.clone());
                    }
                    let marked = copied_cred_id.clone();
                    _ = this.update_in(window, |page: &mut Self, window, cx| {
                        // 卡片 ✓ 原地反馈 + 全局通知双通道
                        page.copied_id = Some(marked);
                        window.push_notification(t("密码已复制到剪贴板"), cx);
                        cx.notify();
                    });
                    // 1.5s 后清除 ✓（期间复制了其他凭据则保留新标记）
                    window
                        .background_executor()
                        .timer(std::time::Duration::from_secs_f64(1.5))
                        .await;
                    _ = this.update_in(window, |page: &mut Self, _, cx| {
                        if page.copied_id.as_deref() == Some(copied_cred_id.as_str()) {
                            page.copied_id = None;
                            cx.notify();
                        }
                    });
                    // 自动清空（内容未被覆盖时）
                    if seconds > 0 {
                        window
                            .background_executor()
                            .timer(std::time::Duration::from_secs(seconds as u64))
                            .await;
                        if let Ok(mut cb) = arboard::Clipboard::new() {
                            if let Ok(cur) = cb.get_text() {
                                if cur == pw {
                                    let _ = cb.set_text(String::new());
                                }
                            }
                        }
                    }
                }
                Ok(_) => {
                    _ = this.update_in(window, |_: &mut Self, window, cx| {
                        window.push_notification(t("该凭据未设置密码"), cx);
                    });
                }
                Err(e) => {
                    _ = this.update_in(window, |_: &mut Self, window, cx| {
                        window.push_notification(tf("复制密码失败：{e}", &[&e.to_string()]), cx);
                    });
                }
            }
        })
        .detach();
    }

    /// 删除凭据（通知带撤销按钮）
    pub fn delete_credential(
        &mut self,
        cred_id: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let store = AppState::global(cx).store.clone();
        let saved = {
            let s = store.lock();
            s.credentials.iter().find(|c| c.id == cred_id).cloned()
        };
        let Some(saved) = saved else { return };

        match cred_cmd::delete_credential(&store, cred_id.clone()) {
            Ok(()) => {
                self.refresh(cx);
                Self::push_delete_undo_notification(vec![saved], store, cx.entity().downgrade(), window, cx);
            }
            Err(e) => window.push_notification(tf("删除失败：{e}", &[&e.to_string()]), cx),
        }
    }

    /// 删除通知（带撤销按钮）
    fn push_delete_undo_notification(
        saved: Vec<Credential>,
        store: sap_backend::Store,
        weak: WeakEntity<Self>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let display = if saved.len() == 1 {
            let name = saved[0]
                .display_name
                .clone()
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| saved[0].connection_id.clone());
            tf("已删除「{name}」", &[&name.to_string()])
        } else {
            tf("已删除 {} 个凭据", &[&(saved.len()).to_string()])
        };

        window.push_notification(
            Notification::new()
                .title(display)
                .action(move |_, _, _| {
                    let store = store.clone();
                    let weak = weak.clone();
                    let saved = saved.clone();
                    Button::new("undo-delete")
                        .ghost()
                        .small()
                        .label(t("撤销"))
                        .on_click(move |_, window, cx| {
                            match cred_cmd::restore_credentials(&store, saved.clone()) {
                                Ok(n) if n > 0 => {
                                    window.push_notification(t("已恢复该凭据"), cx);
                                    if let Some(page) = weak.upgrade() {
                                        page.update(cx, |p, cx| p.refresh(cx));
                                    }
                                }
                                _ => {
                                    window.push_notification(t("恢复失败"), cx);
                                }
                            }
                        })
                }),
            cx,
        );
    }

    // === 批量操作（对应 Tauri 版批量工具栏） ===

    /// 批量登录（逐条执行，间隔取设置）
    pub fn batch_login(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let ids = self.selected_ids();
        if ids.is_empty() {
            return;
        }
        if self.logging_in {
            window.push_notification(t("正在登录中，请稍候…"), cx);
            return;
        }
        self.logging_in = true;
        let count = ids.len();
        cx.notify();

        let store = AppState::global(cx).store.clone();
        let mp = self.master_password.clone();
        let interval = {
            let s = store.lock();
            s.settings.batch_login_interval as u64
        };
        window.push_notification(tf("开始批量登录 {count} 个凭据…", &[&count.to_string()]), cx);

        cx.spawn_in(window, async move |this, window| {
            let result = window
                .background_executor()
                .spawn(async move { cred_cmd::batch_login(&store, mp, ids, interval) })
                .await;

            _ = this.update_in(window, |page: &mut Self, window, cx| {
                page.logging_in = false;
                page.clear_selection(cx);
                match result {
                    Ok(n) => {
                        window.push_notification(tf("批量登录完成：成功 {n} 个", &[&n.to_string()]), cx)
                    }
                    Err(e) => window.push_notification(tf("批量登录中断：{e}", &[&e.to_string()]), cx),
                }
                page.refresh(cx);
            });
        })
        .detach();
    }

    /// 批量收藏 / 取消收藏
    pub fn batch_favorite(
        &mut self,
        favorite: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let ids = self.selected_ids();
        if ids.is_empty() {
            return;
        }
        let store = AppState::global(cx).store.clone();
        match cred_cmd::batch_favorite(&store, ids, favorite) {
            Ok(n) => {
                window.push_notification(
                    if favorite {
                        tf("已收藏 {n} 个凭据", &[&n.to_string()])
                    } else {
                        tf("已取消收藏 {n} 个凭据", &[&n.to_string()])
                    },
                    cx,
                );
                self.refresh(cx);
            }
            Err(e) => window.push_notification(tf("操作失败：{e}", &[&e.to_string()]), cx),
        }
    }

    /// 批量移动到分组
    pub fn batch_move(
        &mut self,
        group_id: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let ids = self.selected_ids();
        if ids.is_empty() {
            return;
        }
        let store = AppState::global(cx).store.clone();
        // 空分组 ID → 默认分组
        let target = if group_id.is_empty() {
            let s = store.lock();
            sap_backend::storage::get_default_group_id(&s)
                .unwrap_or_else(|| group_id.clone())
        } else {
            group_id
        };
        match cred_cmd::batch_move_to_group(&store, ids, target) {
            Ok(n) => {
                window.push_notification(tf("已移动 {n} 个凭据", &[&n.to_string()]), cx);
                self.clear_selection(cx);
                self.refresh(cx);
            }
            Err(e) => window.push_notification(tf("移动失败：{e}", &[&e.to_string()]), cx),
        }
    }

    /// 批量分享连接信息（不含密码，逐条写入剪贴板）
    pub fn batch_share(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let ids = self.selected_ids();
        if ids.is_empty() {
            return;
        }
        let store = AppState::global(cx).store.clone();
        let mut ok = 0;
        for id in &ids {
            if cred_cmd::share_credential(&store, id.clone()).is_ok() {
                ok += 1;
            }
        }
        window.push_notification(
            tf("已分享 {ok}/{} 个连接信息到剪贴板", &[&ok.to_string(), &(ids.len()).to_string()]),
            cx,
        );
        self.clear_selection(cx);
    }

    /// 批量删除（通知带撤销）
    pub fn batch_delete(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let ids = self.selected_ids();
        if ids.is_empty() {
            return;
        }
        let store = AppState::global(cx).store.clone();
        let snapshot: Vec<Credential> = {
            let s = store.lock();
            s.credentials
                .iter()
                .filter(|c| ids.contains(&c.id))
                .cloned()
                .collect()
        };
        let mut failed = 0;
        for id in &ids {
            if cred_cmd::delete_credential(&store, id.clone()).is_err() {
                failed += 1;
            }
        }
        self.clear_selection(cx);
        self.refresh(cx);
        if snapshot.len() > failed {
            Self::push_delete_undo_notification(
                snapshot,
                store,
                cx.entity().downgrade(),
                window,
                cx,
            );
        }
        if failed > 0 {
            window.push_notification(tf("{failed} 个凭据删除失败", &[&failed.to_string()]), cx);
        }
    }

    // === 渲染 ===

    /// 当前选中分组的显示名
    fn current_group_label(&self) -> String {
        match self.selected_group.as_deref() {
            None => t("全部").to_string(),
            Some("favorites") => t("收藏").to_string(),
            Some(gid) => self
                .groups
                .iter()
                .find(|g| g.id == gid)
                .map(group_display_name)
                .unwrap_or_else(|| t("全部").to_string()),
        }
    }

    /// 分组下拉：Popover 自绘弹层
    ///
    /// 结构：顶部固定项（全部/收藏）+ 系统分组分区 + 自定义分组分区，
    /// 分区头可点击折叠（状态存 MainPage，会话内记忆）；每项带数量徽标。
    /// 不放「管理分组」入口（管理走活动栏）。触发器 flex_1 与搜索框等宽；
    /// 弹层 TopLeft 向下展开，顶端低于分组行（同 Tauri 版）。
    /// 弹层宽度跟随触发器：容器 on_prepaint 实测宽度存 group_pop_width
    /// （每帧更新、不 notify 防循环），content 闭包读取后设 panel 宽
    /// （同 gpui-kit Select 的 menu_width=Auto 机制），下限 220px 兜底。
    fn render_group_select(&self, cx: &Context<Self>) -> impl IntoElement + use<> {
        let trigger_watcher = cx.entity();
        let weak = cx.entity().downgrade();

        // 触发器标签：过长分组名截断（按钮不自动截断）
        let current_label = self.current_group_label();
        let short_label = if current_label.chars().count() > 12 {
            let s: String = current_label.chars().take(12).collect();
            format!("{s}…")
        } else {
            current_label.clone()
        };
        let trigger_label = format!("{short_label} · {}", self.filtered.len());

        div()
            .flex_1()
            .min_w_0()
            .overflow_hidden()
            .rounded(px(6.))
            .on_prepaint(move |bounds, _, cx| {
                trigger_watcher.update(cx, |page, _| {
                    page.group_pop_width = f32::from(bounds.size.width);
                });
            })
            .child(
                Popover::new("group-popover")
                    // 抵消 popover_style 自带 p_3（左右共 24px+边框），否则弹层
                    // 总宽 = panel 宽 + 26px，比触发器宽出一截
                    .p_0()
                    .anchor(Anchor::TopLeft)
                    .trigger(
                        Button::new("group-select-trigger")
                            .secondary()
                            .small()
                            .icon(IconName::FolderInput)
                            .dropdown_caret(true)
                            .label(trigger_label)
                            .tooltip(t("分组筛选"))
                            .w_full(),
                    )
                    .content(move |_, _, cx| {
                        let pop_state = cx.entity();
                        let weak = weak.clone();
                        let Some(page) = weak.upgrade() else {
                            return div().into_any_element();
                        };
                        let (hover_bg, muted, primary) = {
                            let th = cx.theme();
                            (th.secondary, th.muted_foreground, th.primary)
                        };

                        // 读取 MainPage 当前状态（作用域结束只保留计算结果）
                        struct Row {
                            id: String,
                            name: String,
                            count: usize,
                        }
                        let (sel, all_count, fav_count, sys_rows, custom_rows, collapsed_system, collapsed_custom, pop_w) = {
                            let p = page.read(cx);
                            let row = |g: &Group| Row {
                                id: g.id.clone(),
                                name: group_display_name(g),
                                count: if let Some(env) = system_group_env(&g.id) {
                                    p.credentials.iter().filter(|c| c.environment == env).count()
                                } else if p.default_group_id.as_deref() == Some(g.id.as_str()) {
                                    p.credentials.len()
                                } else {
                                    p.credentials
                                        .iter()
                                        .filter(|c| c.group_id.as_deref() == Some(g.id.as_str()))
                                        .count()
                                },
                            };
                            (
                                p.selected_group.clone(),
                                p.credentials.len(),
                                p.credentials.iter().filter(|c| c.is_favorite).count(),
                                p.groups.iter().filter(|g| g.is_system || g.is_default).map(row).collect::<Vec<Row>>(),
                                p.groups.iter().filter(|g| !g.is_system && !g.is_default).map(row).collect::<Vec<Row>>(),
                                p.collapsed_system,
                                p.collapsed_custom,
                                p.group_pop_width.max(220.0),
                            )
                        };

                        // 弹层面板：与触发器同宽（实测宽度，下限 220px 兜底）；
                        // px_1 给 hover 行留 4px 边距（外层 p_3 已被 p_0 抵消）；
                        // 可滚动（分组多时不撑爆窗口）+ 滚动条指示
                        let gm_scroll = page.read(cx).group_menu_scroll.clone();
                        let mut panel = v_flex()
                            .id("group-menu")
                            .w(px(pop_w))
                            .px_1()
                            .max_h(px(420.))
                            .overflow_y_scroll()
                            .track_scroll(&gm_scroll)
                            .relative()
                            .vertical_scrollbar(&gm_scroll)
                            .py_1();

                        // 顶部固定项：全部 / 收藏（点击选中并关闭弹层）
                        panel = panel.child(group_menu_row(
                            "group-item-all".into(),
                            t("全部").to_string(),
                            all_count,
                            sel.is_none(),
                            {
                                let weak = weak.clone();
                                let pop = pop_state.clone();
                                move |_, window, cx| {
                                    let _ = weak.update(cx, |page, cx| {
                                        page.selected_group = None;
                                        page.refilter();
                                        cx.notify();
                                    });
                                    pop.update(cx, |s, cx| s.dismiss(window, cx));
                                }
                            },
                            hover_bg,
                            muted,
                            primary,
                        ));
                        panel = panel.child(group_menu_row(
                            "group-item-fav".into(),
                            t("收藏").to_string(),
                            fav_count,
                            sel.as_deref() == Some("favorites"),
                            {
                                let weak = weak.clone();
                                let pop = pop_state.clone();
                                move |_, window, cx| {
                                    let _ = weak.update(cx, |page, cx| {
                                        page.selected_group = Some("favorites".to_string());
                                        page.refilter();
                                        cx.notify();
                                    });
                                    pop.update(cx, |s, cx| s.dismiss(window, cx));
                                }
                            },
                            hover_bg,
                            muted,
                            primary,
                        ));

                        // 系统分组分区（头可折叠，点击不关闭弹层）
                        panel = panel.child(group_menu_section(
                            "group-sec-system",
                            t("系统分组").to_string(),
                            sys_rows.len(),
                            collapsed_system,
                            {
                                let weak = weak.clone();
                                move |_, _, cx| {
                                    let _ = weak.update(cx, |page, cx| {
                                        page.collapsed_system = !page.collapsed_system;
                                        cx.notify();
                                    });
                                }
                            },
                            hover_bg,
                            muted,
                        ));
                        if !collapsed_system {
                            for g in &sys_rows {
                                panel = panel.child(group_menu_row(
                                    format!("group-item-{}", g.id),
                                    g.name.clone(),
                                    g.count,
                                    sel.as_deref() == Some(g.id.as_str()),
                                    {
                                        let weak = weak.clone();
                                        let pop = pop_state.clone();
                                        let gid = g.id.clone();
                                        move |_, window, cx| {
                                            let _ = weak.update(cx, |page, cx| {
                                                page.selected_group = Some(gid.clone());
                                                page.refilter();
                                                cx.notify();
                                            });
                                            pop.update(cx, |s, cx| s.dismiss(window, cx));
                                        }
                                    },
                                    hover_bg,
                                    muted,
                                    primary,
                                ));
                            }
                        }

                        // 自定义分组分区（头可折叠）
                        panel = panel.child(group_menu_section(
                            "group-sec-custom",
                            t("自定义分组").to_string(),
                            custom_rows.len(),
                            collapsed_custom,
                            {
                                let weak = weak.clone();
                                move |_, _, cx| {
                                    let _ = weak.update(cx, |page, cx| {
                                        page.collapsed_custom = !page.collapsed_custom;
                                        cx.notify();
                                    });
                                }
                            },
                            hover_bg,
                            muted,
                        ));
                        if collapsed_custom {
                            // 折叠时不渲染条目
                        } else if custom_rows.is_empty() {
                            panel = panel.child(
                                div()
                                    .px_2()
                                    .py(px(4.))
                                    .pl(px(22.))
                                    .text_size(px(super::tokens::TS_XS))
                                    .text_color(muted)
                                    .child(t("（暂无自定义分组）")),
                            );
                        } else {
                            for g in &custom_rows {
                                panel = panel.child(group_menu_row(
                                    format!("group-item-{}", g.id),
                                    g.name.clone(),
                                    g.count,
                                    sel.as_deref() == Some(g.id.as_str()),
                                    {
                                        let weak = weak.clone();
                                        let pop = pop_state.clone();
                                        let gid = g.id.clone();
                                        move |_, window, cx| {
                                            let _ = weak.update(cx, |page, cx| {
                                                page.selected_group = Some(gid.clone());
                                                page.refilter();
                                                cx.notify();
                                            });
                                            pop.update(cx, |s, cx| s.dismiss(window, cx));
                                        }
                                    },
                                    hover_bg,
                                    muted,
                                    primary,
                                ));
                            }
                        }

                        panel.into_any_element()
                    }),
            )
    }

    /// 排序下拉：默认 / 常用 / 最近登录 / 名称
    fn render_sort_select(&self, cx: &Context<Self>) -> impl IntoElement + use<> {
        let current = self.sort_by;
        let weak = cx.entity().downgrade();
        Button::new("sort-select")
            .secondary()
            .small()
            .icon(IconName::ArrowUpDown)
            .label(current.label())
            .tooltip(t("排序方式"))
            .dropdown_menu(move |menu, _, _| {
                let weak = weak.clone();
                let mut menu = menu.label(t("排序方式"));
                for key in [
                    SortBy::Default,
                    SortBy::Frequently,
                    SortBy::Recent,
                    SortBy::Name,
                ] {
                    let checked = key == current;
                    let weak = weak.clone();
                    menu = menu.item(
                        PopupMenuItem::new(key.label())
                            .checked(checked)
                            .on_click(move |_, _, cx| {
                                if let Some(page) = weak.upgrade() {
                                    page.update(cx, |page, cx| {
                                        page.sort_by = key;
                                        page.refilter();
                                        cx.notify();
                                    });
                                }
                            }),
                    );
                }
                menu
            })
            .anchor(Anchor::BottomLeft)
    }

    /// 批量操作栏（底部居中浮动胶囊，参考 Tauri 版 batch-toolbar）
    ///
    /// 改造要点：
    /// - absolute 浮动定位，不占列表行高（原版贴底横条会少看一条凭据）
    /// - 胶囊圆角 + 白底 + 边框，浮在主界面灰底上层次清晰
    /// - 分组分隔竖线：计数 | 全选 + 中间操作 | 取消选择
    /// - 计数用 primary 色强调；主操作（登录）primary 按钮；删除 danger ghost
    fn render_batch_bar(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let count = self.selected.len();
        let weak = cx.entity().downgrade();

        // 外层：absolute 浮动定位 + 水平居中（w_full 占满父宽，justify_center 让胶囊居中）
        // 内层：胶囊本体（rounded_full + 白底 + 边框）；max_w_full + min_w_0
        // 防止窄窗口下内容比窗口宽时向两侧溢出被裁切（截图问题）
        h_flex()
            .absolute()
            .bottom(px(18.))
            .left_0()
            .w_full()
            .justify_center()
            .px(px(8.))
            .child(
                h_flex()
                    .items_center()
                    .gap(px(2.))
                    .max_w_full()
                    .min_w_0()
                    .h(px(40.))
                    .px(px(10.))
                    .rounded_full()
                    .border_1()
                    .border_color(cx.theme().border)
                    .bg(cx.theme().popover)
                    // 计数（primary 色强调，窄窗口下缩短文案）
                    .child(
                        div()
                            .flex_none()
                            .text_size(px(super::tokens::TS_SM))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(cx.theme().primary)
                            .child(tf("已选 {count}", &[&count.to_string()])),
                    )
                    // 分组分隔竖线 1：计数 | 全选 + 中间操作
                    .child(div().w(px(1.)).h(px(20.)).bg(cx.theme().border))
                    .child(
                        Button::new("batch-select-all")
                            .ghost()
                            .small()
                            .icon(IconName::CheckCheck)
                            .tooltip(t("全选 / 取消全选"))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.toggle_select_all(cx);
                            })),
                    )
                    .child(
                        Button::new("batch-login")
                            .primary()
                            .small()
                            .icon(IconName::LogIn)
                            .tooltip(t("批量登录（逐条执行）"))
                            .disabled(self.logging_in)
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.batch_login(window, cx);
                            })),
                    )
                    .child(
                        Button::new("batch-favorite")
                            .ghost()
                            .small()
                            .icon(IconName::Star)
                            .tooltip(t("批量收藏"))
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.batch_favorite(true, window, cx);
                            })),
                    )
                    .child(
                        Button::new("batch-unfavorite")
                            .ghost()
                            .small()
                            .icon(IconName::StarOff)
                            .tooltip(t("批量取消收藏"))
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.batch_favorite(false, window, cx);
                            })),
                    )
                    .child(
                        // 移动到分组（Popover 自绘弹层）：PopupMenu 无限高，分组多时
                        // 弹出菜单超出窗口下缘被裁切（用户反馈）→ Popover 固定宽 +
                        // 限高 320px 滚动（同 render_group_select 成熟做法）；
                        // BottomRight 向上弹出（批量栏贴窗口底部，向下会出窗）
                        Popover::new("batch-move-popover")
                            .p_0()
                            .anchor(Anchor::BottomRight)
                            .trigger(
                                Button::new("batch-move")
                                    .ghost()
                                    .small()
                                    .icon(IconName::FolderInput)
                                    .tooltip(t("移动到分组")),
                            )
                            .content(move |_, _, cx| {
                                let pop = cx.entity();
                                let weak = weak.clone();
                                let Some(page) = weak.upgrade() else {
                                    return div().into_any_element();
                                };
                                // 每次渲染读取最新分组列表（首项 = 默认分组）
                                let rows: Vec<(String, String)> = {
                                    let p = page.read(cx);
                                    let mut opts =
                                        vec![("".to_string(), t("默认分组").to_string())];
                                    for g in &p.groups {
                                        if !g.is_system && !g.is_default {
                                            opts.push((g.id.clone(), g.group_name.clone()));
                                        }
                                    }
                                    opts
                                };
                                let hover_bg = cx.theme().secondary;
                                let bm_scroll = page.read(cx).batch_move_scroll.clone();
                                let mut panel = v_flex()
                                    .id("batch-move-menu")
                                    .w(px(220.))
                                    .px_1()
                                    .py_1()
                                    .max_h(px(320.))
                                    .overflow_y_scroll()
                                    .track_scroll(&bm_scroll)
                                    .relative()
                                    .vertical_scrollbar(&bm_scroll);
                                for (ix, (gid, name)) in rows.into_iter().enumerate() {
                                    let weak = weak.clone();
                                    let pop = pop.clone();
                                    panel = panel.child(
                                        h_flex()
                                            .id(format!("bm-row-{ix}"))
                                            .items_center()
                                            .px_2()
                                            .py(px(6.))
                                            .rounded(px(6.))
                                            .cursor_pointer()
                                            .hover(move |s| s.bg(hover_bg))
                                            .active(move |s| s.bg(hover_bg.darken(0.08)))
                                            .on_click(move |_, window, cx| {
                                                if let Some(page) = weak.upgrade() {
                                                    let gid = gid.clone();
                                                    page.update(cx, |page, cx| {
                                                        page.batch_move(gid, window, cx);
                                                    });
                                                }
                                                pop.update(cx, |s, cx| s.dismiss(window, cx));
                                            })
                                            .child(
                                                div()
                                                    .flex_1()
                                                    .min_w_0()
                                                    .truncate()
                                                    .text_size(px(super::tokens::TS_SM))
                                                    .child(name),
                                            ),
                                    );
                                }
                                panel.into_any_element()
                            }),
                    )
                    .child(
                        Button::new("batch-share")
                            .ghost()
                            .small()
                            .icon(IconName::Share2)
                            .tooltip(t("分享连接信息"))
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.batch_share(window, cx);
                            })),
                    )
                    .child(
                        Button::new("batch-delete")
                            .danger()
                            .ghost()
                            .small()
                            .icon(IconName::Trash)
                            .tooltip(t("批量删除"))
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.batch_delete(window, cx);
                            })),
                    )
                    // 分组分隔竖线 2：中间操作 | 取消选择
                    .child(div().w(px(1.)).h(px(20.)).bg(cx.theme().border))
                    .child(
                        Button::new("batch-clear")
                            .ghost()
                            .small()
                            .icon(IconName::X)
                            .tooltip(t("取消选择"))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.clear_selection(cx);
                            })),
                    ),
            )
    }
}

impl MainPage {
    /// 全局快捷键
    /// - Ctrl+N 新增凭据 / Ctrl+F 聚焦搜索 / Ctrl+L 锁定 / Ctrl+K 命令面板
    /// - Enter 登录唯一选中项 / ArrowUp·Down 上下移动选中
    /// - 搜索框有内容时（is_searching）不拦截 Enter/Arrow，避免与输入冲突
    fn handle_key_down(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // 本页拦截的快捷键不会冒泡到根视图，在此单独记录活动（自动锁定计时）
        crate::state::touch_activity(cx);
        let key: &str = event.keystroke.key.as_ref();
        let mods = &event.keystroke.modifiers;
        let is_searching = !self.search_query.is_empty();

        if mods.control && key == "n" {
            cx.stop_propagation();
            crate::ui::open_credential_panel(
                None,
                self.master_password.clone(),
                window,
                cx,
            );
        } else if mods.control && key == "k" {
            cx.stop_propagation();
            cx.dispatch_action(&crate::ui::OpenCommandPalette);
        } else if mods.control && key == "f" {
            cx.stop_propagation();
            let input = self.search_input.clone();
            input.update(cx, |s, cx| s.focus(window, cx));
        } else if mods.control && key == "l" {
            cx.stop_propagation();
            cx.dispatch_action(&crate::ui::LockApp);
        } else if key == "enter" && !is_searching && self.selected.len() == 1 {
            cx.stop_propagation();
            let id = self.selected.iter().next().unwrap().clone();
            self.login(id, window, cx);
        } else if key == "arrowdown" && !is_searching && !self.filtered.is_empty() {
            cx.stop_propagation();
            let cur = self.last_selected_ix.unwrap_or(0);
            let next = (cur + 1).min(self.filtered.len() - 1);
            if next != cur {
                self.selected.clear();
                if let Some(c) = self.filtered.get(next) {
                    self.selected.insert(c.id.clone());
                }
                self.last_selected_ix = Some(next);
                cx.notify();
            }
        } else if key == "arrowup" && !is_searching && !self.filtered.is_empty() {
            cx.stop_propagation();
            let cur = self.last_selected_ix.unwrap_or(0);
            let prev = cur.saturating_sub(1);
            if prev != cur {
                self.selected.clear();
                if let Some(c) = self.filtered.get(prev) {
                    self.selected.insert(c.id.clone());
                }
                self.last_selected_ix = Some(prev);
                cx.notify();
            }
        }
    }
}

impl Focusable for MainPage {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for MainPage {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let count = self.filtered.len();
        let selected_count = self.selected.len();
        let is_searching = !self.search_query.is_empty();
        // 空态三分类：真空态（无任何凭据）显示教学文案；搜索/分组筛选空给恢复建议
        let empty_all = self.credentials.is_empty();
        let filter_active = is_searching || self.selected_group.is_some();
        let any_selected = selected_count > 0;
        let compact = {
            let s = AppState::global(cx).store.lock();
            s.settings.compact_mode
        };
        // 卡片列表（uniform_list 虚拟滚动）
        let entity = cx.entity();
        let list = uniform_list(
            "cred-cards",
            self.filtered.len(),
            move |range, _window, cx| {
                let page = entity.read(cx);
                let weak = entity.clone().downgrade();
                range
                    .filter_map(|ix| {
                        let cred = page.filtered.get(ix)?;
                        let is_selected = page.selected.contains(&cred.id);
                        let is_pinned = page.pinned.contains(&cred.id);
                        let is_hovered = page.hovered_card == Some(ix);
                        let just_copied = page.copied_id.as_deref() == Some(cred.id.as_str());
                        Some(
                            div()
                                .px_3()
                                .pt_2()
                                .child(credential_card::render_card(
                                    cred,
                                    ix,
                                    is_selected,
                                    is_pinned,
                                    is_hovered,
                                    any_selected,
                                    compact,
                                    just_copied,
                                    page.logging_in_id.as_deref(),
                                    &weak,
                                    cx,
                                ))
                                .into_any_element(),
                        )
                    })
                    .collect()
            },
        )
        .track_scroll(&self.list_scroll)
        .flex_1()
        .min_h_0();

        // 列表容器（搜索/批量栏下的卡片区域）；
        // relative 锚定滚动条 overlay（ScrollbarLayer 为 absolute inset_0 叠加层）
        let list_area = v_flex()
            .flex_1()
            .min_h_0()
            .min_w_0()
            .relative()
            .pt_1()
            .vertical_scrollbar(&self.list_scroll)
            .when(selected_count > 0, |this| this.pb(px(64.)))
            .when(selected_count == 0, |this| this.pb_3())
            .when(count > 0, |this| this.child(list))
            .when(count == 0, |this| {
                this.child(
                    v_flex()
                        .flex_1()
                        .items_center()
                        .justify_center()
                        .gap_2()
                        .child(
                            Icon::new(if empty_all {
                                IconName::KeyRound
                            } else {
                                IconName::Search
                            })
                            .size_8()
                            .text_color(cx.theme().muted_foreground),
                        )
                        .child(
                            div()
                                .text_center()
                                .text_size(px(super::tokens::TS_BASE))
                                .text_color(cx.theme().muted_foreground)
                                .child(if empty_all {
                                    t("暂无凭据\n按 Ctrl+N 新增，或从 SAP 配置导入")
                                        .to_string()
                                } else if is_searching {
                                    t("未找到匹配的凭据").to_string()
                                } else {
                                    t("该分组暂无凭据").to_string()
                                }),
                        )
                        // 筛选空（非真空态）才给恢复建议；真空态的教学文案已含指引
                        .when(filter_active && !empty_all, |this| {
                            this.child(
                                div()
                                    .text_center()
                                    .text_size(px(super::tokens::TS_SM))
                                    .text_color(cx.theme().muted_foreground)
                                    .child(t("试试更换搜索关键词，或切换分组筛选").to_string()),
                            )
                        }),
                )
            })
            .when(selected_count > 0, |this| {
                this.child(self.render_batch_bar(cx))
            });

        // 第一行：搜索框（占满剩余宽）+ 排序下拉（右侧固定）
        let search_row = h_flex()
            .flex_none()
            .items_center()
            .gap_2()
            .px_3()
            .pt_2()
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .child(
                        Input::new(&self.search_input)
                            .prefix(Icon::new(IconName::Search).size_4())
                            .cleanable(true),
                    ),
            )
            .child(self.render_sort_select(cx));

        // 第二行：分组下拉独立一行（紧凑触发器，不占满整行）
        let filter_row = h_flex()
            .flex_none()
            .items_center()
            .px_3()
            .pt_1()
            .child(self.render_group_select(cx));

        // 右侧内容列：搜索行 + 筛选行 + 列表区
        let content = v_flex()
            .flex_1()
            .min_h_0()
            .min_w_0()
            .child(search_row)
            .child(filter_row)
            .child(list_area);

        // 根：absolute().inset_0() 锚定到 mod.rs 的 relative wrapper——
        // Entity 边界处 flex_1/size_full 高度解析不可靠（卡片列表塌缩的
        // 根因），与 SapApp 根同一修法。
        h_flex()
            .id("main-page")
            .absolute()
            .inset_0()
            .items_stretch()
            .bg(cx.theme().background)
            .track_focus(&self.focus_handle)
            .on_key_down(cx.listener(Self::handle_key_down))
            .child(content)
    }
}
