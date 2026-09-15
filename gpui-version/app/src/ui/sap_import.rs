//! SAP Landscape 连接导入弹窗
//!
//! 对应 Tauri 版 SapImportModal.tsx：
//! - 读取本机 SAP Logon 配置（后台线程，避免阻塞 UI）
//! - 表格展示：勾选 / 连接名 / 系统 / 实例号 / 服务器 / 描述
//! - 已导入的连接禁选并弱化显示
//! - 按 Workspace 自动创建自建分组（后端处理）

use super::i18n::{t, tf};
use gpui_kit::assets::IconName;
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::spinner::Spinner;
use gpui_kit::component::table::{Column, DataTable, TableDelegate, TableState};
use gpui_kit::component::{
    ActiveTheme as _, Disableable as _, Icon, Sizable as _, WindowExt as _,
    button::{Button, ButtonVariants as _},
    checkbox::Checkbox,
    h_flex, v_flex,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use std::collections::HashSet;

use sap_backend::commands::sap as sap_cmd;
use sap_backend::models::SapConnection;

use crate::state::AppState;

use super::main_page::MainPage;

/// 导入表格委托
struct ImportTableDelegate {
    /// 全量连接
    connections: Vec<SapConnection>,
    /// 过滤关键词（小写）
    filter: String,
    /// 可见连接（过滤后）
    visible: Vec<SapConnection>,
    /// 已导入的连接名（禁选）
    existing: HashSet<String>,
    /// 勾选的连接名
    selected: HashSet<String>,
}

impl ImportTableDelegate {
    fn new() -> Self {
        Self {
            connections: Vec::new(),
            filter: String::new(),
            visible: Vec::new(),
            existing: HashSet::new(),
            selected: HashSet::new(),
        }
    }

    /// 设置数据（保留当前过滤词）
    fn set_data(&mut self, connections: Vec<SapConnection>, existing: HashSet<String>) {
        self.existing = existing;
        self.refilter(connections);
    }

    /// 更新过滤词并重算可见行
    fn set_filter(&mut self, filter: String) {
        let connections = self.connections.clone();
        self.filter = filter.to_lowercase();
        self.refilter(connections);
    }

    fn refilter(&mut self, connections: Vec<SapConnection>) {
        let q = self.filter.clone();
        self.visible = connections
            .iter()
            .filter(|c| {
                if q.is_empty() {
                    return true;
                }
                let ws = c.workspace_name.clone().unwrap_or_default();
                let desc = c.description.clone().unwrap_or_default();
                c.name.to_lowercase().contains(&q)
                    || ws.to_lowercase().contains(&q)
                    || desc.to_lowercase().contains(&q)
            })
            .cloned()
            .collect();
        self.connections = connections;
    }

    /// 可选行（可见且未导入）
    fn selectable_names(&self) -> Vec<String> {
        self.visible
            .iter()
            .map(|c| c.name.clone())
            .filter(|n| !self.existing.contains(n))
            .collect()
    }

    /// 全选 / 取消全选（仅可选行）
    fn toggle_select_all(&mut self, cx: &mut Context<TableState<Self>>) {
        let selectable = self.selectable_names();
        let all_selected = selectable.iter().all(|n| self.selected.contains(n));
        if all_selected {
            for n in &selectable {
                self.selected.remove(n);
            }
        } else {
            for n in selectable {
                self.selected.insert(n);
            }
        }
        cx.notify();
    }

    /// 勾选的连接（含被过滤隐藏的）
    fn take_selected(&self) -> Vec<SapConnection> {
        self.connections
            .iter()
            .filter(|c| self.selected.contains(&c.name))
            .cloned()
            .collect()
    }
}

impl TableDelegate for ImportTableDelegate {
    fn columns_count(&self, _: &App) -> usize {
        6
    }

    fn rows_count(&self, _: &App) -> usize {
        self.visible.len()
    }

    fn column(&self, ix: usize, _: &App) -> Column {
        match ix {
            0 => Column::new("check", "")
                .width(px(30.))
                .text_center()
                .selectable(false)
                .resizable(false)
                .movable(false)
                .p_0(),
            1 => Column::new("name", t("连接名"))
                .width(px(150.))
                .min_width(px(100.))
                .max_width(px(280.)),
            2 => Column::new("sysid", t("系统")).width(px(50.)).movable(false),
            3 => Column::new("sysnr", t("实例号")).width(px(56.)).movable(false),
            4 => Column::new("server", t("服务器")).width(px(140.)).movable(false),
            _ => Column::new("desc", t("描述")).width(px(170.)).movable(false),
        }
    }

    fn render_th(
        &mut self,
        col_ix: usize,
        _: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) -> impl IntoElement {
        if col_ix == 0 {
            let selectable = self.selectable_names();
            let all_selected =
                !selectable.is_empty() && selectable.iter().all(|n| self.selected.contains(n));
            return h_flex()
                .size_full()
                .items_center()
                .justify_center()
                .child(
                    Checkbox::new("import-check-all")
                        .checked(all_selected)
                        .on_click(cx.listener(|table, _, _, cx| {
                            table.delegate_mut().toggle_select_all(cx);
                        })),
                )
                .into_any_element();
        }
        div()
            .size_full()
            .child(self.column(col_ix, cx).name.clone())
            .into_any_element()
    }

    fn render_tr(
        &mut self,
        row_ix: usize,
        _: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) -> Stateful<Div> {
        let checked = self
            .visible
            .get(row_ix)
            .is_some_and(|c| self.selected.contains(&c.name));
        let exists = self
            .visible
            .get(row_ix)
            .is_some_and(|c| self.existing.contains(&c.name));
        let sel_bg = {
            let mut c = cx.theme().primary;
            c.a = 0.08;
            c
        };
        div()
            .id(("import-row", row_ix))
            .when(checked && !exists, |this| this.bg(sel_bg))
            .when(!exists, |this| {
                this.on_click(cx.listener(move |table, _, _, cx| {
                    if let Some(c) = table.delegate().visible.get(row_ix) {
                        let name = c.name.clone();
                        let d = table.delegate_mut();
                        if !d.selected.remove(&name) {
                            d.selected.insert(name);
                        }
                        cx.notify();
                    }
                }))
            })
    }

    fn render_empty(
        &mut self,
        _: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) -> impl IntoElement {
        v_flex()
            .size_full()
            .items_center()
            .justify_center()
            .gap_2()
            .child(
                Icon::new(IconName::FileText)
                    .size_8()
                    .text_color(cx.theme().muted_foreground),
            )
            .child(
                div()
                    .text_center()
                    .text_size(px(super::tokens::TS_BASE))
                    .text_color(cx.theme().muted_foreground)
                    .child(if self.filter.is_empty() {
                        t("未找到本机 SAP 连接").to_string()
                    } else {
                        t("未找到匹配的连接").to_string()
                    }),
            )
            .into_any_element()
    }

    fn render_td(
        &mut self,
        row_ix: usize,
        col_ix: usize,
        _: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) -> impl IntoElement {
        let Some(conn) = self.visible.get(row_ix) else {
            return div().into_any_element();
        };
        let name = conn.name.clone();

        match col_ix {
            // 勾选
            0 => {
                let checked = self.selected.contains(&name);
                let exists = self.existing.contains(&name);
                h_flex()
                    .size_full()
                    .items_center()
                    .justify_center()
                    .on_mouse_down(MouseButton::Left, |_, _, cx| {
                        cx.stop_propagation();
                    })
                    .child(
                        Checkbox::new(format!("import-check-{name}"))
                            .checked(checked)
                            .disabled(exists)
                            .on_click(cx.listener(
                                move |table, checked: &bool, _, cx| {
                                    let d = table.delegate_mut();
                                    if *checked {
                                        d.selected.insert(name.clone());
                                    } else {
                                        d.selected.remove(&name);
                                    }
                                    cx.notify();
                                },
                            )),
                    )
                    .into_any_element()
            }
            // 连接名：仅名称（已导入行文字弱化，可导入性由勾选框禁用体现）
            1 => {
                let exists = self.existing.contains(&name);
                h_flex()
                    .size_full()
                    .items_center()
                    .min_w_0()
                    .child(
                        div()
                            .text_size(px(super::tokens::TS_BASE))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(if exists {
                                cx.theme().muted_foreground
                            } else {
                                cx.theme().foreground
                            })
                            .truncate()
                            .child(name),
                    )
                    .into_any_element()
            }
            // 系统
            2 => cell_text(conn.system_id.clone().unwrap_or_default(), cx),
            // 实例号
            3 => cell_text(conn.system_number.clone().unwrap_or_default(), cx),
            // 服务器
            4 => cell_text(conn.server.clone().unwrap_or_default(), cx),
            // 描述
            _ => cell_text(conn.description.clone().unwrap_or_default(), cx),
        }
    }
}

/// 普通文本单元格
fn cell_text(text: String, cx: &App) -> AnyElement {
    h_flex()
        .size_full()
        .items_center()
        .min_w_0()
        .child(
            div()
                .text_size(px(super::tokens::TS_SM))
                .text_color(cx.theme().foreground)
                .truncate()
                .child(text),
        )
        .into_any_element()
}

pub struct SapImportPage {
    master_password: String,
    weak_main: WeakEntity<MainPage>,
    loading: bool,
    error: Option<String>,
    /// 导入表格
    table: Entity<TableState<ImportTableDelegate>>,
    filter_input: Entity<InputState>,
    _filter_subscription: Subscription,
}

impl SapImportPage {
    pub fn new(
        master_password: String,
        weak_main: WeakEntity<MainPage>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let store = AppState::global(cx).store.clone();

        // 后台读取 SAP 配置（文件解析较慢）
        cx.spawn_in(window, async move |this, window| {
            let result = window
                .background_executor()
                .spawn(async move { sap_cmd::get_sap_connections() })
                .await;

            let existing: HashSet<String> = {
                let s = store.lock();
                s.credentials
                    .iter()
                    .map(|c| c.connection_id.clone())
                    .collect()
            };

            _ = this.update_in(window, |page: &mut SapImportPage, _, cx| {
                page.loading = false;
                match result {
                    Ok(conns) => {
                        page.table.update(cx, |t, cx| {
                            t.delegate_mut().set_data(conns, existing);
                            t.refresh(cx);
                        });
                    }
                    Err(e) => {
                        page.error = Some(e);
                    }
                }
                cx.notify();
            });
        })
        .detach();

        let filter_input = cx.new(|cx| {
            InputState::new(window, cx).placeholder(t("筛选连接名 / 描述 / Workspace"))
        });
        let table = cx.new(|cx| {
            TableState::new(ImportTableDelegate::new(), window, cx)
                .row_selectable(false)
                .col_selectable(false)
                .cell_selectable(false)
                .col_movable(false)
                .sortable(false)
                .col_resizable(true)
        });
        let _filter_subscription =
            cx.subscribe_in(&filter_input, window, |this, _, event, _, cx| {
                if matches!(event, InputEvent::Change) {
                    let f = this.filter_input.read(cx).value().to_string();
                    this.table.update(cx, |t, cx| {
                        t.delegate_mut().set_filter(f);
                        t.refresh(cx);
                    });
                    cx.notify();
                }
            });

        Self {
            master_password,
            weak_main,
            loading: true,
            error: None,
            table,
            filter_input,
            _filter_subscription,
        }
    }

    /// 执行导入
    fn do_import(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let conns = self.table.read(cx).delegate().take_selected();
        if conns.is_empty() {
            window.push_notification(t("请先勾选要导入的连接"), cx);
            return;
        }

        let store = AppState::global(cx).store.clone();
        let weak_main = self.weak_main.clone();
        self.loading = true;
        cx.notify();

        cx.spawn_in(window, async move |this, window| {
            let result = window
                .background_executor()
                .spawn(async move { sap_cmd::import_sap_connections(&store, conns) })
                .await;

            _ = this.update_in(window, |page: &mut SapImportPage, window, cx| {
                page.loading = false;
                match result {
                    Ok(n) if n > 0 => {
                        window.close_dialog(cx);
                        window.push_notification(
                            tf("已导入 {n} 个连接，请编辑填写账户密码", &[&n.to_string()]),
                            cx,
                        );
                        if let Some(main) = weak_main.upgrade() {
                            main.update(cx, |p, cx| p.refresh(cx));
                        }
                    }
                    Ok(_) => {
                        window.push_notification(t("所选连接均已导入"), cx);
                    }
                    Err(e) => {
                        window.push_notification(tf("导入失败：{e}", &[&e.to_string()]), cx);
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }
}

impl Render for SapImportPage {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let selected_count = self.table.read(cx).delegate().selected.len();
        let _ = &self.master_password; // 导入操作不涉及密码（后端只建凭据不加密）

        let body = if self.loading {
            v_flex()
                .id("import-loading")
                .h(px(300.))
                .items_center()
                .justify_center()
                .gap_3()
                .child(Spinner::new().color(cx.theme().primary))
                .child(
                    div()
                        .text_size(px(super::tokens::TS_BASE))
                        .text_color(cx.theme().muted_foreground)
                        .child(t("正在读取本机 SAP 配置…")),
                )
                .into_any_element()
        } else if let Some(ref err) = self.error {
            v_flex()
                .id("import-error")
                .h(px(300.))
                .items_center()
                .justify_center()
                .gap_3()
                .child(
                    Icon::new(IconName::FileText)
                        .size_8()
                        .text_color(cx.theme().muted_foreground),
                )
                .child(
                    div()
                        .text_center()
                        .text_size(px(super::tokens::TS_BASE))
                        .text_color(cx.theme().muted_foreground)
                        .child(tf("读取 SAP 配置失败：{err}", &[&err.to_string()])),
                )
                .into_any_element()
        } else {
            let total = self.table.read(cx).delegate().visible.len();
            v_flex()
                .id("import-table-box")
                .h(px(300.))
                .rounded_lg()
                .border_1()
                .border_color(cx.theme().border)
                .overflow_hidden()
                .bg(cx.theme().sidebar)
                // 过滤 + 计数
                .child(
                    h_flex()
                        .flex_none()
                        .items_center()
                        .gap_2()
                        .px_2()
                        .py_2()
                        .border_b_1()
                        .border_color(cx.theme().border)
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .child(Input::new(&self.filter_input).small()),
                        )
                        .child(
                            div()
                                .text_size(px(super::tokens::TS_SM))
                                .text_color(cx.theme().muted_foreground)
                                .child(tf("{total} 个连接", &[&total.to_string()])),
                        ),
                )
                .child(
                    div()
                        .flex_1()
                        .min_h_0()
                        .child(DataTable::new(&self.table).with_size(px(30.))),
                )
                .into_any_element()
        };

        v_flex()
            .p_3()
            .gap_2()
            .child(
                div()
                    .text_size(px(super::tokens::TS_BASE))
                    .text_color(cx.theme().muted_foreground)
                    .child(t("读取本机 SAP Logon 连接，按 Workspace 自动分组。导入后请编辑填写账户与密码。")),
            )
            .child(body)
            // 底部操作
            .child(
                h_flex()
                    .justify_end()
                    .gap_2()
                    .pt_2()
                    .border_t_1()
                    .border_color(cx.theme().border)
                    .child(
                        Button::new("import-cancel")
                            .label(t("取消"))
                            .on_click(|_, window, cx| {
                                window.close_dialog(cx);
                            }),
                    )
                    .child(
                        Button::new("import-confirm")
                            .primary()
                            .disabled(selected_count == 0 || self.loading)
                            .label(tf("导入 {selected_count} 项", &[&selected_count.to_string()]))
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.do_import(window, cx);
                            })),
                    ),
            )
    }
}
