//! 备份导入面板：双来源导入（JSON 备份合并 / BAT 脚本批量录入）。
//!
//! 由 ui/mod.rs 拆分而来（v1.5.9）。

use super::super::i18n::{t, tf};
use super::super::tokens;
use super::super::{ImportSource, SapApp};
use super::export::BatDraftTableDelegate;
use crate::state::AppState;
use gpui_kit::assets::IconName;
use gpui_kit::component::{
    button::{Button, ButtonVariants as _},
    h_flex,
    input::{Input, InputState},
    menu::{DropdownMenu as _, PopupMenuItem},
    switch::Switch,
    table::{DataTable, TableState},
    v_flex, ActiveTheme as _, Icon, Sizable as _, WindowExt as _,
};
use gpui_kit::*;
use sap_backend::bat_parser::{self, BatDraft, LandscapeMatch};
use sap_backend::commands::{auth, transfer};
use sap_backend::models::{ExportBundle, Group, SapConnection};
use sap_backend::sap_parser;
use zeroize::Zeroizing;

/// 备份导入面板：双来源导入（JSON 备份合并 / BAT 脚本批量录入）
///
/// - JSON 备份：按 connection_id 去重（已存在的跳过），明文密码用当前主密码重新加密
/// - BAT 脚本：解析 sapshcut 命令批量录入，按 system_id+client+username 查重
///   （默认跳过，可切换为覆盖更新），无效行标红展示失败原因
pub struct BackupImportPanel {
    /// 导入来源：JSON 备份 / BAT 脚本
    source: ImportSource,
    file: Option<std::path::PathBuf>,
    /// v5 Consider #27：整份明文备份用 Zeroizing 持有；渲染期 clone 与
    /// 确认时传入后台闭包的副本也都在析构时清零。
    bundle: Option<Zeroizing<ExportBundle>>,
    /// BAT 解析出的凭据草稿（含每条的错误/警告）
    drafts: Vec<BatDraft>,
    /// BAT 预览表格（pick_bat 成功后重建；展示 drafts + 匹配结果）
    bat_table: Option<Entity<TableState<BatDraftTableDelegate>>>,
    /// BAT 多选导入的文件个数（统计行显示）
    file_count: usize,
    /// 本机 SAP GUI Landscape 连接（pick_bat 时加载，用于配置比对）
    landscape: Vec<SapConnection>,
    /// BAT 来源：已存在时是否覆盖更新（false = 跳过）
    overwrite: bool,
    /// BAT 来源：目标分组（None = 不分组）
    group_id: Option<String>,
    /// 自定义分组列表（构造时快照，用于目标分组下拉）
    custom_groups: Vec<Group>,
    pw_state: Entity<InputState>,
    pw_for_click: Entity<InputState>,
    weak_app: WeakEntity<SapApp>,
}

impl BackupImportPanel {
    pub fn new(
        pw_state: Entity<InputState>,
        pw_for_click: Entity<InputState>,
        weak_app: WeakEntity<SapApp>,
        cx: &mut Context<Self>,
    ) -> Self {
        let store = AppState::global(cx).store.clone();
        let custom_groups = {
            let s = store.lock();
            s.groups.iter().filter(|g| !g.is_system).cloned().collect()
        };
        Self {
            source: ImportSource::Backup,
            file: None,
            bundle: None,
            drafts: Vec::new(),
            bat_table: None,
            file_count: 0,
            landscape: Vec::new(),
            overwrite: false,
            group_id: None,
            custom_groups,
            pw_state,
            pw_for_click,
            weak_app,
        }
    }

    /// 设置导入来源（嵌入统一导入面板后由外层切换）
    pub fn set_source(&mut self, source: ImportSource) {
        self.source = source;
    }

    /// 用系统对话框选择备份文件并解析预览（异步：rfd 模态循环 + 文件 IO 不阻塞主线程）。
    /// 用户取消（None）静默返回，不与「文件损坏」混为一谈；解析失败时
    /// 把 serde 的具体错误带进提示，用户能知道是哪个字段不合法。
    fn pick_file(&mut self, _: &ClickEvent, window: &mut Window, cx: &mut Context<Self>) {
        cx.spawn_in(window, async move |this, window| {
            let parsed = window
                .background_executor()
                .spawn(async move {
                    let Some(p) = rfd::FileDialog::new()
                        .add_filter("JSON", &["json"])
                        .pick_file()
                    else {
                        return Ok(None); // 用户取消：静默返回
                    };
                    // v5 Consider #27：整份明文备份的磁盘内容副本必须清零
                    let content =
                        Zeroizing::new(std::fs::read_to_string(&p).map_err(|e| e.to_string())?);
                    serde_json::from_str::<ExportBundle>(content.as_str())
                        .map(|bundle| Some((p, Zeroizing::new(bundle))))
                        .map_err(|e| e.to_string())
                })
                .await;
            match parsed {
                Ok(Some((p, bundle))) => {
                    _ = this.update_in(window, |this, _, cx| {
                        this.file = Some(p);
                        this.bundle = Some(bundle);
                        cx.notify();
                    });
                }
                Ok(None) => {}
                Err(e) => {
                    _ = this.update_in(window, move |_this, window, cx| {
                        window.push_notification(tf("备份文件读取失败：{e}", &[&e]), cx);
                        cx.notify();
                    });
                }
            }
        })
        .detach();
    }

    /// 用系统对话框选择 BAT 脚本（支持多选）并解析为凭据草稿
    ///（异步：文件读取、GBK 解码、Landscape 加载与补全均不阻塞主线程）
    fn pick_bat(&mut self, _: &ClickEvent, window: &mut Window, cx: &mut Context<Self>) {
        cx.spawn_in(window, async move |this, window| {
            let outcome = window
                .background_executor()
                .spawn(async move {
                    let Some(paths) = rfd::FileDialog::new()
                        .add_filter("BAT", &["bat", "cmd", "txt"])
                        .pick_files()
                    else {
                        return None; // 用户取消
                    };
                    // 加载本机 SAP GUI Landscape（既用于配置比对，也用于补全
                    // 纯 SID 引用型 bat——原生快捷方式常不含服务器地址）。
                    // v5 Should #8：不再 unwrap_or_default 吞掉「全失败」Err，
                    // 部分失败也计入已有的 read_errors，让用户看到补全依据不完整。
                    let (landscape, landscape_errors) =
                        sap_parser::get_all_sap_connections_with_errors();

                    let mut all: Vec<BatDraft> = Vec::new();
                    let mut read_errors: Vec<String> = landscape_errors
                        .into_iter()
                        .map(|e| format!("SAP GUI 配置: {e}"))
                        .collect();
                    let mut no_cmd_files = 0usize;
                    for p in &paths {
                        let fname = p
                            .file_name()
                            .map(|n| n.to_string_lossy().into_owned())
                            .unwrap_or_default();
                        match bat_parser::read_bat_file(p)
                            .map(|content| bat_parser::parse_bat(&content))
                        {
                            Ok(drafts) if !drafts.is_empty() => {
                                let mut drafts = drafts;
                                for d in &mut drafts {
                                    d.source = fname.clone();
                                    if d.connection_type.is_empty()
                                        && !bat_parser::resolve_from_landscape(d, &landscape)
                                    {
                                        d.errors.push(
                                            "未指定服务器地址，且本机 SAP GUI 配置中未找到该系统（无法补全）"
                                                .to_string(),
                                        );
                                    }
                                }
                                all.extend(drafts);
                            }
                            Ok(_) => no_cmd_files += 1, // 文件里没有 sapshcut 命令，跳过
                            Err(e) => read_errors.push(format!("{fname}: {e}")),
                        }
                    }
                    Some((all, paths.len(), read_errors, no_cmd_files, landscape))
                })
                .await;

            let Some((all, file_count, read_errors, no_cmd_files, landscape)) = outcome else {
                return; // 用户取消
            };
            _ = this.update_in(window, |this, window, cx| {
                if all.is_empty() {
                    if !read_errors.is_empty() {
                        window.push_notification(
                            tf("读取失败：{e}", &[&read_errors.join("；")]),
                            cx,
                        );
                    } else {
                        window.push_notification(t("未在文件中找到 sapshcut 登录命令"), cx);
                    }
                } else {
                    if !read_errors.is_empty() {
                        window.push_notification(
                            tf("部分文件读取失败：{e}", &[&read_errors.join("；")]),
                            cx,
                        );
                    } else if no_cmd_files > 0 {
                        window.push_notification(
                            tf(
                                "{n} 个文件中未找到 sapshcut 命令，已跳过",
                                &[&no_cmd_files.to_string()],
                            ),
                            cx,
                        );
                    }
                    this.file_count = file_count;
                    this.drafts = all;
                    // Landscape 存到实体：渲染期匹配提示与表格「说明」列都要用
                    this.landscape = landscape;
                    // 重建 BAT 预览表格（卡片列表改表格：行高 30，可滚动，不挤占操作区）
                    let drafts = this.drafts.clone();
                    let landscape = this.landscape.clone();
                    this.bat_table = Some(cx.new(|cx| {
                        TableState::new(
                            BatDraftTableDelegate::new(drafts, &landscape),
                            window,
                            cx,
                        )
                        .row_selectable(false)
                        .col_selectable(false)
                        .cell_selectable(false)
                        .col_movable(false)
                        .sortable(false)
                        .col_resizable(true)
                    }));
                }
                cx.notify();
            });
        })
        .detach();
    }
}

impl Render for BackupImportPanel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let pw_state = self.pw_state.clone();
        let pw_for_click = self.pw_for_click.clone();
        let weak_app = self.weak_app.clone();
        let is_bat = self.source == ImportSource::Bat;
        let bundle = self.bundle.clone();
        let drafts = self.drafts.clone();
        let overwrite = self.overwrite;
        let group_id = self.group_id.clone();

        // 说明文字（按来源；来源切换由外层统一导入面板控制）
        let desc = if is_bat {
            t("解析 sapshcut 命令批量录入；按 系统+集团+用户 查重（已存在默认跳过），明文密码加密入库，无效行标红。").to_string()
        } else {
            t("选择此前导出的 JSON 备份，连接将合并导入（同名跳过），密码用当前主密码重新加密。")
                .to_string()
        };

        // JSON 备份：文件信息卡（沿用原实现）
        let file_info: Option<AnyElement> = if !is_bat {
            self.bundle.as_ref().map(|bundle| {
                let name = self
                    .file
                    .as_ref()
                    .and_then(|p| p.file_name())
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default();
                let time = bundle.exported_at.with_timezone(&chrono::Local);
                let time_str = time.format("%Y-%m-%d %H:%M").to_string();
                v_flex()
                    .gap_1()
                    .p_2()
                    .rounded(cx.theme().radius)
                    .border_1()
                    .border_color(cx.theme().border)
                    .bg(cx.theme().secondary)
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
                                        Icon::new(IconName::File)
                                            .size_4()
                                            .text_color(cx.theme().muted_foreground),
                                    )
                                    .child(
                                        div()
                                            .min_w_0()
                                            .truncate()
                                            .text_size(px(tokens::TS_SM))
                                            .font_weight(FontWeight::MEDIUM)
                                            .text_color(cx.theme().foreground)
                                            .child(name),
                                    ),
                            )
                            .child(
                                Button::new("backup-pick-again")
                                    .ghost()
                                    .xsmall()
                                    .label(t("重新选择"))
                                    .on_click(cx.listener(Self::pick_file)),
                            ),
                    )
                    .child(
                        div()
                            .text_size(px(tokens::TS_XS))
                            .text_color(cx.theme().muted_foreground)
                            .child(tf(
                                "包含 {cred_count} 条连接、{group_count} 个分组 · 备份时间：{time}",
                                &[
                                    &bundle.connection_count.to_string(),
                                    &bundle.group_count.to_string(),
                                    &time_str,
                                ],
                            )),
                    )
                    .into_any_element()
            })
        } else {
            None
        };

        // BAT：预览统计 + 表格（表格 flex_1 弹性填充、内部滚动，不挤占下方操作区）
        let bat_info: Option<AnyElement> = if is_bat && !self.drafts.is_empty() {
            let total = self.drafts.len();
            let valid_count = self.drafts.iter().filter(|d| d.is_valid()).count();
            let matched_count = self
                .drafts
                .iter()
                .map(|d| bat_parser::match_landscape(d, &self.landscape))
                .filter(|m| matches!(m, LandscapeMatch::Matched(_)))
                .count();
            // Landscape 状态提示（未检测到配置时提醒用户比对不可用）
            let landscape_hint = if self.landscape.is_empty() {
                t("未检测到本机 SAP GUI 配置，匹配结果仅供参考").to_string()
            } else {
                tf(
                    "已加载本机 SAP GUI 配置（{n} 条连接）",
                    &[&self.landscape.len().to_string()],
                )
            };
            Some(
                v_flex()
                    .flex_1()
                    .min_h_0()
                    .gap_1()
                    .child(
                        div()
                            .flex_none()
                            .text_size(px(tokens::TS_XS))
                            .text_color(cx.theme().muted_foreground)
                            .child(tf(
                                "已选 {files} 个文件 · 共 {total} 条命令，{valid} 条可导入，{matched} 条匹配 SAP GUI",
                                &[
                                    &self.file_count.to_string(),
                                    &total.to_string(),
                                    &valid_count.to_string(),
                                    &matched_count.to_string(),
                                ],
                            )),
                    )
                    .child(
                        div()
                            .flex_none()
                            .text_size(px(tokens::TS_XS))
                            .text_color(cx.theme().muted_foreground)
                            .child(landscape_hint),
                    )
                    .child(
                        v_flex()
                            .id("bat-preview-table-box")
                            .flex_1()
                            .min_h_0()
                            .rounded_lg()
                            .border_1()
                            .border_color(cx.theme().border)
                            .overflow_hidden()
                            .bg(cx.theme().sidebar)
                            .child(
                                div().flex_1().min_h_0().child(
                                    match self.bat_table.as_ref() {
                                        Some(tb) => DataTable::new(tb)
                                            .with_size(px(30.))
                                            .into_any_element(),
                                        None => div().into_any_element(),
                                    },
                                ),
                            ),
                    )
                    .into_any_element(),
            )
        } else {
            None
        };

        // BAT 专属选项：覆盖开关 + 目标分组下拉
        let bat_options: Option<AnyElement> = if is_bat && !self.drafts.is_empty() {
            let weak = cx.entity().downgrade();
            let default_label = t("不分组");
            let current_label = match self.group_id.as_deref() {
                None | Some("") => default_label.to_string(),
                Some(gid) => self
                    .custom_groups
                    .iter()
                    .find(|g| g.id == gid)
                    .map(|g| g.group_name.clone())
                    .unwrap_or_else(|| default_label.to_string()),
            };
            let menu_group_id = self.group_id.clone();
            let menu_custom_groups = self.custom_groups.clone();
            Some(
                v_flex()
                    .gap_2()
                    .child(
                        h_flex()
                            .items_center()
                            .justify_between()
                            .py_1()
                            .child(
                                div()
                                    .text_size(px(tokens::TS_BASE))
                                    .text_color(cx.theme().foreground)
                                    .child(t("已存在时覆盖更新")),
                            )
                            .child(
                                Switch::new("bat-overwrite")
                                    .checked(self.overwrite)
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.overwrite = !this.overwrite;
                                        cx.notify();
                                    })),
                            ),
                    )
                    .child(
                        h_flex()
                            .items_center()
                            .gap_2()
                            .child(
                                div()
                                    .text_size(px(tokens::TS_XS))
                                    .text_color(cx.theme().muted_foreground)
                                    .child(t("导入到分组")),
                            )
                            .child(
                                Button::new("bat-group-select")
                                    .secondary()
                                    .xsmall()
                                    .icon(IconName::FolderInput)
                                    .label(current_label)
                                    .dropdown_menu(move |menu, _, _| {
                                        let mut menu = menu.label(t("分组"));
                                        let checked = menu_group_id.is_none()
                                            || menu_group_id.as_deref() == Some("");
                                        let weak_default = weak.clone();
                                        menu = menu.item(
                                            PopupMenuItem::new(default_label)
                                                .checked(checked)
                                                .on_click(move |_, _, cx| {
                                                    if let Some(p) = weak_default.upgrade() {
                                                        p.update(cx, |p, cx| {
                                                            p.group_id = None;
                                                            cx.notify();
                                                        });
                                                    }
                                                }),
                                        );
                                        for g in menu_custom_groups.iter() {
                                            let checked =
                                                menu_group_id.as_deref() == Some(g.id.as_str());
                                            let gid = g.id.clone();
                                            let weak_item = weak.clone();
                                            menu = menu.item(
                                                PopupMenuItem::new(g.group_name.clone())
                                                    .checked(checked)
                                                    .on_click(move |_, _, cx| {
                                                        if let Some(p) = weak_item.upgrade() {
                                                            p.update(cx, |p, cx| {
                                                                p.group_id = Some(gid.clone());
                                                                cx.notify();
                                                            });
                                                        }
                                                    }),
                                            );
                                        }
                                        menu
                                    })
                                    .anchor(Anchor::TopLeft),
                            ),
                    )
                    .into_any_element(),
            )
        } else {
            None
        };

        // 未选择文件/脚本时的选择入口（按来源，secondary 底色提升可见性）
        let pick_entry: Option<AnyElement> = if is_bat {
            if self.drafts.is_empty() {
                Some(
                    Button::new("bat-pick")
                        .secondary()
                        .icon(IconName::FolderOpen)
                        .label(t("选择 BAT 脚本文件（可多选）"))
                        .w_full()
                        .on_click(cx.listener(Self::pick_bat))
                        .into_any_element(),
                )
            } else {
                None
            }
        } else if self.bundle.is_none() {
            Some(
                Button::new("backup-pick")
                    .secondary()
                    .icon(IconName::FolderOpen)
                    .label(t("选择 JSON 备份文件"))
                    .w_full()
                    .on_click(cx.listener(Self::pick_file))
                    .into_any_element(),
            )
        } else {
            None
        };

        v_flex()
            .flex_1()
            .min_h_0()
            .gap_3()
            .p_3()
            .child(
                v_flex()
                    .flex_1()
                    .min_h_0()
                    .border_1()
                    .border_color(cx.theme().border)
                    .rounded_lg()
                    .overflow_hidden()
                    // 卡头：来源说明
                    .child(
                        v_flex()
                            .flex_none()
                            .p_3()
                            .gap_2()
                            .border_b_1()
                            .border_color(cx.theme().border)
                            .child(
                                div()
                                    .text_size(px(tokens::TS_BASE))
                                    .text_color(cx.theme().muted_foreground)
                                    .child(desc),
                            ),
                    )
                    // 卡体
                    .child(
                        v_flex()
                            .flex_1()
                            .min_h_0()
                            .p_3()
                            .gap_3()
                            .children(pick_entry)
                            .children(file_info)
                            .children(bat_info)
                            .children(bat_options)
            // 主密码输入（带标签，用于验证 + 重新加密）
            .child(
                v_flex().gap_1()
                    .child(
                        div()
                            .text_size(px(tokens::TS_XS))
                            .text_color(cx.theme().muted_foreground)
                            .child(t("主密码")),
                    )
                    .child(Input::new(&pw_state)),
            )
            // 确认按钮（主密码验证与导入均为 Argon2 密集操作，后台执行
            // 避免长阻塞主线程导致 RefCell 借用冲突闪退）
            .child(
                Button::new("backup-import-confirm")
                    .primary()
                    .label(t("验证并导入"))
                    .w_full()
                    .on_click(move |_, window, cx| {
                        // v4 Should #15：面板主密码副本受 Zeroizing 保护，并清空输入缓冲
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
                        if is_bat {
                            if drafts.is_empty() {
                                window.push_notification(t("未选择 BAT 文件"), cx);
                                return;
                            }
                        } else if bundle.is_none() {
                            window.push_notification(t("未选择备份文件"), cx);
                            return;
                        }
                        let weak_app = weak_app.clone();
                        let (drafts, bundle, group_id) =
                            (drafts.clone(), bundle.clone(), group_id.clone());
                        let wh = window.window_handle();
                        cx.spawn(async move |cx| {
                            // 后台执行：验证 + 导入
                            let result: Result<(String, bool), String> = cx
                                .background_executor()
                                .spawn(async move {
                                    if is_bat {
                                        transfer::import_bat_credentials(
                                            &store,
                                            &pw,
                                            drafts,
                                            overwrite,
                                            group_id,
                                        )
                                        .map(|r| {
                                            if r.added + r.updated == 0 {
                                                (
                                                    tf(
                                                        "全部跳过：已存在或无效，共 {skipped} 条",
                                                        &[&r.skipped.to_string()],
                                                    ),
                                                    false,
                                                )
                                            } else {
                                                (
                                                    tf(
                                                        "导入完成：新增 {added} 条、覆盖 {updated} 条、跳过 {skipped} 条",
                                                        &[
                                                            &r.added.to_string(),
                                                            &r.updated.to_string(),
                                                            &r.skipped.to_string(),
                                                        ],
                                                    ),
                                                    true,
                                                )
                                            }
                                        })
                                        .map_err(|e| e.to_string())
                                    } else {
                                        // 消除 check-then-unwrap：守卫与解包相距 40+ 行易被破坏
                                        let Some(bundle) = bundle else {
                                            return Err("未选择备份文件".to_string());
                                        };
                                        auth::verify_master_password(&store, &pw)
                                            .map_err(|e| e.to_string())
                                            .and_then(
                                                |ok| {
                                                    if !ok {
                                                        return Err(t("主密码错误").to_string());
                                                    }
                                                    transfer::import_credentials(&store, &pw, bundle)
                                                        .map(|n| {
                                                            if n == 0 {
                                                                (
                                                                    t("备份中的连接均已存在，未新增")
                                                                        .to_string(),
                                                                    false,
                                                                )
                                                            } else {
                                                                (
                                                                    tf(
                                                                        "已导入 {n} 条连接",
                                                                        &[&n.to_string()],
                                                                    ),
                                                                    true,
                                                                )
                                                            }
                                                        })
                                                        .map_err(|e| e.to_string())
                                                },
                                            )
                                    }
                                })
                                .await;

                            // 回主线程：通知 + 刷新 + 关闭面板
                            _ = wh.update(cx, |_, window, cx| match result {
                                Ok((msg, close)) => {
                                    window.push_notification(msg, cx);
                                    if close {
                                        if let Some(app) = weak_app.upgrade() {
                                            app.update(cx, |app, cx| {
                                                if let Some(page) = app.main_page.clone() {
                                                    page.update(cx, |p, cx| p.refresh(cx));
                                                }
                                                app.active_panel = None;
                                                app.panel_title = None;
                                                app.panel_kind = None;
                                                cx.notify();
                                            });
                                        }
                                    }
                                }
                                Err(e) => {
                                    window.push_notification(tf("导入失败：{e}", &[&e]), cx);
                                }
                            });
                        })
                        .detach();
                    }),
            )
                    )
        )
    }
}
