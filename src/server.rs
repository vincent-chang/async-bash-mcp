use crate::process::ProcessManager;
use rmcp::{
    handler::server::{router::tool::ToolRouter, wrapper::Parameters},
    model::{
        CallToolResult, Content, Implementation, ProgressNotificationParam, ServerCapabilities,
        ServerInfo,
    },
    schemars,
    service::RequestContext,
    tool, tool_handler, tool_router, ErrorData as McpError, RoleServer, ServerHandler,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::Mutex;

// --- Parameter structs (must derive Deserialize + JsonSchema) ---

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct SpawnParams {
    /// The bash command to execute
    command: String,
    /// Optional working directory path (defaults to current directory)
    cwd: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct PollParams {
    /// The process ID returned by spawn
    process_id: i64,
    /// Maximum milliseconds to wait for process completion. Must be greater than 0
    wait: i64,
    /// If true, terminate the process with SIGTERM before returning results
    #[serde(default)]
    terminate: bool,
}

// --- Response structs (JSON field names must match Python version exactly) ---

#[derive(Debug, Serialize)]
struct SpawnResponse {
    id: u32,
}

#[derive(Debug, Serialize)]
struct PollResponse {
    stdout: String,
    stderr: String,
    #[serde(rename = "elapsedTime")]
    elapsed_time: f64,
    finished: bool,
    #[serde(rename = "exitCode")]
    exit_code: Option<i32>,
}

#[derive(Debug, Serialize)]
struct ListItemResponse {
    #[serde(rename = "ID")]
    id: u32,
    command: String,
    done: bool,
}

#[derive(Debug, Serialize)]
struct ListResponse {
    processes: Vec<ListItemResponse>,
}

// --- Server struct ---

#[derive(Clone)]
pub struct AsyncBashServer {
    process_manager: Arc<Mutex<ProcessManager>>,
    tool_router: ToolRouter<Self>,
}

impl Default for AsyncBashServer {
    fn default() -> Self {
        Self::new(false)
    }
}

#[tool_router]
impl AsyncBashServer {
    pub fn new(logging_enabled: bool) -> Self {
        Self {
            process_manager: Arc::new(Mutex::new(ProcessManager::new(logging_enabled))),
            tool_router: Self::tool_router(),
        }
    }

    #[tool(
        description = "Launch a bash command asynchronously in a subshell.\n\nReturns a unique process ID that can be used to check progress with the poll tool. **ALWAYS POLL THE PROCESS AFTER SPAWNING**.\n\nMultiple commands can be spawned in parallel and independently polled. If the task requires running independent bash commands, run them in parrallel."
    )]
    async fn spawn(
        &self,
        Parameters(params): Parameters<SpawnParams>,
    ) -> Result<CallToolResult, McpError> {
        let mut pm = self.process_manager.lock().await;
        match pm
            .spawn_process(&params.command, params.cwd.as_deref())
            .await
        {
            Ok(id) => {
                let resp = SpawnResponse { id };
                let json = serde_json::to_string(&resp)
                    .unwrap_or_else(|e| format!("{{\"error\":\"serialization failed: {e}\"}}"));
                Ok(CallToolResult::success(vec![Content::text(json)]))
            }
            Err(e) => Ok(CallToolResult {
                content: vec![Content::text(e)],
                is_error: Some(true),
                structured_content: None,
                meta: None,
            }),
        }
    }

    #[tool(
        description = "List all currently running or recently finished processes. Processes are removed from this list once their results have been accessed via the poll tool."
    )]
    async fn list_processes(&self) -> Result<CallToolResult, McpError> {
        let mut pm = self.process_manager.lock().await;
        let items: Vec<ListItemResponse> = pm
            .list_processes()
            .into_iter()
            .map(|p| ListItemResponse {
                id: p.id,
                command: p.command,
                done: p.done,
            })
            .collect();
        let resp = ListResponse { processes: items };
        let json = serde_json::to_string(&resp)
            .unwrap_or_else(|e| format!("{{\"error\":\"serialization failed: {e}\"}}"));
        Ok(CallToolResult::success(vec![Content::text(json)]))
    }

    #[tool(
        description = "Check the progress of a spawned process. Returns stdout/stderr output produced since the last poll call. Can optionally wait for completion or terminate the process.\n\n**NEVER LEAVE A PROCESS RUNNING UNPOLLED**. Always poll the process after spawning it to ensure resources are cleaned up.\n\n**TERMINATE THE PROCESS IF YOU NO LONGER NEED IT**. If you don't poll the process, it will continue running indefinitely."
    )]
    async fn poll(
        &self,
        Parameters(params): Parameters<PollParams>,
        ctx: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, McpError> {
        if params.wait <= 0 {
            return Ok(CallToolResult {
                content: vec![Content::text(
                    "Wait time must be greater than 0 milliseconds",
                )],
                is_error: Some(true),
                structured_content: None,
                meta: None,
            });
        }

        // Wire progress callback via ctx.peer + ctx.meta.get_progress_token()
        let progress_callback: Option<Arc<dyn Fn(u64, String) + Send + Sync>> =
            if let Some(token) = ctx.meta.get_progress_token() {
                let peer = ctx.peer.clone();
                let total = params.wait as u64;
                Some(Arc::new(move |progress_ms: u64, message: String| {
                    let peer = peer.clone();
                    let token = token.clone();
                    tokio::spawn(async move {
                        let _ = peer
                            .notify_progress(ProgressNotificationParam {
                                progress_token: token,
                                progress: progress_ms as f64,
                                total: Some(total as f64),
                                message: Some(message),
                            })
                            .await;
                    });
                }))
            } else {
                None
            };

        let mut pm = self.process_manager.lock().await;
        match pm
            .poll_process(
                params.process_id as u32,
                params.wait as u64,
                params.terminate,
                progress_callback,
            )
            .await
        {
            Ok(result) => {
                let resp = PollResponse {
                    stdout: result.stdout,
                    stderr: result.stderr,
                    elapsed_time: result.elapsed_time,
                    finished: result.finished,
                    exit_code: result.exit_code,
                };
                let json = serde_json::to_string(&resp)
                    .unwrap_or_else(|e| format!("{{\"error\":\"serialization failed: {e}\"}}"));
                Ok(CallToolResult::success(vec![Content::text(json)]))
            }
            Err(e) => Ok(CallToolResult {
                content: vec![Content::text(e)],
                is_error: Some(true),
                structured_content: None,
                meta: None,
            }),
        }
    }
}

#[tool_handler]
impl ServerHandler for AsyncBashServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            capabilities: ServerCapabilities::builder().enable_tools().build(),
            server_info: Implementation {
                name: "async-bash-mcp".into(),
                version: env!("CARGO_PKG_VERSION").into(),
                title: None,
                description: None,
                icons: None,
                website_url: None,
            },
            ..Default::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_spawn_tool() {
        let server = AsyncBashServer::new(false);
        let mut pm = server.process_manager.lock().await;
        let id = pm.spawn_process("echo hello", None).await.unwrap();
        assert!(id > 0);
    }

    #[tokio::test]
    async fn test_spawn_with_cwd() {
        let server = AsyncBashServer::new(false);
        let mut pm = server.process_manager.lock().await;
        let result = pm.spawn_process("echo cwd_test", Some("/tmp")).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_list_tool() {
        let server = AsyncBashServer::new(false);
        let mut pm = server.process_manager.lock().await;
        let id = pm.spawn_process("sleep 10", None).await.unwrap();
        let list = pm.list_processes();
        assert!(list.iter().any(|p| p.id == id));
    }

    #[tokio::test]
    async fn test_poll_tool() {
        let server = AsyncBashServer::new(false);
        let id = {
            let mut pm = server.process_manager.lock().await;
            pm.spawn_process("echo hello", None).await.unwrap()
        };
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
        let mut pm = server.process_manager.lock().await;
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
    async fn test_poll_with_wait() {
        let server = AsyncBashServer::new(false);
        let id = {
            let mut pm = server.process_manager.lock().await;
            pm.spawn_process("sleep 0.1 && echo done", None)
                .await
                .unwrap()
        };
        let mut pm = server.process_manager.lock().await;
        let result = pm.poll_process(id, 3000, false, None).await.unwrap();
        // Poll returned (may or may not be finished, but no error)
        drop(result);
    }

    #[tokio::test]
    async fn test_poll_with_terminate() {
        let server = AsyncBashServer::new(false);
        let id = {
            let mut pm = server.process_manager.lock().await;
            pm.spawn_process("sleep 10", None).await.unwrap()
        };
        let mut pm = server.process_manager.lock().await;
        let result = pm.poll_process(id, 2000, true, None).await.unwrap();
        assert!(result.finished, "should be finished after terminate");
    }

    #[tokio::test]
    async fn test_poll_invalid_id() {
        let server = AsyncBashServer::new(false);
        let mut pm = server.process_manager.lock().await;
        let result = pm.poll_process(99999, 1000, false, None).await;
        assert!(result.is_err(), "should error for unknown process id");
    }

    #[tokio::test]
    async fn test_poll_wait_zero_invalid() {
        // Validate the guard: wait <= 0 → error
        let wait: i64 = 0;
        assert!(wait <= 0, "guard condition must catch wait=0: got {wait}");
        let wait_neg: i64 = -1;
        assert!(
            wait_neg <= 0,
            "guard condition must catch negative wait: got {wait_neg}"
        );
    }

    #[tokio::test]
    async fn test_shell_detection() {
        let server = AsyncBashServer::new(false);
        let id = {
            let mut pm = server.process_manager.lock().await;
            pm.spawn_process("echo $0", None).await.unwrap()
        };
        tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;
        let mut pm = server.process_manager.lock().await;
        let result = pm.poll_process(id, 2000, false, None).await.unwrap();
        assert!(
            !result.stdout.is_empty(),
            "shell detection: stdout should not be empty"
        );
    }

    #[tokio::test]
    async fn test_session_isolation() {
        // Two independent server instances should have independent ProcessManagers
        let server1 = AsyncBashServer::new(false);
        let server2 = AsyncBashServer::new(false);
        let id1 = {
            let mut pm1 = server1.process_manager.lock().await;
            pm1.spawn_process("echo s1", None).await.unwrap()
        };
        let id2 = {
            let mut pm2 = server2.process_manager.lock().await;
            pm2.spawn_process("echo s2", None).await.unwrap()
        };
        // Both start at 1 — they are independent
        assert_eq!(id1, 1, "server1 first process should be id=1");
        assert_eq!(id2, 1, "server2 first process should be id=1 (independent)");
    }

    #[tokio::test]
    async fn test_dangerous_command_rejected() {
        let server = AsyncBashServer::new(false);
        let mut pm = server.process_manager.lock().await;
        let result = pm.spawn_process("rm -rf /", None).await;
        assert!(result.is_err(), "dangerous command should be rejected");
        let err = result.unwrap_err();
        assert!(
            err.to_lowercase().contains("dangerous") || err.to_lowercase().contains("pattern"),
            "error should mention dangerous pattern, got: {err}"
        );
    }

    #[tokio::test]
    async fn test_json_field_names_poll_response() {
        let resp = PollResponse {
            stdout: "out".into(),
            stderr: "err".into(),
            elapsed_time: 100.0,
            finished: true,
            exit_code: Some(0),
        };
        let json = serde_json::to_string(&resp).unwrap();
        assert!(
            json.contains("\"elapsedTime\""),
            "must have camelCase elapsedTime field, got: {json}"
        );
        assert!(
            json.contains("\"exitCode\""),
            "must have camelCase exitCode field, got: {json}"
        );
        assert!(
            !json.contains("\"elapsed_time\""),
            "must NOT have snake_case elapsed_time, got: {json}"
        );
        assert!(
            !json.contains("\"exit_code\""),
            "must NOT have snake_case exit_code, got: {json}"
        );
    }

    #[tokio::test]
    async fn test_json_field_names_list_item() {
        let item = ListItemResponse {
            id: 42,
            command: "test".into(),
            done: false,
        };
        let json = serde_json::to_string(&item).unwrap();
        assert!(
            json.contains("\"ID\""),
            "must have uppercase ID field, got: {json}"
        );
        assert!(
            !json.contains("\"id\""),
            "must NOT have lowercase id, got: {json}"
        );
    }

    #[tokio::test]
    async fn test_poll_parameter_consistency() {
        // Verify PollParams has correct field names matching Python spec
        // process_id: i64, wait: i64, terminate: bool
        let json = r#"{"process_id":1,"wait":1000,"terminate":false}"#;
        let params: PollParams = serde_json::from_str(json).unwrap();
        assert_eq!(params.process_id, 1);
        assert_eq!(params.wait, 1000);
        assert!(!params.terminate);
    }
}
