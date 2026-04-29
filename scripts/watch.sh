#!/usr/bin/env bash
# watch.sh — auto-run the test cycle whenever a HolyC file is saved.
#
# Watches src/ and tests/ for modifications to .HC / .HH / .ZC files and
# kicks off `make test-temple` on every save. Coalesces save bursts via
# a short debounce so editor-atomic-write storms count as one event.
#
# Edit-save-test loop:
#   make dev-temple                    # cold boot once (~90s)
#   make vm-warmup                     # save snapshot once (~60s, after B1)
#   make watch T=Render WARM=1         # auto-test on every save (~10s/iter)
#                                      # Ctrl-C to stop
#
# Args (positional, all optional — empty string OK):
#   $1  T              filter passed to test-temple (e.g. "Math")
#   $2  WARM           "1" to use the snapshot fast-path
#   $3  PUSH_TIMEOUT   forwarded to temple-run.py
#
# Watcher: prefers `fswatch` (macOS), falls back to `inotifywait` (Linux).
# If neither is found, prints install instructions and exits 1.

set -uo pipefail

T_FILTER="${1:-}"
WARM_FLAG="${2:-}"
PUSH_TIMEOUT_VAL="${3:-}"

# scripts/ lives one level under the devkit root.
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"

# Watch dirs: prefer the parent project's src/ and tests/ if this devkit
# is checked out as a submodule under ../../src; otherwise watch the
# devkit's own src/ tests/. The parent-project case is the common one
# (every downstream uses src/ tests/ at repo root).
PARENT_ROOT="$(cd "$ROOT_DIR/.." && pwd)"
WATCH_DIRS=()
if [ -d "$PARENT_ROOT/src" ] && [ -d "$PARENT_ROOT/tests" ]; then
    WATCH_DIRS+=("$PARENT_ROOT/src" "$PARENT_ROOT/tests")
    MAKE_DIR="$PARENT_ROOT"
elif [ -d "$ROOT_DIR/src" ] && [ -d "$ROOT_DIR/tests" ]; then
    WATCH_DIRS+=("$ROOT_DIR/src" "$ROOT_DIR/tests")
    MAKE_DIR="$ROOT_DIR"
else
    echo "error: no src/ + tests/ found in $PARENT_ROOT or $ROOT_DIR" >&2
    exit 1
fi

# Pick a watcher.
WATCHER=""
if command -v fswatch >/dev/null 2>&1; then
    WATCHER="fswatch"
elif command -v inotifywait >/dev/null 2>&1; then
    WATCHER="inotifywait"
else
    cat >&2 <<EOF
error: neither fswatch nor inotifywait is installed.
  macOS:  brew install fswatch
  Linux:  apt install inotify-tools  (or your distro equivalent)
EOF
    exit 1
fi

run_test() {
    local trigger="$1"
    echo ""
    echo "==> save detected: ${trigger#$MAKE_DIR/}"
    # Build the make invocation. Pass T/WARM/PUSH_TIMEOUT through the
    # environment so empty values don't override user overrides.
    local env_args=()
    [ -n "$T_FILTER" ]        && env_args+=("T=$T_FILTER")
    [ -n "$WARM_FLAG" ]       && env_args+=("WARM=$WARM_FLAG")
    [ -n "$PUSH_TIMEOUT_VAL" ] && env_args+=("PUSH_TIMEOUT=$PUSH_TIMEOUT_VAL")
    ( cd "$MAKE_DIR" && make test-temple "${env_args[@]}" ) || \
        echo "==> test-temple exited non-zero (re-arming anyway)"
}

echo "==> make watch — watcher: $WATCHER"
echo "==> watching: ${WATCH_DIRS[*]}"
echo "==> filter: T='${T_FILTER}'  WARM='${WARM_FLAG}'  PUSH_TIMEOUT='${PUSH_TIMEOUT_VAL}'"
echo "==> Ctrl-C to stop"

# Trap Ctrl-C cleanly.
trap 'echo ""; echo "==> watch stopped"; exit 0' INT TERM

# Common debounce: after each event, sleep briefly to coalesce save
# bursts (atomic renames, formatter follow-ups, etc.).
DEBOUNCE_MS=500

if [ "$WATCHER" = "fswatch" ]; then
    # -o = batch events, prints a numeric count per batch. We ignore
    # the count and read latest path via --format. Use --event flags
    # for write-only (skip metadata/inode noise).
    fswatch \
        --event Created --event Updated --event Renamed \
        --latency 0.3 \
        --format '%p' \
        --include '\.(HC|HH|ZC)$' \
        --exclude '.*' \
        "${WATCH_DIRS[@]}" \
    | while IFS= read -r path; do
        run_test "$path"
        # Drain any stacked events that arrived during the test run.
        sleep "$(awk "BEGIN{print $DEBOUNCE_MS/1000}")"
    done
else
    # inotifywait: -m monitor, -r recursive, -e modify,close_write.
    inotifywait -m -r -q \
        -e close_write -e moved_to \
        --format '%w%f' \
        "${WATCH_DIRS[@]}" \
    | while IFS= read -r path; do
        case "$path" in
            *.HC|*.HH|*.ZC) ;;
            *) continue ;;
        esac
        run_test "$path"
        sleep "$(awk "BEGIN{print $DEBOUNCE_MS/1000}")"
    done
fi
