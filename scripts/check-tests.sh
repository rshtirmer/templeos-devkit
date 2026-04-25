#!/usr/bin/env bash
# Parse build/serial.log after a test run. Exit 0 if green, 1 if red.

set -euo pipefail

REPO="$(cd "$(dirname "$0")/.." && pwd)"
LOG="$REPO/build/serial.log"

[ -f "$LOG" ] || { echo "error: $LOG missing — did the run happen?" >&2; exit 1; }

echo "==== test run summary ===="
grep -E "^(TEST_RUN_BEGIN|TEST_RUN_END|TEST_SUMMARY|TEST_PASS|TEST_FAIL|TEST_PANIC)" "$LOG" || true
echo "=========================="

# Panic marker (compile error or HolyC exception during the run). The
# watcher in run-tests.sh snaps build/screen.png before quitting — point
# the user at it.
if grep -q "^TEST_PANIC:" "$LOG"; then
  panic=$(grep "^TEST_PANIC:" "$LOG" | head -1)
  echo "PANIC during run — $panic"
  [ -f "$REPO/build/screen.png" ] && echo "       see $REPO/build/screen.png"
  exit 1
fi

if grep -q "^TEST_FAIL:" "$LOG"; then
  fails=$(grep -c "^TEST_FAIL:" "$LOG")
  echo "FAILED ($fails test(s))"
  exit 1
fi

if ! grep -q "^TEST_RUN_END" "$LOG"; then
  echo "INCOMPLETE — run did not reach TEST_RUN_END (timeout or panic?)"
  [ -f "$REPO/build/screen.png" ] && echo "       see $REPO/build/screen.png"
  exit 1
fi

passes=$(grep -c "^TEST_PASS:" "$LOG" || echo 0)
echo "OK ($passes test(s) passed)"
