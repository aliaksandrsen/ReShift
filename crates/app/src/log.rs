//! Debug log. Nothing is written in a release build unless `--log-file` is given. The user's text must
//! never reach the log - only lengths and counts (see AGENTS.md).

use std::fmt;
use std::fs::{File, OpenOptions};
use std::io::{self, Write};
use std::path::Path;
use std::sync::{Mutex, OnceLock, PoisonError};
use std::time::{Duration, Instant};

pub struct Logger {
    start: Instant,
    file: Mutex<Option<File>>,
    stderr: bool,
}

impl Logger {
    pub fn new(file: Option<File>, stderr: bool) -> Self {
        Self {
            start: Instant::now(),
            file: Mutex::new(file),
            stderr,
        }
    }

    pub fn line(&self, args: fmt::Arguments<'_>) {
        let line = format_line(self.start.elapsed(), args);
        if self.stderr {
            let _ = io::stderr().write_all(line.as_bytes());
        }
        let mut file = self.file.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(file) = file.as_mut() {
            let _ = file.write_all(line.as_bytes());
        }
    }
}

/// "[  1.234] message\n"
fn format_line(elapsed: Duration, args: fmt::Arguments<'_>) -> String {
    format!(
        "[{:>3}.{:03}] {args}\n",
        elapsed.as_secs(),
        elapsed.subsec_millis()
    )
}

static LOGGER: OnceLock<Logger> = OnceLock::new();

/// Starts logging: to stderr in debug builds, to `path` if given (appended, created if needed).
/// A release build without `--log-file` stays silent and formats nothing.
pub fn init(path: Option<&Path>) -> io::Result<()> {
    let file = path
        .map(|path| OpenOptions::new().create(true).append(true).open(path))
        .transpose()?;
    let stderr = cfg!(debug_assertions);
    if file.is_some() || stderr {
        let _ = LOGGER.set(Logger::new(file, stderr));
    }
    Ok(())
}

pub fn logger() -> Option<&'static Logger> {
    LOGGER.get()
}

macro_rules! debug {
    ($($arg:tt)*) => {
        if let Some(logger) = $crate::log::logger() {
            logger.line(format_args!($($arg)*));
        }
    };
}
pub(crate) use debug;

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn temp_file(name: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!("reshift-log-{}-{name}", std::process::id()))
    }

    #[test]
    fn lines_have_a_timestamp_and_a_newline() {
        let line = format_line(Duration::from_millis(1234), format_args!("hello {}", 5));
        assert_eq!(line, "[  1.234] hello 5\n");
        let line = format_line(Duration::from_millis(61_007), format_args!("x"));
        assert_eq!(line, "[ 61.007] x\n");
    }

    #[test]
    fn a_logger_appends_to_its_file() {
        let path = temp_file("append");
        let _ = fs::remove_file(&path);
        {
            let file = OpenOptions::new()
                .create(true)
                .append(true)
                .open(&path)
                .unwrap();
            let logger = Logger::new(Some(file), false);
            logger.line(format_args!("first"));
            logger.line(format_args!("second {}", 2));
        }
        let text = fs::read_to_string(&path).unwrap();
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.len(), 2);
        assert!(lines[0].ends_with("] first"), "{text}");
        assert!(lines[1].ends_with("] second 2"), "{text}");
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn a_logger_without_sinks_is_silent_and_does_not_panic() {
        Logger::new(None, false).line(format_args!("nobody listens"));
    }

    #[test]
    fn an_unwritable_log_path_is_an_error_not_a_panic() {
        let path = std::env::temp_dir()
            .join("reshift-no-such-dir-xyz")
            .join("log.txt");
        assert!(init(Some(&path)).is_err());
    }
}
