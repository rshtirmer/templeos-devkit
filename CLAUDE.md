# CLAUDE.md — agent guide for templeos-devkit

A HolyC development environment. You write `.ZC` files on the host
with a real editor, and run them inside ZealOS (a maintained 64-bit
fork of TempleOS) running under QEMU. The dev loop is closed and
scriptable.

If you're an agent landing here for the first time, read this whole
file before touching anything. The README is for humans; this file
covers the parts that matter for autonomous work.

## Pick the right tool

There are two control planes for the VM:

- **`make test`** — cold-boot dev loop. Builds shuttle, boots ZealOS,
  runs every `tests/T_*.ZC`, parses `build/serial.log`, exits 0/1. ~30s
  per cycle. Use this for CI-style verdicts.
- **`scripts/zctl`** — long-lived REPL VM. One synchronous CLI that
  starts/stops the VM, pushes HolyC, captures output, takes
  screenshots. Use this for everything else: iterating on code,
  debugging compile errors, watching the screen, building up state.

`zctl` is the agent-friendly path. Default to it unless you specifically
want a fresh boot per change.

## zctl workflow

Stdlib-only Python, no extra deps. From the repo root:

```sh
scripts/zctl up                # start VM, wait until daemon listens
scripts/zctl status            # state: down | up + monitor/com2/daemon health
scripts/zctl eval 'Print("hi\n");'        # send HolyC inline
scripts/zctl eval -f tests/T_X.ZC         # send a file
scripts/zctl shell             # line-by-line interactive REPL
scripts/zctl logs -n 50        # last N lines of build/serial.log
scripts/zctl logs -f           # follow
scripts/zctl screenshot        # snap to build/screen.png
scripts/zctl wire              # one-shot post-install: mount shuttle as E: + Setup.ZC
scripts/zctl down              # quit cleanly via monitor socket
```

Flags worth knowing on `up`:
- `--headless` — no QEMU window. Default is display-on so a watching
  human can see what you're doing.
- `--no-auto-boot` — by default `up` sendkeys `1<enter>` to dismiss
  ZealOS's "Selection: 0/1/2" boot menu. Disable if you've customized
  the bootloader.
- `--timeout N` — how long to wait for `DAEMON_LISTEN:COM2` before
  giving up.

### Output capture

`zctl eval` blocks until the daemon prints `DAEMON_DONE` and writes
everything that hit the serial log between submission and that marker
to stdout. **Important:** `Print(...)` writes to the screen, not to
COM1. Use `CommPrint(1, ...)` if you want output to come back to your
shell.

```sh
scripts/zctl eval 'CommPrint(1, "value=%d\n", 42);'
# stdout: DAEMON_RECV:35
#         value=42
#         DAEMON_DONE
```

### First-time setup

Fresh clone, fresh disk:

```sh
make setup           # fetch ZealOS ISO (~44 MB)
make disk            # create blank 4 GB qcow2
make install         # boot CD + disk, walk install (~15 min on TCG)
                     # close QEMU when ZealOS desktop is up
scripts/zctl up      # boots disk + shuttle, daemon won't start (no MakeHome yet)
scripts/zctl wire    # one-shot: manually mount shuttle as E: and source Setup.ZC
                     # writes ~/MakeHome.ZC inside the VM
scripts/zctl down
scripts/zctl up      # subsequent boots auto-mount E: and spawn the daemon
```

After `wire`, every boot reaches `DAEMON_LISTEN:COM2` autonomously.

## HolyC quirks worth knowing

These bit us on first contact and are easy to surface again. None of
them are documented in one place, so write down anything new you find.

- **`RandF64()` does not exist.** Use `Rand()` — TempleOS/ZealOS's
  built-in unit-interval F64 RNG.

- **`if (cond) continue;` triggers `ParseStatement ERROR: Undefined
  identifier at ";"`.** Refactor to inverted-condition + braced body.
  May or may not be the same for `break` — verify if you need it.

- **Float literals silently truncate past ~16 significant digits.**
  `#define MY_PI 3.14159265358979323846` evaluates to ~0.005646, not π.
  Bisected via the daemon: 16 digits OK, 20 not OK. Use HolyC's
  built-in `pi` constant when you need π.

- **Top-level `return` / `goto` and `for`/`while` with type-decls
  trip the boot-phase parser** (NOTES.md has the long-form explanation
  with a `CTCPSocket` repro). The host-side linter
  (`scripts/holyc-lint.py`) flags these. Run `make lint` before pushing
  code into the VM — it's the offline pre-flight.

- **`Print` writes to screen, not COM1.** Use `CommPrint(1, ...)` for
  output the host sees.

- **`Sys()` with a bare-identifier body deadlocks** when called from
  `sys_task` during boot. `Daemon.ZC` documents the workaround
  (`TaskExe(sys_task, Fs, "Spawn(...);", 0)`).

When you discover a new quirk, append it here AND ideally add a rule to
`scripts/holyc-lint.py` so the next agent catches it offline.

## Layout

```
src/              persistent HolyC: Setup.ZC, Daemon.ZC, your tools  (committed)
tests/            test framework (Assert.ZC) + battery (T_*.ZC)      (committed)
scripts/          bash + python utilities, including zctl            (committed)
tooling/          host-side editor support (VSCode, Neovim, linter)  (committed)
build/            shuttle.img, serial.log, screen.png, qemu sockets  (gitignored)
vendor/zealos/    ZealOS BIOS ISO + installed disk.qcow2             (gitignored)
```

The build script (`scripts/build-shuttle.sh`) auto-discovers
`tests/T_*.ZC` and includes them from `Boot.ZC` in sorted order. To add
a test: drop a file matching `tests/T_*.ZC`. To add reusable HolyC: drop
it in `src/`, then `#include "E:/Whatever.ZC"` from your test.

## Debugging compile errors

The kernel debugger ("I Fault 0x32") fires on parse errors and bad
operations. From the host:

1. `scripts/zctl screenshot` — read the error text and the file:line.
2. Fix the source on the host. The VM's running daemon won't know about
   the change until you rebuild the shuttle.
3. `scripts/zctl down && scripts/zctl up` to pick up the new shuttle.

For pure HolyC iteration without re-cycling the VM, push code through
the daemon (`zctl eval`) — it executes against the live system without
rebooting. Useful for poking at functions, probing ZealOS APIs, and
verifying small fixes before committing them to the test battery.

### COM1 compile-error capture

The daemon (both `src/Daemon.ZC` for ZealOS and the inline `D2()` in
`scripts/temple-run.py` for original TempleOS) wraps every `ExePutS` /
`ExeFile` call with a `Fs->put_doc` redirect + `Fs->catch_except`
sample. Per-chunk output on COM1 is now:

  COMPILE_OK\n  D_DONE\n                     # success
  COMPILE_ERR_BEGIN\n<lexer text>\n
  COMPILE_ERR_END\nCOMPILE_FAIL\n  D_DONE\n   # compile failed

`temple-run.py` watches its per-chunk wait window for `COMPILE_FAIL`
and auto-runs `scripts/screenshot.sh`, saving the framebuffer to
`build/fail-<label>-NN.png` (numbered to avoid stomping on retries).
This means: you no longer need to OCR the screen to read a HolyC
compile error — the lexer text lands in `build/serial-temple.log` and
the screen is captured automatically.

Background and the investigation that motivates the design are in
`NOTES-A2.md`. Headline: `ExePutS` swallows the `'Compiler'` throw
internally and sets `Fs->catch_except=TRUE`; lexer error strings route
through `Fs->put_doc`, so capturing them is a doc-redirect away.

## When to add to vs read from this file

- **Read:** at the start of any session in this repo, especially before
  writing HolyC.
- **Write:** when you discover a new HolyC quirk, a new ZealOS API
  detail, or a workflow that should be standard. Keep entries terse —
  this is an operational guide, not documentation.
