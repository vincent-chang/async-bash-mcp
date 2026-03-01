#!/usr/bin/env bash
# Test: spawn a long-running process, then terminate it via poll.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
BINARY="${1:-$SCRIPT_DIR/../target/release/async-bash-mcp}"

echo "=== test_terminate: spawn 'sleep 60' + terminate ==="

RESPONSE=$(
  (
    echo '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-03-26","capabilities":{},"clientInfo":{"name":"test","version":"0.1"}}}'
    sleep 0.2
    echo '{"jsonrpc":"2.0","method":"notifications/initialized"}'
    sleep 0.2
    echo '{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"spawn","arguments":{"command":"sleep 60"}}}'
    sleep 0.3
    echo '{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"poll","arguments":{"process_id":1,"wait":500,"terminate":true}}}'
    sleep 1
  ) | timeout 10 "$BINARY" 2>/dev/null
)

POLL_LINE=$(echo "$RESPONSE" | grep '"id":3' || true)

if [ -z "$POLL_LINE" ]; then
  echo "FAIL: no poll response received"
  echo "$RESPONSE"
  exit 1
fi

if echo "$POLL_LINE" | grep -q '"finished":true\|\\"finished\\":true'; then
  echo "PASS: process was terminated (finished=true)"
else
  echo "FAIL: process not terminated"
  echo "  Response: $POLL_LINE"
  exit 1
fi

echo "  Response: $POLL_LINE"
