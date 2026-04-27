#!/usr/bin/env bash
# Snap a screenshot of the running QEMU VM via the monitor socket.
# Outputs build/screen.png (converted from QEMU's PPM with macOS `sips`).

set -euo pipefail

REPO="$(cd "$(dirname "$0")/.." && pwd)"
SOCK="${QEMU_SOCK:-$REPO/build/qemu.sock}"
PPM="${SCREEN_PPM:-$REPO/build/screen.ppm}"
PNG="${SCREEN_PNG:-$REPO/build/screen.png}"

if [ ! -S "$SOCK" ]; then
  echo "error: $SOCK not found — is QEMU running with -monitor?" >&2
  exit 1
fi

rm -f "$PPM" "$PNG"

# QEMU monitor: screendump <path>
{ echo "screendump $PPM"; sleep 0.5; } | nc -U "$SOCK" >/dev/null 2>&1 || true

# Wait briefly for the file to appear.
for _ in $(seq 1 20); do
  [ -f "$PPM" ] && break
  sleep 0.1
done

if [ ! -f "$PPM" ]; then
  echo "error: screenshot file did not appear" >&2
  exit 1
fi

sips -s format png "$PPM" --out "$PNG" >/dev/null
rm -f "$PPM"
echo "$PNG"
