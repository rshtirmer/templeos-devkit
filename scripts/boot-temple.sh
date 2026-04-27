#!/usr/bin/env bash
# Boot original TempleOS (Terry's 2017 Distro) in QEMU. Mirrors boot.sh
# but uses the i440FX (`pc`) machine + plain IDE — TempleOS has no
# AHCI driver, so q35 won't see disks.
#
# Modes:
#   scripts/boot-temple.sh install   CD + disk, walk the installer once.
#   scripts/boot-temple.sh disk      Boot installed disk only (no daemon).
#   scripts/boot-temple.sh dev       Boot disk + COM2 daemon socket.
#                                    No shuttle — the dev loop pushes
#                                    HolyC over COM2 (see scripts/temple-run.py).
#
# COM1 -> file:build/serial-temple.log (host-readable trace)
# COM2 -> chardev socket build/com2-temple.sock (live-REPL daemon)

set -euo pipefail

REPO="$(cd "$(dirname "$0")/.." && pwd)"
ISO="$REPO/vendor/templeos/templeos.iso"
DISK="$REPO/vendor/templeos/disk.qcow2"
LOG="$REPO/build/serial-temple.log"
MON="$REPO/build/qemu-temple.sock"
COM2="$REPO/build/com2-temple.sock"

MODE="${1:-disk}"
AUTO_BOOT="${AUTO_BOOT:-1}"  # set 0 to skip the auto-press of '1<enter>' at the bootloader menu
ZOOM="${ZOOM:-1.5}"          # window zoom vs 1024x768 framebuffer; 0 disables, -1 auto-fits ~70% of main screen

mkdir -p "$REPO/build"
: > "$LOG"
rm -f "$MON" "$COM2"

# Auto-dismiss the TempleOS bootloader menu ("0. Old Boot Record /
# 1. Drive C / 2. Drive D / Selection:_") for autonomous runs. We
# fork a subshell that waits for SeaBIOS + the TempleOS loader to be
# at the prompt, then sendkey "1<enter>" via the QEMU monitor socket.
# Mirrors the ZealOS path's `zctl up` auto-boot. Disable with
# AUTO_BOOT=0. Only meaningful for `disk` and `dev` modes — the
# installer (mode=install) boots from the CD with a different
# sequence and shouldn't be auto-driven.
if [ "$AUTO_BOOT" = "1" ] && { [ "$MODE" = "disk" ] || [ "$MODE" = "dev" ]; }; then
  (
    sleep 4
    QEMU_SOCK="$MON" "$REPO/scripts/send.py" 1 --enter --delay 0.05 \
      >/dev/null 2>&1 || true
  ) &
fi

# Resize the QEMU cocoa window post-launch. QEMU has no CLI flag for
# initial window size, so we drive it via AppleScript once the
# NSWindow is up. Mirrors zctl --zoom for the ZealOS path. PID is the
# current shell's PID — we `exec qemu` below, so $$ becomes qemu's
# PID. macOS-only; silently no-ops elsewhere.
#
# First run on a given Mac: Terminal must be granted Accessibility
# permission (System Settings → Privacy & Security → Accessibility),
# otherwise the resize silently no-ops. The VM still works at default
# size.
if [ "$ZOOM" != "0" ] && [ "$(uname)" = "Darwin" ] && command -v osascript >/dev/null 2>&1; then
  QEMU_PID=$$
  (
    # Wait briefly for the NSWindow to come up.
    deadline=$(($(date +%s) + 5))
    while [ "$(date +%s)" -lt "$deadline" ]; do
      kill -0 "$QEMU_PID" 2>/dev/null || exit 0
      n=$(osascript -e "tell application \"System Events\" to count of windows of (first process whose unix id is $QEMU_PID)" 2>/dev/null || echo 0)
      [ "$n" -ge 1 ] 2>/dev/null && break
      sleep 0.3
    done
    osascript <<APPLESCRIPT >/dev/null 2>&1 || true
tell application "System Events"
  set qProc to first process whose unix id is $QEMU_PID
  if (count of windows of qProc) = 0 then return
  set qWin to window 1 of qProc
  tell application "Finder" to set screenBounds to bounds of window of desktop
  set screenW to (item 3 of screenBounds) - (item 1 of screenBounds)
  set z to ($ZOOM as real)
  if z < 0 then
    set newW to (screenW * 0.7) as integer
    set newH to ((screenW * 0.7) * 768 / 1024) as integer
  else
    set newW to (1024 * z) as integer
    set newH to (768 * z) as integer
  end if
  set position of qWin to {40, 40}
  set size of qWin to {newW, newH}
end tell
APPLESCRIPT
  ) &
fi

ARGS=(
  -machine pc
  -rtc base=localtime
  -m 512M
  -vga std
  -display cocoa,zoom-to-fit=on,full-grab=off
  -serial "file:$LOG"
  -monitor "unix:$MON,server,nowait"
)

case "$MODE" in
  install)
    [ -f "$ISO" ]  || { echo "error: $ISO missing — run 'make setup-temple'." >&2; exit 1; }
    [ -f "$DISK" ] || { echo "error: $DISK missing — run 'make disk-temple'." >&2; exit 1; }
    ARGS+=(
      -boot d
      -drive file="$DISK",if=ide,index=0,format=qcow2
      -drive file="$ISO",if=ide,index=2,media=cdrom,format=raw
    )
    ;;
  disk)
    [ -f "$DISK" ] || { echo "error: $DISK missing — install first." >&2; exit 1; }
    ARGS+=(
      -boot c
      -drive file="$DISK",if=ide,index=0,format=qcow2
    )
    ;;
  dev)
    # Just disk + COM2 socket. No shuttle / payload disk — the host
    # pushes the entire test battery over COM2 via temple-run.py.
    [ -f "$DISK" ] || { echo "error: $DISK missing." >&2; exit 1; }
    ARGS+=(
      -boot c
      -drive file="$DISK",if=ide,index=0,format=qcow2
      -chardev "socket,id=com2,path=$COM2,server=on,wait=off"
      -serial chardev:com2
    )
    ;;
  *)
    echo "error: unknown mode '$MODE'. Use install | disk | dev." >&2
    exit 1
    ;;
esac

echo "==> mode: $MODE  (TempleOS / pc machine / IDE)"
echo "==> serial: $LOG"
echo "==> monitor: $MON"
exec qemu-system-x86_64 "${ARGS[@]}"
