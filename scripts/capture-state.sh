#!/usr/bin/env bash
# capture-state.sh — "I'm stuck — give me everything" VM state dump.
#
# Captures a screenshot, the tail of the serial log, the QEMU process
# inventory, and a human-readable summary into build/capture-<ts>/.
# Intended for human/agent debugging and CI post-mortems — not for
# automated test runner integration.
#
# Outputs (in build/capture-<timestamp>/):
#   screen.png       — current framebuffer (via screenshot.sh)
#   serial-tail.log  — last 200 lines of build/serial-temple.log
#   qemu-info.txt    — running QEMU processes + sock paths
#   summary.md       — human-readable state digest
#
# Exit code is always 0 if the directory was created, even if some
# captures failed (each failure is recorded in summary.md). This way
# CI can always upload the artefact.
#
# Dependencies: bash, pgrep, tail, date. screen.png additionally
# needs the same deps as screenshot.sh.

set -uo pipefail

REPO="$(cd "$(dirname "$0")/.." && pwd)"
BUILD="$REPO/build"
TS="$(date +%Y%m%d-%H%M%S)"

# Optional --label=<slug> places the capture in build/capture-<slug>-<ts>/
# instead of build/capture-<ts>/, so per-failure bundles (e.g. one per
# failed test) are easy to spot in retrospect. Non-alphanumeric chars in
# the label are replaced with `_` to keep the path well-behaved.
LABEL=""
for arg in "$@"; do
  case "$arg" in
    --label=*)
      LABEL="${arg#--label=}"
      ;;
    *)
      echo "capture-state: unknown argument: $arg" >&2
      exit 2
      ;;
  esac
done

if [ -n "$LABEL" ]; then
  SAFE_LABEL="$(printf '%s' "$LABEL" | tr -c 'A-Za-z0-9._-' '_' | cut -c1-60)"
  OUT="$BUILD/capture-$SAFE_LABEL-$TS"
else
  OUT="$BUILD/capture-$TS"
fi
mkdir -p "$OUT"

SERIAL_LOG="$BUILD/serial-temple.log"

echo "capture-state: writing to $OUT" >&2

# ---- 1. screenshot ----------------------------------------------------------
SCREEN_STATUS="ok"
SCREEN_ERR=""
# Force screenshot.sh to write into our capture dir.
if SCREEN_PNG="$OUT/screen.png" "$REPO/scripts/screenshot.sh" >/dev/null 2>"$OUT/.screenshot.err"; then
  :
else
  SCREEN_STATUS="failed"
  SCREEN_ERR="$(cat "$OUT/.screenshot.err" 2>/dev/null || true)"
fi
rm -f "$OUT/.screenshot.err"

# ---- 2. serial tail ---------------------------------------------------------
SERIAL_STATUS="ok"
if [ -f "$SERIAL_LOG" ]; then
  tail -n 200 "$SERIAL_LOG" > "$OUT/serial-tail.log" 2>/dev/null || SERIAL_STATUS="read-failed"
else
  SERIAL_STATUS="missing"
  : > "$OUT/serial-tail.log"
fi

# ---- 3. qemu-info -----------------------------------------------------------
{
  echo "# QEMU process inventory ($(date))"
  echo
  echo "## pgrep -fl qemu-system-x86_64"
  pgrep -fl qemu-system-x86_64 2>/dev/null || echo "(no QEMU processes)"
  echo
  echo "## monitor sockets in $BUILD"
  for s in "$BUILD"/qemu-temple.sock "$BUILD"/qemu.sock; do
    if [ -S "$s" ]; then
      echo "  ALIVE  $s"
    elif [ -e "$s" ]; then
      echo "  STALE  $s (exists but not a socket)"
    else
      echo "  ABSENT $s"
    fi
  done
  echo
  echo "## chardev (COM2) sockets in $BUILD"
  for s in "$BUILD"/com2-temple.sock "$BUILD"/com2.sock; do
    if [ -S "$s" ]; then
      echo "  ALIVE  $s"
    elif [ -e "$s" ]; then
      echo "  STALE  $s"
    else
      echo "  ABSENT $s"
    fi
  done
} > "$OUT/qemu-info.txt"

# ---- 4. summary.md ----------------------------------------------------------
QEMU_RUNNING="no"
if pgrep -f qemu-system-x86_64 >/dev/null 2>&1; then
  QEMU_RUNNING="yes"
fi

LAST_DDONE="(none seen)"
LAST_TEST_SUMMARY="(none seen)"
LAST_ERROR_TAIL="(none)"
if [ -f "$SERIAL_LOG" ]; then
  # D_DONE / D_OK markers — temple-run.py uses these.
  d_line="$(grep -E "D_DONE|D_OK" "$SERIAL_LOG" 2>/dev/null | tail -n 1 || true)"
  [ -n "$d_line" ] && LAST_DDONE="$d_line"

  # Test summary: temple-run.py emits "pass=N fail=M" style lines.
  t_line="$(grep -Ei "pass=[0-9]+ +fail=[0-9]+" "$SERIAL_LOG" 2>/dev/null | tail -n 1 || true)"
  [ -n "$t_line" ] && LAST_TEST_SUMMARY="$t_line"

  # Last compile error block (COMPILE_ERR_BEGIN..COMPILE_ERR_END).
  err_block="$(awk '/COMPILE_ERR_BEGIN/{flag=1;buf="";next} /COMPILE_ERR_END/{flag=0;last=buf;next} flag{buf=buf $0 "\n"} END{printf "%s", last}' "$SERIAL_LOG" 2>/dev/null || true)"
  if [ -n "$err_block" ]; then
    LAST_ERROR_TAIL="$err_block"
  fi
fi

{
  echo "# Capture summary — $TS"
  echo
  if [ -n "$LABEL" ]; then
    echo "- Label: \`$LABEL\`"
  fi
  echo "- VM running? **$QEMU_RUNNING**"
  echo "- Capture dir: \`$OUT\`"
  echo "- Screenshot: **$SCREEN_STATUS**"
  if [ "$SCREEN_STATUS" != "ok" ] && [ -n "$SCREEN_ERR" ]; then
    echo
    echo "  Screenshot error:"
    echo
    echo '  ```'
    printf '%s\n' "$SCREEN_ERR" | sed 's/^/  /'
    echo '  ```'
  fi
  echo "- Serial log: **$SERIAL_STATUS** (\`$SERIAL_LOG\`)"
  echo "- Last D_DONE/D_OK marker: \`$LAST_DDONE\`"
  echo "- Last test summary: \`$LAST_TEST_SUMMARY\`"
  echo
  echo "## Last compile error block"
  echo
  if [ "$LAST_ERROR_TAIL" = "(none)" ]; then
    echo "_(none seen in serial log)_"
  else
    echo '```'
    printf '%s' "$LAST_ERROR_TAIL"
    echo '```'
  fi
  echo
  echo "## Serial tail (last 20 lines)"
  echo
  echo '```'
  tail -n 20 "$OUT/serial-tail.log" 2>/dev/null || echo "(empty)"
  echo '```'
  echo
  echo "## Files in this capture"
  echo
  ( cd "$OUT" && ls -la | sed 's/^/    /' )
} > "$OUT/summary.md"

echo "$OUT"
