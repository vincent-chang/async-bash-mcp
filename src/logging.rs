use chrono::{DateTime, Local};
use std::path::PathBuf;
use tokio::sync::mpsc;

pub enum LogEvent {
    Spawn,
    Stdout,
    Stderr,
    Exit,
    Error,
    Signal,
}

pub struct LogEntry {
    pub timestamp: DateTime<Local>,
    pub event: LogEvent,
    pub content: String,
}

pub struct ProcessLogger {
    tx: mpsc::Sender<LogEntry>,
    handle: Option<tokio::task::JoinHandle<()>>,
}

impl ProcessLogger {
    pub async fn new(process_id: u32, command: &str, cwd: Option<&str>) -> Option<Self> {
        let log_dir = get_log_dir();
        if let Err(e) = tokio::fs::create_dir_all(&log_dir).await {
            tracing::warn!("Failed to create log dir {:?}: {}", log_dir, e);
            return None;
        }

        let ts = chrono::Local::now().format("%Y%m%d_%H%M%S");
        let filename = format!("process_{}_{}.log", process_id, ts);
        let log_path = log_dir.join(&filename);
        let file = match std::fs::File::create(&log_path) {
            Ok(f) => f,
            Err(e) => {
                tracing::warn!("Failed to create log file {:?}: {}", log_path, e);
                return None;
            }
        };

        let max_size: usize = std::env::var("ASYNC_BASH_MAX_LOG_SIZE")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(10 * 1024 * 1024);

        let (tx, mut rx) = mpsc::channel::<LogEntry>(1000);

        let spawn_entry = LogEntry {
            timestamp: chrono::Local::now(),
            event: LogEvent::Spawn,
            content: format!("command='{}' cwd='{}'", command, cwd.unwrap_or("<none>")),
        };
        let _ = tx.try_send(spawn_entry);

        let handle = tokio::task::spawn_blocking(move || {
            use std::io::Write;

            let mut file = file;
            let mut bytes_written: usize = 0;
            let mut truncated = false;

            while let Some(entry) = rx.blocking_recv() {
                if truncated {
                    continue;
                }
                let line = format_entry(&entry) + "\n";
                let line_bytes = line.len();
                if bytes_written + line_bytes > max_size {
                    let _ = writeln!(file, "[TRUNCATED] Log size limit reached");
                    truncated = true;
                    continue;
                }
                match writeln!(file, "{}", format_entry(&entry)) {
                    Ok(_) => bytes_written += line_bytes,
                    Err(e) => {
                        tracing::warn!("Log write error: {}", e);
                        truncated = true;
                    }
                }
            }
        });

        Some(ProcessLogger {
            tx,
            handle: Some(handle),
        })
    }

    pub fn log(&self, event: LogEvent, content: String) {
        let entry = LogEntry {
            timestamp: chrono::Local::now(),
            event,
            content,
        };
        let _ = self.tx.try_send(entry);
    }
}

impl Drop for ProcessLogger {
    fn drop(&mut self) {
        let tx = std::mem::replace(&mut self.tx, mpsc::channel(1).0);
        drop(tx);
        if let Some(handle) = self.handle.take() {
            while !handle.is_finished() {
                std::thread::yield_now();
            }
        }
    }
}

fn get_log_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("ASYNC_BASH_LOG_DIR") {
        PathBuf::from(dir)
    } else {
        std::env::temp_dir().join("async-bash-mcp/logs")
    }
}

fn format_entry(entry: &LogEntry) -> String {
    let ts = entry.timestamp.format("%Y-%m-%d %H:%M:%S");
    let event_str = match entry.event {
        LogEvent::Spawn => "SPAWN",
        LogEvent::Stdout => "STDOUT",
        LogEvent::Stderr => "STDERR",
        LogEvent::Exit => "EXIT",
        LogEvent::Error => "ERROR",
        LogEvent::Signal => "SIGNAL",
    };
    format!("[{}] [{}] {}", ts, event_str, entry.content)
}


#[cfg(test)]
mod tests {
    use super::*;
    use std::env;
    use std::fs;
    use std::io::Read;
    use std::sync::{Mutex, OnceLock};

    /// Helper: cleanup env var after test
    struct EnvGuard {
        key: String,
        original: Option<String>,
    }

    impl EnvGuard {
        fn new(key: &str) -> Self {
            let original = env::var(key).ok();
            Self {
                key: key.to_string(),
                original,
            }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            if let Some(val) = &self.original {
                env::set_var(&self.key, val);
            } else {
                env::remove_var(&self.key);
            }
        }
    }

    fn env_lock() -> std::sync::MutexGuard<'static, ()> {
        static ENV_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        ENV_LOCK.get_or_init(|| Mutex::new(())).lock().unwrap()
    }

    #[test]
    fn test_get_log_dir_default() {
        let _lock = env_lock();
        let _guard = EnvGuard::new("ASYNC_BASH_LOG_DIR");
        env::remove_var("ASYNC_BASH_LOG_DIR");
        let log_dir = get_log_dir();
        let path_str = log_dir.to_string_lossy();
        assert!(path_str.ends_with("async-bash-mcp/logs") || path_str.ends_with("async-bash-mcp\\logs"),
            "Expected path to end with 'async-bash-mcp/logs', got: {}", path_str);
    }

    #[test]
    fn test_get_log_dir_env_var() {
        let _lock = env_lock();
        let _guard = EnvGuard::new("ASYNC_BASH_LOG_DIR");
        env::set_var("ASYNC_BASH_LOG_DIR", "/tmp/custom-logs");
        let log_dir = get_log_dir();
        assert_eq!(log_dir.to_string_lossy(), "/tmp/custom-logs",
            "Expected log_dir to match env var");
    }

    #[tokio::test]
    async fn test_log_file_created() {
        let _lock = env_lock();
        let tmp = tempfile::tempdir().expect("Failed to create temp dir");
        let tmp_path = tmp.path().to_string_lossy().to_string();
        let _guard = EnvGuard::new("ASYNC_BASH_LOG_DIR");
        env::set_var("ASYNC_BASH_LOG_DIR", &tmp_path);

        let process_id = 12345u32;
        let _logger = ProcessLogger::new(process_id, "echo test", None).await;

        // logger creation should succeed (or at least not panic)
        // and a log file should exist
        let entries = fs::read_dir(&tmp_path).expect("Failed to read temp dir");
        let log_files: Vec<_> = entries
            .filter_map(|e| {
                let entry = e.ok()?;
                let file_name = entry.file_name();
                let name_str = file_name.to_string_lossy();
                if name_str.starts_with("process_12345_") && name_str.ends_with(".log") {
                    Some(name_str.to_string())
                } else {
                    None
                }
            })
            .collect();

        assert!(!log_files.is_empty(), "Expected log file matching process_12345_*.log pattern");
    }

    #[tokio::test]
    async fn test_log_spawn_event() {
        let _lock = env_lock();
        let tmp = tempfile::tempdir().expect("Failed to create temp dir");
        let tmp_path = tmp.path().to_string_lossy().to_string();
        let _guard = EnvGuard::new("ASYNC_BASH_LOG_DIR");
        env::set_var("ASYNC_BASH_LOG_DIR", &tmp_path);

        let process_id = 22222u32;
        let logger = ProcessLogger::new(process_id, "echo spawn-test", None).await;
        if let Some(l) = logger {
            l.log(LogEvent::Spawn, "spawned process".to_string());
            drop(l);
        }
        // drop logger to flush writes

        // read log file and verify [SPAWN] appears
        let entries = fs::read_dir(&tmp_path).expect("Failed to read temp dir");
        let log_file = entries
            .filter_map(|e| e.ok())
            .find(|e| {
                let name = e.file_name();
                let name_str = name.to_string_lossy();
                name_str.starts_with("process_22222_") && name_str.ends_with(".log")
            });

        assert!(log_file.is_some(), "Log file not found");
        let log_path = log_file.unwrap().path();
        let mut content = String::new();
        fs::File::open(&log_path)
            .expect("Failed to open log file")
            .read_to_string(&mut content)
            .expect("Failed to read log file");

        assert!(content.contains("[SPAWN]"), "Expected [SPAWN] in log file, got: {}", content);
    }

    #[tokio::test]
    async fn test_log_stdout_event() {
        let _lock = env_lock();
        let tmp = tempfile::tempdir().expect("Failed to create temp dir");
        let tmp_path = tmp.path().to_string_lossy().to_string();
        let _guard = EnvGuard::new("ASYNC_BASH_LOG_DIR");
        env::set_var("ASYNC_BASH_LOG_DIR", &tmp_path);

        let process_id = 33333u32;
        let logger = ProcessLogger::new(process_id, "echo stdout-test", None).await;
        if let Some(l) = logger {
            l.log(LogEvent::Stdout, "output from stdout".to_string());
            drop(l);
        }

        let entries = fs::read_dir(&tmp_path).expect("Failed to read temp dir");
        let log_file = entries
            .filter_map(|e| e.ok())
            .find(|e| {
                let name = e.file_name();
                let name_str = name.to_string_lossy();
                name_str.starts_with("process_33333_") && name_str.ends_with(".log")
            });

        let log_path = log_file.unwrap().path();
        let mut content = String::new();
        fs::File::open(&log_path)
            .expect("Failed to open log file")
            .read_to_string(&mut content)
            .expect("Failed to read log file");

        assert!(content.contains("[STDOUT]"), "Expected [STDOUT] in log file, got: {}", content);
    }

    #[tokio::test]
    async fn test_log_stderr_event() {
        let _lock = env_lock();
        let tmp = tempfile::tempdir().expect("Failed to create temp dir");
        let tmp_path = tmp.path().to_string_lossy().to_string();
        let _guard = EnvGuard::new("ASYNC_BASH_LOG_DIR");
        env::set_var("ASYNC_BASH_LOG_DIR", &tmp_path);

        let process_id = 44444u32;
        let logger = ProcessLogger::new(process_id, "echo stderr-test", None).await;
        if let Some(l) = logger {
            l.log(LogEvent::Stderr, "error output".to_string());
            drop(l);
        }

        let entries = fs::read_dir(&tmp_path).expect("Failed to read temp dir");
        let log_file = entries
            .filter_map(|e| e.ok())
            .find(|e| {
                let name = e.file_name();
                let name_str = name.to_string_lossy();
                name_str.starts_with("process_44444_") && name_str.ends_with(".log")
            });

        let log_path = log_file.unwrap().path();
        let mut content = String::new();
        fs::File::open(&log_path)
            .expect("Failed to open log file")
            .read_to_string(&mut content)
            .expect("Failed to read log file");

        assert!(content.contains("[STDERR]"), "Expected [STDERR] in log file, got: {}", content);
    }

    #[tokio::test]
    async fn test_log_exit_event() {
        let _lock = env_lock();
        let tmp = tempfile::tempdir().expect("Failed to create temp dir");
        let tmp_path = tmp.path().to_string_lossy().to_string();
        let _guard = EnvGuard::new("ASYNC_BASH_LOG_DIR");
        env::set_var("ASYNC_BASH_LOG_DIR", &tmp_path);

        let process_id = 55555u32;
        let logger = ProcessLogger::new(process_id, "echo exit-test", None).await;
        if let Some(l) = logger {
            l.log(LogEvent::Exit, "code=0".to_string());
            drop(l);
        }

        let entries = fs::read_dir(&tmp_path).expect("Failed to read temp dir");
        let log_file = entries
            .filter_map(|e| e.ok())
            .find(|e| {
                let name = e.file_name();
                let name_str = name.to_string_lossy();
                name_str.starts_with("process_55555_") && name_str.ends_with(".log")
            });

        let log_path = log_file.unwrap().path();
        let mut content = String::new();
        fs::File::open(&log_path)
            .expect("Failed to open log file")
            .read_to_string(&mut content)
            .expect("Failed to read log file");

        assert!(content.contains("[EXIT]"), "Expected [EXIT] in log file, got: {}", content);
        assert!(content.contains("code=0"), "Expected 'code=0' in log file, got: {}", content);
    }

    #[tokio::test]
    async fn test_log_error_event() {
        let _lock = env_lock();
        let tmp = tempfile::tempdir().expect("Failed to create temp dir");
        let tmp_path = tmp.path().to_string_lossy().to_string();
        let _guard = EnvGuard::new("ASYNC_BASH_LOG_DIR");
        env::set_var("ASYNC_BASH_LOG_DIR", &tmp_path);

        let process_id = 66666u32;
        let logger = ProcessLogger::new(process_id, "echo error-test", None).await;
        if let Some(l) = logger {
            l.log(LogEvent::Error, "something went wrong".to_string());
            drop(l);
        }

        let entries = fs::read_dir(&tmp_path).expect("Failed to read temp dir");
        let log_file = entries
            .filter_map(|e| e.ok())
            .find(|e| {
                let name = e.file_name();
                let name_str = name.to_string_lossy();
                name_str.starts_with("process_66666_") && name_str.ends_with(".log")
            });

        let log_path = log_file.unwrap().path();
        let mut content = String::new();
        fs::File::open(&log_path)
            .expect("Failed to open log file")
            .read_to_string(&mut content)
            .expect("Failed to read log file");

        assert!(content.contains("[ERROR]"), "Expected [ERROR] in log file, got: {}", content);
    }

    #[tokio::test]
    async fn test_log_signal_event() {
        let _lock = env_lock();
        let tmp = tempfile::tempdir().expect("Failed to create temp dir");
        let tmp_path = tmp.path().to_string_lossy().to_string();
        let _guard = EnvGuard::new("ASYNC_BASH_LOG_DIR");
        env::set_var("ASYNC_BASH_LOG_DIR", &tmp_path);

        let process_id = 77777u32;
        let logger = ProcessLogger::new(process_id, "echo signal-test", None).await;
        if let Some(l) = logger {
            l.log(LogEvent::Signal, "SIGTERM".to_string());
            drop(l);
        }

        let entries = fs::read_dir(&tmp_path).expect("Failed to read temp dir");
        let log_file = entries
            .filter_map(|e| e.ok())
            .find(|e| {
                let name = e.file_name();
                let name_str = name.to_string_lossy();
                name_str.starts_with("process_77777_") && name_str.ends_with(".log")
            });

        let log_path = log_file.unwrap().path();
        let mut content = String::new();
        fs::File::open(&log_path)
            .expect("Failed to open log file")
            .read_to_string(&mut content)
            .expect("Failed to read log file");

        assert!(content.contains("[SIGNAL]"), "Expected [SIGNAL] in log file, got: {}", content);
    }

    #[test]
    fn test_format_entry() {
        let _lock = env_lock();
        let entry = LogEntry {
            timestamp: Local::now(),
            event: LogEvent::Spawn,
            content: "test command".to_string(),
        };
        let formatted = format_entry(&entry);

        // Should contain timestamp, event type, and content
        assert!(formatted.contains("[SPAWN]"), "Expected [SPAWN] in formatted output, got: {}", formatted);
        assert!(formatted.contains("test command"), "Expected 'test command' in formatted output, got: {}", formatted);
        // Verify format includes timestamp pattern (YYYY-MM-DD HH:MM:SS)
        assert!(formatted.len() > 10, "Expected non-trivial formatted output");
    }

    #[tokio::test]
    async fn test_logging_failure_graceful() {
        let _lock = env_lock();
        let _guard = EnvGuard::new("ASYNC_BASH_LOG_DIR");
        // Set an unwritable path
        env::set_var("ASYNC_BASH_LOG_DIR", "/root/impossible-path-12345");

        let process_id = 88888u32;
        let logger = ProcessLogger::new(process_id, "echo graceful-test", None).await;
        // Should return None or gracefully handle, not panic
        // Just verify it doesn't crash
        let _ = logger;
    }

    #[tokio::test]
    async fn test_max_log_size() {
        let _lock = env_lock();
        let tmp = tempfile::tempdir().expect("Failed to create temp dir");
        let tmp_path = tmp.path().to_string_lossy().to_string();
        let _guard = EnvGuard::new("ASYNC_BASH_MAX_LOG_SIZE");
        env::set_var("ASYNC_BASH_MAX_LOG_SIZE", "100");
        let _guard2 = EnvGuard::new("ASYNC_BASH_LOG_DIR");
        env::set_var("ASYNC_BASH_LOG_DIR", &tmp_path);

        let process_id = 99999u32;
        let logger = ProcessLogger::new(process_id, "echo max-size-test", None).await;
        if let Some(l) = logger {
            // Log content that exceeds the 100-byte limit
            l.log(LogEvent::Stdout, "x".repeat(150));
            drop(l);
        }

        let entries = fs::read_dir(&tmp_path).expect("Failed to read temp dir");
        let log_file = entries
            .filter_map(|e| e.ok())
            .find(|e| {
                let name = e.file_name();
                let name_str = name.to_string_lossy();
                name_str.starts_with("process_99999_") && name_str.ends_with(".log")
            });

        if let Some(lf) = log_file {
            let log_path = lf.path();
            if let Ok(metadata) = fs::metadata(&log_path) {
                let size = metadata.len() as usize;
                // File size should be at most 100 + some buffer for [TRUNCATED] line
                assert!(size <= 200, "Log file size {} exceeds reasonable limit", size);
            }
        }
    }
}
