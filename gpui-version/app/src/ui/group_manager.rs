//! 分组管理面板 + 子弹窗：新建 / 重命名 / 删除 / 管理列表
//!
//! 对应 Tauri 版 GroupManagerModal / CreateGroupModal / RenameGroupModal + 删除弹窗（move/delete 两模式）。
//! 后端命令已就绪：add_group / rename_group / delete_group_with_options。
//! 所有操作成功后通过 weak_main 刷新 MainPage 列表。

use super::i18n::{t, tf};
use chrono::Utc;
use gpui_kit::assets::IconName;
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::{
    ActiveTheme as _, Icon, Sizable as _, WindowExt as _,
    button::{Button, ButtonVariants as _},
    h_flex, v_flex,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use sap_backend::commands::credentials as cred_cmd;
use sap_backend::models::Group;

use crate::state::AppState;

use super::main_page::MainPage;

/// 操作成功后刷新 MainPage
fn refresh_main(weak: &WeakEntity<MainPage>, cx: &mut App) {
    if let Some(page) = weak.upgrade() {
        page.update(cx, |t, cx| t.refresh(cx));
    }
}

/// 设为/取消默认分组（写入 settings.default_group 并持久化）。
/// 再次点击已默认的分组即取消（回到启动时打开全部分组）。
fn set_default_group(gid: &str, window: &mut Window, cx: &mut App) {
    let store = AppState::global(cx).store.clone();
    let settings = {
        let mut s = store.lock();
        s.settings.default_group = if s.settings.default_group == gid {
            String::new()
        } else {
            gid.to_string()
        };
        s.settings.clone()
    };
    if let Err(e) = sap_backend::commands::settings::save_settings(&store, settings) {
        log::warn!("保存设置失败：{e}");
    }
    // GroupManagerPage 每次渲染从 store 读取，刷新窗口即可更新勾选状态
    window.refresh();
}

/// 分组管理面板（替代弹窗，作为右侧主界面面板显示）
///
/// 两个分区：系统分组（只读：不可重命名/删除，仅可设默认）+ 自定义分组；
/// 分区头可点击折叠/展开。
pub struct GroupManagerPage {
    weak_main: WeakEntity<MainPage>,
    /// 系统分组分区折叠状态
    collapsed_system: bool,
    /// 自定义分组分区折叠状态
    collapsed_custom: bool,
    /// 整页滚动 handle（滚动条 overlay 读取位置用）
    scroll: ScrollHandle,
}

impl GroupManagerPage {
    pub fn new(
        weak_main: WeakEntity<MainPage>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Self {
        Self {
            weak_main,
            collapsed_system: false,
            collapsed_custom: false,
            scroll: ScrollHandle::new(),
        }
    }

    /// 分区头：chevron + 标题 + 计数，点击切换折叠；自定义分区带「新建」按钮
    fn render_section_header(
        &self,
        id: &'static str,
        title: String,
        collapsed: bool,
        create_btn: Option<Button>,
        on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
        cx: &Context<Self>,
    ) -> impl IntoElement {
        h_flex()
            .id(id)
            .items_center()
            .gap_1()
            .px_2()
            .py_1()
            .rounded(px(6.))
            .cursor_pointer()
            .hover(|s| s.bg(cx.theme().secondary))
            .on_click(on_click)
            .child(
                Icon::new(if collapsed {
                    IconName::ChevronRight
                } else {
                    IconName::ChevronDown
                })
                .size_3()
                .flex_none()
                .text_color(cx.theme().muted_foreground),
            )
            .child(
                div()
                    .text_size(px(super::tokens::TS_SM))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(cx.theme().muted_foreground)
                    .child(title),
            )
            .child(div().flex_1())
            .children(create_btn)
    }
}

impl Render for GroupManagerPage {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let weak = self.weak_main.clone();

        // 每次渲染从 AppState 读取最新分组列表 + 凭据（系统分组计数按环境统计）
        let store = AppState::global(cx).store.clone();
        let credentials = cred_cmd::get_credentials(&store).unwrap_or_default();
        let (system_groups, custom_groups, default_group_id): (Vec<Group>, Vec<Group>, String) = {
            let s = store.lock();
            (
                s.groups
                    .iter()
                    .filter(|g| g.is_system || g.is_default)
                    .cloned()
                    .collect(),
                s.groups
                    .iter()
                    .filter(|g| !g.is_system && !g.is_default)
                    .cloned()
                    .collect(),
                s.settings.default_group.clone(),
            )
        };
        // 计数：系统分组按 environment 匹配；自定义分组按 group_id（系统分组 entries 为空，
        // 不能用 g.entries.len()）
        let group_count = |g: &Group| -> usize {
            if let Some(env) = super::main_page::system_group_env(&g.id) {
                credentials.iter().filter(|c| c.environment == env).count()
            } else {
                credentials
                    .iter()
                    .filter(|c| c.group_id.as_deref() == Some(g.id.as_str()))
                    .count()
            }
        };

        let collapsed_system = self.collapsed_system;
        let collapsed_custom = self.collapsed_custom;

        // 根：锚定面板容器 + 整页滚动（分组多时不撑爆面板）+ 滚动条指示
        // （根已 absolute 定位，自带 containing block，无需再 relative）
        v_flex()
            .absolute()
            .inset_0()
            .id("gm-scroll")
            .overflow_y_scroll()
            .track_scroll(&self.scroll)
            .vertical_scrollbar(&self.scroll)
            .gap_1()
            .p_3()
            // ── 系统分组分区（只读）──
            .child(self.render_section_header(
                "gm-sec-system",
                tf("系统分组（{}）", &[&(system_groups.len()).to_string()]),
                collapsed_system,
                None,
                cx.listener(|this, _, _, cx| {
                    this.collapsed_system = !this.collapsed_system;
                    cx.notify();
                }),
                cx,
            ))
            .when(!collapsed_system, |col| {
                col.children(system_groups.iter().map(|g| {
                    let gid = g.id.clone();
                    let is_default = default_group_id == gid;
                    group_row(
                        g,
                        is_default,
                        group_count(g),
                        None,
                        None,
                        cx.theme().secondary,
                        cx.theme().muted_foreground,
                        cx.theme().primary,
                    )
                }))
            })
            // ── 自定义分组分区 ──
            .child(self.render_section_header(
                "gm-sec-custom",
                tf("自定义分组（{}）", &[&(custom_groups.len()).to_string()]),
                collapsed_custom,
                Some(
                    Button::new("gm-create-top")
                        .ghost()
                        .small()
                        .icon(IconName::Plus)
                        .label(t("新建"))
                        .on_click({
                            let weak = weak.clone();
                            move |_, window, cx| {
                                open_create_group_dialog(weak.clone(), window, cx);
                            }
                        }),
                ),
                cx.listener(|this, _, _, cx| {
                    this.collapsed_custom = !this.collapsed_custom;
                    cx.notify();
                }),
                cx,
            ))
            .when(!collapsed_custom, |col| {
                if custom_groups.is_empty() {
                    col.child(
                        div()
                            .px_2()
                            .py_1()
                            .text_size(px(super::tokens::TS_SM))
                            .text_color(cx.theme().muted_foreground)
                            .child(t("暂无自定义分组")),
                    )
                } else {
                    col.children(custom_groups.iter().map(|g| {
                        let weak_page = weak.clone();
                        let count = group_count(g);
                        let is_default = default_group_id == g.id;
                        group_row(
                            g,
                            is_default,
                            count,
                            // 重命名按钮
                            Some({
                                let weak = weak_page.clone();
                                let gid = g.id.clone();
                                let name = g.group_name.clone();
                                Button::new(format!("gm-rename-{}", g.id))
                                    .ghost()
                                    .small()
                                    .label(t("重命名"))
                                    .tooltip(t("重命名分组"))
                                    .on_click(move |_, window, cx| {
                                        open_rename_group_dialog(
                                            weak.clone(),
                                            gid.clone(),
                                            name.clone(),
                                            window,
                                            cx,
                                        );
                                    })
                            }),
                            // 删除按钮
                            Some({
                                let weak = weak_page.clone();
                                let gid = g.id.clone();
                                let name = g.group_name.clone();
                                Button::new(format!("gm-delete-{}", g.id))
                                    .ghost()
                                    .small()
                                    .icon(IconName::Trash)
                                    .tooltip(t("删除分组"))
                                    .on_click(move |_, window, cx| {
                                        open_delete_group_dialog(
                                            weak.clone(),
                                            gid.clone(),
                                            name.clone(),
                                            count,
                                            window,
                                            cx,
                                        );
                                    })
                            }),
                            cx.theme().secondary,
                            cx.theme().muted_foreground,
                            cx.theme().primary,
                        )
                    }))
                }
            })
    }
}

/// 分组行：图标 + 名称 + 计数 + [默认] + 可选 [重命名][删除]
///
/// 系统分组传 None 的操作按钮（只读）。主题色由调用方传入（自由函数无 cx）。
/// 计数由调用方按分组类型统计后传入（系统分组按环境、自定义按 group_id）。
fn group_row(
    g: &Group,
    is_default: bool,
    count: usize,
    rename_btn: Option<Button>,
    delete_btn: Option<Button>,
    hover_bg: gpui_kit::Hsla,
    muted: gpui_kit::Hsla,
    primary: gpui_kit::Hsla,
) -> impl IntoElement + use<> {
    let gid = g.id.clone();
    h_flex()
        .id(format!("gm-row-{gid}"))
        .items_center()
        .justify_between()
        .pl(px(22.))
        .pr_2()
        .py(px(6.))
        .rounded(px(8.))
        .hover(move |s| s.bg(hover_bg))
        .child(
            h_flex()
                .items_center()
                .gap_2()
                .min_w_0()
                .child(
                    Icon::new(IconName::FolderInput)
                        .size_3()
                        .flex_none()
                        .text_color(muted),
                )
                .child(
                    div()
                        .min_w_0()
                        .text_size(px(super::tokens::TS_BASE))
                        .truncate()
                        .child(super::main_page::group_display_name(g)),
                )
                .child(
                    div()
                        .flex_none()
                        .text_size(px(super::tokens::TS_XS))
                        .text_color(muted)
                        .child(tf("{count} 项", &[&count.to_string()])),
                ),
        )
        .child(
            h_flex()
                .items_center()
                .gap_1()
                .child({
                    // 默认分组勾选：已默认 → 主色高亮，点击取消
                    let btn = Button::new(format!("gm-default-{gid}"))
                        .ghost()
                        .small()
                        .icon(IconName::Check)
                        .label(t("默认"))
                        .tooltip(if is_default {
                            t("取消默认（启动时打开全部分组）")
                        } else {
                            t("设为默认分组")
                        });
                    let btn = if is_default { btn.text_color(primary) } else { btn };
                    btn.on_click(move |_, window, cx| {
                        set_default_group(&gid, window, cx);
                    })
                })
                .children(rename_btn)
                .children(delete_btn),
        )
}

/// 新建/重命名分组表单（弹窗内容的独立视图）：
/// 校验错误显示在输入框下方红字，而不是顶部飘通知
struct GroupNameForm {
    input: Entity<InputState>,
    /// None = 新建；Some(gid) = 重命名
    rename_id: Option<String>,
    error: Option<String>,
    weak_main: WeakEntity<MainPage>,
}

impl GroupNameForm {
    fn submit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let name = self.input.read(cx).value().trim().to_string();
        self.error = None;
        if name.is_empty() {
            self.error = Some(t("请输入分组名称").into());
            cx.notify();
            return;
        }
        let store = AppState::global(cx).store.clone();
        let result = match &self.rename_id {
            Some(gid) => cred_cmd::rename_group(&store, gid.clone(), name).map(|_| t("分组已重命名")),
            None => cred_cmd::add_group(
                &store,
                Group {
                    id: String::new(),
                    group_name: name,
                    entries: vec![],
                    is_system: false,
                    is_default: false,
                    created_at: Utc::now(),
                    updated_at: Utc::now(),
                },
            )
            .map(|_| t("分组已创建")),
        };
        match result {
            Ok(msg) => {
                window.close_dialog(cx);
                window.push_notification(msg, cx);
                refresh_main(&self.weak_main.clone(), cx);
            }
            Err(e) => {
                self.error = Some(e.to_string());
                cx.notify();
            }
        }
    }
}

impl Render for GroupNameForm {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let weak = cx.entity().downgrade();
        let error = self.error.clone();
        let is_rename = self.rename_id.is_some();

        let mut col = v_flex().gap_2().pb_2().child(Input::new(&self.input));
        if let Some(msg) = error {
            col = col.child(
                div()
                    .text_size(px(super::tokens::TS_XS))
                    .text_color(cx.theme().danger)
                    .child(msg),
            );
        }
        col.child(
            Button::new(if is_rename {
                "rename-group-confirm"
            } else {
                "create-group-confirm"
            })
            .primary()
            .label(if is_rename { t("保存") } else { t("创建") })
            .w_full()
            .on_click(move |_, window, cx| {
                if let Some(form) = weak.upgrade() {
                    form.update(cx, |f, cx| f.submit(window, cx));
                }
            }),
        )
    }
}

/// 新建分组弹窗（输入名 → add_group）
pub fn open_create_group_dialog(
    weak_main: WeakEntity<MainPage>,
    window: &mut Window,
    cx: &mut App,
) {
    let input = cx.new(|cx| InputState::new(window, cx).placeholder(t("分组名称（如：测试环境集合）")));
    let form = cx.new(|_cx| GroupNameForm {
        input,
        rename_id: None,
        error: None,
        weak_main,
    });

    window.open_dialog(cx, move |dialog, _, _| {
        let form = form.clone();
        dialog
            .title(t("新建分组"))
            .min_w(px(340.))
            .content(move |content, _, _| content.child(form.clone()))
    });
}

/// 重命名分组弹窗（输入新名 → rename_group）
pub fn open_rename_group_dialog(
    weak_main: WeakEntity<MainPage>,
    group_id: String,
    current_name: String,
    window: &mut Window,
    cx: &mut App,
) {
    let input = cx.new(|cx| {
        InputState::new(window, cx)
            .placeholder(tf("新名称（当前：{current_name}）", &[&current_name.to_string()]))
    });
    let form = cx.new(|_cx| GroupNameForm {
        input,
        rename_id: Some(group_id),
        error: None,
        weak_main,
    });

    window.open_dialog(cx, move |dialog, _, _| {
        let form = form.clone();
        dialog
            .title(t("重命名分组"))
            .min_w(px(340.))
            .content(move |content, _, _| content.child(form.clone()))
    });
}

/// 删除分组弹窗（move/delete 两模式 → delete_group_with_options）
/// 用两个独立按钮代替单选状态，避免引入未验证的中间状态 Entity
pub fn open_delete_group_dialog(
    weak_main: WeakEntity<MainPage>,
    group_id: String,
    group_name: String,
    entry_count: usize,
    window: &mut Window,
    cx: &mut App,
) {
    window.open_dialog(cx, move |dialog, _, _| {
        let gid_move = group_id.clone();
        let gid_del = group_id.clone();
        let gname = group_name.clone();
        let weak_move = weak_main.clone();
        let weak_del = weak_main.clone();
        dialog
            .title(t("删除分组"))
            .min_w(px(360.))
            .content(move |content, _, cx| {
                let gname = gname.clone();
                let count = entry_count;
                let gid_move = gid_move.clone();
                let gid_del = gid_del.clone();
                let weak_move = weak_move.clone();
                let weak_del = weak_del.clone();
                content.child(
                    v_flex()
                        .gap_3()
                        .pb_2()
                        .child(
                            v_flex()
                                .gap_1()
                                .child(
                                    div()
                                        .text_size(px(super::tokens::TS_MD))
                                        .child(tf("确认删除分组「{gname}」？", &[&gname.to_string()])),
                                )
                                .child(
                                    div()
                                        .text_size(px(super::tokens::TS_BASE))
                                        .text_color(cx.theme().muted_foreground)
                                        .child(tf("该分组下有 {count} 个凭据，请选择处理方式：", &[&count.to_string()])),
                                ),
                        )
                        .child(
                            v_flex()
                                .gap_2()
                                .child(
                                    Button::new("del-mode-move")
                                        .primary()
                                        .icon(IconName::FolderInput)
                                        .label(tf("移入默认分组（保留 {count} 项凭据）", &[&count.to_string()]))
                                        .w_full()
                                        .on_click(move |_, window, cx| {
                                            let store = AppState::global(cx).store.clone();
                                            match cred_cmd::delete_group_with_options(
                                                &store,
                                                gid_move.clone(),
                                                "move".to_string(),
                                            ) {
                                                Ok(()) => {
                                                    window.close_dialog(cx);
                                                    window.push_notification(
                                                        t("分组已删除，凭据已移入默认分组"),
                                                        cx,
                                                    );
                                                    refresh_main(&weak_move, cx);
                                                }
                                                Err(e) => {
                                                    window.push_notification(
                                                        tf("删除失败：{e}", &[&e.to_string()]),
                                                        cx,
                                                    );
                                                }
                                            }
                                        }),
                                )
                                .child(
                                    Button::new("del-mode-delete")
                                        .ghost()
                                        .icon(IconName::Trash)
                                        .label(tf("同时删除 {count} 项凭据（不可恢复）", &[&count.to_string()]))
                                        .w_full()
                                        .on_click(move |_, window, cx| {
                                            let store = AppState::global(cx).store.clone();
                                            match cred_cmd::delete_group_with_options(
                                                &store,
                                                gid_del.clone(),
                                                "delete".to_string(),
                                            ) {
                                                Ok(()) => {
                                                    window.close_dialog(cx);
                                                    window.push_notification(
                                                        t("分组及内部凭据已删除"),
                                                        cx,
                                                    );
                                                    refresh_main(&weak_del, cx);
                                                }
                                                Err(e) => {
                                                    window.push_notification(
                                                        tf("删除失败：{e}", &[&e.to_string()]),
                                                        cx,
                                                    );
                                                }
                                            }
                                        }),
                                ),
                        ),
                )
            })
    });
}
