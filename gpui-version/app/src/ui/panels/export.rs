//! 导出凭据面板（JSON 备份 / BAT 脚本）与 BAT 草稿表格委托。
//!
//! 自包含面板：不访问 SapApp 私有字段，经 WeakEntity 回调根视图。
//! 由 ui/mod.rs 拆分而来（v1.5.9）。

use super::super::i18n::{t, tf};
use super::super::tokens;
use super::super::SapApp;
use crate::state::AppState;
use gpui_kit::assets::IconName;
use gpui_kit::component::{
    button::{Button, ButtonVariants as _},
    h_flex,
    input::{Input, InputState},
    table::{Column, TableDelegate, TableState},
    v_flex, ActiveTheme as _, Icon, Sizable as _, WindowExt as _,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use sap_backend::bat_parser::{self, BatDraft, LandscapeMatch};
use sap_backend::commands::transfer;
use sap_backend::models::SapConnection;

/// 导出格式（导出面板二选一）
#[derive(Clone, Copy, PartialEq, Eq)]
enum ExportFormat {
    /// 明文备份 JSON（含所有凭据密码，可用「备份导入」还原，切勿外发）
    Json,
    /// sapshcut BAT 脚本（每条凭据一个 .bat 文件，可直接命令行登录）
    Bat,
}

/// 滑块式分段标签的单项（设计稿方案一：灰色胶囊容器 + 白底选中块）
///
/// 容器由调用方拼装：`h_flex().bg(secondary).rounded_md().p(px(2.)).gap(px(2.))`
/// 内放多个 [`segmented_item`]；选中项白底描边、未选中项透明。
pub(crate) fn segmented_item(
    id: &'static str,
    label: impl IntoElement,
    active: bool,
    cx: &App,
) -> Stateful<Div> {
    div()
        .id(id)
        .flex()
        .flex_1()
        .items_center()
        .justify_center()
        .h(px(30.))
        .rounded(px(6.))
        .cursor_pointer()
        .text_size(px(tokens::TS_BASE))
        .map(|d| {
            if active {
                d.bg(cx.theme().background)
                    .border_1()
                    .border_color(cx.theme().border)
                    .text_color(cx.theme().foreground)
                    .font_weight(FontWeight::MEDIUM)
            } else {
                d.text_color(cx.theme().muted_foreground)
            }
        })
        .child(label)
}

/// 导出凭据面板（替代弹窗）
pub struct ExportPanel {
    /// 导出格式：JSON 备份 / BAT 脚本
    format: ExportFormat,
    pw_state: Entity<InputState>,
    pw_for_click: Entity<InputState>,
    cred_count: usize,
    weak_app: WeakEntity<SapApp>,
    /// 用户通过系统对话框选择的导出路径（None = 默认 data/exports/ 路径）
    save_path: Option<std::path::PathBuf>,
    /// BAT 导出目标目录（None = 默认 data/exports/bat-{时间戳}/）
    bat_dir: Option<std::path::PathBuf>,
}

impl ExportPanel {
    pub fn new(
        pw_state: Entity<InputState>,
        pw_for_click: Entity<InputState>,
        cred_count: usize,
        weak_app: WeakEntity<SapApp>,
        _cx: &mut Context<Self>,
    ) -> Self {
        Self {
            format: ExportFormat::Json,
            pw_state,
            pw_for_click,
            cred_count,
            weak_app,
            save_path: None,
            bat_dir: None,
        }
    }
}

impl Render for ExportPanel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let pw_state = self.pw_state.clone();
        let pw_for_click = self.pw_for_click.clone();
        let cred_count = self.cred_count;
        let weak_app = self.weak_app.clone();
        let is_bat = self.format == ExportFormat::Bat;

        // v4 Consider #22：export_path()/bat_export_dir() 按秒生成时间戳，
        // 同一帧只算一次，避免显示 / 成功 toast / 实际落盘三者路径不一致
        let default_json = export_path();
        let default_bat = bat_export_dir();
        // 导出位置显示（JSON = 文件路径；BAT = 目标目录）
        let path_display = if is_bat {
            self.bat_dir
                .clone()
                .unwrap_or_else(|| default_bat.clone())
                .display()
                .to_string()
        } else {
            self.save_path
                .clone()
                .unwrap_or_else(|| default_json.clone())
                .display()
                .to_string()
        };
        // 确认时实际使用的目标（克隆进闭包）
        let json_target = self
            .save_path
            .clone()
            .unwrap_or_else(|| default_json.clone());
        let bat_target = self.bat_dir.clone().unwrap_or_else(|| default_bat.clone());

        v_flex()
            .gap_3()
            .p_3()
            // 卡片：卡头（滑块式格式切换 + 危险提示）+ 卡体（预览/路径/密码/确认）
            .child(
                v_flex()
                    .border_1()
                    .border_color(cx.theme().border)
                    .rounded_lg()
                    .overflow_hidden()
                    // 卡头
                    .child(
                        v_flex()
                            .p_3()
                            .gap_3()
                            .border_b_1()
                            .border_color(cx.theme().border)
                            // 滑块式分段标签（设计稿方案一）
                            .child(
                                h_flex()
                                    .bg(cx.theme().secondary)
                                    .rounded_md()
                                    .p_1()
                                    .gap_1()
                                    .child(
                                        segmented_item(
                                            "export-fmt-json",
                                            t("JSON 备份"),
                                            !is_bat,
                                            cx,
                                        )
                                        .on_click(
                                            cx.listener(|this, _, _, cx| {
                                                this.format = ExportFormat::Json;
                                                cx.notify();
                                            }),
                                        ),
                                    )
                                    .child(
                                        segmented_item("export-fmt-bat", t("BAT 脚本"), is_bat, cx)
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.format = ExportFormat::Bat;
                                                cx.notify();
                                            })),
                                    ),
                            )
                            // 危险提示
                            .child(
                                h_flex()
                                    .items_center()
                                    .gap_1p5()
                                    .px_2p5()
                                    .py_2()
                                    .rounded(cx.theme().radius)
                                    .bg({
                                        let mut c = cx.theme().danger;
                                        c.a = 0.08;
                                        c
                                    })
                                    .child(
                                        Icon::new(IconName::TriangleAlert)
                                            .size_4()
                                            .text_color(cx.theme().danger),
                                    )
                                    .child(if is_bat {
                                        div()
                                            .text_size(px(tokens::TS_BASE))
                                            .text_color(cx.theme().danger)
                                            .child(t("BAT 脚本包含明文密码，请妥善保管！"))
                                    } else {
                                        div()
                                            .text_size(px(tokens::TS_BASE))
                                            .text_color(cx.theme().danger)
                                            .child(t("导出文件包含明文密码，请妥善保管！"))
                                    }),
                            ),
                    )
                    // 卡体
                    .child(
                        v_flex()
                            .p_3()
                            .gap_3()
                            // 预览数量
                            .child(
                                div()
                                    .text_size(px(tokens::TS_BASE))
                                    .text_color(cx.theme().muted_foreground)
                                    .child(if is_bat {
                                        tf(
                                            "将导出 {cred_count} 条凭据为 BAT 脚本（每条一个文件）",
                                            &[&cred_count.to_string()],
                                        )
                                    } else {
                                        tf(
                                            "将导出 {cred_count} 条凭据到 JSON 文件",
                                            &[&cred_count.to_string()],
                                        )
                                    }),
                            )
                            // 导出位置：JSON 可点「浏览…」另存；BAT 选择目标目录
                            .child(
                                h_flex()
                                    .items_center()
                                    .justify_between()
                                    .gap_2()
                                    .child(
                                        h_flex()
                                            .items_center()
                                            .gap_1p5()
                                            .min_w_0()
                                            .child(
                                                Icon::new(IconName::FolderOpen)
                                                    .size_4()
                                                    .text_color(cx.theme().muted_foreground),
                                            )
                                            .child(
                                                div()
                                                    .min_w_0()
                                                    .truncate()
                                                    .text_size(px(tokens::TS_SM))
                                                    .text_color(cx.theme().muted_foreground)
                                                    .child(path_display),
                                            ),
                                    )
                                    .child(
                                        Button::new("export-browse")
                                            .ghost()
                                            .xsmall()
                                            .icon(IconName::FolderOpen)
                                            .label(t("浏览…"))
                                            .tooltip(t("选择导出位置"))
                                            .on_click(cx.listener(|this, _, window, cx| {
                                                // 异步打开文件对话框：rfd 的模态消息循环会重入
                                                // GPUI 事件，与实体回调的 App 借用冲突会 panic
                                                //（RefCell already borrowed，见 data/panic.log）
                                                let is_bat = this.format == ExportFormat::Bat;
                                                cx.spawn_in(window, async move |this, window| {
                                                    let picked = window
                                                        .background_executor()
                                                        .spawn(async move {
                                                            if is_bat {
                                                                // BAT：选择目标目录
                                                                let default = bat_export_dir();
                                                                let mut dialog =
                                                                    rfd::FileDialog::new();
                                                                if let Some(dir) = default.parent()
                                                                {
                                                                    dialog =
                                                                        dialog.set_directory(dir);
                                                                }
                                                                dialog.pick_folder()
                                                            } else {
                                                                // JSON：另存为
                                                                let default = export_path();
                                                                let mut dialog =
                                                                    rfd::FileDialog::new()
                                                                        .add_filter(
                                                                            "JSON",
                                                                            &["json"],
                                                                        );
                                                                if let Some(dir) = default.parent()
                                                                {
                                                                    dialog =
                                                                        dialog.set_directory(dir);
                                                                }
                                                                if let Some(name) = default
                                                                    .file_name()
                                                                    .and_then(|n| n.to_str())
                                                                {
                                                                    dialog =
                                                                        dialog.set_file_name(name);
                                                                }
                                                                dialog.save_file()
                                                            }
                                                        })
                                                        .await;
                                                    if let Some(p) = picked {
                                                        _ = this.update_in(
                                                            window,
                                                            |this, _, cx| {
                                                                if is_bat {
                                                                    this.bat_dir = Some(p);
                                                                } else {
                                                                    this.save_path = Some(p);
                                                                }
                                                                cx.notify();
                                                            },
                                                        );
                                                    }
                                                })
                                                .detach();
                                            })),
                                    ),
                            )
                            // 主密码输入（带标签，提升可见性）
                            .child(
                                v_flex()
                                    .gap_1()
                                    .child(
                                        div()
                                            .text_size(px(tokens::TS_XS))
                                            .text_color(cx.theme().muted_foreground)
                                            .child(t("主密码")),
                                    )
                                    .child(Input::new(&pw_state)),
                            )
                            // 确认按钮（导出为耗时操作：主密码验证 + 逐条 Argon2 解密，
                            // 必须在后台线程执行，避免长阻塞主线程与托盘轮询泵产生
                            // RefCell 借用冲突闪退）
                            .child(
                                Button::new("export-confirm")
                                    .primary()
                                    .label(t("验证并导出"))
                                    .w_full()
                                    .on_click(move |_, window, cx| {
                                        // v4 Should #15：面板主密码副本受 Zeroizing 保护，
                                        // 并在点击瞬间清空 InputState 缓冲
                                        let pw = zeroize::Zeroizing::new(
                                            pw_for_click.read(cx).value().to_string(),
                                        );
                                        pw_for_click.update(cx, |s, cx| {
                                            s.set_value(String::new(), window, cx)
                                        });
                                        if pw.is_empty() {
                                            window.push_notification(t("请输入主密码"), cx);
                                            return;
                                        }
                                        let store = AppState::global(cx).store.clone();
                                        let weak_app = weak_app.clone();
                                        let (bat_target, json_target) =
                                            (bat_target.clone(), json_target.clone());
                                        let wh = window.window_handle();
                                        cx.spawn(async move |cx| {
                                            // 后台执行导出（Argon2 派生耗时）
                                            let result =
                                                cx.background_executor()
                                                    .spawn(async move {
                                                        if is_bat {
                                                            transfer::export_bat_files(
                                                                &store,
                                                                &pw,
                                                                bat_target
                                                                    .to_string_lossy()
                                                                    .into_owned(),
                                                            )
                                                            .map(|n| {
                                                                tf(
                                                "已导出 {n} 个 BAT 脚本到 {dir}",
                                                &[
                                                    &n.to_string(),
                                                    &bat_target.display().to_string(),
                                                ],
                                            )
                                                            })
                                                        } else {
                                                            transfer::export_credentials(
                                                                &store,
                                                                &pw,
                                                                json_target
                                                                    .to_string_lossy()
                                                                    .into_owned(),
                                                            )
                                                            .map(|_| {
                                                                tf(
                                                                    "导出成功：{}",
                                                                    &[&json_target
                                                                        .display()
                                                                        .to_string()],
                                                                )
                                                            })
                                                        }
                                                    })
                                                    .await;

                                            // 回主线程：通知 + 关闭面板
                                            _ = wh.update(cx, |_, window, cx| match result {
                                                Ok(msg) => {
                                                    window.push_notification(msg, cx);
                                                    if let Some(app) = weak_app.upgrade() {
                                                        app.update(cx, |app, cx| {
                                                            app.active_panel = None;
                                                            app.panel_title = None;
                                                            app.panel_kind = None;
                                                            cx.notify();
                                                        });
                                                    }
                                                }
                                                Err(e) => {
                                                    window.push_notification(
                                                        tf("导出失败：{e}", &[&e.to_string()]),
                                                        cx,
                                                    );
                                                }
                                            });
                                        })
                                        .detach();
                                    }),
                            ),
                    ),
            )
    }
}

/// 导出文件路径：exe 目录 data/exports/sap-credentials-{时间戳}.json
fn export_path() -> std::path::PathBuf {
    let dir = crate::data_dir().join("exports");
    let _ = std::fs::create_dir_all(&dir);
    let ts = chrono::Local::now().format("%Y%m%d-%H%M%S");
    dir.join(format!("sap-credentials-{ts}.json"))
}

/// BAT 导出默认目录：exe 目录 data/exports/bat-{时间戳}/（每条凭据一个 .bat 文件）
fn bat_export_dir() -> std::path::PathBuf {
    let ts = chrono::Local::now().format("%Y%m%d-%H%M%S");
    crate::data_dir().join("exports").join(format!("bat-{ts}"))
}

/// BAT 预览表格委托（卡片列表改表格：行高 30，随面板弹性伸缩）
///
/// 列：状态 / 来源 / 连接名 / 系统 / 集团 / 用户 / 服务器 / 说明
/// 无效行整行淡红底；说明列优先展示错误（红），其次警告（灰），
/// 都没有则展示本机 SAP GUI 配置匹配结果
pub(crate) struct BatDraftTableDelegate {
    drafts: Vec<BatDraft>,
    matches: Vec<LandscapeMatch>,
}

impl BatDraftTableDelegate {
    pub(crate) fn new(drafts: Vec<BatDraft>, landscape: &[SapConnection]) -> Self {
        let matches = drafts
            .iter()
            .map(|d| bat_parser::match_landscape(d, landscape))
            .collect();
        Self { drafts, matches }
    }

    /// 服务器列文本：直连 host:port / 负载均衡 消息服务器/组
    fn server_text(d: &BatDraft) -> String {
        if d.connection_type == "load_balancing" {
            let mut s = d.message_server.clone();
            if !d.logon_group.is_empty() {
                if !s.is_empty() {
                    s.push_str(" / ");
                }
                s.push_str(&d.logon_group);
            }
            s
        } else {
            let mut s = d.app_server.clone();
            if !d.system_number.is_empty() {
                if !s.is_empty() {
                    s.push(':');
                }
                s.push_str(&d.system_number);
            }
            s
        }
    }
}

impl TableDelegate for BatDraftTableDelegate {
    fn columns_count(&self, _: &App) -> usize {
        8
    }

    fn rows_count(&self, _: &App) -> usize {
        self.drafts.len()
    }

    fn column(&self, ix: usize, _: &App) -> Column {
        match ix {
            0 => Column::new("status", "")
                .width(px(36.))
                .text_center()
                .selectable(false)
                .resizable(false)
                .movable(false)
                .p_0(),
            1 => Column::new("source", t("来源"))
                .width(px(150.))
                .movable(false),
            2 => Column::new("conn", t("连接名"))
                .width(px(110.))
                .movable(false),
            3 => Column::new("sid", t("系统")).width(px(48.)).movable(false),
            4 => Column::new("client", t("集团"))
                .width(px(48.))
                .movable(false),
            5 => Column::new("user", t("用户"))
                .width(px(100.))
                .movable(false),
            6 => Column::new("server", t("服务器"))
                .width(px(170.))
                .movable(false),
            _ => Column::new("note", t("说明"))
                .width(px(220.))
                .movable(false),
        }
    }

    fn render_tr(
        &mut self,
        row_ix: usize,
        _: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) -> Stateful<Div> {
        let invalid = self.drafts.get(row_ix).is_some_and(|d| !d.is_valid());
        let d = self.drafts.get(row_ix);
        let key = d
            .map(|d| format!("bat-row-{}-{}-{}", row_ix, d.source, d.line_no))
            .unwrap_or_else(|| format!("bat-row-{row_ix}"));
        div().id(key).when(invalid, |this| {
            let mut c = cx.theme().danger;
            c.a = 0.08;
            this.bg(c)
        })
    }

    fn render_td(
        &mut self,
        row_ix: usize,
        col_ix: usize,
        _: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) -> impl IntoElement {
        let Some(d) = self.drafts.get(row_ix) else {
            return div().into_any_element();
        };
        let lm = self
            .matches
            .get(row_ix)
            .cloned()
            .unwrap_or(LandscapeMatch::NotFound);
        let valid = d.is_valid();
        let (fg, muted, danger_c, accent_c) = (
            cx.theme().foreground,
            cx.theme().muted_foreground,
            cx.theme().danger,
            cx.theme().accent,
        );

        let cell = move |text: String| {
            h_flex()
                .size_full()
                .items_center()
                .min_w_0()
                .child(
                    div()
                        .text_size(px(tokens::TS_SM))
                        .text_color(fg)
                        .truncate()
                        .child(text),
                )
                .into_any_element()
        };

        match col_ix {
            0 => h_flex()
                .size_full()
                .items_center()
                .justify_center()
                .child(if valid {
                    Icon::new(IconName::File).size_4().text_color(muted)
                } else {
                    Icon::new(IconName::TriangleAlert)
                        .size_4()
                        .text_color(danger_c)
                })
                .into_any_element(),
            1 => {
                let text = if d.source.is_empty() {
                    tf("第 {line} 行", &[&d.line_no.to_string()])
                } else {
                    tf("{file} · L{line}", &[&d.source, &d.line_no.to_string()])
                };
                cell(text)
            }
            2 => cell(d.connection_name.clone()),
            3 => cell(d.system_id.clone()),
            4 => cell(d.client.clone()),
            5 => cell(d.username.clone()),
            6 => cell(Self::server_text(d)),
            _ => {
                // 说明：错误（红）优先，其次警告（灰），否则匹配结果
                let (text, color) = if let Some(e) = d.errors.first() {
                    (e.clone(), danger_c)
                } else if let Some(w) = d.warnings.first() {
                    (w.clone(), muted)
                } else {
                    match &lm {
                        LandscapeMatch::Matched(name) => {
                            (tf("已匹配 SAP GUI 配置：{name}", &[name]), accent_c)
                        }
                        LandscapeMatch::Partial(name, reason) => {
                            (tf("部分匹配：{name}（{reason}）", &[name, reason]), muted)
                        }
                        LandscapeMatch::NotFound => {
                            (t("未匹配到本机 SAP GUI 配置").to_string(), muted)
                        }
                    }
                };
                h_flex()
                    .size_full()
                    .items_center()
                    .min_w_0()
                    .child(
                        div()
                            .text_size(px(tokens::TS_SM))
                            .text_color(color)
                            .truncate()
                            .child(text),
                    )
                    .into_any_element()
            }
        }
    }
}
