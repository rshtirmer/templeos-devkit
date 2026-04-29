#!/usr/bin/env bash
# screenshot.sh — snap a PNG of the running QEMU VM via its monitor socket.
#
# Multi-VM aware: probes a list of candidate monitor sockets and uses the
# first one that exists. Order:
#   1. $QEMU_SOCK if set (explicit override)
#   2. build/qemu-temple.sock  (TempleOS VM, current default)
#   3. build/qemu.sock         (legacy ZealOS VM)
#
# Output path defaults to build/screen-<vm>-<timestamp>.png so repeated
# captures never clobber each other. Honor $SCREEN_PNG to force a path.
#
# On success prints the PNG path on stdout. On failure (no live sock,
# screendump never produced a file) exits 1 with a clear message.
#
# Dependencies: bash, nc, sips (macOS default). No Python.

set -euo pipefail

REPO="$(cd "$(dirname "$0")/.." && pwd)"
BUILD="$REPO/build"
mkdir -p "$BUILD"

# Build the candidate list. Preserve order; explicit override wins.
CANDIDATES=()
if [ -n "${QEMU_SOCK:-}" ]; then
  CANDIDATES+=("$QEMU_SOCK")
fi
CANDIDATES+=("$BUILD/qemu-temple.sock" "$BUILD/qemu.sock")

SOCK=""
for c in "${CANDIDATES[@]}"; do
  if [ -S "$c" ]; then
    SOCK="$c"
    break
  fi
done

if [ -z "$SOCK" ]; then
  echo "error: no QEMU monitor socket found at:" >&2
  for c in "${CANDIDATES[@]}"; do
    echo "  - $c" >&2
  done
  echo "is QEMU running? (try: make dev-temple)" >&2
  exit 1
fi

# Derive a VM tag from the sock filename for the default PNG name.
SOCK_BASE="$(basename "$SOCK" .sock)"
case "$SOCK_BASE" in
  qemu-temple) VM_TAG="temple" ;;
  qemu)        VM_TAG="zealos" ;;
  *)           VM_TAG="$SOCK_BASE" ;;
esac

TS="$(date +%Y%m%d-%H%M%S)"
PPM="${SCREEN_PPM:-$BUILD/screen-$VM_TAG-$TS.ppm}"
PNG="${SCREEN_PNG:-$BUILD/screen-$VM_TAG-$TS.png}"

echo "screenshot: using monitor sock $SOCK" >&2

rm -f "$PPM"

# QEMU monitor: screendump <path>
{ echo "screendump $PPM"; sleep 0.5; } | nc -U "$SOCK" >/dev/null 2>&1 || true

# Wait briefly for the file to appear.
for _ in $(seq 1 20); do
  [ -f "$PPM" ] && break
  sleep 0.1
done

if [ ! -f "$PPM" ]; then
  echo "error: screendump produced no file at $PPM" >&2
  echo "  sock=$SOCK — is the VM responsive?" >&2
  exit 1
fi

sips -s format png "$PPM" --out "$PNG" >/dev/null
rm -f "$PPM"
echo "$PNG"
