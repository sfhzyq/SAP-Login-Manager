//! 解析 SAP GUI 录制器生成的 .vbs，转换为原生 [`ScriptAction`] 序列。
//!
//! SAP 录制器输出的核心就是 `session.findById("...").text/press/sendVKey/select`
//! 这类语句，本解析器只提取这些有效行，忽略初始化样板（GetObject / Children /
//! ConnectObject / If-End If 等）。

use super::model::ScriptAction;

/// 解析整段 .vbs 文本，返回动作序列（兼容入口，丢弃跳过记录）。
pub fn parse_vbs(content: &str) -> Vec<ScriptAction> {
    parse_vbs_with_report(content).0
}

/// v5 Should #14：解析 .vbs 并**累积**「看似有效但无法识别」的行号与原因。
///
/// 旧实现返回裸 `Vec<ScriptAction>`，不支持的属性赋值、格式错误的
/// `sendVKey`、缺引号的 `StartTransaction` 全部静默丢弃；一个含 10 条语句
/// 但 3 条不支持的脚本会解析成 7 条动作并报「执行成功」，在 SAP 事务里
/// 产生半完成状态。现在把跳过记录回传，由 `parse_vbs_file` 决定是否中止。
pub fn parse_vbs_with_report(content: &str) -> (Vec<ScriptAction>, Vec<String>) {
    let mut actions = Vec::new();
    let mut skipped = Vec::new();
    for (i, raw_line) in content.lines().enumerate() {
        let line_no = i + 1;
        let line = raw_line.trim();
        if line.is_empty() || is_boilerplate(line) {
            continue;
        }
        let mut reason: Option<String> = None;

        // session.findById("...") 形式
        if let Some((id, base)) = find_id(line) {
            let rest = &line[base..];
            if let Some(eq) = rest.find('=') {
                let lhs = rest[..eq].trim();
                // 只剥包裹引号（首尾各一）：trim_matches('"') 会把值内结尾的
                // 转义引号一起剥掉（值 `他说 "hi"` 写作 `"他说 ""hi"""`，
                // 尾部三个引号被全剥 → 丢字）。配对剥离对合法 VBS 安全，
                // 无引号包裹的裸值原样保留
                let raw = rest[eq + 1..].trim();
                let rhs = raw
                    .strip_prefix('"')
                    .and_then(|s| s.strip_suffix('"'))
                    .unwrap_or(raw);
                let rhs = unescape_vbs(rhs);
                if lhs.contains("text") {
                    if id.ends_with("okcd") && rhs.starts_with('/') {
                        actions.push(ScriptAction::Command { okcode: rhs });
                    } else {
                        actions.push(ScriptAction::SetText { id, value: rhs });
                    }
                } else if lhs.contains("key") {
                    actions.push(ScriptAction::SelectCombo { id, key: rhs });
                } else if lhs.contains("setFocus") {
                    // 无副作用样板，按设计忽略
                } else {
                    reason = Some(format!(
                        "第 {line_no} 行：不支持的 findById 属性赋值「{lhs}」"
                    ));
                }
            } else if rest.contains("press") {
                actions.push(ScriptAction::PressButton { id });
            } else if rest.contains("select") {
                actions.push(ScriptAction::SelectTab { id });
            } else if let Some(pos) = rest.find("sendVKey") {
                let num_part = &rest[pos + "sendVKey".len()..];
                // 解析不出数字时必须记录跳过原因——默认发 Enter（VKey 0）
                // 可能在 SAP 事务里触发保存/过账等真实副作用
                match num_part.trim().parse::<i32>() {
                    Ok(n) => actions.push(ScriptAction::SendVKey { n }),
                    Err(_) => {
                        reason = Some(format!(
                            "第 {line_no} 行：sendVKey 参数不是数字，已跳过（不会默认发 Enter）"
                        ))
                    }
                }
            } else if rest.contains("setFocus") || rest.contains("maximize") {
                // 无副作用样板
            } else {
                reason = Some(format!("第 {line_no} 行：未识别的 findById 语句"));
            }
        } else if line.contains("StartTransaction") {
            // session.StartTransaction "SE16N"
            if let Some(q) = line.find('"') {
                // 只剥尾部包裹引号（配对剥离理由同上）
                let raw = line[q + 1..].trim_end();
                let code = raw.strip_suffix('"').unwrap_or(raw).replace("\"\"", "\"");
                if !code.is_empty() {
                    actions.push(ScriptAction::GotoTransaction { code });
                } else {
                    reason = Some(format!("第 {line_no} 行：StartTransaction 事务码为空"));
                }
            } else {
                reason = Some(format!(
                    "第 {line_no} 行：StartTransaction 缺少带引号的事务码"
                ));
            }
        } else {
            reason = Some(format!("第 {line_no} 行：未识别的有效语句「{line}」"));
        }

        if let Some(r) = reason {
            skipped.push(r);
        }
    }
    (actions, skipped)
}

/// 录制器样板行（注释 / 初始化 / 无副作用窗口操作），按设计忽略且不计入跳过记录。
fn is_boilerplate(line: &str) -> bool {
    line.starts_with("If ")
        || line.starts_with("End If")
        || line.starts_with("Set ")
        || line.starts_with("WScript")
        || line.starts_with('\'')
        || line.starts_with("rem ")
        || line.contains("GetObject")
        || line.contains("GetScriptingEngine")
        || line.contains("ConnectObject")
        || line.contains(".maximize")
}

/// 从 `session.findById("...")` 中提取引号内的控件 id 及其后文本的起始偏移。
/// 返回 `(id, offset)`：调用方用 `&line[offset..]` 取 `findById(...)` 之后的
/// 剩余部分，无需再自行 find + unwrap（消除两处函数间的隐式不变量耦合）。
fn find_id(line: &str) -> Option<(String, usize)> {
    let marker = "findById(\"";
    let start = line.find(marker)? + marker.len();
    let end = line[start..].find('"')? + start;
    Some((line[start..end].to_string(), end + 1))
}

/// 还原 VBScript 里的双引号转义（"" -> "）。
fn unescape_vbs(s: &str) -> String {
    s.replace("\"\"", "\"")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_typical_recording() {
        let vbs = r#"
If Not IsObject(application) Then
Set SapGuiAuto = GetObject("SAPGUI")
Set application = SapGuiAuto.GetScriptingEngine
End If
session.findById("wnd[0]/tbar[0]/okcd").text = "/nSE16N"
session.findById("wnd[0]").sendVKey 0
session.findById("wnd[0]/usr/ctxtSPFLI-CARRID").text = "LH"
session.findById("wnd[0]/usr/btnGET").press
session.findById("wnd[0]").sendVKey 8
"#;
        let actions = parse_vbs(vbs);
        assert_eq!(actions.len(), 5);
        assert!(matches!(
            &actions[0],
            ScriptAction::Command { okcode } if okcode == "/nSE16N"
        ));
        assert!(matches!(actions[1], ScriptAction::SendVKey { n: 0 }));
        assert!(matches!(
            &actions[2],
            ScriptAction::SetText { id, .. } if id == "wnd[0]/usr/ctxtSPFLI-CARRID"
        ));
        assert!(matches!(actions[3], ScriptAction::PressButton { .. }));
        assert!(matches!(actions[4], ScriptAction::SendVKey { n: 8 }));
    }

    #[test]
    fn skips_unparseable_sendvkey_instead_of_silent_enter() {
        // 手编脚本缺数字：解析失败必须跳过该行——静默回退 VKey 0（Enter）
        // 会在 SAP 事务里触发保存/过账等真实副作用
        let actions = parse_vbs(r#"session.findById("wnd[0]").sendVKey abc"#);
        assert!(actions.is_empty());
    }

    /// v5 Should #14：有效语句被跳过时必须留下行号与原因，不能静默丢弃。
    #[test]
    fn parse_vbs_with_report_records_skipped_statements() {
        let vbs = r#"session.findById("wnd[0]/usr/txtA").text = "OK"
session.findById("wnd[0]").sendVKey abc
session.StartTransaction SE16N
session.findById("wnd[0]/usr/txtB").foo = 1
"#;
        let (actions, skipped) = parse_vbs_with_report(vbs);
        assert_eq!(actions.len(), 1, "只应解析出 1 条有效动作");
        assert_eq!(
            skipped.len(),
            3,
            "3 条有效但未识别语句必须被记录: {skipped:?}"
        );
        assert!(skipped[0].contains("第 2 行") && skipped[0].contains("sendVKey"));
        assert!(skipped[1].contains("第 3 行") && skipped[1].contains("StartTransaction"));
        assert!(skipped[2].contains("第 4 行"));
    }

    /// 样板行按设计忽略，不应污染跳过记录（否则正常录制脚本会被误判残缺）。
    #[test]
    fn boilerplate_lines_are_not_reported_as_skipped() {
        let vbs = r#"
If Not IsObject(application) Then
Set SapGuiAuto = GetObject("SAPGUI")
Windows(CStr(WParam)).maximize
session.findById("wnd[0]").sendVKey 0
"#;
        let (actions, skipped) = parse_vbs_with_report(vbs);
        assert_eq!(actions.len(), 1);
        assert!(skipped.is_empty(), "样板行不应报跳过: {skipped:?}");
    }

    #[test]
    fn unescapes_doubled_quotes_in_text_values() {
        let actions = parse_vbs(r##"session.findById("wnd[0]/usr/txtDESC").text = "他说 ""hi"""##);
        assert_eq!(actions.len(), 1);
        match &actions[0] {
            ScriptAction::SetText { value, .. } => assert_eq!(value, "他说 \"hi\""),
            other => panic!("unexpected action: {other:?}"),
        }
    }

    #[test]
    fn skips_comments_and_boilerplate_lines() {
        let vbs = r#"
' 单引号注释
rem rem 注释
Windows(CStr(WParam)).maximize
"#;
        assert!(parse_vbs(vbs).is_empty());
    }

    #[test]
    fn parses_combo_select_and_transaction() {
        let vbs = r##"
session.findById("wnd[0]/usr/cmbLIST").key = "A"
session.findById("wnd[0]/usr/tabsTAB").select
session.StartTransaction "SE16N"
"##;
        let actions = parse_vbs(vbs);
        assert_eq!(actions.len(), 3);
        assert!(matches!(&actions[0], ScriptAction::SelectCombo { key, .. } if key == "A"));
        assert!(matches!(actions[1], ScriptAction::SelectTab { .. }));
        assert!(matches!(
            &actions[2],
            ScriptAction::GotoTransaction { code } if code == "SE16N"
        ));
    }

    #[test]
    fn okcd_without_slash_falls_back_to_set_text() {
        // Command 语义仅限 okcode 以 '/' 开头（事务码跳转）；其余按普通文本赋值
        let actions = parse_vbs(r#"session.findById("wnd[0]/tbar[0]/okcd").text = "SE16N""#);
        assert_eq!(actions.len(), 1);
        assert!(matches!(
            &actions[0],
            ScriptAction::SetText { id, value } if id.ends_with("okcd") && value == "SE16N"
        ));
    }
}
