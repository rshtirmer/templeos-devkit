#!/usr/bin/env python3
"""Regression test for A2 — HolyC compile errors captured over COM1.

Requires a running stage-2 daemon (boot the VM with `make dev-temple`,
then run `python3 scripts/temple-run.py --filter __NEVERMATCH__` once
to bring stage-2 up; or just run after a normal `make test-temple`).

Asserts:
  - A known-good chunk emits COMPILE_OK and no COMPILE_FAIL.
  - A known-bad chunk emits COMPILE_ERR_BEGIN, COMPILE_FAIL, and the
    captured error text contains a recognizable lexer-error fragment.
"""
import socket
import sys
import time
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
LOG = REPO / "build" / "serial-temple.log"
COM2 = REPO / "build" / "com2-temple.sock"


def push(payload: bytes):
    with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as s:
        s.connect(str(COM2))
        s.sendall(payload)
        s.sendall(b"\x04")


def push_and_capture(payload: bytes, timeout: float = 15.0) -> str:
    since = LOG.stat().st_size
    push(payload)
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        data = LOG.read_bytes()[since:]
        if b"D_DONE" in data:
            return data.decode(errors="replace")
        time.sleep(0.1)
    raise TimeoutError(f"D_DONE not seen for {payload!r}")


def main() -> int:
    if not LOG.exists() or not COM2.exists():
        sys.exit("error: VM not running — boot via boot-temple.sh dev "
                 "and bring up the daemon with temple-run.py first.")

    # Known good.
    out = push_and_capture(b"F64 x=1.0;")
    assert "COMPILE_OK" in out, f"good chunk missing COMPILE_OK: {out!r}"
    assert "COMPILE_FAIL" not in out, f"good chunk leaked FAIL: {out!r}"
    print("ok: COMPILE_OK on F64 x=1.0;")

    # Known bad.
    out = push_and_capture(b"zzz garbage @!! ;")
    assert "COMPILE_ERR_BEGIN" in out, f"bad chunk missing BEGIN: {out!r}"
    assert "COMPILE_ERR_END" in out, f"bad chunk missing END: {out!r}"
    assert "COMPILE_FAIL" in out, f"bad chunk missing FAIL: {out!r}"
    # Error text should mention the offending identifier.
    assert "garbage" in out, f"bad chunk lost error text: {out!r}"
    print("ok: COMPILE_FAIL on garbage chunk; error text captured")

    print("PASS")
    return 0


if __name__ == "__main__":
    sys.exit(main())
