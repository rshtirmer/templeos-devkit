#!/usr/bin/env bash
# zpush — send a HolyC source file (or stdin) to the running ZealOS REPL
# daemon via COM2 (Unix socket). Appends EOT (0x04) to mark
# end-of-command; the daemon writes the bytes to C:/Tmp/Cmd.HC and
# ExeFile()s it.
#
#   scripts/zpush.sh tests/T_Hello.ZC
#   echo 'Print("hi\n");' | scripts/zpush.sh
#
# Daemon trace (DAEMON_RECV, prints, DAEMON_DONE) lands in serial.log
# via COM1's file backend.
#
# Implementation: Python over UNIX socket. We previously used `nc -w 2`
# which forced a 2s idle wait after EOT — kills test-fast iteration.

set -euo pipefail

REPO="$(cd "$(dirname "$0")/.." && pwd)"
SOCK="$REPO/build/com2.sock"

[ -S "$SOCK" ] || { echo "error: $SOCK not found — is 'make repl' running?" >&2; exit 1; }

if [ $# -gt 0 ]; then
  exec python3 -c "
import socket, sys
s = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
s.connect('$SOCK')
with open('$1', 'rb') as f:
    s.sendall(f.read())
s.sendall(b'\\x04')
s.close()
"
else
  exec python3 -c "
import socket, sys
s = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
s.connect('$SOCK')
s.sendall(sys.stdin.buffer.read())
s.sendall(b'\\x04')
s.close()
"
fi
