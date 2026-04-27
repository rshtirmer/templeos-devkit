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

mkdir -p "$REPO/build"
: > "$LOG"
rm -f "$MON" "$COM2"

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
