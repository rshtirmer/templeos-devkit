#!/usr/bin/env bash
# One-time post-install: sendkey `#include "B:/Setup.ZC";` so Setup.ZC
# writes ~/MakeHome.ZC on the installed disk. From then on, every boot
# auto-runs B:/Boot.ZC.
#
# Pre-req: make dev is running and is at the home> prompt.

set -euo pipefail

REPO="$(cd "$(dirname "$0")/.." && pwd)"

if [ ! -S "$REPO/build/qemu.sock" ]; then
  echo "error: VM not running. Start 'make dev' first." >&2
  exit 1
fi

echo "==> mounting shuttle (AHCI port 2) as E: and sourcing Setup.ZC"
# ZealOS does not auto-mount the shuttle on first boot — we have to
# attach it explicitly. This is the same block Setup.ZC writes into
# ~/MakeHome.ZC for subsequent boots. Sleep gives AHCI a moment before
# we read the FAT.
"$REPO/scripts/send.py" 'CBlkDev *bd; bd = BlkDevNextFreeSlot('"'"'E'"'"', 2); AHCIPortInit(bd, &blkdev.ahci_hba->ports[2], 2); BlkDevAdd(bd, 1, 0, 1); Sleep(2000);' --enter --delay 0.05
sleep 3
"$REPO/scripts/send.py" '#include "E:/Setup.ZC";' --enter --delay 0.05

# Give the FileWrite a beat, then snap so we can verify SETUP_OK.
sleep 3
"$REPO/scripts/screenshot.sh" >/dev/null
echo "==> screenshot at build/screen.png"
echo "==> tail of serial.log:"
tail -5 "$REPO/build/serial.log" || true
