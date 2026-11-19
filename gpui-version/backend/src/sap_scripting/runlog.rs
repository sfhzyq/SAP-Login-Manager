//! 脚本执行日志：`<exe_dir>/data/logs/script-YYYYMMDD.log`，按天滚动追加写。
//!
//! 每次执行写入一组行（无日志框架依赖，直接 fs append，写入失败静默忽略）：
//!
//! ```text
//! [2026-10-20 14:30:00.123] BEGIN 入口=登录挂载 凭据=RS测试环境-2 脚本=MSC2N修改 path=E:\...\MSC2N修改.json
//! [14:30:03.456]   .. 附着 SAP GUI 首会话 attach(0,0)
//! [14:30:05.678]   #1 SetText wnd[0]/usr/... = 'osheet.cells(i,1).VALUE' -> OK
//! [14:30:06.789]   #3 SendVKey wnd[0] n=0 -> ERR: COM 错误: 无效的字符
//! [14:30:06.790] END 失败(6.7s): COM 错误: 无效的字符
//! ```
//!
//! 设计动机（2026-10-16 诊断）：登录挂载脚本失败只进 log::error（应用未接
//! 日志实现，等于静默失败），用户只看到"脚本没有执行"；且 COM 报错不知道
//! 卡在第几条动作。本模块让每次执行的每条动作可追溯。

use std::io::Write as _;
use std::path::PathBuf;
use std::time::Instant;

use chrono::Local;

use super::ScriptError;
use crate::storage;

/// 一次脚本执行的日志句柄：begin 到 finish 的所有行共享同一耗时起点。
pub struct RunLog {
    start: Instant,
}

impl RunLog {
    /// 记录一次执行开始（写入 BEGIN 行，含入口/凭据/脚本/路径）。
    pub fn begin(entry: &str, cred: &str, script: &str, path: &str) -> Self {
        let log = RunLog {
            start: Instant::now(),
        };
        append(&format!(
            "[{}] BEGIN 入口={entry} 凭据={cred} 脚本={script} path={path}",
            Local::now().format("%Y-%m-%d %H:%M:%S%.3f"),
        ));
        log
    }

    /// 中间事件（附着会话 / 自动登录 / 轮询结果等）。
    pub fn note(&self, msg: &str) {
        append(&format!("[{}]   .. {}", short_ts(), msg));
    }

    /// 单条动作执行结果（err=None 为成功）。
    pub fn action(&self, idx: usize, desc: &str, err: Option<&str>) {
        match err {
            None => append(&format!("[{}]   #{idx} {desc} -> OK", short_ts())),
            Some(e) => append(&format!("[{}]   #{idx} {desc} -> ERR: {e}", short_ts())),
        }
    }

    /// 记录执行结束：成功/失败 + 总耗时。
    pub fn finish(&self, result: &Result<(), ScriptError>) {
        let secs = self.start.elapsed().as_secs_f32();
        match result {
            Ok(()) => append(&format!("[{}] END 成功({secs:.1}s)", short_ts())),
            Err(e) => append(&format!("[{}] END 失败({secs:.1}s): {e}", short_ts())),
        }
    }
}

/// 行内短时间戳（BEGIN 行已含完整日期）。
fn short_ts() -> String {
    Local::now().format("%H:%M:%S%.3f").to_string()
}

/// 当前日志文件：<exe_dir>/data/logs/script-YYYYMMDD.log（按天滚动）。
pub fn log_file() -> PathBuf {
    storage::logs_root().join(format!("script-{}.log", Local::now().format("%Y%m%d")))
}

/// 日志目录（UI「打开日志」入口用）。
pub fn log_dir() -> PathBuf {
    storage::logs_root()
}

fn append(line: &str) {
    let _ = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_file())
        .and_then(|mut f| f.write_all(format!("{line}\n").as_bytes()));
}
