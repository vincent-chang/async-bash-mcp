#!/usr/bin/env bash
# Run all test scripts against the async-bash-mcp binary.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
BINARY="${1:-$SCRIPT_DIR/../target/release/async-bash-mcp}"

if [ ! -x "$BINARY" ]; then
  echo "ERROR: binary not found at $BINARY"
  echo "Usage: $0 [path/to/async-bash-mcp]"
  exit 1
fi

PASS=0
FAIL=0

for test_script in "$SCRIPT_DIR"/test_spawn.sh \
                    "$SCRIPT_DIR"/test_poll.sh \
                    "$SCRIPT_DIR"/test_list.sh \
                    "$SCRIPT_DIR"/test_terminate.sh; do
  echo "─────────────────────────────────────────"
  if bash "$test_script" "$BINARY"; then
    PASS=$((PASS + 1))
  else
    FAIL=$((FAIL + 1))
  fi
done

echo "═════════════════════════════════════════"
echo "Results: $PASS passed, $FAIL failed"
echo "═════════════════════════════════════════"

[ "$FAIL" -eq 0 ]
