//! 日志落盘 —— release 下没有 stderr，关键失败必须留下痕迹.
//!
//! # 为什么需要
//!
//! 本程序在 release 下是 `windows_subsystem = "windows"`，**没有控制台、没有 stderr**。
//! 这意味着：
//! - `core` 里 [`mechrevo_core::RestoreGuard`] 在 `Drop` 中打印的"自动回滚失败，
//!   硬件可能停在中间状态"——直接丢失；
//! - Tauri 自身在 9 个文件里的 20 处 `log::error!` / `log::warn!`——同样被丢弃
//!   （因为整个进程从未安装过 logger）。
//!
//! 对一个会写真实硬件、出错可能损坏设备的工具，"没有日志"是不可接受的：
//! 用户报障时只能描述"点了没反应"。
//!
//! # 设计约束
//!
//! 1. **不联网**：只写本地文件，不上报（`policy.md` §2）。
//! 2. **路径必须绝对**：提权实例由 `Start-Process -Verb RunAs` 启动，未指定工作目录，
//!    进程 cwd 可能是 `System32`。因此一律落在 `%LOCALAPPDATA%\Mechrevo-Max\logs`。
//! 3. **日志失败不得影响业务**：任何 IO 错误都静默降级（`log` 的 `Log::log` 返回 `()`）。
//! 4. **有上限**：按天分文件，只保留最近 [`KEEP_DAYS`] 天，避免无限增长。
//! 5. **不写敏感数据**：不记录 EC 全量 dump（体积大且含本机特征值）。

use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, SystemTime};

/// 日志保留天数（超期文件在启动时删除）.
pub const KEEP_DAYS: u64 = 7;

/// 单次启动内允许写入的最大字节数，超过后停止写入（防止异常循环刷爆磁盘）.
pub const MAX_BYTES_PER_RUN: u64 = 5 * 1024 * 1024;

/// 日志目录：`%LOCALAPPDATA%\Mechrevo-Max\logs`.
///
/// 取不到 `LOCALAPPDATA` 时退回临时目录 —— 宁可写到临时目录，也不要因为取路径失败而不记日志。
#[must_use]
pub fn log_dir() -> PathBuf {
    std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join("Mechrevo-Max")
        .join("logs")
}

/// 应用数据目录：`%LOCALAPPDATA%\Mechrevo-Max`（日志、配置都在其下）.
#[must_use]
pub fn data_dir() -> PathBuf {
    log_dir()
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(std::env::temp_dir)
}

/// 当天的日志文件路径（`app-YYYYMMDD.log`）.
#[must_use]
pub fn log_file_for(dir: &Path, at: SystemTime) -> PathBuf {
    dir.join(format!("app-{}.log", yyyymmdd(at)))
}

fn yyyymmdd(at: SystemTime) -> String {
    // 不引入 chrono：用 UNIX 天数换算到"距今多少天"，只用于文件名与保留策略。
    // 为避免日期换算 bug，这里直接用 UNIX 秒数分段（每天 86400 秒）。
    let secs = at
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    format!("epoch-{:05}", secs / 86_400)
}

/// 删除超过 [`KEEP_DAYS`] 天的日志文件；返回删除数量.
pub fn prune_old_logs(dir: &Path, now: SystemTime) -> usize {
    prune_logs_older_than(dir, now, Duration::from_secs(KEEP_DAYS * 86_400))
}

/// 删除比 `max_age` 更旧的日志文件；返回删除数量.
///
/// 只处理本模块产物（文件名以 `app-` 开头且以 `.log` 结尾），不碰目录里的其它东西。
pub fn prune_logs_older_than(dir: &Path, now: SystemTime, max_age: Duration) -> usize {
    let Ok(entries) = fs::read_dir(dir) else {
        return 0;
    };
    let mut removed = 0;
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        if !(name.starts_with("app-") && name.ends_with(".log")) {
            continue;
        }
        let too_old = entry
            .metadata()
            .and_then(|m| m.modified())
            .ok()
            .and_then(|m| now.duration_since(m).ok())
            .is_some_and(|age| age > max_age);
        if too_old && fs::remove_file(&path).is_ok() {
            removed += 1;
        }
    }
    removed
}

/// 一个按天滚动、限量、失败静默的文件 logger.
#[derive(Debug)]
pub struct FileLogger {
    dir: PathBuf,
    state: Mutex<LoggerState>,
}

#[derive(Debug)]
struct LoggerState {
    file: Option<File>,
    written: u64,
}

impl FileLogger {
    /// 在指定目录构造（目录不存在时创建）.
    ///
    /// # Errors
    /// 目录无法创建时返回错误；调用方应降级而不是让程序起不来。
    pub fn new(dir: PathBuf) -> std::io::Result<Self> {
        fs::create_dir_all(&dir)?;
        Ok(Self {
            dir,
            state: Mutex::new(LoggerState {
                file: None,
                written: 0,
            }),
        })
    }

    fn ensure_file<'a>(&self, state: &'a mut LoggerState) -> Option<&'a mut File> {
        if state.file.is_none() {
            let path = log_file_for(&self.dir, SystemTime::now());
            state.file = OpenOptions::new().create(true).append(true).open(path).ok();
        }
        state.file.as_mut()
    }
}

impl log::Log for FileLogger {
    fn enabled(&self, _metadata: &log::Metadata<'_>) -> bool {
        true
    }

    fn log(&self, record: &log::Record<'_>) {
        let Ok(mut state) = self.state.lock() else {
            return;
        };
        if state.written >= MAX_BYTES_PER_RUN {
            return;
        }
        let line = format!(
            "{:?} [{}] {}: {}\n",
            SystemTime::now(),
            record.level(),
            record.target(),
            record.args()
        );
        if let Some(file) = self.ensure_file(&mut state) {
            if file.write_all(line.as_bytes()).is_ok() {
                state.written += line.len() as u64;
            }
        }
    }

    fn flush(&self) {
        if let Ok(mut state) = self.state.lock() {
            if let Some(file) = state.file.as_mut() {
                let _ = file.flush();
            }
        }
    }
}

/// 全局 logger 实例（`set_logger` 需要 `&'static`，用 `OnceLock` 提供）.
static LOGGER: OnceLock<FileLogger> = OnceLock::new();

/// 安装全局 logger（幂等；重复调用不会重复注册）.
///
/// 返回日志目录，便于上层记录下来（也可以显示在"诊断"里）。
pub fn init() -> PathBuf {
    let dir = log_dir();
    prune_old_logs(&dir, SystemTime::now());
    match FileLogger::new(dir.clone()) {
        Ok(logger) => {
            let installed = LOGGER.get_or_init(|| logger);
            if log::set_logger(installed).is_ok() {
                log::set_max_level(log::LevelFilter::Info);
            }
        }
        // 拿不到日志目录时不能让程序起不来：降级到 stderr（调试期可见）
        Err(e) => eprintln!("[logging] 无法创建日志目录 {dir:?}: {e}"),
    }
    dir
}

/// 安装 panic hook：把 panic 位置与消息写进日志，再到 stderr（调试时可见）.
pub fn install_panic_hook() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        log::error!("panic: {info}");
        previous(info);
    }));
}

#[cfg(test)]
mod tests {
    use super::*;
    use log::Log as _;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn unique_dir(tag: &str) -> PathBuf {
        static N: AtomicUsize = AtomicUsize::new(0);
        let n = N.fetch_add(1, Ordering::SeqCst);
        std::env::temp_dir().join(format!("mechrevo-logtest-{tag}-{}-{n}", std::process::id()))
    }

    #[test]
    fn writes_line_to_file_and_increments_counter() {
        let dir = unique_dir("write");
        let logger = FileLogger::new(dir.clone()).unwrap();
        logger.log(
            &log::Record::builder()
                .args(format_args!("回滚失败: 测试"))
                .level(log::Level::Error)
                .target("mechrevo-max")
                .build(),
        );
        logger.flush();

        let mut found = false;
        for entry in fs::read_dir(&dir).unwrap().flatten() {
            let text = fs::read_to_string(entry.path()).unwrap_or_default();
            if text.contains("回滚失败: 测试") && text.contains("[ERROR]") {
                found = true;
            }
        }
        assert!(found, "日志内容未落到文件");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn prune_logs_older_than_removes_only_this_modules_files() {
        let dir = unique_dir("prune");
        fs::create_dir_all(&dir).unwrap();

        let app_log = dir.join("app-00001.log");
        fs::write(&app_log, b"old").unwrap();
        let unrelated = dir.join("notes.txt");
        fs::write(&unrelated, b"keep me").unwrap();

        // max_age = 0 → 任何已存在文件都算"过期"，避免测试依赖改 mtime（那要额外依赖）
        let removed = prune_logs_older_than(&dir, SystemTime::now(), Duration::ZERO);

        assert_eq!(removed, 1, "只应删除本模块的日志文件");
        assert!(!app_log.exists(), "过期日志应被删除");
        assert!(unrelated.exists(), "非本模块产物不得触碰");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn prune_keeps_recent_logs() {
        let dir = unique_dir("keep");
        fs::create_dir_all(&dir).unwrap();
        let app_log = dir.join("app-00002.log");
        fs::write(&app_log, b"new").unwrap();

        let removed = prune_logs_older_than(&dir, SystemTime::now(), Duration::from_secs(3600));

        assert_eq!(removed, 0, "刚写的日志不应被删");
        assert!(app_log.exists());
        let _ = fs::remove_dir_all(&dir);
    }
}
