//! Append-only log file of the loader: `%LOCALAPPDATA%\PhoeCord\loader.log` on Windows.
//!
//! Plain `std::fs`, no logging framework: the loader lives inside someone else's process and must
//! not install global state (a `tracing` subscriber, a panic hook) the client could trip over.

use std::{
    ffi::OsString,
    fs::{self, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

/// `<local app data>/PhoeCord/loader.log`.
pub fn log_path(local_app_data: Option<OsString>) -> Option<PathBuf> {
    let base = PathBuf::from(local_app_data?);
    (!base.as_os_str().is_empty()).then(|| base.join("PhoeCord").join("loader.log"))
}

/// Default location from the `LOCALAPPDATA` environment variable.
pub fn default_log_path() -> Option<PathBuf> {
    log_path(std::env::var_os("LOCALAPPDATA"))
}

/// One line: `[<unix seconds>.<millis>] [pid <pid>] <message>`.
pub fn format_line(now: SystemTime, pid: u32, msg: &str) -> String {
    let d = now.duration_since(UNIX_EPOCH).unwrap_or_default();
    format!(
        "[{}.{:03}] [pid {pid}] {msg}\n",
        d.as_secs(),
        d.subsec_millis()
    )
}

/// Appends a line, creating the folder and file if needed.
pub fn append(path: &Path, msg: &str) -> io::Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let mut f = OpenOptions::new().create(true).append(true).open(path)?;
    f.write_all(format_line(SystemTime::now(), std::process::id(), msg).as_bytes())
}

/// Logger that never fails the caller: a log problem must not affect the client.
pub struct Logger(Option<PathBuf>);

impl Logger {
    pub fn new(path: Option<PathBuf>) -> Logger {
        Logger(path)
    }

    pub fn from_env() -> Logger {
        Logger(default_log_path())
    }

    pub fn log(&self, msg: &str) {
        if let Some(p) = &self.0 {
            let _ = append(p, msg);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn path_layout() {
        assert_eq!(
            log_path(Some("C:/Users/x/AppData/Local".into())),
            Some(PathBuf::from(
                "C:/Users/x/AppData/Local/PhoeCord/loader.log"
            ))
        );
        assert_eq!(log_path(None), None);
        assert_eq!(log_path(Some("".into())), None);
    }

    #[test]
    fn line_format() {
        let t = UNIX_EPOCH + Duration::from_millis(1_700_000_000_042);
        assert_eq!(format_line(t, 7, "hi"), "[1700000000.042] [pid 7] hi\n");
    }

    #[test]
    fn appends_and_creates_folders() {
        let dir = std::env::temp_dir().join(format!("phoecord-log-test-{}", std::process::id()));
        let path = dir.join("PhoeCord").join("loader.log");
        let log = Logger::new(Some(path.clone()));
        log.log("one");
        log.log("two");
        let text = fs::read_to_string(&path).unwrap();
        fs::remove_dir_all(&dir).unwrap();
        assert_eq!(text.lines().count(), 2);
        assert!(text.lines().next().unwrap().ends_with("one"));
    }
}
