#!/usr/bin/env python3
"""temple-run.py — push the test battery into a running original-TempleOS VM
via a tiny COM2-fed in-memory JIT daemon.

Why this exists: original TempleOS (2017 distro) can't share files with
the host the way ZealOS can. FAT32 reads from secondary IDE slots
return aliased C: contents; ISO9660 mount errors `File System Not
Supported`; `ISO1FileRead` exists but isn't `public`. So the ZealOS
shuttle pattern (mount FAT image, `#include "E:/..."`) doesn't carry
over. We sidestep all of that by pushing source bytes directly over a
COM2 chardev socket and JIT-compiling each chunk with `ExePutS`.

Pipeline:
  1. `bash scripts/boot-temple.sh dev` boots the VM with COM1 -> file
     and COM2 -> chardev socket. User picks Drive C in the bootloader
     and answers 'n' to the Once.HC tour.
  2. We `sendkey` a small bootstrap into adam_task: include Comm.HC,
     allocate a 128 KB RX buffer, init COM2 at 115200 baud, define
     a receive function `D()` and call it. `D()` blocks adam in a
     read-buffer-then-ExePutS loop and prints `D_OK` to COM1.
  3. For each `.ZC` file: send raw bytes over COM2, terminate with
     EOT (0x04). `D()` calls `ExePutS(buf)` to JIT-compile and run
     the source. We watch COM1 for `D_DONE` to know it finished.
     Top-level statements (PASS / ASSERT_EQ in test files) execute
     during the push; output streams back over COM1.
  4. Final push is `TEST_SUMMARY; CommPrint(1, "TEST_RUN_END\\n");`.
"""

import argparse
import os
import socket
import sys
import time
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
LOG = REPO / "build" / "serial-temple.log"
COM2 = REPO / "build" / "com2-temple.sock"
MON = REPO / "build" / "qemu-temple.sock"
SEND = REPO / "scripts" / "send.py"

# Bootstrap split into small commands. The TempleOS REPL appears to
# have a per-Enter line buffer somewhere around 256 chars; sending one
# 400+ char "monster line" caused parser truncation. Splitting into
# four ~150-char commands sidesteps the limit.
#
# Globals (Db, Di, Dc) are shared between the function and any future
# poke. RX FIFO is enlarged to 128 KB so a 34 KB OrbitalUI.ZC pushed
# in one chunk doesn't overflow the default 256-byte FIFO.
BOOTSTRAP_CMDS = [
    # Pull in Comm.HC — comm_ports, CommInit8n1, CommPrint, FifoU8*.
    # In original TempleOS these aren't auto-loaded. Use no-extension
    # form so ExtDft appends `.HC.Z` (the compressed on-disk form);
    # writing `Comm.HC` literally would look for an uncompressed file
    # that doesn't exist on the install.
    '#include "::/Doc/Comm";',
    'U8 *Db=MAlloc(131072);I64 Di=0;U8 Dc;',
    'CommInit8n1(2,115200);CommInit8n1(1,115200);'
    'FifoU8Del(comm_ports[2].RX_fifo);'
    'comm_ports[2].RX_fifo=FifoU8New(131072);',
    # Receive function — runs in adam_task directly (no Spawn). Why not
    # Spawn: spawned tasks get a fresh JIT compile context that doesn't
    # see adam's #include'd symbols (CommPrint, FifoU8Rem, comm_ports)
    # via the hash_table->next chain in practice. Running in adam means
    # adam's REPL blocks forever, but we don't need the REPL once the
    # daemon is up — every push goes via COM2.
    #
    # Uses ExePutS — JIT-compiles the buffer in memory, no disk write/
    # read. FileWrite+ExeFile (earlier approach) churned the RedSea FS
    # hard enough to panic Adam after ~10 chunks.
    #
    # NOTE: original TempleOS uses FifoU8Rem (ZealOS renamed it).
    'U0 D(){'
    'CommPrint(1,"D_OK\\n");'
    'while(1){if(FifoU8Rem(comm_ports[2].RX_fifo,&Dc)){'
    'if(Dc==4){Db[Di]=0;ExePutS(Db);'
    'CommPrint(1,"D_DONE\\n");Di=0;}'
    'else if(Di<131071){Db[Di++]=Dc;}}else Sleep(10);}}',
    # Call D directly. Adam's REPL blocks here forever — that's fine,
    # everything from now on goes via COM2.
    'D();',
]


def log_size():
    try:
        return LOG.stat().st_size
    except FileNotFoundError:
        return 0


def wait_for(token, *, since=0, timeout=30.0):
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        try:
            data = LOG.read_bytes()[since:]
        except FileNotFoundError:
            data = b""
        if token.encode() in data:
            return True, data.decode(errors="replace")
        time.sleep(0.1)
    return False, LOG.read_bytes()[since:].decode(errors="replace") if LOG.exists() else ""


def sendkey(text, *, enter=False, delay=0.1):
    env = {**os.environ, "QEMU_SOCK": str(MON)}
    cmd = [str(SEND), text, "--delay", str(delay)]
    if enter:
        cmd.append("--enter")
    import subprocess
    subprocess.run(cmd, env=env, check=True)


def push_chunk(payload: bytes):
    """Send raw bytes to COM2, then EOT."""
    with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as s:
        s.connect(str(COM2))
        # Throttle so the guest UART driver can keep up. At 115200 baud
        # the guest can drain ~11.5 KB/s; 1024B chunks at 10ms ≈ 100 KB/s
        # offered, but the chardev socket buffers absorb the difference.
        # The real limit is the guest FIFO size (we enlarged it to 128 KB).
        for i in range(0, len(payload), 1024):
            s.sendall(payload[i:i+1024])
            time.sleep(0.01)
        s.sendall(b"\x04")


def _strip_shuttle_includes(body: bytes) -> bytes:
    """Drop `#include "E:/..."` lines — those are ZealOS shuttle paths
    that don't resolve on TempleOS. With ExePutS we push deps directly
    in order, so the includes are redundant anyway."""
    return b"\n".join(
        line for line in body.splitlines()
        if not line.lstrip().startswith(b'#include "E:/')
    )


def push_and_wait(payload: bytes, label: str, timeout: float = 60.0):
    since = log_size()
    push_chunk(payload)
    ok, captured = wait_for("D_DONE", since=since, timeout=timeout)
    sys.stdout.write(f"--- after pushing {label} ({len(payload)}B) ---\n")
    sys.stdout.write(captured)
    sys.stdout.write("\n")
    if not ok:
        print(f"!! D_DONE timeout for {label}", file=sys.stderr)
        return False
    return True


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--skip-bootstrap", action="store_true",
                    help="assume daemon already running (D_OK already in log)")
    ap.add_argument("--filter", default="",
                    help="substring filter for tests (T= equivalent)")
    ap.add_argument("--skip", default="",
                    help="comma-separated src filenames to skip "
                         "(default: Daemon.ZC,Setup.ZC — both ZealOS-only)")
    ap.add_argument("--order", default="",
                    help="comma-separated src push order if alphabetical "
                         "doesn't satisfy deps (e.g. Wavefunc.ZC,Orbital.ZC)")
    args = ap.parse_args()

    if not COM2.exists() or not MON.exists():
        sys.exit("error: VM not up — run 'bash scripts/boot-temple.sh dev' first")

    if not args.skip_bootstrap:
        print(f"==> typing bootstrap ({len(BOOTSTRAP_CMDS)} commands)")
        since = log_size()  # capture BEFORE typing — D_OK may land between cmds
        for i, cmd in enumerate(BOOTSTRAP_CMDS, 1):
            print(f"    cmd {i}/{len(BOOTSTRAP_CMDS)} ({len(cmd)} chars)")
            sendkey(cmd, enter=True, delay=0.05)
            time.sleep(1.0)  # let TempleOS parse before next line
        ok, _ = wait_for("D_OK", since=since, timeout=20.0)
        if not ok:
            sys.exit("error: D_OK never appeared after typing daemon. "
                     "Check screen-temple.png for parser errors.")
        print("==> daemon up (D_OK)")

    src_dir = REPO / "src"
    test_dir = REPO / "tests"
    # Default skip list: src files that depend on ZealOS-only APIs and
    # won't compile on stock TempleOS. Override with --skip if your fork
    # has more (or fewer).
    #   Daemon.ZC uses FifoU8Remove (ZealOS-renamed; TempleOS = FifoU8Rem)
    #   Setup.ZC  uses AHCIPortInit (TempleOS has no AHCI)
    skip = set(args.skip.split(",")) if args.skip else {"Daemon.ZC", "Setup.ZC"}
    # Push order: src/*.ZC alphabetically, then Assert.ZC, then T_*.ZC.
    # If a src file depends on another, alphabetical order may be wrong
    # — pass --order to override (comma-separated). We strip
    # `#include "E:/..."` lines from every file (those are ZealOS shuttle
    # paths) and rely on push order to satisfy dependencies.
    if args.order:
        src_files = [src_dir / n for n in args.order.split(",")
                     if n not in skip]
    else:
        src_files = [f for f in sorted(src_dir.glob("*.ZC"))
                     if f.name not in skip]
    src_files.append(test_dir / "Assert.ZC")
    test_files = [f for f in sorted(test_dir.glob("T_*.ZC"))
                  if not args.filter or args.filter in f.name]

    # Phase 1: source + Assert. Each is JIT-compiled; defs persist.
    print(f"==> phase 1: pushing {len(src_files)} source files")
    for f in src_files:
        if not push_and_wait(_strip_shuttle_includes(f.read_bytes()),
                             f.name, timeout=60.0):
            sys.exit(1)

    # Phase 2: TEST_RUN_BEGIN marker, then each test (top-level
    # PASS/ASSERT_EQ runs on push), then TEST_SUMMARY + TEST_RUN_END.
    print(f"==> phase 2: TEST_RUN_BEGIN + {len(test_files)} tests")
    push_and_wait(b'CommPrint(1,"TEST_RUN_BEGIN\\n");', "begin-marker", 10.0)
    for f in test_files:
        if not push_and_wait(_strip_shuttle_includes(f.read_bytes()),
                             f.name, timeout=60.0):
            sys.exit(1)
    boot = (
        b'TEST_SUMMARY;'
        b'CommPrint(1,"TEST_RUN_END\\n");'
    )

    print("==> finalizing — TEST_SUMMARY + TEST_RUN_END")
    since = log_size()
    push_chunk(boot)
    ok, captured = wait_for("TEST_RUN_END", since=since, timeout=180.0)
    sys.stdout.write(captured)
    if not ok:
        print("!! TEST_RUN_END never seen", file=sys.stderr)
        sys.exit(2)
    print("\n==> done")


if __name__ == "__main__":
    main()
