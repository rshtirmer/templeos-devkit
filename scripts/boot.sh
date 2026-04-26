#!/usr/bin/env bash
# Boot ZealOS in QEMU. Three modes:
#
#   scripts/boot.sh cd        Boot from the live CD only. Use for poking.
#   scripts/boot.sh install   Boot CD + qcow2 disk attached. Use once to install.
#   scripts/boot.sh disk      Boot installed qcow2 only. Use to verify install.
#   scripts/boot.sh dev       Boot installed qcow2 + shuttle disk. Main dev loop.
#
# COM1 is piped to build/serial.log so HolyC `Print` output is visible
# from the host (when code calls CommPrint(1, ...)).
#
# A QEMU monitor socket is exposed at build/qemu.sock for keystroke
# injection (scripts/send.py) and screenshots (scripts/screenshot.sh).

set -euo pipefail

REPO="$(cd "$(dirname "$0")/.." && pwd)"
ISO="$REPO/vendor/zealos/zealos.iso"
DISK="$REPO/vendor/zealos/disk.qcow2"
SHUTTLE="$REPO/build/shuttle.img"
LOG="$REPO/build/serial.log"
MON="$REPO/build/qemu.sock"
COM2="$REPO/build/com2.sock"

MODE="${1:-cd}"

mkdir -p "$REPO/build"
: > "$LOG"
rm -f "$MON" "$COM2"

ARGS=(
  # Wiki-recommended config: q35 chipset, RTC localtime, 1G RAM.
  -machine q35
  -rtc base=localtime
  -m 1G
  -vga std
  -display cocoa,zoom-to-fit=on,full-grab=off
  # COM1 = file backend (output capture, what `make test` greps).
  -serial "file:$LOG"
  -monitor "unix:$MON,server,nowait"
  -device ahci,id=ahci
)

case "$MODE" in
  cd)
    [ -f "$ISO" ] || { echo "error: $ISO not found. Run 'make setup'." >&2; exit 1; }
    ARGS+=(
      -boot d
      -drive id=cd,file="$ISO",if=none,format=raw,media=cdrom
      -device ide-cd,bus=ahci.0,drive=cd
    )
    ;;
  install)
    [ -f "$ISO" ] || { echo "error: $ISO not found. Run 'make setup'." >&2; exit 1; }
    [ -f "$DISK" ] || { echo "error: $DISK not found. Run 'make disk'." >&2; exit 1; }
    ARGS+=(
      -boot d
      -drive id=cd,file="$ISO",if=none,format=raw,media=cdrom
      -device ide-cd,bus=ahci.0,drive=cd
      -drive id=hd,file="$DISK",if=none,format=qcow2
      -device ide-hd,bus=ahci.1,drive=hd
    )
    ;;
  disk)
    # Match the install drive layout: CD on ahci.0, disk on ahci.1.
    # The installed kernel hardcoded the mount info expecting this.
    [ -f "$DISK" ] || { echo "error: $DISK not found. Run 'make install' first." >&2; exit 1; }
    [ -f "$ISO" ] || { echo "error: $ISO not found. Run 'make setup'." >&2; exit 1; }
    ARGS+=(
      -boot c
      -drive id=cd,file="$ISO",if=none,format=raw,media=cdrom
      -device ide-cd,bus=ahci.0,drive=cd
      -drive id=hd,file="$DISK",if=none,format=qcow2
      -device ide-hd,bus=ahci.1,drive=hd
    )
    ;;
  dev)
    # CD on ahci.0 (kernel expects it), disk on ahci.1, shuttle on ahci.2.
    # COM1 = file (TX out, what `make test` greps).
    # COM2 = chardev socket (RX in, for the live-REPL daemon — see
    # src/Daemon.ZC). server=on,wait=off so the VM boots without a
    # connected client; nc -U build/com2.sock connects on demand.
    [ -f "$DISK" ] || { echo "error: $DISK not found. Run 'make install' first." >&2; exit 1; }
    [ -f "$ISO" ] || { echo "error: $ISO not found. Run 'make setup'." >&2; exit 1; }
    [ -f "$SHUTTLE" ] || { echo "error: $SHUTTLE not found. Run 'make shuttle'." >&2; exit 1; }
    ARGS+=(
      -boot c
      -drive id=cd,file="$ISO",if=none,format=raw,media=cdrom
      -device ide-cd,bus=ahci.0,drive=cd
      -drive id=hd,file="$DISK",if=none,format=qcow2
      -device ide-hd,bus=ahci.1,drive=hd
      -drive id=sh,file="$SHUTTLE",if=none,format=raw
      -device ide-hd,bus=ahci.2,drive=sh
      -chardev "socket,id=com2,path=$COM2,server=on,wait=off"
      -serial chardev:com2
    )
    ;;
  *)
    echo "error: unknown mode '$MODE'. Use cd, install, disk, or dev." >&2
    exit 1
    ;;
esac

echo "==> mode: $MODE"
echo "==> serial output: $LOG"
echo "==> monitor socket: $MON"
exec qemu-system-x86_64 "${ARGS[@]}"
