//! 命令面板（Ctrl+K，Raycast 风格）
//!
//! - 模糊搜索全部凭据：显示名/连接名/系统/用户名/描述/分组名/环境（无视当前分组筛选）
//! - 子序列匹配评分：开头加分、连续加分，多字段取最高分；支持多关键词（空格分隔，均需命中）
//! - 空查询显示「最近使用」Top 8；↑↓ 选择，Enter 登录，Esc 关闭；点击行直接登录

use super::i18n::{t, tf};
use gpui_kit::assets::IconName;
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::{ActiveTheme as _, Icon, WindowExt as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use sap_backend::commands::credentials as cred_cmd;
use sap_backend::models::{Credential, Group};

use crate::state::AppState;

use super::credential_card;
use super::main_page::MainPage;

/// 列表最多展示条数
const MAX_ROWS: usize = 8;

/// 子序列模糊匹配评分（None = 不匹配，分数越高越好）
/// 打分：文本开头命中 +16，词首（分隔符后）+10，连续命中 +8，基础 +1；越靠前额外微加分
fn fuzzy_score(query: &str, text: &str) -> Option<i64> {
    let q: Vec<char> = query.chars().collect();
    let t: Vec<char> = text.chars().collect();
    if q.is_empty() {
        return Some(0);
    }
    if q.len() > t.len() {
        return None;
    }

    let mut score: i64 = 0;
    let mut ti = 0;
    let mut prev_match: Option<usize> = None;

    for (qi, qc) in q.iter().enumerate() {
        let mut found: Option<usize> = None;
        let mut j = ti;
        while j < t.len() {
            if t[j].eq_ignore_ascii_case(qc) {
                found = Some(j);
                break;
            }
            j += 1;
        }
        let j = found?;
        score += 1;
        if j == 0 {
            score += 16;
        } else {
            let pc = t[j - 1];
            if pc == ' ' || pc == '-' || pc == '_' || pc == '/' || pc == '.' {
                score += 10;
            }
        }
        if let Some(p) = prev_match {
            if j == p + 1 {
                score += 8;
            }
        }
        if qi == 0 {
            // 首个字符命中位置越靠前越好
            score += (t.len() as i64 - j as i64).clamp(0, 20);
        }
        prev_match = Some(j);
        ti = j + 1;
    }
    Some(score)
}

/// 单条凭据的最佳匹配分（多字段取最高）
fn best_score(query: &str, cred: &Credential, group_name: &str) -> Option<i64> {
    let display = cred
        .display_name
        .clone()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| cred.connection_id.clone());
    let fields: [&str; 7] = [
        display.as_str(),
        cred.connection_id.as_str(),
        cred.system_id.as_str(),
        cred.username.as_str(),
        cred.description.as_str(),
        group_name,
        cred.environment.as_str(),
    ];
    let mut best: Option<i64> = None;
    for (i, f) in fields.iter().enumerate() {
        if f.is_empty() {
            continue;
        }
        if let Some(s) = fuzzy_score(query, f) {
            // 显示名（首个字段）权重略高
            let s = if i == 0 { s + 4 } else { s };
            if best.map(|b| s > b).unwrap_or(true) {
                best = Some(s);
            }
        }
    }
    best
}

/// 命令面板
pub struct CommandPalette {
    focus_handle: FocusHandle,
    search_input: Entity<InputState>,
    query: String,
    /// 全量凭据与分组（打开时快照）
    credentials: Vec<Credential>,
    groups: Vec<Group>,
    /// 匹配结果（评分降序）
    matched: Vec<Credential>,
    /// 键盘选中序号
    selected_ix: usize,
    /// 主界面弱引用（复用其登录流程）
    main_page: WeakEntity<MainPage>,
    _subscription: Subscription,
}

impl CommandPalette {
    pub fn new(
        main_page: WeakEntity<MainPage>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let store = AppState::global(cx).store.clone();
        let credentials = cred_cmd::get_credentials(&store).unwrap_or_default();
        let groups = cred_cmd::get_groups(&store).unwrap_or_default();

        let search_input = cx.new(|cx| {
            InputState::new(window, cx).placeholder(t("搜索凭据，Enter 直接登录…"))
        });
        // 打开即聚焦输入框（与解锁页同一先例）
        search_input.update(cx, |s, cx| s.focus(window, cx));

        let subscription = cx.subscribe_in(&search_input, window, |this, _, event, _, cx| {
            if matches!(event, InputEvent::Change) {
                this.query = this.search_input.read(cx).value().to_string();
                this.rematch();
                cx.notify();
            }
        });

        let mut palette = Self {
            focus_handle: cx.focus_handle(),
            search_input,
            query: String::new(),
            credentials,
            groups,
            matched: Vec::new(),
            selected_ix: 0,
            main_page,
            _subscription: subscription,
        };
        palette.rematch();
        palette
    }

    /// 分组名辅助（系统分组走 i18n 显示名，搜索与结果展示一致）
    fn group_name_of(&self, cred: &Credential) -> String {
        cred.group_id
            .as_ref()
            .and_then(|gid| self.groups.iter().find(|g| &g.id == gid))
            .map(super::main_page::group_display_name)
            .unwrap_or_default()
    }

    /// 重算匹配：空查询 → 最近使用 Top；否则模糊评分降序
    fn rematch(&mut self) {
        let q = self.query.trim();
        if q.is_empty() {
            let mut v = self.credentials.clone();
            v.sort_by_key(|c| std::cmp::Reverse(c.last_login_at));
            self.matched = v.into_iter().take(MAX_ROWS).collect();
        } else {
            // 多关键词：空格分隔，每个词都需命中至少一个字段
            let terms: Vec<&str> = q.split_whitespace().collect();
            let mut scored: Vec<(i64, Credential)> = self
                .credentials
                .iter()
                .filter_map(|c| {
                    let gname = self.group_name_of(c);
                    let mut total = 0i64;
                    for t in &terms {
                        total += best_score(t, c, &gname)?;
                    }
                    Some((total, c.clone()))
                })
                .collect();
            scored.sort_by_key(|s| std::cmp::Reverse(s.0));
            self.matched = scored.into_iter().take(MAX_ROWS).map(|(_, c)| c).collect();
        }
        self.selected_ix = 0;
    }

    /// 登录选中项并关闭面板
    fn login_selected(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(cred) = self.matched.get(self.selected_ix) else {
            return;
        };
        let cred_id = cred.id.clone();
        if let Some(page) = self.main_page.upgrade() {
            page.update(cx, |p, cx| p.login(cred_id, window, cx));
        }
        window.close_dialog(cx);
    }

    /// 面板键盘导航（输入框无 Enter 行为时事件冒泡到根 div）
    fn handle_key_down(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let key = event.keystroke.key.as_ref();
        match key {
            "arrowdown" if !self.matched.is_empty() => {
                cx.stop_propagation();
                self.selected_ix = (self.selected_ix + 1).min(self.matched.len() - 1);
                cx.notify();
            }
            "arrowup" if !self.matched.is_empty() => {
                cx.stop_propagation();
                self.selected_ix = self.selected_ix.saturating_sub(1);
                cx.notify();
            }
            "enter" => {
                cx.stop_propagation();
                self.login_selected(window, cx);
            }
            "escape" => {
                cx.stop_propagation();
                window.close_dialog(cx);
            }
            _ => {}
        }
    }

    /// 单条结果行
    fn render_row(&self, cred: &Credential, ix: usize, cx: &Context<Self>) -> impl IntoElement + use<> {
        let selected = ix == self.selected_ix;
        let env_c = credential_card::env_color(&cred.environment);
        let display = cred
            .display_name
            .clone()
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| cred.connection_id.clone());
        let mut sub_parts: Vec<String> = Vec::new();
        if !cred.system_id.is_empty() {
            sub_parts.push(cred.system_id.clone());
        }
        if !cred.username.is_empty() {
            sub_parts.push(cred.username.clone());
        }
        let gname = self.group_name_of(cred);
        if !gname.is_empty() {
            sub_parts.push(gname);
        }
        let sub = sub_parts.join(" · ");

        let main_page = self.main_page.clone();
        let cred_id = cred.id.clone();

        h_flex()
            .id(("palette-row", ix))
            .items_center()
            .gap(px(8.))
            .h(px(36.))
            .px(px(10.))
            .rounded(px(6.))
            .cursor_pointer()
            .when(selected, |this| this.bg(cx.theme().secondary))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_click(move |_, window, cx| {
                if let Some(page) = main_page.upgrade() {
                    page.update(cx, |p, cx| p.login(cred_id.clone(), window, cx));
                }
                window.close_dialog(cx);
            })
            // 环境色条
            .child(
                div()
                    .flex_none()
                    .w(px(3.))
                    .h(px(18.))
                    .rounded(px(2.))
                    .bg(env_c),
            )
            // 名称 + 副标题
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .child(
                        div()
                            .text_size(px(super::tokens::TS_BASE))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(cx.theme().foreground)
                            .truncate()
                            .child(display),
                    )
                    .when(!sub.is_empty(), |this| {
                        this.child(
                            div()
                                .text_size(px(super::tokens::TS_XS))
                                .text_color(cx.theme().muted_foreground)
                                .truncate()
                                .child(sub),
                        )
                    }),
            )
            // 选中行提示
            .when(selected, |this| {
                this.child(
                    div()
                        .flex_none()
                        .text_size(px(super::tokens::TS_XS))
                        .text_color(cx.theme().muted_foreground)
                        .child(t("Enter 登录")),
                )
            })
    }
}

impl Focusable for CommandPalette {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for CommandPalette {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let searching = !self.query.trim().is_empty();

        v_flex()
            .id("command-palette")
            .track_focus(&self.focus_handle)
            .on_key_down(cx.listener(Self::handle_key_down))
            .w(px(380.))
            .gap(px(6.))
            .child(
                Input::new(&self.search_input)
                    .prefix(Icon::new(IconName::Search).size_4())
                    .cleanable(true),
            )
            // 分区小标题
            .child(
                div()
                    .px(px(2.))
                    .text_size(px(super::tokens::TS_XS))
                    .text_color(cx.theme().muted_foreground)
                    .child(if searching {
                        tf("匹配 {} 条", &[&(self.matched.len()).to_string()])
                    } else {
                        t("最近使用").to_string()
                    }),
            )
            // 结果列表
            .child(
                v_flex()
                    .gap(px(1.))
                    .children(
                        self.matched
                            .iter()
                            .enumerate()
                            .map(|(ix, cred)| self.render_row(cred, ix, cx).into_any_element()),
                    )
                    .when(self.matched.is_empty(), |this| {
                        this.child(
                            div()
                                .py(px(18.))
                                .text_center()
                                .text_size(px(super::tokens::TS_BASE))
                                .text_color(cx.theme().muted_foreground)
                                .child(t("未找到匹配的凭据")),
                        )
                    }),
            )
    }
}

/// 打开命令面板（Ctrl+K / 全局热键 / 菜单入口）
pub fn open_command_palette_dialog(
    main_page: WeakEntity<MainPage>,
    window: &mut Window,
    cx: &mut App,
) {
    // 面板每次打开重建：数据与「最近使用」保持最新
    let palette = cx.new(|cx| CommandPalette::new(main_page, window, cx));
    let palette_for_content = palette.clone();

    window.open_dialog(cx, move |dialog, _, _| {
        dialog
            .title(t("命令面板"))
            .min_w(px(400.))
            .content({
                let palette = palette_for_content.clone();
                move |content, _, _| content.child(palette.clone())
            })
    });
}
