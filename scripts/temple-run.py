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
    # Stage-1 D() — minimal receive loop. Uses plain ExePutS, errors
    # still go to the framebuffer. We need this stage so the host can
    # push the (much longer) stage-2 daemon body over COM2 — the typed
    # REPL has a per-line buffer of ~256 chars, but ExePutS via COM2
    # has no such limit. After D_OK we upgrade to D2(), which adds
    # COM1 error capture.
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


# Stage-2 daemon body: defines _DRun (ExePutS wrapped with put_doc
# redirect + Fs->catch_except sampling) and D2 (the upgraded receive
# loop that calls _DRun and emits COMPILE_OK / COMPILE_ERR_BEGIN ...
# COMPILE_ERR_END / COMPILE_FAIL sentinels per chunk).
#
# Pushed over COM2 once stage-1 D() is alive. Then we push
# `_D_exit=TRUE;` to break stage-1, and sendkey `D2();` to start
# stage-2 in adam_task.
#
# Why this is split from BOOTSTRAP_CMDS: the REPL's per-line parser
# limit is ~256 chars. _DRun's body is ~440 chars. We sidestep by
# letting stage-1 ExePutS the whole multi-statement program at once.
#
# See NOTES-A2.md for the investigation that motivates this design.
DAEMON_V2_SOURCE = r"""
U0 _DRun(U8 *b)
{
  CDoc *_s = Fs->put_doc;
  CDoc *_c = DocNew;
  Fs->put_doc = _c;
  Fs->catch_except = FALSE;
  ExePutS(b);
  Bool _ok = !Fs->catch_except;
  Fs->put_doc = _s;
  if (_ok) {
    CommPrint(1, "COMPILE_OK\n");
  } else {
    CommPrint(1, "COMPILE_ERR_BEGIN\n");
    CDocEntry *e = _c->head.next;
    while (e != &_c->head) {
      if (e->type_u8 == DOCT_TEXT && e->tag)
        CommPrint(1, "%s", e->tag);
      else if (e->type_u8 == DOCT_NEW_LINE)
        CommPrint(1, "\n");
      e = e->next;
    }
    CommPrint(1, "\nCOMPILE_ERR_END\nCOMPILE_FAIL\n");
  }
  DocDel(_c);
}

U0 D2()
{
  CommPrint(1, "D2_OK\n");
  while (!_D_exit) {
    if (FifoU8Rem(comm_ports[2].RX_fifo, &Dc)) {
      if (Dc == 4) {
        Db[Di] = 0;
        _DRun(Db);
        CommPrint(1, "D_DONE\n");
        Di = 0;
      } else if (Di < 131071) {
        Db[Di++] = Dc;
      }
    } else {
      Sleep(10);
    }
  }
  CommPrint(1, "D_EXIT\n");
}
"""


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


def _auto_screenshot(label: str) -> str | None:
    """Snap the QEMU framebuffer when a COMPILE_FAIL is seen. Numbered
    so successive failures don't stomp each other. Returns saved path
    (or None on error)."""
    import subprocess
    sname = "".join(c if c.isalnum() else "_" for c in label)[:40]
    n = 0
    while True:
        png = REPO / "build" / f"fail-{sname}-{n:02d}.png"
        if not png.exists():
            break
        n += 1
    ppm = png.with_suffix(".ppm")
    env = {**os.environ,
           "QEMU_SOCK": str(MON),
           "SCREEN_PNG": str(png),
           "SCREEN_PPM": str(ppm)}
    try:
        subprocess.run(["bash", str(REPO / "scripts" / "screenshot.sh")],
                       env=env, check=True, capture_output=True)
    except subprocess.CalledProcessError as e:
        print(f"!! screenshot.sh failed: {e.stderr.decode(errors='replace')}",
              file=sys.stderr)
        return None
    return str(png)


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
    if "COMPILE_FAIL" in captured:
        # Daemon caught a 'Compiler' throw inside ExePutS. The captured
        # text between COMPILE_ERR_BEGIN/END already contains the lexer
        # error. Snap the framebuffer too — sometimes Print attrs get
        # eaten by DolDoc and the captured text is partial.
        shot = _auto_screenshot(label)
        msg = f"!! COMPILE_FAIL on {label}"
        if shot:
            msg += f" — screenshot: {shot}"
        print(msg, file=sys.stderr)
        return False
    return True


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--skip-bootstrap", action="store_true",
                    help="assume daemon already running (D_OK already in log)")
    ap.add_argument("--filter", default="",
                    help="substring filter for tests (T= equivalent)")
    ap.add_argument("--push-timeout", type=float,
                    default=float(os.environ.get("PUSH_TIMEOUT", "60")),
                    help="seconds to wait for D_DONE per pushed chunk "
                         "(env: PUSH_TIMEOUT). Bump for large test "
                         "batteries that exceed the 60s default.")
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
    ap.add_argument("--keep-daemon", action="store_true",
                    help="push src files (skip tests) but LEAVE the daemon "
                         "running on COM2 so external tools can stream "
                         "more HolyC into the live VM via the same "
                         "socket. Mutually exclusive with --launch.")
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
        print("==> stage-1 daemon up (D_OK)")

        # Upgrade to stage-2 (D2) — pushes _DRun + D2 source through
        # stage-1 ExePutS, exits stage-1, and starts D2 in adam.
        # Stage-2 emits COMPILE_OK / COMPILE_ERR_BEGIN..END / COMPILE_FAIL
        # sentinels per chunk, capturing the lexer's framebuffer output
        # over COM1.
        print(f"==> upgrading to stage-2 daemon "
              f"({len(DAEMON_V2_SOURCE)}B over COM2)")
        since = log_size()
        push_chunk(DAEMON_V2_SOURCE.encode())
        ok, captured = wait_for("D_DONE", since=since, timeout=15.0)
        if not ok:
            sys.exit(f"error: stage-2 source did not D_DONE. log:\n{captured}")
        # Now break stage-1 D() loop so adam returns to its REPL.
        since = log_size()
        push_chunk(b"_D_exit=TRUE;")
        ok, _ = wait_for("D_EXIT", since=since, timeout=10.0)
        if not ok:
            sys.exit("error: stage-1 D_EXIT not seen")
        # Reset _D_exit and start D2 from adam's REPL.
        sendkey("_D_exit=FALSE;D2();", enter=True, delay=0.05)
        ok, _ = wait_for("D2_OK", since=since, timeout=10.0)
        if not ok:
            sys.exit("error: D2_OK never appeared. Check screen-temple.png.")
        print("==> stage-2 daemon up (D2_OK)")

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
    # Discover source + test files. ZealOS uses `.ZC` (renamed
    # HolyC); pure TempleOS uses `.HC`. We accept either to support
    # projects that follow the TempleOS native convention. `.HH`
    # (HolyC headers) are picked up too — they're forward-decls that
    # need to compile before consumers.
    src_globs = ("*.HC", "*.HH", "*.ZC")
    if args.order:
        src_files = [src_dir / n for n in args.order.split(",")
                     if n not in skip]
    else:
        src_files = []
        seen = set()
        for pat in src_globs:
            for f in sorted(src_dir.glob(pat)):
                if f.name in skip or f.name in seen:
                    continue
                src_files.append(f)
                seen.add(f.name)
    # Locate Assert (test framework) — `.HC` preferred, `.ZC` fallback.
    if args.launch is None:
        for cand in ("Assert.HC", "Assert.ZC"):
            p = test_dir / cand
            if p.exists():
                src_files.append(p)
                break
    test_files = []
    seen = set()
    for pat in ("T_*.HC", "T_*.ZC"):
        for f in sorted(test_dir.glob(pat)):
            if f.name in seen:
                continue
            if args.filter and args.filter not in f.name:
                continue
            test_files.append(f)
            seen.add(f.name)

    # Phase 1: source + (optionally) Assert. Each is JIT-compiled.
    print(f"==> phase 1: pushing {len(src_files)} source files")
    for f in src_files:
        if not push_and_wait(_prep(f.read_bytes()),
                             f.name, timeout=args.push_timeout):
            sys.exit(1)

    if args.keep_daemon:
        print("==> src/ pushed; daemon left listening on COM2")
        return

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
                             f.name, timeout=args.push_timeout):
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
