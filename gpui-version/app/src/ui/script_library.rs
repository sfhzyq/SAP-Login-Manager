//! SAP GUI 脚本库面板（P4：左侧活动栏入口 + 脚本管理）
//!
//! 「分组」一级视角：选中分组后直接列出该分组下**所有凭据的全部脚本**，
//! 按凭据分小节展示（同名凭据自动加 client/账号后缀区分）。
//! 脚本存储目录随凭据归属：`data/scripts/<分组ID>/<凭据ID>/*.json`（见 backend store.rs）。
//! - 导入 SAP 原生录制 `.vbs`（解析为原生动作）→ 归属小节对应凭据
//! - 凭据首次导入的脚本自动设为「登录后自动执行」（对应 `Credential.script_path`）
//! - 凭据环境为正式（production）时，小节头红色胶囊 + 列表上方红色横幅警示
//! - 执行为「会话感知」（v1.5.7）：先枚举已登录会话，匹配凭据的
//!   (system_id + client) 直接在该会话执行；未匹配则自动登录后定向执行
//!
//! 录制器（P3）未做：本面板只消费由 SAP GUI 自带「脚本录制」生成的 .vbs。

use super::credential_card;
use super::i18n::{t, tf};
use crate::state::AppState;

use gpui_kit::assets::IconName;
use gpui_kit::component::menu::{DropdownMenu as _, PopupMenuItem};
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::{
    button::{Button, ButtonVariants as _},
    h_flex, v_flex, ActiveTheme as _, Disableable as _, Icon, Sizable as _, WindowExt as _,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use std::collections::HashMap;
use std::path::PathBuf;

use sap_backend::commands::sap as sap_cmd;
use sap_backend::sap_scripting::store as script_store;
use sap_backend::sap_scripting::{self, Script};
use sap_backend::storage;

/// 面板种类标识（活动栏按钮高亮 + open_panel kind 用）
pub const SCRIPT_PANEL_KIND: &str = "scripts";

/// 分组下拉项
#[derive(Clone)]
struct GroupLite {
    id: String,
    name: String,
}

/// 凭据聚合项（全量存储，按分组过滤后组小节）
#[derive(Clone)]
struct CredLite {
    id: String,
    label: String,
    environment: String,
    group_id: String,
    /// 登录要素（小节头展示用，均为结构化短字段）
    client: String,
    username: String,
    language: String,
    system_id: String,
    app_server: String,
    system_number: String,
    message_server: String,
}

/// 列表中的一条脚本（元数据 + 磁盘路径 + 是否登录自动执行）
#[derive(Clone)]
struct ScriptEntry {
    name: String,
    path: PathBuf,
    description: String,
    action_count: usize,
    is_auto: bool,
}

/// 凭据小节：一个凭据 + 它在该分组下的全部脚本
struct CredSection {
    cred: CredLite,
    /// 分组内重名时的区分显示名（label · client/账号）
    label: String,
    scripts: Vec<ScriptEntry>,
}

/// 两步式执行：当前选中的脚本快照（选择 → 底部执行栏确认执行）
#[derive(Clone)]
struct SelectedScript {
    cred_id: String,
    /// 归属凭据显示名（小节 label，重名带后缀）
    cred_label: String,
    script_name: String,
    path: PathBuf,
    is_prod: bool,
    is_auto: bool,
}

pub struct ScriptLibraryPanel {
    /// 分组列表（含「未分组」虚拟桶，id=""）
    groups: Vec<GroupLite>,
    /// 当前选中分组 id（"" = 未分组）
    group_id: String,
    /// 全量凭据（按分组过滤后组小节）
    creds: Vec<CredLite>,
    /// 当前分组的凭据小节（每凭据一节，含其脚本）
    sections: Vec<CredSection>,
    /// 两步式：当前选中的脚本（None = 未选择，执行栏置灰）
    selected: Option<SelectedScript>,
    /// 列表滚动句柄
    scroll: ScrollHandle,
    /// 是否有脚本正在执行（防并发点击）
    running: bool,
    /// 待确认的删除操作（脚本名 + 凭据ID + 脚本条目）；Some = 弹出确认框中
    pending_delete: Option<(String, String, ScriptEntry)>,
}

impl ScriptLibraryPanel {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let mut this = Self {
            groups: Vec::new(),
            group_id: String::new(),
            creds: Vec::new(),
            sections: Vec::new(),
            selected: None,
            scroll: ScrollHandle::new(),
            running: false,
            pending_delete: None,
        };
        this.refresh_groups(cx);
        this
    }

    /// 从 store 收集分组与凭据，并做脚本目录自愈迁移
    fn refresh_groups(&mut self, cx: &mut Context<Self>) {
        let (mut groups, creds, has_ungrouped) = {
            let s = AppState::global(cx).store.lock();
            let groups: Vec<GroupLite> = s
                .groups
                .iter()
                .map(|g| GroupLite {
                    id: g.id.clone(),
                    name: g.group_name.clone(),
                })
                .collect();
            let creds: Vec<CredLite> = s
                .credentials
                .iter()
                .map(|c| CredLite {
                    id: c.id.clone(),
                    label: cred_label(c),
                    environment: c.environment.clone(),
                    group_id: c.group_id.clone().unwrap_or_default(),
                    client: c.client.clone(),
                    username: c.username.clone(),
                    language: c.language.clone(),
                    system_id: c.system_id.clone(),
                    app_server: c.app_server.clone(),
                    system_number: c.system_number.clone(),
                    message_server: c.message_server.clone(),
                })
                .collect();
            let has_ungrouped = creds.iter().any(|c| c.group_id.is_empty());
            (groups, creds, has_ungrouped)
        };
        if has_ungrouped {
            groups.push(GroupLite {
                id: String::new(),
                name: t("未分组").to_string(),
            });
        }
        self.groups = groups;
        self.creds = creds;
        // 脚本目录自愈迁移：凭据换分组后目录跟随；同时把历史 bug 按
        // connection_id 落盘的孤儿目录归属回正确凭据（不迁移旧版单层结构）
        {
            let s = AppState::global(cx).store.lock();
            let entries: Vec<script_store::CredDirInfo> = s
                .credentials
                .iter()
                .map(|c| script_store::CredDirInfo {
                    id: c.id.clone(),
                    group_id: c.group_id.clone(),
                    connection_id: c.connection_id.clone(),
                    script_path: c.script_path.clone(),
                })
                .collect();
            script_store::sync_credential_dirs(&entries);
        }
        if !self.groups.iter().any(|g| g.id == self.group_id) {
            self.group_id = self
                .groups
                .first()
                .map(|g| g.id.clone())
                .unwrap_or_default();
        }
        self.refresh_sections(cx);
    }

    /// 按当前分组聚合凭据小节（重名凭据加区分后缀），并加载各凭据脚本
    fn refresh_sections(&mut self, cx: &mut Context<Self>) {
        let in_group: Vec<CredLite> = self
            .creds
            .iter()
            .filter(|c| c.group_id == self.group_id)
            .cloned()
            .collect();
        // 重名计数（分组内同名凭据 → 加 client/账号后缀区分）
        let mut name_count: HashMap<String, usize> = HashMap::new();
        for c in &in_group {
            *name_count.entry(c.label.clone()).or_default() += 1;
        }
        let mut sections = Vec::with_capacity(in_group.len());
        for c in in_group {
            let label = if name_count.get(&c.label).copied().unwrap_or(0) > 1 {
                let suffix = if !c.client.is_empty() {
                    c.client.clone()
                } else if !c.username.is_empty() {
                    c.username.clone()
                } else {
                    c.id.chars().take(6).collect()
                };
                format!("{} · {}", c.label, suffix)
            } else {
                c.label.clone()
            };
            let scripts = load_scripts_for(&c.id, cx);
            sections.push(CredSection {
                cred: c,
                label,
                scripts,
            });
        }
        // 选中校验：所选脚本已不存在（删除/切分组）→ 清除选中，执行栏回置灰态
        if let Some(sel) = &self.selected {
            let still_exists = sections
                .iter()
                .any(|s| s.scripts.iter().any(|e| e.path == sel.path));
            if !still_exists {
                self.selected = None;
            }
        }
        self.sections = sections;
        cx.notify();
    }

    fn set_group(&mut self, group_id: String, cx: &mut Context<Self>) {
        if group_id == self.group_id {
            return;
        }
        self.group_id = group_id;
        self.refresh_sections(cx);
    }

    /// 刷新当前分组（外部操作后回调：重读凭据与脚本）
    fn refresh_current(&mut self, cx: &mut Context<Self>) {
        self.refresh_groups(cx);
    }

    /// 当前分组是否包含正式环境凭据
    fn has_production(&self) -> bool {
        self.sections
            .iter()
            .any(|s| s.cred.environment == "production")
    }

    /// 导入 .vbs 到指定凭据：文件选择 → 后台解析 → 存到该凭据目录；
    /// 若该凭据尚无登录自动执行脚本，则自动将本脚本设为自动执行。
    fn import_vbs(&mut self, cred_id: String, window: &mut Window, cx: &mut Context<Self>) {
        let (connection_id, group_id) = {
            let s = AppState::global(cx).store.lock();
            match s.credentials.iter().find(|c| c.id == cred_id) {
                Some(c) => (
                    c.connection_id.clone(),
                    c.group_id.clone().unwrap_or_default(),
                ),
                None => return,
            }
        };
        let group_opt = if group_id.is_empty() {
            None
        } else {
            Some(group_id)
        };
        cx.spawn_in(window, async move |this, window| {
            let picked = window
                .background_executor()
                .spawn(async move {
                    rfd::FileDialog::new()
                        .add_filter("VBScript", &["vbs"])
                        .pick_file()
                })
                .await;
            let Some(path) = picked else {
                return;
            };
            let path_str = path.to_string_lossy().to_string();
            let fname = path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            let parsed = window
                .background_executor()
                .spawn(
                    async move { sap_scripting::parse_vbs_file(std::path::Path::new(&path_str)) },
                )
                .await;
            match parsed {
                Ok(actions) if actions.is_empty() => {
                    _ = this.update_in(window, |_, window, cx| {
                        window.push_notification(t("未从文件中解析到任何脚本步骤"), cx);
                    });
                }
                Ok(actions) => {
                    let n = actions.len();
                    let default_name = path
                        .file_stem()
                        .map(|s| s.to_string_lossy().into_owned())
                        .unwrap_or_else(|| "script".to_string());
                    let script = Script {
                        name: default_name,
                        system: connection_id.clone(),
                        description: tf("从 {file} 导入", &[&fname]),
                        default_delay_ms: 3000,
                        actions,
                    };
                    // 存储位置由 cred_id 决定；script.system 仅作记录。
                    // 曾误用 system 当目录名 → 与读取（按凭据 ID）不一致，列表读不到。
                    let save = script_store::save_script(&script, group_opt.as_deref(), &cred_id);
                    let saved_path = save.as_ref().ok().map(|p| p.to_string_lossy().to_string());
                    _ = this.update_in(window, move |this, window, cx| match save {
                        Ok(_) => {
                            // 首次导入（该凭据尚无自动执行脚本）→ 自动设为登录执行
                            let mut now_auto = false;
                            let mut save_error: Option<String> = None;
                            {
                                let mut s = AppState::global(cx).store.lock();
                                let had_auto = s
                                    .credentials
                                    .iter()
                                    .any(|c| c.id == cred_id && c.script_path.is_some());
                                if !had_auto {
                                    if let Some(c) =
                                        s.credentials.iter_mut().find(|c| c.id == cred_id)
                                    {
                                        c.script_path = saved_path.clone();
                                    }
                                    if let Err(e) = storage::save_store(&s) {
                                        // v4 Should #8：回滚内存指针，失败必须让用户看见，
                                        // 不能继续推「已设为登录自动执行」
                                        if let Some(c) =
                                            s.credentials.iter_mut().find(|c| c.id == cred_id)
                                        {
                                            c.script_path = None;
                                        }
                                        // v5 Consider #27：save_script 已把文件落盘，
                                        // 回滚必须一并删除该文件，否则重试会重复导入
                                        if let Some(ref p) = saved_path {
                                            if let Err(rm_err) = std::fs::remove_file(p) {
                                                log::warn!("回滚已导入脚本文件失败：{rm_err}");
                                            }
                                        }
                                        save_error = Some(tf(
                                            "保存自动执行脚本指针失败：{e}",
                                            &[&e.to_string()],
                                        ));
                                    } else {
                                        now_auto = true;
                                    }
                                }
                            }
                            if let Some(msg) = save_error {
                                window.push_notification(msg, cx);
                                // v5 Consider #27：失败也要刷新列表，否则用户看到
                                // 旧列表会反复重试，造成重复导入 / 悬挂文件
                                this.refresh_current(cx);
                                return;
                            }
                            window.push_notification(
                                if now_auto {
                                    tf(
                                        "已导入脚本（{n} 个步骤），已设为登录自动执行",
                                        &[&n.to_string()],
                                    )
                                } else {
                                    tf("已导入脚本（{n} 个步骤）", &[&n.to_string()])
                                },
                                cx,
                            );
                            this.refresh_current(cx);
                        }
                        Err(e) => {
                            window.push_notification(tf("脚本保存失败：{e}", &[&e.to_string()]), cx)
                        }
                    });
                }
                Err(e) => {
                    _ = this.update_in(window, |_, window, cx| {
                        window.push_notification(tf("解析 .vbs 失败：{e}", &[&e.to_string()]), cx);
                    });
                }
            }
        })
        .detach();
    }

    /// 打开脚本执行日志目录（data/logs，explorer）
    fn open_logs_dir(&mut self, _ev: &ClickEvent, _window: &mut Window, _cx: &mut Context<Self>) {
        let dir = sap_backend::sap_scripting::runlog::log_dir();
        let _ = std::process::Command::new("explorer").arg(dir).spawn();
    }

    /// 运行脚本（会话感知，v1.5.7）：
    /// 先做登录信息完整性守卫（与主界面双击卡片同源），再后台执行
    /// `run_script_for_credential`——已登录且匹配 (system+client) 的会话
    /// 直接执行；未匹配则自动登录（不跑挂载脚本）后定向执行。
    fn run_script_smart(
        &mut self,
        cred_id: String,
        path: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let disabled_hint = {
            let s = AppState::global(cx).store.lock();
            s.credentials
                .iter()
                .find(|c| c.id == cred_id)
                .and_then(credential_card::login_disabled_hint)
        };
        if let Some(hint) = disabled_hint {
            window.push_notification(hint, cx);
            return;
        }
        if self.running {
            window.push_notification(t("正在执行脚本，请稍候…"), cx);
            return;
        }
        // 运行时主密码驻留 SapApp（解锁后）；未解锁场景面板不可达，兜底空串
        let mp = AppState::global(cx)
            .weak_app
            .clone()
            .and_then(|w| w.upgrade())
            .map(|app| app.read(cx).master_password())
            .unwrap_or_default();
        self.running = true;
        cx.notify();

        let store = AppState::global(cx).store.clone();
        let path_s = path.to_string_lossy().to_string();
        cx.spawn_in(window, async move |this, window| {
            let result = window
                .background_executor()
                .spawn(
                    async move { sap_cmd::run_script_for_credential(&store, &mp, cred_id, path_s) },
                )
                .await;
            _ = this.update_in(window, move |this, window, cx| {
                this.running = false;
                match result {
                    Ok(r) => window.push_notification(
                        if r.used_existing_session {
                            t("已在已登录的会话中执行脚本")
                        } else {
                            t("未检测到对应会话，已自动登录并执行脚本")
                        },
                        cx,
                    ),
                    Err(e) => {
                        window.push_notification(tf("脚本执行失败：{e}", &[&e.to_string()]), cx)
                    }
                }
                this.refresh_current(cx);
            });
        })
        .detach();
    }

    /// 删除脚本文件；若该脚本正被设为登录自动执行，则一并清除
    fn delete_script(
        &mut self,
        cred_id: String,
        entry: ScriptEntry,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let store = AppState::global(cx).store.clone();
        // v5 Should #5：先落盘清指针、再删文件。任一失败都如实报错并 return，
        // 不再「文件已删 + save 失败 + 仍推已删除」留下磁盘悬挂指针。
        let old_path: Option<String> = if entry.is_auto {
            let mut s = store.lock();
            let old = s
                .credentials
                .iter()
                .find(|c| c.id == cred_id)
                .and_then(|c| c.script_path.clone());
            if let Some(c) = s.credentials.iter_mut().find(|c| c.id == cred_id) {
                c.script_path = None;
            }
            if let Err(e) = storage::save_store(&s) {
                // 回滚内存指针，保持内存/磁盘一致（此时文件还没删，完全可恢复）
                if let Some(c) = s.credentials.iter_mut().find(|c| c.id == cred_id) {
                    c.script_path = old;
                }
                window.push_notification(
                    tf("清除自动执行脚本指针后保存失败：{e}", &[&e.to_string()]),
                    cx,
                );
                return;
            }
            old
        } else {
            None
        };

        if let Err(e) = std::fs::remove_file(&entry.path) {
            // 指针已落盘清空但文件删不掉 → 尽量把指针恢复回去
            if entry.is_auto {
                let mut s = store.lock();
                if let Some(c) = s.credentials.iter_mut().find(|c| c.id == cred_id) {
                    c.script_path = old_path;
                }
                if let Err(e2) = storage::save_store(&s) {
                    log::warn!("回滚自动执行脚本指针失败：{e2}");
                }
            }
            window.push_notification(tf("删除脚本失败：{e}", &[&e.to_string()]), cx);
            return;
        }
        window.push_notification(tf("已删除脚本 {name}", &[&entry.name]), cx);
        self.refresh_current(cx);
    }

    /// 设为 / 取消登录自动执行脚本（该凭据同一时刻只有一个）
    fn set_auto_script(
        &mut self,
        cred_id: String,
        script_path: Option<PathBuf>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let is_set = script_path.is_some();
        let mut save_error: Option<String> = None;
        {
            let mut s = AppState::global(cx).store.lock();
            let old_path = s
                .credentials
                .iter()
                .find(|c| c.id == cred_id)
                .and_then(|c| c.script_path.clone());
            if let Some(c) = s.credentials.iter_mut().find(|c| c.id == cred_id) {
                c.script_path = script_path.map(|p| p.to_string_lossy().to_string());
            }
            if let Err(e) = storage::save_store(&s) {
                // v4 Should #8：回滚内存指针并阻断成功/取消 toast
                if let Some(c) = s.credentials.iter_mut().find(|c| c.id == cred_id) {
                    c.script_path = old_path;
                }
                save_error = Some(tf("保存自动执行脚本设置失败：{e}", &[&e.to_string()]));
            }
        }
        if let Some(msg) = save_error {
            window.push_notification(msg, cx);
            return;
        }
        window.push_notification(
            if is_set {
                t("已设为登录后自动执行")
            } else {
                t("已取消登录后自动执行")
            },
            cx,
        );
        self.refresh_current(cx);
    }

    /// 分组下拉
    fn render_group_select(&self, weak: WeakEntity<Self>, _cx: &Context<Self>) -> impl IntoElement {
        let current = self
            .groups
            .iter()
            .find(|g| g.id == self.group_id)
            .map(|g| g.name.clone())
            .unwrap_or_else(|| t("选择分组").to_string());
        let groups = self.groups.clone();
        Button::new("sl-group-select")
            .secondary()
            .small()
            .icon(IconName::Tags)
            .label(current)
            .dropdown_menu(move |menu, _, _| {
                let mut menu = menu;
                for g in &groups {
                    let gid = g.id.clone();
                    let gname = g.name.clone();
                    let w = weak.clone();
                    menu = menu.item(PopupMenuItem::new(gname).on_click(move |_, _, cx| {
                        if let Some(p) = w.upgrade() {
                            p.update(cx, |p, cx| p.set_group(gid.clone(), cx));
                        }
                    }));
                }
                menu
            })
            .anchor(Anchor::TopLeft)
    }

    /// 凭据小节头：凭据名（重名带区分后缀）+ 生产胶囊 + 登录要素行 + 导入按钮
    fn render_section_head(
        &self,
        si: usize,
        section: &CredSection,
        cx: &Context<Self>,
    ) -> impl IntoElement {
        let cred = &section.cred;
        let is_prod = cred.environment == "production";
        // 导入按钮闭包要求 'static：预克隆凭据 id（借用 section 不可入闭包）
        let cred_id = section.cred.id.clone();

        // 登录要素行：client · 账号 · 语言 · 服务器（非空字段拼接，短字段不破版）
        let mut facts: Vec<String> = Vec::new();
        if !cred.client.is_empty() {
            facts.push(format!("{} {}", t("客户端"), cred.client));
        }
        if !cred.username.is_empty() {
            facts.push(format!("{} {}", t("账号"), cred.username));
        }
        if !cred.language.is_empty() {
            facts.push(format!("{} {}", t("语言"), cred.language));
        }
        let server = server_label(cred);
        if !server.is_empty() {
            facts.push(format!("{} {}", t("服务器"), server));
        }
        let facts_line = facts.join(" · ");

        h_flex()
            .gap_2()
            .flex_none()
            .items_center()
            .justify_between()
            .px(px(14.))
            .py_3()
            .child(
                v_flex()
                    .gap(px(2.))
                    .min_w_0()
                    .child(
                        h_flex()
                            .gap(px(6.))
                            .items_center()
                            .child(
                                div()
                                    .text_size(px(13.))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(cx.theme().foreground)
                                    .child(section.label.clone()),
                            )
                            .when(is_prod, |head| {
                                head.child(
                                    h_flex()
                                        .gap_1()
                                        .items_center()
                                        .px(px(6.))
                                        .h(px(18.))
                                        .rounded(px(9.))
                                        .bg(with_alpha(cx.theme().danger, 0.10))
                                        .border_1()
                                        .border_color(with_alpha(cx.theme().danger, 0.45))
                                        .child(
                                            Icon::new(IconName::TriangleAlert)
                                                .size(px(11.))
                                                .text_color(cx.theme().danger),
                                        )
                                        .child(
                                            div()
                                                .text_size(px(11.))
                                                .font_weight(FontWeight::MEDIUM)
                                                .text_color(cx.theme().danger)
                                                .child(t("生产环境")),
                                        ),
                                )
                            }),
                    )
                    .when(!facts_line.is_empty(), |head| {
                        head.child(
                            div()
                                .text_size(px(12.))
                                .text_color(cx.theme().muted_foreground)
                                .child(facts_line),
                        )
                    }),
            )
            .child(
                Button::new(format!("sl-import-{si}"))
                    .small()
                    .icon(IconName::Import)
                    .label(t("导入 .vbs"))
                    .on_click(cx.listener(move |this, _, window, cx| {
                        // 闭包要求 'static：借用 section 不可入闭包，先克隆所有权值
                        this.import_vbs(cred_id.clone(), window, cx)
                    })),
            )
    }

    /// 渲染单条脚本行（两步式：点行选中，执行统一在底部执行栏）。
    /// 归属凭据由所在小节决定；行内只保留「自动执行」「删除」两个次要操作。
    fn render_script_row(
        &self,
        si: usize,
        idx: usize,
        section: &CredSection,
        entry: &ScriptEntry,
        cx: &Context<Self>,
    ) -> impl IntoElement {
        // 预先克隆出闭包需要的所有权值：on_click 是 Fn 闭包（可多次调用），
        // 捕获值不能被 move，借用 &entry / &section 也不能进 'static 闭包。
        let cred_id_sel = section.cred.id.clone();
        let cred_label_sel = section.label.clone();
        let is_prod_sel = section.cred.environment == "production";
        let name_sel = entry.name.clone();
        let path_sel = entry.path.clone();
        let cred_id_auto = section.cred.id.clone();
        let cred_id_del = section.cred.id.clone();
        let name = entry.name.clone();
        let description = entry.description.clone();
        let action_count = entry.action_count;
        let path_del = entry.path.clone();
        let path_auto = entry.path.clone();
        let is_auto = entry.is_auto;
        let is_selected = self
            .selected
            .as_ref()
            .map(|s| s.path == entry.path)
            .unwrap_or(false);
        let meta = if description.is_empty() {
            tf("共 {n} 个步骤", &[&action_count.to_string()])
        } else {
            format!(
                "{} · {}",
                description,
                tf("共 {n} 个步骤", &[&action_count.to_string()])
            )
        };

        // 名称行：脚本名 + （自动执行标记）
        let title_row = h_flex()
            .gap(px(6.))
            .items_center()
            .child(
                div()
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(cx.theme().foreground)
                    .child(entry.name.clone()),
            )
            .when(is_auto, |row| {
                row.child(
                    div()
                        .px(px(6.))
                        .h(px(18.))
                        .rounded(px(9.))
                        .bg(with_alpha(cx.theme().accent, 0.12))
                        .text_size(px(11.))
                        .text_color(cx.theme().accent)
                        .child(t("自动执行")),
                )
            });

        // 可点击选中区（左侧信息区）；右侧按钮区独立，避免按钮点击冒泡误选
        let info_area = v_flex()
            .id(format!("sl-row-{si}-{idx}"))
            .gap(px(2.))
            .flex_1()
            .min_w_0()
            .py(px(2.))
            .child(title_row)
            .child(
                div()
                    .text_size(px(12.))
                    .text_color(cx.theme().muted_foreground)
                    .child(meta),
            )
            .when(!is_selected, |area| {
                area.hover(|s| s.bg(with_alpha(cx.theme().accent, 0.05)))
            })
            .on_click(cx.listener(move |this, _, _window, cx| {
                this.selected = Some(SelectedScript {
                    cred_id: cred_id_sel.clone(),
                    cred_label: cred_label_sel.clone(),
                    script_name: name_sel.clone(),
                    path: path_sel.clone(),
                    is_prod: is_prod_sel,
                    is_auto,
                });
                cx.notify();
            }));

        h_flex()
            .items_center()
            .gap_2()
            .px_3()
            .py_2()
            // 选中态：淡蓝底填充（桌面惯例，不用左侧色条）
            .when(is_selected, |row| {
                row.bg(with_alpha(cx.theme().accent, 0.07))
            })
            .child(info_area)
            .child(
                h_flex()
                    .gap_1()
                    .flex_none()
                    .child(
                        Button::new(format!("sl-auto-{si}-{idx}"))
                            .small()
                            .ghost()
                            .label(if is_auto {
                                t("取消自动执行")
                            } else {
                                t("设为自动执行")
                            })
                            .on_click(cx.listener(move |this, _, window, cx| {
                                let p = path_auto.clone();
                                this.set_auto_script(
                                    cred_id_auto.clone(),
                                    if is_auto { None } else { Some(p) },
                                    window,
                                    cx,
                                )
                            })),
                    )
                    .child(
                        Button::new(format!("sl-del-{si}-{idx}"))
                            .small()
                            .ghost()
                            .label(t("删除"))
                            .on_click(cx.listener(move |this, _, _window, _cx| {
                                this.pending_delete = Some((
                                    cred_id_del.clone(),
                                    name.clone(),
                                    ScriptEntry {
                                        name: name.clone(),
                                        path: path_del.clone(),
                                        description: description.clone(),
                                        action_count,
                                        is_auto,
                                    },
                                ));
                            })),
                    ),
            )
    }

    /// 渲染一个凭据小节：卡片容器（与凭据卡片同一视觉语言：popover 底 + 发丝线边框）
    /// + 小节头 + 脚本行（行间分隔线，避免卡中嵌卡）。
    fn render_section(
        &self,
        si: usize,
        section: &CredSection,
        cx: &Context<Self>,
    ) -> impl IntoElement {
        let has_scripts = !section.scripts.is_empty();
        v_flex()
            .flex_none()
            .bg(cx.theme().popover)
            .border_1()
            .border_color(cx.theme().border)
            .rounded_lg()
            .overflow_hidden()
            .child(self.render_section_head(si, section, cx))
            .when(!has_scripts, |sec| {
                sec.child(
                    div()
                        .px_4()
                        .py(px(14.))
                        .text_size(px(12.))
                        .text_color(cx.theme().muted_foreground)
                        .border_t_1()
                        .border_color(cx.theme().border)
                        .child(t("该凭据暂无脚本，点右上「导入 .vbs」添加")),
                )
            })
            .children(section.scripts.iter().enumerate().map(|(i, e)| {
                // 行间分隔线（首行与小节头之间也用分隔线）；
                // render_script_row 返回 impl IntoElement 不能直接链 Styled，用 div 包装
                div()
                    .border_t_1()
                    .border_color(cx.theme().border)
                    .child(self.render_script_row(si, i, section, e, cx))
            }))
    }

    /// 空状态（分组下无凭据时）
    fn render_empty(&self, cx: &Context<Self>) -> impl IntoElement {
        let (title, subtitle) = if self.creds.is_empty() {
            (
                t("还没有凭据"),
                t("先在主界面创建或导入凭据，再回到这里管理它的登录脚本"),
            )
        } else {
            (
                t("该分组下暂无凭据"),
                t("切换到其他分组，或在主界面把凭据归入此分组"),
            )
        };
        v_flex()
            .flex_1()
            .items_center()
            .justify_center()
            .gap(px(10.))
            .p_6()
            // 图标软色底：primary 8% 圆形衬底，比裸图标更有重心（指南：空态图标容器）
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_center()
                    .size(px(56.))
                    .rounded_full()
                    .bg(with_alpha(cx.theme().primary, 0.08))
                    .child(
                        Icon::new(IconName::Code)
                            .size(px(26.))
                            .text_color(cx.theme().primary),
                    ),
            )
            // 主文案：foreground 高一档字重，指向「缺什么」
            .child(
                div()
                    .text_size(px(14.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(cx.theme().foreground)
                    .child(title),
            )
            // 副说明：muted，讲清「下一步做什么」
            .child(
                div()
                    .max_w(px(360.))
                    .text_center()
                    .text_size(px(12.))
                    .text_color(cx.theme().muted_foreground)
                    .child(subtitle),
            )
    }

    /// 生产环境警示横幅（列表上方，分组内含任一生产凭据时展示）
    fn render_prod_banner(&self, cx: &Context<Self>) -> impl IntoElement {
        h_flex()
            .flex_none()
            .items_center()
            .gap_2()
            .px_4()
            .py_2()
            .bg(with_alpha(cx.theme().danger, 0.08))
            .border_b_1()
            .border_color(with_alpha(cx.theme().danger, 0.35))
            .child(
                Icon::new(IconName::TriangleAlert)
                    .size(px(14.))
                    .text_color(cx.theme().danger),
            )
            .child(
                div()
                    .text_size(px(12.))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(cx.theme().danger)
                    .child(t(
                        "该分组包含正式环境凭据：脚本将在生产系统执行，请务必确认。",
                    )),
            )
    }

    /// 底部固定执行栏（两步式的第二步）：显示所选脚本 + 归属凭据 + 行为说明，
    /// 大号 primary「执行脚本」主按钮；未选择时置灰提示。
    /// 生产凭据脚本选中时追加红色警示（高危操作执行前的确认缓冲）。
    fn render_execute_bar(&self, cx: &Context<Self>) -> impl IntoElement {
        let is_running = self.running;
        match &self.selected {
            None => v_flex()
                .flex_none()
                .items_center()
                .justify_between()
                .gap_2()
                .px_4()
                .py(px(10.))
                .border_t_1()
                .border_color(cx.theme().border)
                .bg(super::tokens::page_bg(cx))
                .child(
                    div()
                        .text_size(px(12.))
                        .text_color(cx.theme().muted_foreground)
                        .child(t("尚未选择脚本——点击列表中的脚本行以选中")),
                )
                .child(
                    Button::new("sl-execute")
                        .primary()
                        .icon(IconName::Play)
                        .label(t("执行脚本"))
                        .disabled(true),
                ),
            Some(sel) => {
                let prod_warning = sel.is_prod;
                // 执行按钮闭包要求 'static：预克隆所需值（&sel 借用不可入闭包）
                let ex_cred_id = sel.cred_id.clone();
                let ex_path = sel.path.clone();
                v_flex()
                    .flex_none()
                    .px_4()
                    .py(px(10.))
                    .gap_1()
                    .border_t_1()
                    .border_color(if prod_warning {
                        with_alpha(cx.theme().danger, 0.35)
                    } else {
                        cx.theme().border
                    })
                    .bg(if prod_warning {
                        with_alpha(cx.theme().danger, 0.06)
                    } else {
                        cx.theme().background
                    })
                    .child(
                        h_flex()
                            .items_center()
                            .justify_between()
                            .gap_3()
                            .child(
                                v_flex()
                                    .gap(px(2.))
                                    .min_w_0()
                                    .child(
                                        h_flex()
                                            .gap(px(6.))
                                            .items_center()
                                            .child(
                                                div()
                                                    .text_size(px(13.))
                                                    .font_weight(FontWeight::SEMIBOLD)
                                                    .text_color(cx.theme().foreground)
                                                    .child(sel.script_name.clone()),
                                            )
                                            .when(sel.is_auto, |row| {
                                                row.child(
                                                    div()
                                                        .px(px(6.))
                                                        .h(px(18.))
                                                        .rounded(px(9.))
                                                        .bg(with_alpha(cx.theme().accent, 0.12))
                                                        .text_size(px(11.))
                                                        .text_color(cx.theme().accent)
                                                        .child(t("自动执行")),
                                                )
                                            }),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(12.))
                                            .text_color(cx.theme().muted_foreground)
                                            .truncate()
                                            .child(format!(
                                                "{} · {}",
                                                sel.cred_label,
                                                t("已登录则直接执行，未登录将自动登录")
                                            )),
                                    ),
                            )
                            .child(
                                Button::new("sl-execute")
                                    .primary()
                                    .icon(IconName::Play)
                                    .label(if is_running {
                                        t("正在执行…")
                                    } else {
                                        t("执行脚本")
                                    })
                                    .disabled(is_running)
                                    .on_click(cx.listener(move |this, _, window, cx| {
                                        this.run_script_smart(
                                            ex_cred_id.clone(),
                                            ex_path.clone(),
                                            window,
                                            cx,
                                        )
                                    })),
                            ),
                    )
                    .when(prod_warning, |bar| {
                        bar.child(
                            div()
                                .text_size(px(12.))
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(cx.theme().danger)
                                .child(t("该脚本归属正式环境凭据，将在生产系统执行，请务必确认。")),
                        )
                    })
            }
        }
    }

    fn render_list(&self, cx: &Context<Self>) -> impl IntoElement {
        v_flex().flex_1().min_h_0().child(
            // 两层滚动姿势（设置页同款）：外层锚容器挂滚动条（自身不滚动），内层真正滚动。
            // overflow_y_scroll 是 StatefulInteractiveElement 方法，必须先 .id() 转 Stateful<Div>。
            v_flex()
                .flex_1()
                .min_h_0()
                .relative()
                .vertical_scrollbar(&self.scroll)
                .child(
                    v_flex()
                        .id("sl-list")
                        .flex_1()
                        .min_h_0()
                        .overflow_y_scroll()
                        .track_scroll(&self.scroll)
                        .gap(px(10.))
                        .p_3()
                        .children(
                            self.sections
                                .iter()
                                .enumerate()
                                .map(|(si, s)| self.render_section(si, s, cx)),
                        ),
                ),
        )
    }
}

/// 加载单个凭据的脚本列表（含自动执行标记）
fn load_scripts_for(cred_id: &str, cx: &Context<ScriptLibraryPanel>) -> Vec<ScriptEntry> {
    let (group_id, auto_path) = {
        let s = AppState::global(cx).store.lock();
        match s.credentials.iter().find(|c| c.id == cred_id) {
            Some(c) => (
                c.group_id.clone().unwrap_or_default(),
                c.script_path.clone().unwrap_or_default(),
            ),
            None => return Vec::new(),
        }
    };
    let auto_canon = canonical(&auto_path);
    let mut out = Vec::new();
    for path in script_store::list_scripts(
        if group_id.is_empty() {
            None
        } else {
            Some(group_id.as_str())
        },
        cred_id,
    ) {
        if let Ok(script) = script_store::load_script(&path) {
            let is_auto =
                !auto_canon.is_empty() && canonical(&path.to_string_lossy()) == auto_canon;
            out.push(ScriptEntry {
                name: script.name.clone(),
                path,
                description: script.description.clone(),
                action_count: script.actions.len(),
                is_auto,
            });
        }
    }
    out
}

/// 颜色加透明度（Hsla 无 .opacity() 颜色方法，须改 a 字段）
fn with_alpha(c: gpui_kit::Hsla, a: f32) -> gpui_kit::Hsla {
    let mut c = c;
    c.a = a;
    c
}

/// 凭据显示名：display_name > connection_id > username > id
fn cred_label(c: &sap_backend::models::Credential) -> String {
    match &c.display_name {
        Some(n) if !n.is_empty() => n.clone(),
        _ if !c.connection_id.is_empty() => c.connection_id.clone(),
        _ if !c.username.is_empty() => c.username.clone(),
        _ => c.id.clone(),
    }
}

/// 服务器短标签（小节头用，均为结构化短字段而非原始连接描述）：
/// 直连 = `SID (实例号) 应用服务器`；负载均衡 = 消息服务器；全空则空串
fn server_label(c: &CredLite) -> String {
    let mut s = String::new();
    if !c.system_id.is_empty() {
        s.push_str(&c.system_id);
    }
    if !c.system_number.is_empty() {
        if !s.is_empty() {
            s.push(' ');
        }
        s.push_str(&format!("({})", c.system_number));
    }
    if !c.app_server.is_empty() {
        if !s.is_empty() {
            s.push(' ');
        }
        s.push_str(&c.app_server);
    }
    if s.is_empty() && !c.message_server.is_empty() {
        s.push_str(&c.message_server);
    }
    s
}

/// 路径规整（比较 script_path 与脚本文件路径时忽略大小写/分隔符差异）
fn canonical(p: &str) -> String {
    p.replace('/', "\\").to_lowercase()
}

impl Render for ScriptLibraryPanel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let weak = cx.entity().downgrade();
        let is_prod = self.has_production();

        // 注意：面板根节点必须用 absolute().inset_0() 锚定。
        // 直接挂在面板宿主容器下的 Entity 视图，flex_1 拿不到高度
        // （Entity 边界高度链断裂），会导致内部滚动区塌陷、面板无法滚动。
        // 与 credential_form / group_manager / settings 等可用面板保持同一模式。
        v_flex()
            .absolute()
            .inset_0()
            .bg(super::tokens::page_bg(cx))
            .child(
                // 顶部工具条：分组下拉（分组即一级视角，脚本按凭据分小节）
                h_flex()
                    .flex_none()
                    .items_center()
                    .gap_2()
                    .px_4()
                    .h(px(48.))
                    .border_b_1()
                    .border_color(cx.theme().border)
                    .child(self.render_group_select(weak, cx))
                    .child(
                        div()
                            .text_size(px(12.))
                            .text_color(cx.theme().muted_foreground)
                            .child(t("分组内全部凭据的脚本，按凭据分节展示")),
                    )
                    .child(div().flex_1())
                    .child(
                        // 执行日志目录（data/logs/script-YYYYMMDD.log，逐动作明细）
                        Button::new("sl-open-logs")
                            .ghost()
                            .small()
                            .icon(IconName::FolderOpen)
                            .label(t("打开日志"))
                            .on_click(cx.listener(Self::open_logs_dir)),
                    ),
            )
            .when(is_prod, |panel| panel.child(self.render_prod_banner(cx)))
            // 无凭据（全局/分组）→ 空态；有凭据 → 分节列表
            .child(if self.sections.is_empty() {
                self.render_empty(cx).into_any_element()
            } else {
                self.render_list(cx).into_any_element()
            })
            // 两步式第二步：底部固定执行栏（选择脚本 → 执行脚本）
            .when(!self.sections.is_empty(), |panel| {
                panel.child(self.render_execute_bar(cx))
            })
            // 删除脚本确认弹窗（不可逆操作，必须二次确认）
            .when_some(self.pending_delete.clone(), |panel, pending| {
                let (ref cred_id, ref name, ref entry) = pending;
                let cred_id_del = cred_id.clone();
                let entry_del = entry.clone();
                panel.child(
                    div()
                        .absolute()
                        .inset_0()
                        .flex()
                        .items_center()
                        .justify_center()
                        .bg(gpui::black().opacity(0.35))
                        .child(
                            v_flex()
                                .w(px(400.))
                                .rounded(px(8.))
                                .bg(cx.theme().background)
                                .border_1()
                                .border_color(cx.theme().border)
                                .shadow_lg()
                                .overflow_hidden()
                                .child(
                                    h_flex()
                                        .gap_3()
                                        .p_4()
                                        .items_center()
                                        .child(
                                            Icon::new(IconName::TriangleAlert)
                                                .size_5()
                                                .text_color(cx.theme().danger),
                                        )
                                        .child(
                                            v_flex()
                                                .gap_1()
                                                .child(
                                                    div()
                                                        .text_base()
                                                        .font_weight(FontWeight::SEMIBOLD)
                                                        .text_color(cx.theme().foreground)
                                                        .child(t("删除脚本")),
                                                )
                                                .child(
                                                    div()
                                                        .text_sm()
                                                        .text_color(
                                                            cx.theme().muted_foreground,
                                                        )
                                                        .child(tf(
                                                            "确定要永久删除「{name}」吗？此操作无法撤销。",
                                                            &[name.as_str()],
                                                        )),
                                                ),
                                        ),
                                )
                                .child(div().h(px(1.)).bg(cx.theme().border))
                                .child(
                                    h_flex()
                                        .gap_2()
                                        .p_4()
                                        .justify_end()
                                        .child(
                                            Button::new("sl-del-cancel")
                                                .small()
                                                .ghost()
                                                .label(t("取消"))
                                                .on_click(cx.listener(|this, _, _w, _cx| {
                                                    this.pending_delete = None;
                                                })),
                                        )
                                        .child(
                                            Button::new("sl-del-confirm")
                                                .small()
                                                .danger() // #4 同步修复：danger 语义
                                                .label(t("删除"))
                                                .on_click(
                                                    cx.listener(move |this, _, window, cx| {
                                                        this.delete_script(
                                                            cred_id_del.clone(),
                                                            entry_del.clone(),
                                                            window,
                                                            cx,
                                                        );
                                                        this.pending_delete = None;
                                                    }),
                                                ),
                                        ),
                                ),
                        ),
                )
            })
    }
}
