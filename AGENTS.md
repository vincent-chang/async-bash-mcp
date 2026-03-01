AGENTS.md

Build / lint / test

- Build: cargo build
- Run full tests: cargo test
- Run a single test: cargo test test_name
- Lint: cargo clippy -- -D warnings
- Format check: cargo fmt -- --check
- Format fix: cargo fmt

Code style & conventions

- Use Rust edition 2021. Keep code simple and idiomatic.
- Formatting: enforced by rustfmt (cargo fmt).
- Naming: snake_case for functions/variables, PascalCase for types, UPPER_SNAKE for constants.
- Errors: use specific error types. Validate inputs early. Return plain serializable types for MCP responses.
- Logging: use tracing crate (tracing::info!, tracing::error!). Logs MUST go to stderr (stdout is MCP JSON-RPC wire).

Source layout

- src/main.rs     — binary entry point, tokio runtime, rmcp stdio transport
- src/lib.rs      — module declarations
- src/validation.rs — command and cwd validation
- src/process.rs  — ProcessManager: spawn/poll/list processes
- src/server.rs   — AsyncBashServer: MCP tool handlers

Agents operating here

- All 3 MCP tools: spawn, poll, list_processes
- Store all notes about progress in the notes/ directory
- Plans and notepads in .sisyphus/

Keep this file short and machine-readable; update when source layout changes.
