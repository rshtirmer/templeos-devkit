#!/usr/bin/env python3
"""test-fast — push HolyC source to the running daemon, parse verdict.

Assumes `make repl` is running with the daemon listening on
build/com2.sock. Concatenates Assert.ZC + all src/*.ZC + selected
tests/T_*.ZC, wraps with a unique done-marker, sends in one
EOT-terminated push, polls serial.log for the marker, parses verdict.

CAVEAT (current state): on Apple Silicon QEMU TCG, HolyC compilation
of a 1KB+ payload takes ~30-40s, comparable to a cold boot. So this
isn't yet a speedup vs `make test` for full test runs. It's still
useful for:
  - Pushing a single tiny experiment (T= filter to one test).
  - Watching iterative debug output without the boot dance.
Optimization angles (untaken): pre-load Assert.ZC into daemon scope
once at startup so subsequent pushes only carry the SUT delta.
"""
import argparse
import os
import secrets
import socket
import sys
import time
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
SOCK = REPO / "build" / "com2.sock"
LOG = REPO / "build" / "serial.log"

BOOT_ONLY = {"Setup.ZC", "Daemon.ZC"}
# A unique per-run nonce in the done marker prevents matching the tail of
# a previous push that's still draining through the UART.
DONE_NONCE = secrets.token_hex(6)
DONE_MARKER = f"TEST_FAST_DONE:{DONE_NONCE}"


def build_payload(t_filter: str) -> bytes:
    """Build the source blob the daemon ExeFile()s.

    Critical: each task in ZealOS has its own copy of `comm_ports` (it's
    declared in `#include "::/Doc/Comm";` which adds globals to the
    including task's heap). Assert.ZC's PASS / FAIL / ASSERT_EQ were
    compiled in sys_task's heap during cold boot — their CommPrint
    writes go to sys_task's comm_ports[1].TX_fifo, which the daemon
    invalidated when it called CommInit8n1 on its own heap. So we must
    include Assert.ZC in EVERY push so its functions get redefined in
    the daemon's scope, where they reach the FIFO that's actually
    drained to UART. HolyC tolerates the redefinitions.
    """
    parts = []
    # Test framework — re-declares PASS/FAIL/ASSERT_EQ + counters in
    # daemon scope. Resets _pass_count/_fail_count to zero each push.
    parts.append((REPO / "tests" / "Assert.ZC").read_text())
    # Source under test.
    for f in sorted((REPO / "src").glob("*.ZC")):
        if f.name in BOOT_ONLY:
            continue
        parts.append(f.read_text())
    # Selected tests.
    matched = 0
    for f in sorted((REPO / "tests").glob("T_*.ZC")):
        if t_filter and t_filter not in f.name:
            continue
        parts.append(f.read_text())
        matched += 1
    if matched == 0:
        sys.exit(f"no tests matched T={t_filter!r}")
    parts.append("TEST_SUMMARY;")
    parts.append(f'CommPrint(1, "{DONE_MARKER}\\n");')
    return ("\n".join(parts) + "\n").encode("utf-8")


def push(payload: bytes) -> None:
    if not SOCK.exists():
        sys.exit(f"error: {SOCK} not found — start 'make repl' first")
    s = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    s.connect(str(SOCK))
    s.sendall(payload + b"\x04")
    s.close()


def wait_for_done(start_offset: int, timeout_s: float) -> tuple[str, bool]:
    """Poll serial.log; return (new output, saw_done_marker)."""
    deadline = time.time() + timeout_s
    while time.time() < deadline:
        with open(LOG, "rb") as f:
            f.seek(start_offset)
            new = f.read().decode("utf-8", errors="replace")
        if DONE_MARKER in new:
            return new, True
        time.sleep(0.05)
    with open(LOG, "rb") as f:
        f.seek(start_offset)
        new = f.read().decode("utf-8", errors="replace")
    return new, False


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--timeout", type=float, default=30.0)
    args = ap.parse_args()

    t_filter = os.environ.get("T", "")

    if not LOG.exists():
        sys.exit(f"error: {LOG} missing — has the VM written serial.log?")
    start = LOG.stat().st_size

    payload = build_payload(t_filter)
    t0 = time.time()
    push(payload)
    new, ok = wait_for_done(start, args.timeout)
    dt_ms = (time.time() - t0) * 1000

    print("==== test run summary ====")
    for line in new.splitlines():
        if any(line.startswith(p) for p in (
                "TEST_PASS:", "TEST_FAIL:", "TEST_PANIC:", "TEST_SUMMARY:")):
            print(line)
    print("==========================")

    if not ok:
        print(f"TIMEOUT — no {DONE_MARKER} within {args.timeout}s ({dt_ms:.0f}ms elapsed)")
        # Show tail of new output for context.
        tail = "\n".join(new.splitlines()[-20:])
        if tail:
            print("--- tail of new serial output ---")
            print(tail)
        return 1

    fails = sum(1 for ln in new.splitlines() if ln.startswith("TEST_FAIL:"))
    panics = sum(1 for ln in new.splitlines() if ln.startswith("TEST_PANIC:"))
    passes = sum(1 for ln in new.splitlines() if ln.startswith("TEST_PASS:"))
    if fails or panics:
        print(f"FAILED ({fails} fail, {panics} panic) — {dt_ms:.0f}ms")
        return 1
    print(f"OK ({passes} test(s) passed) — {dt_ms:.0f}ms")
    return 0


if __name__ == "__main__":
    sys.exit(main())
