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
    # `_D_exit` is the escape hatch: a pushed chunk doing `_D_exit=TRUE;`
    # breaks D() out of its receive loop, returning adam to its REPL so
    # follow-up code (e.g. an interactive viewer) can take over the
    # window. See --launch.
    'U8 *Db=MAlloc(131072);I64 Di=0;U8 Dc;Bool _D_exit=FALSE;',
    'CommInit8n1(2,115200);CommInit8n1(1,115200);'
    'FifoU8Del(comm_ports[2].RX_fifo);'
    'comm_ports[2].RX_fifo=FifoU8New(131072);',
    # Receive function — runs in adam_task directly (no Spawn). Why not
    # Spawn: spawned tasks' JIT compile context doesn't reliably resolve
    # adam's ExePutS'd symbols via the hash_table->next chain. Running
    # in adam means we own adam's window when D() exits.
    #
    # Uses ExePutS — JIT-compiles the buffer in memory, no disk write/
    # read. FileWrite+ExeFile (earlier approach) churned the RedSea FS
    # hard enough to panic Adam after ~10 chunks.
    #
    # NOTE: original TempleOS uses FifoU8Rem (ZealOS renamed it).
    'U0 D(){'
    'CommPrint(1,"D_OK\\n");'
    'while(!_D_exit){if(FifoU8Rem(comm_ports[2].RX_fifo,&Dc)){'
    'if(Dc==4){Db[Di]=0;ExePutS(Db);'
    'CommPrint(1,"D_DONE\\n");Di=0;}'
    'else if(Di<131071){Db[Di++]=Dc;}}else Sleep(10);}'
    'CommPrint(1,"D_EXIT\\n");}',
    # Call D directly — adam blocks in this loop until a pushed chunk
    # sets _D_exit=TRUE.
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


# Compat substitutions: ZealOS renamed a handful of TempleOS APIs and
# globals. We do plain text replacement on the host before push. Tried
# HolyC `#define`s first — works in tiny test chunks but mysteriously
# fails to expand inside larger source files. Substitution sidesteps
# the problem entirely.
COMPAT_SUBS = [
    # ZealOS name → TempleOS name.
    # `MessageGet` is the blocking message wait; TempleOS calls it `GetMsg`.
    (b"MessageGet",            b"GetMsg"),
    (b"MESSAGE_KEY_DOWN",      b"MSG_KEY_DOWN"),
    (b"MESSAGE_KEY_UP",        b"MSG_KEY_UP"),
    (b"WIG_USER_TASK_DEFAULT", b"WIG_USER_TASK_DFT"),
    # `mouse` is the ZealOS mouse global; TempleOS calls it `ms`. We
    # match common field-access patterns rather than the bare token to
    # avoid colliding with identifiers that happen to contain "mouse".
    (b"mouse.",                b"ms."),
    (b"mouse,",                b"ms,"),
    (b"mouse)",                b"ms)"),
    (b"mouse;",                b"ms;"),
    # `tS` is ZealOS's F64 seconds-since-boot global. TempleOS doesn't
    # have it; the standard substitute is `cnts.jiffies(F64) / 1000.0`.
    (b" tS ",                  b" (cnts.jiffies(F64)/1000.0) "),
    (b" tS;",                  b" (cnts.jiffies(F64)/1000.0);"),
    (b"=tS",                   b"=(cnts.jiffies(F64)/1000.0)"),
    (b"= tS",                  b"= (cnts.jiffies(F64)/1000.0)"),
    (b"-tS",                   b"-(cnts.jiffies(F64)/1000.0)"),
    (b"- tS",                  b"- (cnts.jiffies(F64)/1000.0)"),
]


def _prep(body: bytes) -> bytes:
    """Apply ZealOS-→TempleOS compat subs and strip shuttle includes."""
    body = _strip_shuttle_includes(body)
    for src, dst in COMPAT_SUBS:
        body = body.replace(src, dst)
    return body


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
    ap.add_argument("--src-dir", default=os.environ.get("SRC_DIR", ""),
                    help="override src/ directory (env: SRC_DIR). "
                         "Default: <devkit>/src. Use this when the devkit is "
                         "consumed as a submodule and the .ZC files live in "
                         "the parent project.")
    ap.add_argument("--test-dir", default=os.environ.get("TEST_DIR", ""),
                    help="override tests/ directory (env: TEST_DIR). "
                         "Default: <devkit>/tests. Assert.ZC must live there "
                         "(symlink to ../devkit/tests/Assert.ZC works).")
    ap.add_argument("--skip", default="",
                    help="comma-separated src filenames to skip "
                         "(default: Daemon.ZC,Setup.ZC — both ZealOS-only)")
    ap.add_argument("--order", default="",
                    help="comma-separated src push order if alphabetical "
                         "doesn't satisfy deps (e.g. Wavefunc.ZC,Orbital.ZC)")
    ap.add_argument("--launch", nargs="?", const="", default=None,
                    metavar="CMD",
                    help="push src files (skip tests) then exit the daemon "
                         "and sendkey CMD into adam's REPL. With no CMD, "
                         "just exits the daemon and leaves adam at its "
                         "prompt for manual driving. Use this to take over "
                         "adam's window with an interactive viewer like "
                         "an editor or a custom tool — the spawned-task "
                         "hash-chain workaround.")
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

    src_dir = Path(args.src_dir).resolve() if args.src_dir else REPO / "src"
    test_dir = Path(args.test_dir).resolve() if args.test_dir else REPO / "tests"
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
    if args.launch is None:
        src_files.append(test_dir / "Assert.ZC")
    test_files = [f for f in sorted(test_dir.glob("T_*.ZC"))
                  if not args.filter or args.filter in f.name]

    # Phase 1: source + (optionally) Assert. Each is JIT-compiled.
    print(f"==> phase 1: pushing {len(src_files)} source files")
    for f in src_files:
        if not push_and_wait(_prep(f.read_bytes()),
                             f.name, timeout=60.0):
            sys.exit(1)

    if args.launch is not None:
        # Two steps: (1) tell D() to exit so adam returns to its REPL;
        # (2) optionally sendkey CMD into adam's prompt. Whatever CMD
        # invokes will own adam's window with full WM chrome (resize,
        # drag, [X]) — the standard TempleOS user-task experience.
        print("==> stopping daemon (_D_exit = TRUE)")
        since = log_size()
        push_chunk(b'_D_exit=TRUE;')
        ok, _ = wait_for("D_EXIT", since=since, timeout=10.0)
        if not ok:
            print("!! D_EXIT not seen — adam may still be in D() loop",
                  file=sys.stderr)
        if args.launch:
            print(f"==> sendkey: {args.launch!r}")
            sendkey(args.launch, enter=True, delay=0.05)
            print("==> command sent — adam now owns the foreground task")
        else:
            print("==> daemon stopped; adam back at REPL for manual driving")
        return

    # Phase 2: TEST_RUN_BEGIN marker, then each test (top-level
    # PASS/ASSERT_EQ runs on push), then TEST_SUMMARY + TEST_RUN_END.
    print(f"==> phase 2: TEST_RUN_BEGIN + {len(test_files)} tests")
    push_and_wait(b'CommPrint(1,"TEST_RUN_BEGIN\\n");', "begin-marker", 10.0)
    for f in test_files:
        if not push_and_wait(_prep(f.read_bytes()),
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
