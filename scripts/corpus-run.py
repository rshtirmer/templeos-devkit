#!/usr/bin/env python3
"""corpus-run.py — drive the TempleOS daemon to validate corpus snippets.

Modes:
  bootstrap    : run BOOTSTRAP_CMDS + upgrade to D2 (only once per VM boot)
  validate DIR : push every *.hc file in DIR through the daemon, capture
                 COMPILE_OK / COMPILE_FAIL + error text, write .actual
                 sibling files. Does NOT decide pass/fail — that's the
                 caller's job. The .expected files are authored by hand.
"""
import argparse, os, socket, subprocess, sys, time
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
LOG = REPO / "build" / "serial-temple.log"
COM2 = REPO / "build" / "com2-temple.sock"
MON  = REPO / "build" / "qemu-temple.sock"
SEND = REPO / "scripts" / "send.py"

# Reuse temple-run.py's constants by exec'ing it as a module
import importlib.util
spec = importlib.util.spec_from_file_location("temple_run",
    REPO / "scripts" / "temple-run.py")
tr = importlib.util.module_from_spec(spec)
spec.loader.exec_module(tr)


def push_chunk_and_capture(payload: bytes, timeout=15.0):
    since = tr.log_size()
    tr.push_chunk(payload)
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        try:
            data = LOG.read_bytes()[since:]
        except FileNotFoundError:
            data = b""
        if b"D_DONE" in data:
            return data.decode(errors="replace")
        time.sleep(0.05)
    return LOG.read_bytes()[since:].decode(errors="replace")


def parse_result(captured: str):
    """Return (status, error_text). status in {"OK","FAIL","TIMEOUT"}."""
    if "COMPILE_OK" in captured:
        return "OK", ""
    if "COMPILE_FAIL" in captured:
        # Extract between markers if present
        beg = captured.find("COMPILE_ERR_BEGIN")
        end = captured.find("COMPILE_ERR_END")
        if beg != -1 and end != -1:
            txt = captured[beg+len("COMPILE_ERR_BEGIN"):end].strip()
            return "FAIL", txt
        return "FAIL", "(no error frame captured)"
    return "TIMEOUT", captured.strip()


def cmd_bootstrap():
    if not COM2.exists() or not MON.exists():
        sys.exit("VM not up — boot-temple.sh dev first")
    print(f"==> typing bootstrap ({len(tr.BOOTSTRAP_CMDS)} commands)")
    since = tr.log_size()
    for i, c in enumerate(tr.BOOTSTRAP_CMDS, 1):
        print(f"   cmd {i}/{len(tr.BOOTSTRAP_CMDS)} ({len(c)} chars)")
        tr.sendkey(c, enter=True, delay=0.05)
        time.sleep(1.0)
    ok, _ = tr.wait_for("D_OK", since=since, timeout=30.0)
    if not ok:
        sys.exit("D_OK not seen")
    print("==> stage-1 daemon up")
    since = tr.log_size()
    tr.push_chunk(tr.DAEMON_V2_SOURCE.encode())
    ok, captured = tr.wait_for("D_DONE", since=since, timeout=20.0)
    if not ok:
        sys.exit(f"stage-2 source not D_DONE\n{captured}")
    since = tr.log_size()
    tr.push_chunk(b"_D_exit=TRUE;")
    ok, _ = tr.wait_for("D_EXIT", since=since, timeout=10.0)
    if not ok:
        sys.exit("D_EXIT not seen")
    tr.sendkey("_D_exit=FALSE;D2();", enter=True, delay=0.05)
    ok, _ = tr.wait_for("D2_OK", since=since, timeout=10.0)
    if not ok:
        sys.exit("D2_OK not seen")
    print("==> stage-2 daemon up (D2_OK)")


def cmd_validate(dirpath, write_expected=False):
    d = Path(dirpath)
    files = sorted(d.glob("*.hc"))
    print(f"==> validating {len(files)} snippet(s) in {d}")
    results = []
    for f in files:
        body = f.read_bytes()
        cap = push_chunk_and_capture(body, timeout=20.0)
        status, errtxt = parse_result(cap)
        actual = f.with_suffix(".hc.actual")
        if status == "OK":
            actual.write_text("OK\n")
        else:
            actual.write_text(errtxt + ("\n" if not errtxt.endswith("\n") else ""))
        if write_expected:
            exp = f.with_suffix(".hc.expected")
            if status == "OK":
                exp.write_text("OK\n")
            else:
                exp.write_text(errtxt + ("\n" if not errtxt.endswith("\n") else ""))
        marker = {"OK":"ok","FAIL":"FAIL","TIMEOUT":"TIMEOUT"}[status]
        line = f"  [{marker}] {f.name}"
        if status == "FAIL" and errtxt:
            line += f"  -- {errtxt.splitlines()[0][:80]}"
        print(line)
        results.append((f.name, status, errtxt))
    return results


def main():
    ap = argparse.ArgumentParser()
    sub = ap.add_subparsers(dest="cmd", required=True)
    sub.add_parser("bootstrap")
    pv = sub.add_parser("validate")
    pv.add_argument("dir")
    pv.add_argument("--write-expected", action="store_true",
                    help="also write .expected files derived from VM output")
    args = ap.parse_args()
    if args.cmd == "bootstrap":
        cmd_bootstrap()
    elif args.cmd == "validate":
        cmd_validate(args.dir, write_expected=args.write_expected)


if __name__ == "__main__":
    main()
