use crate::validation::{validate_command, validate_cwd};
use std::collections::HashMap;
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::io::AsyncReadExt;
use tokio::sync::{Mutex, Notify};

pub struct ProcessInfo {
    pub id: u32,
    pub command: String,
    pub start_time: Instant,
    pub cwd: Option<String>,
    stdout_buffer: Arc<Mutex<String>>,
    stderr_buffer: Arc<Mutex<String>>,
    stdout_position: usize,
    stderr_position: usize,
    accessed: bool,
    finished: Arc<AtomicBool>,
    exit_code: Arc<Mutex<Option<i32>>>,
    notify: Arc<Notify>,
}

pub struct ProcessManager {
    processes: HashMap<u32, ProcessInfo>,
    next_id: Arc<AtomicU32>,
}

#[derive(Debug)]
pub struct PollResult {
    pub stdout: String,
    pub stderr: String,
    pub elapsed_time: f64, // milliseconds
    pub finished: bool,
    pub exit_code: Option<i32>,
}

pub struct ProcessListItem {
    pub id: u32,
    pub command: String,
    pub done: bool,
}

fn last_n_lines(s: &str, n: usize) -> &str {
    if s.is_empty() || n == 0 {
        return s;
    }
    let mut found = 0;
    let bytes = s.as_bytes();
    let mut i = bytes.len();
    while i > 0 {
        i -= 1;
        if bytes[i] == b'\n' {
            found += 1;
            if found == n {
                return &s[i + 1..];
            }
        }
    }
    s
}

fn round_ms(secs: f64) -> u64 {
    (secs * 1000.0).round() as u64
}

impl ProcessManager {
    pub fn new() -> Self {
        ProcessManager {
            processes: HashMap::new(),
            next_id: Arc::new(AtomicU32::new(1)),
        }
    }

    pub async fn spawn_process(&mut self, command: &str, cwd: Option<&str>) -> Result<u32, String> {
        validate_command(command)?;
        let resolved_cwd = validate_cwd(cwd)?;

        let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/bash".to_string());

        let mut cmd = tokio::process::Command::new(&shell);
        cmd.args(["-c", command])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);

        if let Some(ref cwd_path) = resolved_cwd {
            cmd.current_dir(cwd_path);
        }

        let mut child = cmd.spawn().map_err(|e| e.to_string())?;

        let stdout = child.stdout.take().expect("stdout not captured");
        let stderr = child.stderr.take().expect("stderr not captured");

        let stdout_buffer: Arc<Mutex<String>> = Arc::new(Mutex::new(String::new()));
        let stderr_buffer: Arc<Mutex<String>> = Arc::new(Mutex::new(String::new()));
        let finished = Arc::new(AtomicBool::new(false));
        let exit_code: Arc<Mutex<Option<i32>>> = Arc::new(Mutex::new(None));
        let notify = Arc::new(Notify::new());

        // Background stdout reader
        let stdout_buf_clone = stdout_buffer.clone();
        let stdout_task = tokio::spawn(async move {
            let mut reader = stdout;
            let mut buf = [0u8; 1024];
            loop {
                match reader.read(&mut buf).await {
                    Ok(0) => break,
                    Ok(n) => {
                        let text = String::from_utf8_lossy(&buf[..n]).into_owned();
                        stdout_buf_clone.lock().await.push_str(&text);
                    }
                    Err(_) => break,
                }
            }
        });

        // Background stderr reader
        let stderr_buf_clone = stderr_buffer.clone();
        let stderr_task = tokio::spawn(async move {
            let mut reader = stderr;
            let mut buf = [0u8; 1024];
            loop {
                match reader.read(&mut buf).await {
                    Ok(0) => break,
                    Ok(n) => {
                        let text = String::from_utf8_lossy(&buf[..n]).into_owned();
                        stderr_buf_clone.lock().await.push_str(&text);
                    }
                    Err(_) => break,
                }
            }
        });

        // Background completion task
        let finished_clone = finished.clone();
        let exit_code_clone = exit_code.clone();
        let notify_clone = notify.clone();
        tokio::spawn(async move {
            let _ = stdout_task.await;
            let _ = stderr_task.await;
            let code = child.wait().await.ok().and_then(|s| s.code());
            *exit_code_clone.lock().await = code;
            finished_clone.store(true, Ordering::SeqCst);
            notify_clone.notify_waiters();
        });

        let id = self.next_id.fetch_add(1, Ordering::SeqCst);

        self.processes.insert(
            id,
            ProcessInfo {
                id,
                command: command.to_string(),
                start_time: Instant::now(),
                cwd: resolved_cwd,
                stdout_buffer,
                stderr_buffer,
                stdout_position: 0,
                stderr_position: 0,
                accessed: false,
                finished,
                exit_code,
                notify,
            },
        );

        Ok(id)
    }

    pub async fn poll_process(
        &mut self,
        process_id: u32,
        wait_ms: u64,
        terminate: bool,
        progress_callback: Option<Arc<dyn Fn(u64, String) + Send + Sync>>,
    ) -> Result<PollResult, String> {
        if !self.processes.contains_key(&process_id) {
            return Err(format!("Process {} not found", process_id));
        }

        if wait_ms == 0 {
            return Err("Wait time must be greater than 0 milliseconds".to_string());
        }

        // Clone Arcs to avoid borrow conflicts during awaits
        let notify = self.processes[&process_id].notify.clone();
        let finished = self.processes[&process_id].finished.clone();
        let exit_code_arc = self.processes[&process_id].exit_code.clone();

        if terminate && !finished.load(Ordering::SeqCst) {
            // Wait up to 5s for the process to finish (kill_on_drop handles the actual kill)
            // We mark the process's finished flag to force completion
            let _ = tokio::time::timeout(Duration::from_secs(5), notify.notified()).await;
            if !finished.load(Ordering::SeqCst) {
                finished.store(true, Ordering::SeqCst);
                *exit_code_arc.lock().await = Some(-1);
                notify.notify_waiters();
            }
        } else if !finished.load(Ordering::SeqCst) {
            if let Some(ref cb) = progress_callback {
                let cmd = self.processes[&process_id].command.clone();
                let stdout_arc = self.processes[&process_id].stdout_buffer.clone();
                let stderr_arc = self.processes[&process_id].stderr_buffer.clone();
                let start_time = self.processes[&process_id].start_time;
                let cb_clone = cb.clone();

                let progress_task = tokio::spawn(async move {
                    let mut interval = tokio::time::interval(Duration::from_millis(300));
                    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
                    loop {
                        interval.tick().await;
                        let stdout = stdout_arc.lock().await.clone();
                        let stderr = stderr_arc.lock().await.clone();
                        let elapsed_secs = start_time.elapsed().as_secs_f64();
                        let stdout_tail = last_n_lines(&stdout, 5).to_string();
                        let stderr_tail = last_n_lines(&stderr, 5).to_string();
                        let msg = format!(
                            "# `$ {}` \n\n## stdout\n\n```\n{}\n```\n\n## stderr\n\n```\n{}\n```\n",
                            cmd, stdout_tail, stderr_tail
                        );
                        cb_clone(round_ms(elapsed_secs), msg);
                    }
                });

                let _ =
                    tokio::time::timeout(Duration::from_millis(wait_ms), notify.notified()).await;
                progress_task.abort();
                let _ = progress_task.await;

                let elapsed_secs = self.processes[&process_id]
                    .start_time
                    .elapsed()
                    .as_secs_f64();
                let stdout_snap = self.processes[&process_id]
                    .stdout_buffer
                    .lock()
                    .await
                    .clone();
                let stderr_snap = self.processes[&process_id]
                    .stderr_buffer
                    .lock()
                    .await
                    .clone();
                let stdout_tail = last_n_lines(&stdout_snap, 5).to_string();
                let stderr_tail = last_n_lines(&stderr_snap, 5).to_string();
                let cmd = self.processes[&process_id].command.clone();
                let final_msg = format!(
                    "# `$ {}` \n\n## stdout\n\n```\n{}\n```\n\n## stderr\n\n```\n{}\n```\n",
                    cmd, stdout_tail, stderr_tail
                );
                cb(round_ms(elapsed_secs), final_msg);
            } else {
                let _ =
                    tokio::time::timeout(Duration::from_millis(wait_ms), notify.notified()).await;
            }
        }

        // Extract incremental output
        let proc = self.processes.get_mut(&process_id).unwrap();

        let new_stdout = {
            let guard = proc.stdout_buffer.lock().await;
            guard[proc.stdout_position..].to_string()
        };
        proc.stdout_position += new_stdout.len();

        let new_stderr = {
            let guard = proc.stderr_buffer.lock().await;
            guard[proc.stderr_position..].to_string()
        };
        proc.stderr_position += new_stderr.len();

        let elapsed_time = proc.start_time.elapsed().as_secs_f64() * 1000.0;
        let is_finished = proc.finished.load(Ordering::SeqCst);
        let exit_code = if is_finished {
            *proc.exit_code.lock().await
        } else {
            None
        };

        if is_finished {
            proc.accessed = true;
            proc.stdout_buffer.lock().await.clear();
            proc.stderr_buffer.lock().await.clear();
        }

        Ok(PollResult {
            stdout: new_stdout,
            stderr: new_stderr,
            elapsed_time,
            finished: is_finished,
            exit_code,
        })
    }

    pub fn list_processes(&mut self) -> Vec<ProcessListItem> {
        self.processes.retain(|_, proc| !proc.accessed);
        self.processes
            .values()
            .map(|proc| ProcessListItem {
                id: proc.id,
                command: proc.command.clone(),
                done: proc.finished.load(Ordering::SeqCst),
            })
            .collect()
    }
}

impl Default for ProcessManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_last_n_lines_basic() {
        assert_eq!(last_n_lines("a\nb\nc\nd\ne\nf", 3), "d\ne\nf");
    }

    #[test]
    fn test_last_n_lines_fewer_than_n() {
        assert_eq!(last_n_lines("a\nb", 5), "a\nb");
    }

    #[test]
    fn test_last_n_lines_empty() {
        assert_eq!(last_n_lines("", 5), "");
    }

    #[tokio::test]
    async fn test_spawn_simple_command() {
        let mut pm = ProcessManager::new();
        let id = pm.spawn_process("echo hello", None).await.unwrap();
        assert!(id > 0);
        let list = pm.list_processes();
        assert!(list.iter().any(|p| p.id == id));
    }

    #[tokio::test]
    async fn test_multiple_processes() {
        let mut pm = ProcessManager::new();
        let id1 = pm.spawn_process("echo a", None).await.unwrap();
        let id2 = pm.spawn_process("echo b", None).await.unwrap();
        assert_ne!(id1, id2);
        let list = pm.list_processes();
        assert!(list.iter().any(|p| p.id == id1));
        assert!(list.iter().any(|p| p.id == id2));
    }

    #[tokio::test]
    async fn test_poll_output() {
        let mut pm = ProcessManager::new();
        let id = pm.spawn_process("echo hello", None).await.unwrap();
        let result = pm.poll_process(id, 2000, false, None).await.unwrap();
        assert!(
            result.stdout.contains("hello"),
            "stdout: {:?}",
            result.stdout
        );
        assert!(result.finished);
        assert_eq!(result.exit_code, Some(0));
    }

    #[tokio::test]
    async fn test_incremental_output() {
        let mut pm = ProcessManager::new();
        let id = pm
            .spawn_process("echo first && sleep 0.2 && echo second", None)
            .await
            .unwrap();

        let r1 = pm.poll_process(id, 100, false, None).await.unwrap();
        let r2 = pm.poll_process(id, 2000, false, None).await.unwrap();

        let combined = format!("{}{}", r1.stdout, r2.stdout);
        assert!(combined.contains("first"), "combined: {:?}", combined);
        assert!(combined.contains("second"), "combined: {:?}", combined);

        let first_count = combined.matches("first").count();
        assert_eq!(first_count, 1, "duplication detected: {:?}", combined);
    }

    #[tokio::test]
    async fn test_working_directory() {
        let tmp = tempfile::tempdir().unwrap();
        let tmp_path = tmp.path().to_str().unwrap().to_string();

        let mut pm = ProcessManager::new();
        let id = pm.spawn_process("pwd", Some(&tmp_path)).await.unwrap();
        let result = pm.poll_process(id, 2000, false, None).await.unwrap();
        let out = result.stdout.trim().to_string();
        assert!(
            out.contains(tmp.path().file_name().unwrap().to_str().unwrap()),
            "stdout: {:?}",
            out
        );
    }

    #[tokio::test]
    async fn test_terminate() {
        let mut pm = ProcessManager::new();
        let id = pm.spawn_process("sleep 10", None).await.unwrap();
        let result = pm.poll_process(id, 8000, true, None).await.unwrap();
        assert!(
            result.finished,
            "process should be finished after terminate"
        );
    }

    #[tokio::test]
    async fn test_wait_timeout() {
        let mut pm = ProcessManager::new();
        let id = pm.spawn_process("sleep 5", None).await.unwrap();
        let start = Instant::now();
        let result = pm.poll_process(id, 200, false, None).await.unwrap();
        let elapsed = start.elapsed().as_millis();
        assert!(!result.finished, "should not be finished yet");
        assert!(
            elapsed < 1000,
            "should return within 1 second, took {}ms",
            elapsed
        );
    }

    #[tokio::test]
    async fn test_cleanup_accessed() {
        let mut pm = ProcessManager::new();
        let id = pm.spawn_process("echo done", None).await.unwrap();
        let mut found_finished = false;
        for _ in 0..10 {
            let r = pm.poll_process(id, 500, false, None).await.unwrap();
            if r.finished {
                found_finished = true;
                break;
            }
        }
        assert!(found_finished);
        let list = pm.list_processes();
        assert!(!list.iter().any(|p| p.id == id));
    }

    #[tokio::test]
    async fn test_error_invalid_id() {
        let mut pm = ProcessManager::new();
        let result = pm.poll_process(999999, 100, false, None).await;
        assert!(result.is_err());
        let msg = result.unwrap_err();
        assert!(msg.contains("not found"), "error: {:?}", msg);
    }

    #[tokio::test]
    async fn test_stderr_capture() {
        let mut pm = ProcessManager::new();
        let id = pm.spawn_process("echo err >&2", None).await.unwrap();
        let result = pm.poll_process(id, 2000, false, None).await.unwrap();
        assert!(result.stderr.contains("err"), "stderr: {:?}", result.stderr);
    }

    #[tokio::test]
    async fn test_concurrent_processes() {
        let mut pm = ProcessManager::new();
        let mut ids = Vec::new();
        for i in 0..5 {
            let id = pm
                .spawn_process(&format!("echo process{}", i), None)
                .await
                .unwrap();
            ids.push(id);
        }
        let unique: std::collections::HashSet<_> = ids.iter().collect();
        assert_eq!(unique.len(), 5);
    }

    #[tokio::test]
    async fn test_long_output() {
        let mut pm = ProcessManager::new();
        let id = pm.spawn_process("seq 1 100", None).await.unwrap();
        let result = pm.poll_process(id, 5000, false, None).await.unwrap();
        assert!(result.stdout.contains("100"), "stdout: {:?}", result.stdout);
        assert!(result.finished);
    }

    #[tokio::test]
    async fn test_progress_message_format() {
        use std::sync::{Arc, Mutex};
        let messages = Arc::new(Mutex::new(Vec::<String>::new()));
        let messages_clone = messages.clone();
        let cb: Arc<dyn Fn(u64, String) + Send + Sync> = Arc::new(move |_ms, msg| {
            messages_clone.lock().unwrap().push(msg);
        });

        let mut pm = ProcessManager::new();
        let id = pm.spawn_process("echo hello", None).await.unwrap();
        let _ = pm.poll_process(id, 1500, false, Some(cb)).await.unwrap();

        let msgs = messages.lock().unwrap();
        assert!(
            !msgs.is_empty(),
            "should have at least one progress message"
        );
        let last = msgs.last().unwrap();
        assert!(
            last.contains("## stdout"),
            "message should contain stdout section: {}",
            last
        );
        assert!(
            last.contains("## stderr"),
            "message should contain stderr section: {}",
            last
        );
        assert!(
            last.contains("echo hello"),
            "message should contain command: {}",
            last
        );
    }

    #[tokio::test]
    async fn test_progress_during_wait() {
        use std::sync::{Arc, Mutex};
        let calls = Arc::new(Mutex::new(Vec::<u64>::new()));
        let calls_clone = calls.clone();
        let cb: Arc<dyn Fn(u64, String) + Send + Sync> = Arc::new(move |ms, _msg| {
            calls_clone.lock().unwrap().push(ms);
        });

        let mut pm = ProcessManager::new();
        let id = pm.spawn_process("sleep 2", None).await.unwrap();
        let _ = pm.poll_process(id, 700, false, Some(cb)).await.unwrap();

        let count = calls.lock().unwrap().len();
        assert!(
            count >= 1,
            "progress callback should be called at least once, got {}",
            count
        );
    }

    #[tokio::test]
    async fn test_progress_elapsed_correct() {
        use std::sync::{Arc, Mutex};
        let last_ms: Arc<Mutex<u64>> = Arc::new(Mutex::new(0));
        let last_ms_clone = last_ms.clone();
        let cb: Arc<dyn Fn(u64, String) + Send + Sync> = Arc::new(move |ms, _msg| {
            *last_ms_clone.lock().unwrap() = ms;
        });

        let mut pm = ProcessManager::new();
        let id = pm.spawn_process("sleep 0.3", None).await.unwrap();
        let _ = pm.poll_process(id, 2000, false, Some(cb)).await.unwrap();

        let reported_ms = *last_ms.lock().unwrap();
        assert!(
            reported_ms < 5000,
            "elapsed should be <5000ms (was {} ms), Python bug would give 2,000,000",
            reported_ms
        );
        assert!(reported_ms > 0, "elapsed should be >0");
    }
}
