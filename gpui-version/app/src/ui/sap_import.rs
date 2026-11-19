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
    button::{Button, ButtonVariants as _},
    checkbox::Checkbox,
    h_flex, v_flex, ActiveTheme as _, Disableable as _, Icon, Sizable as _, WindowExt as _,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use std::collections::HashSet;

use sap_backend::commands::sap as sap_cmd;
use sap_backend::models::SapConnection;

use crate::state::AppState;

use super::main_page::MainPage;
use super::SapApp;

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

    /// 更新过滤词并重算可见行（take/存回，避免每次按键全量 clone 连接列表）
    fn set_filter(&mut self, filter: String) {
        let connections = std::mem::take(&mut self.connections);
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
            2 => Column::new("sysid", t("系统"))
                .width(px(50.))
                .movable(false),
            3 => Column::new("sysnr", t("实例号"))
                .width(px(56.))
                .movable(false),
            4 => Column::new("server", t("服务器"))
                .width(px(140.))
                .movable(false),
            _ => Column::new("desc", t("描述"))
                .width(px(170.))
                .movable(false),
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
        // 行身份用连接名（领域 ID）——搜索过滤会重排 visible，索引 id 会让
        // 选中底色/点击状态错位到别的行（编码规范「Stable identity」）
        let row = self.visible.get(row_ix);
        let name = row.map(|c| c.name.clone()).unwrap_or_default();
        let checked = row.is_some_and(|c| self.selected.contains(&c.name));
        let exists = row.is_some_and(|c| self.existing.contains(&c.name));
        let sel_bg = {
            let mut c = cx.theme().primary;
            c.a = 0.08;
            c
        };
        div()
            .id(format!("import-row-{}", name))
            .when(checked && !exists, |this| this.bg(sel_bg))
            .when(!exists, |this| {
                let name = name.clone();
                this.on_click(cx.listener(move |table, _, _, cx| {
                    let d = table.delegate_mut();
                    if !d.selected.remove(&name) {
                        d.selected.insert(name.clone());
                    }
                    cx.notify();
                }))
            })
    }

    fn render_empty(
        &mut self,
        _: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) -> impl IntoElement {
        // 恢复建议：无过滤时指路设置项；有过滤时提示清空搜索
        let (empty_hint, has_filter) = if self.filter.is_empty() {
            (
                t("检查 SAP Logon 是否已安装，或在设置中手动指定路径"),
                false,
            )
        } else {
            (t("试试更换或清空搜索关键词"), true)
        };
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
                    .child(if has_filter {
                        t("未找到匹配的连接").to_string()
                    } else {
                        t("未找到本机 SAP 连接").to_string()
                    }),
            )
            .child(
                div()
                    .text_center()
                    .text_size(px(super::tokens::TS_SM))
                    .text_color(cx.theme().muted_foreground)
                    .child(empty_hint.to_string()),
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
                            .on_click(cx.listener(move |table, checked: &bool, _, cx| {
                                let d = table.delegate_mut();
                                if *checked {
                                    d.selected.insert(name.clone());
                                } else {
                                    d.selected.remove(&name);
                                }
                                cx.notify();
                            })),
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
    weak_main: WeakEntity<MainPage>,
    /// 根视图弱引用：面板模式下关闭自身（取消/导入成功后返回主界面）
    weak_app: WeakEntity<SapApp>,
    /// 导入来源（顶部三态切换，与导出面板同款布局）
    source: super::ImportSource,
    /// 备份/BAT 导入子面板（首次切到对应来源时惰性创建）
    backup_panel: Option<Entity<super::BackupImportPanel>>,
    loading: bool,
    error: Option<String>,
    /// v5 Should #8：部分 Landscape 文件解析失败的原因（不阻断导入，但可见）
    warnings: Option<String>,
    /// 导入表格
    table: Entity<TableState<ImportTableDelegate>>,
    filter_input: Entity<InputState>,
    _filter_subscription: Subscription,
}

impl SapImportPage {
    pub fn new(
        weak_main: WeakEntity<MainPage>,
        weak_app: WeakEntity<SapApp>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let filter_input = cx
            .new(|cx| InputState::new(window, cx).placeholder(t("筛选连接名 / 描述 / Workspace")));
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

        let mut page = Self {
            weak_main,
            weak_app,
            source: super::ImportSource::SapConfig,
            backup_panel: None,
            loading: false,
            error: None,
            warnings: None,
            table,
            filter_input,
            _filter_subscription,
        };
        page.load_sap_connections(window, cx);
        page
    }

    /// 后台读取 SAP 配置（文件解析较慢）。
    /// v5 Consider #25：抽成可重入方法 —— 首次加载、错误页「重试」按钮、
    /// 切回「SAP 配置」数据源时都可重新读取，error 不再是终态。
    fn load_sap_connections(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.loading {
            return;
        }
        self.loading = true;
        self.error = None;
        self.warnings = None;
        cx.notify();

        let store = AppState::global(cx).store.clone();
        cx.spawn_in(window, async move |this, window| {
            let result = window
                .background_executor()
                .spawn(async move { sap_cmd::get_sap_connections_with_errors() })
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
                    Ok((conns, errs)) => {
                        page.table.update(cx, |t, cx| {
                            t.delegate_mut().set_data(conns, existing);
                            t.refresh(cx);
                        });
                        // v5 Should #8：部分失败不再被静默丢弃
                        page.warnings = if errs.is_empty() {
                            None
                        } else {
                            Some(errs.join("；"))
                        };
                    }
                    Err(e) => {
                        page.error = Some(e.to_string());
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// 切换导入来源（首次切到备份/BAT 时创建子面板）
    fn switch_source(
        &mut self,
        source: super::ImportSource,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.source = source;
        // v5 Consider #25：切回 SAP 配置时若上次读取失败，自动重试而不是停留在终态错误
        if source == super::ImportSource::SapConfig && self.error.is_some() {
            self.load_sap_connections(window, cx);
        }
        if source != super::ImportSource::SapConfig && self.backup_panel.is_none() {
            let pw_state = cx.new(|cx| {
                InputState::new(window, cx)
                    .masked(true)
                    .placeholder(t("输入主密码以验证"))
            });
            let pw_for_click = pw_state.clone();
            self.backup_panel = Some(cx.new(|cx| {
                super::BackupImportPanel::new(pw_state, pw_for_click, self.weak_app.clone(), cx)
            }));
        }
        if let Some(p) = &self.backup_panel {
            p.update(cx, |p, _| p.set_source(source));
        }
        cx.notify();
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
        let weak_app = self.weak_app.clone();
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
                        // 面板模式：直接关面板返回主界面（面板无 dialog，close_dialog 无效）
                        if let Some(app) = weak_app.upgrade() {
                            app.update(cx, |app, cx| app.close_panel_impl(cx));
                        }
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

        let body = if self.loading {
            v_flex()
                .id("import-loading")
                .flex_1()
                .min_h_0()
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
                .flex_1()
                .min_h_0()
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
                .child(
                    // v5 Consider #25：错误页提供重试入口，不必关掉整个导入面板重开
                    Button::new("import-retry")
                        .primary()
                        .label(t("重新读取"))
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.load_sap_connections(window, cx);
                        })),
                )
                .into_any_element()
        } else {
            let total = self.table.read(cx).delegate().visible.len();
            v_flex()
                .id("import-table-box")
                .flex_1()
                .min_h_0()
                .rounded_lg()
                .border_1()
                .border_color(cx.theme().border)
                .overflow_hidden()
                .bg(cx.theme().sidebar)
                // v5 Should #8：部分文件解析失败时显式提示「列表可能不完整」
                .when_some(self.warnings.clone(), |this, w| {
                    this.child(
                        div()
                            .id("import-partial-warning")
                            .px_2()
                            .py_1()
                            .text_size(px(super::tokens::TS_SM))
                            .text_color(cx.theme().danger)
                            .child(tf("部分配置读取失败，列表可能不完整：{e}", &[&w])),
                    )
                })
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

        let is_sap = self.source == super::ImportSource::SapConfig;
        let is_backup = self.source == super::ImportSource::Backup;

        v_flex()
            .absolute()
            .inset_0()
            .p_3()
            .gap_2()
            // 顶部：导入来源三段滑块（与导出面板同款样式）
            .child(
                h_flex()
                    .bg(cx.theme().secondary)
                    .rounded_md()
                    .p(px(2.))
                    .gap(px(2.))
                    .child(
                        super::segmented_item("import-src-sap", t("SAP 配置"), is_sap, cx)
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.switch_source(super::ImportSource::SapConfig, window, cx);
                            })),
                    )
                    .child(
                        super::segmented_item(
                            "import-src-backup",
                            t("备份 JSON 文件"),
                            is_backup,
                            cx,
                        )
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.switch_source(super::ImportSource::Backup, window, cx);
                        })),
                    )
                    .child(
                        super::segmented_item(
                            "import-src-bat",
                            t("BAT 脚本"),
                            !is_sap && !is_backup,
                            cx,
                        )
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.switch_source(super::ImportSource::Bat, window, cx);
                        })),
                    ),
            )
            // SAP 配置来源的说明
            .when(is_sap, |this| {
                this.child(
                    div()
                        .text_size(px(super::tokens::TS_BASE))
                        .text_color(cx.theme().muted_foreground)
                        .child(t("读取本机 SAP Logon 连接，按 Workspace 自动分组。导入后请编辑填写账户与密码。")),
                )
            })
            // 主体：SAP 配置表格 / 备份与 BAT 子面板（子面板自带确认按钮）
            .child(if !is_sap {
                self.backup_panel
                    .clone()
                    .map(|p| p.into_any_element())
                    .unwrap_or_else(|| div().into_any_element())
            } else {
                body
            })
            // 底部操作（仅 SAP 配置来源需要；备份/BAT 由子面板自带确认按钮）
            .when(is_sap, |this| {
                this.child(
                    h_flex()
                        .justify_end()
                        .gap_2()
                        .pt_2()
                        .border_t_1()
                        .border_color(cx.theme().border)
                        .child(
                            Button::new("import-cancel")
                                .label(t("取消"))
                                .on_click({
                                    let weak_app = self.weak_app.clone();
                                    move |_, _, cx| {
                                        if let Some(app) = weak_app.upgrade() {
                                            app.update(cx, |app, cx| {
                                                app.close_panel_impl(cx)
                                            });
                                        }
                                    }
                                }),
                        )
                        .child(
                            Button::new("import-confirm")
                                .primary()
                                .disabled(selected_count == 0 || self.loading)
                                .label(tf(
                                    "导入 {selected_count} 项",
                                    &[&selected_count.to_string()],
                                ))
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.do_import(window, cx);
                                })),
                        ),
                )
            })
    }
}
