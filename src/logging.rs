use chrono::{DateTime, Local};
use std::path::PathBuf;

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
    // placeholder — mpsc sender added in Task 3
    _process_id: u32,
}

impl ProcessLogger {
    pub async fn new(_process_id: u32, _command: &str, _cwd: Option<&str>) -> Option<Self> {
        todo!()
    }

    pub fn log(&self, _event: LogEvent, _content: String) {
        todo!()
    }
}

fn get_log_dir() -> PathBuf {
    todo!()
}

fn format_entry(_entry: &LogEntry) -> String {
    todo!()
}
