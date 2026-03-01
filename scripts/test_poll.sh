#!/usr/bin/env bash
# Test: spawn a command, then poll for its output.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
BINARY="${1:-$SCRIPT_DIR/../target/release/async-bash-mcp}"

echo "=== test_poll: spawn 'echo hello world' + poll ==="

RESPONSE=$(
  (
    echo '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-03-26","capabilities":{},"clientInfo":{"name":"test","version":"0.1"}}}'
    sleep 0.2
    echo '{"jsonrpc":"2.0","method":"notifications/initialized"}'
    sleep 0.2
    echo '{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"spawn","arguments":{"command":"echo hello world"}}}'
    sleep 0.5
    echo '{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"poll","arguments":{"process_id":1,"wait":3000}}}'
    sleep 1
  ) | timeout 10 "$BINARY" 2>/dev/null
)

POLL_LINE=$(echo "$RESPONSE" | grep '"id":3' || true)

if [ -z "$POLL_LINE" ]; then
  echo "FAIL: no poll response received"
  echo "$RESPONSE"
  exit 1
fi

if echo "$POLL_LINE" | grep -q 'hello world'; then
  echo "PASS: poll returned stdout containing 'hello world'"
else
  echo "FAIL: 'hello world' not found in poll response"
  echo "  Response: $POLL_LINE"
  exit 1
fi

if echo "$POLL_LINE" | grep -q '"finished":true\|\\"finished\\":true'; then
  echo "PASS: process finished"
else
  echo "WARN: process may not have finished yet"
fi

if echo "$POLL_LINE" | grep -q '"exitCode":0\|\\"exitCode\\":0'; then
  echo "PASS: exitCode is 0"
else
  echo "WARN: exitCode may not be 0"
fi

echo "  Response: $POLL_LINE"
