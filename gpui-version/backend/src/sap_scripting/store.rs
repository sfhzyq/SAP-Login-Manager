//! 脚本存储：按「分组 / 凭据」两级目录归档，JSON 格式序列化 [`Script`]。
//!
//! 目录结构：`<exe_dir>/data/scripts/<group_dir>/<cred_id>/<name>.json`
//! - `group_dir`：凭据所属分组 ID（`None`/空 → `_none` 虚拟桶）；分组重命名不影响目录
//! - `cred_id`：凭据 ID（全局唯一）；凭据换分组时由 [`sync_credential_dirs`] 自愈迁移
//!
//! 旧版单层结构 `scripts/<connection_id>/*.json` 不再读取、不做迁移（功能上线次日即改版，无存量数据）。
//!
//! 历史坑：保存曾误用 `script.system`（connection_id）当目录名，落盘到
//! `<group_dir>/<connection_id>/`，而列表按凭据 ID 读取 → 导入的脚本永远不可见。
//! 这类「孤儿目录」由 [`sync_credential_dirs`] 按线索自动归属回正确凭据。

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use serde_json;

use super::model::Script;
use crate::storage;

/// 脚本存储错误（库公共 API：区分 IO 失败与 JSON 失败，调用方可编程处理）
#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("IO 错误: {0}")]
    Io(#[from] std::io::Error),
    #[error("脚本 JSON 无效: {0}")]
    Json(#[from] serde_json::Error),
    #[error("脚本文件名冲突：{path} 已存在（现有「{existing}」，待保存「{incoming}」），已中止以免静默覆盖")]
    NameCollision {
        path: String,
        existing: String,
        incoming: String,
    },
}

/// 未分组凭据的虚拟桶目录名（分组 ID 为空时使用）
const NO_GROUP_DIR: &str = "_none";

/// 自愈迁移所需的凭据最小快照：目录归属判定同时需要
/// 凭据 ID / 分组 / connection_id（孤儿目录线索）/ script_path（最强线索）。
pub struct CredDirInfo {
    pub id: String,
    pub group_id: Option<String>,
    pub connection_id: String,
    pub script_path: Option<String>,
}

/// 脚本存储根目录：<exe_dir>/data/scripts
pub fn scripts_root() -> PathBuf {
    storage::scripts_root()
}

/// 某凭据的脚本目录：`scripts/<group_dir>/<cred_id>/`（自动创建）。
pub fn script_dir_for(group_id: Option<&str>, cred_id: &str) -> PathBuf {
    script_dir_for_at(&scripts_root(), group_id, cred_id)
}

fn script_dir_for_at(root: &Path, group_id: Option<&str>, cred_id: &str) -> PathBuf {
    let dir = root
        .join(expected_group_dir(group_id))
        .join(sanitize(cred_id));
    let _ = fs::create_dir_all(&dir);
    dir
}

/// 分组 ID → 组目录名（`None`/空 → `_none` 虚拟桶）
fn expected_group_dir(group_id: Option<&str>) -> String {
    match group_id {
        Some(g) if !g.is_empty() => sanitize(g),
        _ => NO_GROUP_DIR.to_string(),
    }
}

/// 保存脚本到 `<分组目录>/<凭据目录>/<name>.json`，返回文件路径。
///
/// 存储位置由 `cred_id` 决定（凭据 ID 全局唯一）；`script.system` 仅作记录
/// （写入 connection_id），不参与存储路径——曾误用其当目录名，导致保存位置
/// （按 connection_id）与读取位置（按凭据 ID）不一致、导入的脚本列表读不到。
pub fn save_script(
    script: &Script,
    group_id: Option<&str>,
    cred_id: &str,
) -> Result<PathBuf, StoreError> {
    save_script_at(&scripts_root(), script, group_id, cred_id)
}

fn save_script_at(
    root: &Path,
    script: &Script,
    group_id: Option<&str>,
    cred_id: &str,
) -> Result<PathBuf, StoreError> {
    let dir = script_dir_for_at(root, group_id, cred_id);
    let fname = format!("{}.json", sanitize(&script.name));
    let path = dir.join(fname);
    // v5 Should #6：sanitize 可能把不同脚本名映射到同一文件名（保留设备名、
    // 纯点名、控制字符）。目标已存在且不是「同名脚本的更新」时中止，绝不覆盖。
    if path.exists() {
        let existing_name = load_script(&path)
            .map(|s| s.name)
            .unwrap_or_else(|_| "（已存在但无法解析）".to_string());
        if existing_name != script.name {
            return Err(StoreError::NameCollision {
                path: path.display().to_string(),
                existing: existing_name,
                incoming: script.name.clone(),
            });
        }
    }
    let json = serde_json::to_string_pretty(script)?;
    fs::write(&path, json)?;
    Ok(path)
}

/// 从 JSON 文件加载脚本。
pub fn load_script(path: &Path) -> Result<Script, StoreError> {
    let s = fs::read_to_string(path)?;
    Ok(serde_json::from_str(&s)?)
}

/// 列出某凭据下所有脚本文件。
pub fn list_scripts(group_id: Option<&str>, cred_id: &str) -> Vec<PathBuf> {
    list_scripts_at(&scripts_root(), group_id, cred_id)
}

fn list_scripts_at(root: &Path, group_id: Option<&str>, cred_id: &str) -> Vec<PathBuf> {
    let dir = script_dir_for_at(root, group_id, cred_id);
    let mut out = Vec::new();
    if let Ok(entries) = fs::read_dir(&dir) {
        for e in entries.flatten() {
            let p = e.path();
            if p.extension().and_then(|x| x.to_str()) == Some("json") {
                out.push(p);
            }
        }
    }
    out.sort();
    out
}

/// 凭据目录自愈迁移 + 孤儿目录归属（生产入口，根目录取 [`scripts_root`]）。
pub fn sync_credential_dirs(entries: &[CredDirInfo]) {
    sync_credential_dirs_at(&scripts_root(), entries);
}

/// 根目录可注入的内部实现（单测隔离用）。
///
/// 规则：
/// - 凭据 ID 全局唯一，`scripts/<任意组>/<cred_id>` 唯一归属；
/// - 实际目录 ≠ 期望目录且期望位置不存在时，整目录 rename 过去；
/// - 期望位置已存在（异常冲突）则跳过，不动用户数据；
/// - 目录名不匹配任何凭据 ID 的「孤儿目录」（历史 bug 按 connection_id 落盘）
///   → 按线索归属迁移（见 [`migrate_stray_dir_at`]），归属不了则原样保留。
fn sync_credential_dirs_at(root: &Path, entries: &[CredDirInfo]) {
    let Ok(group_entries) = fs::read_dir(root) else {
        return;
    };
    // 已知凭据目录名（sanitize 后）→ 期望组目录名
    let known: HashMap<String, String> = entries
        .iter()
        .map(|e| (sanitize(&e.id), expected_group_dir(e.group_id.as_deref())))
        .collect();
    // 收集：<实际组目录名>/<子目录名> -> 完整路径
    let mut found: Vec<(String, String, PathBuf)> = Vec::new();
    for g in group_entries.flatten() {
        let gpath = g.path();
        if !gpath.is_dir() {
            continue; // 旧版单层 .json 文件，跳过
        }
        let gname = g.file_name().to_string_lossy().into_owned();
        if let Ok(creds) = fs::read_dir(&gpath) {
            for c in creds.flatten() {
                if c.path().is_dir() {
                    found.push((
                        gname.clone(),
                        c.file_name().to_string_lossy().into_owned(),
                        c.path(),
                    ));
                }
            }
        }
    }
    for (gname, dirname, path) in &found {
        if let Some(expect_group) = known.get(dirname) {
            // 凭据自己的目录：不在期望组则整体搬过去
            if gname == expect_group {
                continue; // 已在期望位置
            }
            let dest = root.join(expect_group).join(dirname);
            if dest.exists() {
                continue; // 冲突跳过，不覆盖
            }
            let _ = fs::create_dir_all(root.join(expect_group));
            let _ = fs::rename(path, &dest);
            remove_dir_if_empty(&root.join(gname));
        } else {
            // 目录名不匹配任何凭据 ID：历史 bug（按 connection_id 落盘）孤儿目录
            migrate_stray_dir_at(root, gname, path, entries);
        }
    }
}

/// 孤儿目录归属迁移：`root/<gname>/<dirpath>`（目录名不匹配任何凭据 ID）。
///
/// 归属线索按可靠度排序，找不到唯一归属则原样保留，绝不破坏用户数据：
/// 1. 某凭据 `script_path` 精确指向该目录（首次导入设为自动执行留下的精确证据）；
/// 2. 目录内脚本 `system`（= connection_id）在「同分组」凭据中唯一命中。
///
/// 目标目录不存在 → 整目录搬迁；已存在 → 逐文件搬迁、重名跳过（绝不覆盖）。
fn migrate_stray_dir_at(root: &Path, gname: &str, dirpath: &Path, entries: &[CredDirInfo]) {
    let owner = stray_owner_by_script_path(entries, dirpath)
        .or_else(|| stray_owner_by_connection(entries, gname, dirpath));
    let Some(owner) = owner else {
        return; // 归属不了：原样保留
    };
    let dest_group = root.join(expected_group_dir(owner.group_id.as_deref()));
    let _ = fs::create_dir_all(&dest_group);
    let dest = dest_group.join(sanitize(&owner.id));
    if same_dir(&dest, dirpath) {
        return;
    }
    if !dest.exists() {
        // 目标不存在：整目录搬迁
        if fs::rename(dirpath, &dest).is_ok() {
            remove_dir_if_empty(&root.join(gname));
        }
        return;
    }
    // 目标已存在：逐文件搬迁，重名跳过（绝不覆盖）
    if let Ok(files) = fs::read_dir(dirpath) {
        for f in files.flatten() {
            let fp = f.path();
            if fp.extension().and_then(|x| x.to_str()) != Some("json") {
                continue;
            }
            let target = dest.join(f.file_name());
            if !target.exists() {
                let _ = fs::rename(&fp, &target);
            }
        }
    }
    remove_dir_if_empty(dirpath);
    remove_dir_if_empty(&root.join(gname));
}

/// 线索 1：script_path 的父目录精确等于孤儿目录的凭据（必须唯一命中）。
fn stray_owner_by_script_path<'a>(
    entries: &'a [CredDirInfo],
    dirpath: &Path,
) -> Option<&'a CredDirInfo> {
    let mut hit: Option<&CredDirInfo> = None;
    for e in entries {
        let pointed = e
            .script_path
            .as_deref()
            .and_then(|p| Path::new(p).parent())
            .map(|p| same_dir(p, dirpath))
            .unwrap_or(false);
        if pointed {
            if hit.is_some() {
                return None; // 多个凭据指向同一目录 → 证据失效
            }
            hit = Some(e);
        }
    }
    hit
}

/// 线索 2：目录内脚本的 system（= connection_id）在「同分组」凭据中唯一命中。
/// （同分组同 connection_id 的重复凭据无法区分 → 放弃，留在原地。）
fn stray_owner_by_connection<'a>(
    entries: &'a [CredDirInfo],
    gname: &str,
    dirpath: &Path,
) -> Option<&'a CredDirInfo> {
    let systems = dir_systems(dirpath);
    if systems.len() != 1 {
        return None; // 空目录或混多个 system → 证据不确定
    }
    let sys = systems.iter().next()?;
    let matches: Vec<&CredDirInfo> = entries
        .iter()
        .filter(|e| expected_group_dir(e.group_id.as_deref()) == gname && &e.connection_id == sys)
        .collect();
    if matches.len() == 1 {
        Some(matches[0])
    } else {
        None
    }
}

/// 两路径是否指向同一目录（canonicalize 归一化；失败退回字面比较）
fn same_dir(a: &Path, b: &Path) -> bool {
    match (fs::canonicalize(a), fs::canonicalize(b)) {
        (Ok(x), Ok(y)) => x == y,
        _ => a == b,
    }
}

/// 读取目录内全部 .json 的 `system` 字段，去重集合（孤儿归属线索）
fn dir_systems(dir: &Path) -> HashSet<String> {
    let mut out = HashSet::new();
    if let Ok(files) = fs::read_dir(dir) {
        for f in files.flatten() {
            let p = f.path();
            if p.extension().and_then(|x| x.to_str()) != Some("json") {
                continue;
            }
            if let Ok(txt) = fs::read_to_string(&p) {
                if let Ok(v) = serde_json::from_str::<serde_json::Value>(&txt) {
                    if let Some(s) = v.get("system").and_then(|x| x.as_str()) {
                        out.insert(s.to_string());
                    }
                }
            }
        }
    }
    out
}

/// 目录为空则删除（顺手清理，失败忽略）
fn remove_dir_if_empty(dir: &Path) {
    if let Ok(mut it) = fs::read_dir(dir) {
        if it.next().is_none() {
            let _ = fs::remove_dir(dir);
        }
    }
}

/// FNV-1a 64 位：给「不安全的原名」生成稳定且互不相同的安全文件名。
fn fnv1a(s: &str) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in s.as_bytes() {
        h ^= u64::from(*b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

/// 文件名非法字符替换为下划线；结果为空、纯点目录名（"." / ".."）或 Windows
/// 保留设备名时，改用「原名哈希」生成安全名——防止 `root.join("..")` 逃出目录，
/// 也避免 v5 Should #6 的等长下划线碰撞（CON/AUX/NUL 曾全部变成 `___`）。
fn sanitize(s: &str) -> String {
    let out: String = s
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || matches!(c, '-' | '_' | '.') {
                c
            } else {
                '_'
            }
        })
        .collect();
    let reserved = crate::bat_parser::WINDOWS_RESERVED_NAMES.iter().any(|r| {
        out.split('.')
            .next()
            .unwrap_or(&out)
            .eq_ignore_ascii_case(r)
    });
    if out.is_empty() || out == "." || out == ".." || reserved {
        // 哈希取自**原名**，不同原名不会落到同一个文件名；前缀 `_` 也保证
        // 与合法原名（如 `_`、`___`）区分开。
        format!("_{:016x}", fnv1a(s))
    } else {
        out
    }
}

#[cfg(test)]
mod sanitize_tests {
    use super::sanitize;

    #[test]
    fn replaces_illegal_chars_with_underscore() {
        assert_eq!(sanitize("a b/c\\d:e"), "a_b_c_d_e");
    }

    #[test]
    fn blocks_pure_dot_names_that_escape_the_root() {
        // "." / ".." 参与 root.join(group) 会逃出目标目录，必须整体替换
        assert_ne!(sanitize("."), ".");
        assert_ne!(sanitize(".."), "..");
        assert_ne!(sanitize(""), "");
        assert_ne!(sanitize("."), sanitize(".."));
        assert_ne!(sanitize(""), sanitize("."));
        // 结果必须仍是安全字符，且不含路径分隔符 / 纯点
        for s in [".", "..", ""] {
            let safe = sanitize(s);
            assert!(safe.starts_with('_'));
            assert!(safe.chars().all(|c| c.is_ascii_hexdigit() || c == '_'));
        }
    }

    #[test]
    fn keeps_safe_names_unchanged() {
        assert_eq!(sanitize("SE16N-run_1.vbs"), "SE16N-run_1.vbs");
        // is_alphanumeric 覆盖中文等 Unicode 字母
        assert_eq!(sanitize("报表导出"), "报表导出");
    }

    /// 回归 v4 Consider #20 + v5 Should #6：保留设备名必须被替换，
    /// 且任意两个不同原名 sanitize 后不得相等（等长下划线曾让 CON/AUX/NUL
    /// 全部变成 `___`，同一凭据下两个脚本静默互相覆盖）。
    #[test]
    fn blocks_windows_reserved_device_names_without_collisions() {
        let reserved = [
            "CON", "AUX", "NUL", "COM1", "COM2", "LPT1", "LPT2", "PRN", "CON.vbs", "NUL.txt",
        ];
        for a in &reserved {
            let sa = sanitize(a);
            assert_ne!(&sa, a, "保留名 {a} 必须被替换");
            assert!(sa.starts_with('_'), "替换结果必须安全: {sa}");
        }
        // 与合法原名一起做两两不碰撞断言（含历史会撞车的 `___` / `____`）
        let mut names = reserved.to_vec();
        names.extend(["___", "____", "_", "__", "正常脚本", "SE16N"]);
        for (i, a) in names.iter().enumerate() {
            let sa = sanitize(a);
            for b in &names[i + 1..] {
                assert_ne!(
                    sa,
                    sanitize(b),
                    "原名 {a} 与 {b} sanitize 后碰撞，会静默互相覆盖"
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    static SEQ: AtomicU32 = AtomicU32::new(0);

    /// 唯一临时根目录（并行测试互不干扰）
    fn temp_root(tag: &str) -> PathBuf {
        let n = SEQ.fetch_add(1, Ordering::SeqCst);
        let dir =
            std::env::temp_dir().join(format!("slm-store-{}-{}-{}", tag, std::process::id(), n));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn cred(id: &str, group: Option<&str>, conn: &str, script_path: Option<&str>) -> CredDirInfo {
        CredDirInfo {
            id: id.to_string(),
            group_id: group.map(|g| g.to_string()),
            connection_id: conn.to_string(),
            script_path: script_path.map(|p| p.to_string()),
        }
    }

    fn write_json(dir: &Path, name: &str, system: &str) -> PathBuf {
        fs::create_dir_all(dir).unwrap();
        let p = dir.join(format!("{}.json", name));
        fs::write(
            &p,
            format!(
                r#"{{"name":"{}","system":"{}","description":"","default_delay_ms":100,"actions":[]}}"#,
                name, system
            ),
        )
        .unwrap();
        p
    }

    #[test]
    fn save_script_uses_cred_id_dir() {
        let root = temp_root("save");
        let script = Script {
            name: "smoke".into(),
            system: "RS".into(),
            description: String::new(),
            default_delay_ms: 0,
            actions: vec![],
        };
        let p = save_script_at(&root, &script, Some("g1"), "cred-1").unwrap();
        assert!(p.starts_with(root.join("g1").join("cred-1")));
        assert_eq!(list_scripts_at(&root, Some("g1"), "cred-1"), vec![p]);
        // 未分组 → _none 虚拟桶
        let p2 = save_script_at(&root, &script, None, "cred-2").unwrap();
        assert!(p2.starts_with(root.join("_none").join("cred-2")));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn stray_dir_migrates_by_script_path() {
        let root = temp_root("stray-sp");
        let stray = root.join("g1").join("RS");
        let f = write_json(&stray, "order", "RS");
        let entries = vec![cred("cred-1", Some("g1"), "RS", Some(&f.to_string_lossy()))];
        sync_credential_dirs_at(&root, &entries);
        assert!(root.join("g1").join("cred-1").join("order.json").is_file());
        assert!(!stray.exists());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn stray_dir_migrates_by_unique_connection_id() {
        let root = temp_root("stray-conn");
        let stray = root.join("g1").join("RS");
        write_json(&stray, "a", "RS");
        write_json(&stray, "b", "RS");
        let entries = vec![cred("cred-9", Some("g1"), "RS", None)];
        sync_credential_dirs_at(&root, &entries);
        let dest = root.join("g1").join("cred-9");
        assert!(dest.join("a.json").is_file());
        assert!(dest.join("b.json").is_file());
        assert!(!stray.exists());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn ambiguous_stray_dir_is_untouched() {
        let root = temp_root("stray-amb");
        let stray = root.join("g1").join("RS");
        write_json(&stray, "a", "RS");
        // 同分组两条同 connection_id 凭据，无 script_path 证据 → 归属不了，保留原地
        let entries = vec![
            cred("c1", Some("g1"), "RS", None),
            cred("c2", Some("g1"), "RS", None),
        ];
        sync_credential_dirs_at(&root, &entries);
        assert!(stray.join("a.json").is_file());
        assert!(!root.join("g1").join("c1").exists());
        assert!(!root.join("g1").join("c2").exists());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn stray_dir_merges_into_existing_dest_without_overwrite() {
        let root = temp_root("stray-merge");
        let stray = root.join("g1").join("RS");
        let f = write_json(&stray, "imported", "RS");
        // 目标凭据目录已存在：含同名文件 + 一个不同名文件
        let dest = root.join("g1").join("cred-1");
        write_json(&dest, "imported", "RS");
        write_json(&dest, "native", "RS");
        let entries = vec![cred("cred-1", Some("g1"), "RS", Some(&f.to_string_lossy()))];
        sync_credential_dirs_at(&root, &entries);
        assert!(dest.join("native.json").is_file());
        // 重名冲突不覆盖：源文件留在原地
        assert!(stray.join("imported.json").is_file());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn cred_dir_moves_to_expected_group() {
        let root = temp_root("cred-move");
        let wrong = root.join("g-old").join("cred-1");
        write_json(&wrong, "s", "RS");
        let entries = vec![cred("cred-1", Some("g-new"), "RS", None)];
        sync_credential_dirs_at(&root, &entries);
        assert!(root.join("g-new").join("cred-1").join("s.json").is_file());
        assert!(!wrong.exists());
        let _ = fs::remove_dir_all(&root);
    }

    /// v5 Should #6：目标路径已存在且不是同名脚本更新时，必须报错而不是
    /// 静默覆盖（sanitize 碰撞的最后一道防线）。
    #[test]
    fn save_script_at_rejects_collision_with_different_script() {
        let root = temp_root("collision");
        let dir = root.join("g1").join("cred-1");
        let existing = write_json(&dir, "same-target", "RS");
        let script = Script {
            name: "same-target".into(),
            system: "RS".into(),
            description: String::new(),
            default_delay_ms: 0,
            actions: vec![],
        };
        // write_json 写的 name 恰是 "same-target"，属于同名更新 → 允许
        assert!(save_script_at(&root, &script, Some("g1"), "cred-1").is_ok());

        // 手工把文件内容改成另一个脚本名，再保存 → 必须中止
        let other = Script {
            name: "另一个脚本".into(),
            ..script.clone()
        };
        let other_json = serde_json::to_string_pretty(&other).unwrap();
        fs::write(&existing, &other_json).unwrap();
        let res = save_script_at(&root, &script, Some("g1"), "cred-1");
        assert!(
            matches!(res, Err(StoreError::NameCollision { .. })),
            "不同脚本占用同一路径必须返回 NameCollision，实际: {res:?}"
        );
        assert_eq!(
            fs::read_to_string(&existing).unwrap(),
            other_json,
            "冲突时不得覆盖已有脚本文件"
        );
        let _ = fs::remove_dir_all(&root);
    }
}
