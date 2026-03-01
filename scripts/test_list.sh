#!/usr/bin/env bash
# Test: spawn two commands, then list_processes to see them.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
BINARY="${1:-$SCRIPT_DIR/../target/release/async-bash-mcp}"

echo "=== test_list: spawn two commands + list_processes ==="

RESPONSE=$(
  (
    echo '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-03-26","capabilities":{},"clientInfo":{"name":"test","version":"0.1"}}}'
    sleep 0.2
    echo '{"jsonrpc":"2.0","method":"notifications/initialized"}'
    sleep 0.2
    echo '{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"spawn","arguments":{"command":"sleep 10"}}}'
    sleep 0.2
    echo '{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"spawn","arguments":{"command":"echo done"}}}'
    sleep 0.3
    echo '{"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"list_processes","arguments":{}}}'
    sleep 0.3
    # Clean up: terminate the sleep process
    echo '{"jsonrpc":"2.0","id":5,"method":"tools/call","params":{"name":"poll","arguments":{"process_id":1,"wait":100,"terminate":true}}}'
    sleep 0.3
  ) | timeout 10 "$BINARY" 2>/dev/null
)

LIST_LINE=$(echo "$RESPONSE" | grep '"id":4' || true)

if [ -z "$LIST_LINE" ]; then
  echo "FAIL: no list_processes response received"
  echo "$RESPONSE"
  exit 1
fi

if echo "$LIST_LINE" | grep -q '"ID":1\|\\"ID\\":1'; then
  echo "PASS: process 1 visible in list"
else
  echo "FAIL: process 1 not found in list"
  echo "  Response: $LIST_LINE"
  exit 1
fi

if echo "$LIST_LINE" | grep -q '"ID":2\|\\"ID\\":2'; then
  echo "PASS: process 2 visible in list"
else
  echo "FAIL: process 2 not found in list"
  echo "  Response: $LIST_LINE"
  exit 1
fi

echo "  Response: $LIST_LINE"
