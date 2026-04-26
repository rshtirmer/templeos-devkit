# templeos

A HolyC development environment. We write `.ZC` files on the host with a
real editor, and run them inside [ZealOS](https://github.com/Zeal-Operating-System/ZealOS)
(a maintained 64-bit fork of TempleOS) running in QEMU. The whole loop is
closed and scriptable: `make test` builds, boots, runs, and reports
pass/fail.

ZealOS is the dev VM. Pure TempleOS is reserved for the canonical altar.

## The dev loop

```
   host                                  guest (ZealOS in QEMU)
   ────                                  ──────────────────────
   src/*.ZC, tests/*.ZC  ──┐
                           │  hdiutil    ┌─────────────────────┐
                           └─► shuttle.img ──► drive B:        │
                                         │   ~/MakeHome.ZC     │
                                         │     #include B:/Boot.ZC
                                         │   Boot.ZC:          │
                                         │     CommInit8n1     │
                                         │     #include tests  │
                                         │     TEST_SUMMARY    │
   build/serial.log  ◄───── -serial file ◄  CommPrint PASS/FAIL│
                                         │   OutU16 0x604 → ACPI off
                                         └─────────────────────┘
   make test ─► grep TEST_FAIL ─► exit 0/1
```

Five pieces:

1. **Shuttle disk.** A FAT32 image built from `src/` and `tests/`, attached
   to QEMU as a second drive. ZealOS sees it as `B:`. `scripts/build-shuttle.sh`
   also generates `Boot.ZC` automatically by enumerating `tests/T_*.ZC`.
2. **Auto-run via MakeHome.** ZealOS's `~/MakeHome.ZC` runs on every boot.
   We wire it once (via `make wire-makehome`) to `#include "B:/Boot.ZC";`.
3. **Test framework.** `tests/Assert.ZC` defines `PASS`, `FAIL`,
   `ASSERT_EQ`, `TEST_SUMMARY` — each writes to both the screen (`Print`)
   and COM1 (`CommPrint`).
4. **Serial out.** QEMU pipes COM1 to `build/serial.log`. Every
   `CommPrint` lands in a host file we can grep.
5. **ACPI shutdown.** `Boot.ZC` ends with `OutU16(0x604, 0x2000)`, which
   QEMU intercepts as ACPI sleep state 5 — the VM exits cleanly and `make
   test` returns control to the host.

## Layout

```
vendor/zealos/    ZealOS BIOS ISO + installed disk.qcow2  (gitignored)
src/              persistent HolyC: Setup.ZC, future tools  (committed)
tests/            test framework + battery (T_*.ZC files)   (committed)
scripts/          bash + python utilities                   (committed)
tooling/          editor extensions (VSCode, Neovim)        (committed)
build/            shuttle.img, serial.log, screen.png       (gitignored)
Makefile          all the targets below
```

## Prerequisites

- macOS, Apple Silicon or Intel
- `qemu-system-x86_64` — `brew install qemu`
- Standard macOS tools: `hdiutil`, `dd`, `make`, `python3`, `nc`, `sips`
- ~5GB free disk

## Setup (one-time)

```sh
make setup           # fetch the ZealOS BIOS ISO (~44MB)
make disk            # create a fresh 4G qcow2 install disk
make install         # boot CD + disk for the install (~15min on TCG)
                     # walks itself through y → I → Y via sendkey
make dev             # boot the installed disk + shuttle
make wire-makehome   # one-time: writes ~/MakeHome.ZC to auto-run B:/Boot.ZC
```

After `wire-makehome`, the loop is live. Quit the dev VM (Ctrl-C the make,
or close the QEMU window) and any future `make test` is fully autonomous.

## Daily use

```sh
make test           # rebuilds shuttle from src/+tests/, boots, runs, parses log
make test T=Hello   # only run tests whose filename contains 'Hello'
make watch          # re-run on src/ or tests/ change (needs `brew install fswatch`)
make dev            # interactive: same boot, no auto-exit, you see the desktop
make repl           # dev + live REPL daemon on COM2 — push code with scripts/zpush.sh
```

`make repl` boots once and brings up a serial REPL inside the VM. From
the host:

```sh
echo 'Print("hi from host\n");' | scripts/zpush.sh
scripts/zpush.sh tests/T_Hello.ZC
```

Daemon trace (`DAEMON_RECV`, prints, `DAEMON_DONE`) lands in
`build/serial.log`. This is the experimental fast-feedback path —
when it works, you skip the ~30s cold-boot per iteration.

## Why this shape

I (Claude) write HolyC less reliably than I write Python. The loop is the
mitigation: every piece of code is anchored to a passing test. The
validation battery in `tests/T_*.ZC` is the rosetta stone — once basics
pass, I have proven patterns to copy from for everything else.

## Why ZealOS instead of pure TempleOS

ZealOS is the most actively maintained 64-bit TempleOS fork (active as of
2025-11, vs Shrine which was archived in 2024). It adds: a real TCP/IP
stack with `TCPSocketListen`, modern bootloader (Limine), `Once()`/
`SysOnce()` persistent boot scripts, and drivers for E1000/RTL8139/
VirtIONet. It renames HolyC to ZealC; same language.

## Live REPL (experimental — `make repl`)

`src/Daemon.ZC` polls COM2's RX FIFO, executes received HolyC source on
EOT (0x04), and prints daemon-side trace to COM1. `scripts/zpush.sh`
sends source to `build/com2.sock`. The function body is invoked via
`Sys("RunDaemon;")` from a Sys()-queued context so its types resolve
post-boot — see NOTES.md for why direct top-level invocation tripped
the boot-phase parser.

Earlier TCP-via-PCnet attempts (with `hostfwd=tcp::7777-:7777`) reached
listen but the handshake never completed end-to-end through QEMU
user-mode → PCnet → ZealOS TCP. Chardev-socket on COM1 swapping out the
file backend lost MakeHome's CommPrint output entirely. The current
shape — COM1=file (TX, unchanged), COM2=chardev-socket (RX) — was
untested before and is the path of least resistance.

## Editor support

Two local extensions, install via symlink — no marketplace, no plugin
manager required.

```sh
# VSCode
ln -s "$(pwd)/tooling/holyc-vscode" ~/.vscode/extensions/local.holyc-0.1.0

# Neovim (native package layout)
mkdir -p ~/.config/nvim/pack/local/start
ln -s "$(pwd)/tooling/holyc-nvim" ~/.config/nvim/pack/local/start/holyc
```

Both extensions cover the same surface: HolyC primitive types
(`U0`/`U8`/…/`F64`/`Bool`), ZealOS class types (`C[A-Z]…`), control flow
including sub-switch `start`/`end`, storage modifiers (`extern`,
`public`, `interrupt`, `lastclass`, `lock`, …), DolDoc `$$` escape,
multi-char literals, preprocessor directives, and the kernel/stdlib
functions this repo uses. Diagnostics for the boot-phase quirks
documented in `NOTES.md` are out of scope — for ground truth, push
through `make repl` + `scripts/zpush.sh`.

See `tooling/holyc-vscode/README.md` and `tooling/holyc-nvim/README.md`
for details.

## Credits

- [Terry A. Davis](https://en.wikipedia.org/wiki/Terry_A._Davis), 1969–2018
  — wrote TempleOS, HolyC, the editor, the compiler, the games, the oracle,
  alone.
- [ZealOS](https://github.com/Zeal-Operating-System/ZealOS) — modernized
  64-bit fork; what we actually run.
- [TinkerOS](https://github.com/tinkeros/TinkerOS) — sister fork, kept the
  TempleOS look. Worth knowing about.

It is good.
