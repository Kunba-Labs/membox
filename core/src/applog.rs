//! The log file — §11.3. A GUI app's stderr goes nowhere, so every `log::` call
//! in the core and the shell also lands in `<data dir>/membox.log`, which is
//! what you read when an item enriched into something strange three days ago.
//!
//! ponytail: 40 lines and the `log` crate we already have, instead of a logging
//! framework. One rotation, no config, no filters beyond the level. If the log
//! ever needs structure (JSON, per-item files), the call sites don't change.

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use parking_lot::Mutex;

const MAX_BYTES: u64 = 8 * 1024 * 1024;

pub struct FileLog {
    file: Mutex<Option<File>>,
    path: PathBuf,
    level: log::LevelFilter,
}

/// Start logging to `<dir>/membox.log`, keeping one previous generation.
/// Returns the path so the app can show it.
pub fn init(dir: &Path, level: log::LevelFilter) -> PathBuf {
    let path = dir.join("membox.log");
    if std::fs::metadata(&path).map(|m| m.len() > MAX_BYTES).unwrap_or(false) {
        let _ = std::fs::rename(&path, dir.join("membox.log.1"));
    }
    let file = OpenOptions::new().create(true).append(true).open(&path).ok();
    let logger = Box::new(FileLog { file: Mutex::new(file), path: path.clone(), level });
    log::set_max_level(level);
    // A second init (tests, a restart in-process) is not an error worth dying for.
    let _ = log::set_boxed_logger(logger);
    log::info!("--- membox {} starting ---", env!("CARGO_PKG_VERSION"));
    path
}

impl log::Log for FileLog {
    fn enabled(&self, m: &log::Metadata) -> bool {
        m.level() <= self.level
    }

    fn log(&self, r: &log::Record) {
        if !self.enabled(r.metadata()) {
            return;
        }
        let line = format!(
            "{} {:<5} {} {}\n",
            crate::model::now(),
            r.level(),
            r.target(),
            r.args()
        );
        eprint!("{line}");
        if let Some(f) = self.file.lock().as_mut() {
            let _ = f.write_all(line.as_bytes());
        }
    }

    fn flush(&self) {
        if let Some(f) = self.file.lock().as_mut() {
            let _ = f.flush();
        }
    }
}

impl FileLog {
    pub fn path(&self) -> &Path {
        &self.path
    }
}
