#!/usr/bin/env bash
# Test: spawn a simple command and verify we get a process ID back.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
BINARY="${1:-$SCRIPT_DIR/../target/release/async-bash-mcp}"

echo "=== test_spawn: spawn 'echo hello' ==="

RESPONSE=$(
  (
    echo '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-03-26","capabilities":{},"clientInfo":{"name":"test","version":"0.1"}}}'
    sleep 0.2
    echo '{"jsonrpc":"2.0","method":"notifications/initialized"}'
    sleep 0.2
    echo '{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"spawn","arguments":{"command":"echo hello"}}}'
    sleep 0.5
  ) | timeout 5 "$BINARY" 2>/dev/null
)

SPAWN_LINE=$(echo "$RESPONSE" | grep '"id":2' || true)

if [ -z "$SPAWN_LINE" ]; then
  echo "FAIL: no spawn response received"
  echo "$RESPONSE"
  exit 1
fi

echo "PASS: spawn returned response"
echo "  Response: $SPAWN_LINE"
