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
make lint           # host-side static lint of HolyC — boot-phase quirks, balance
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

## Running the same battery on stock TempleOS

Side-by-side path that runs `tests/T_*.ZC` on Terry's 2017 Distro
itself, no ZealOS. Useful for portability sanity checks and for the
"works on the actual altar" verdict.

```sh
make setup-temple    # fetch templeos.org/Downloads/TempleOS.ISO
make disk-temple     # blank 4G qcow2 in vendor/templeos/
make install-temple  # interactive — answer 'n' to tour, accept defaults
                     # close the QEMU window when desktop appears
make dev-temple      # boot disk + COM2 socket
                     #   in QEMU window: '1<Enter>' at the boot menu,
                     #   then 'n<Enter>' at the Once.HC tour prompt
make test-temple     # in another shell — types daemon + pushes battery
                     # ~2 min for a full battery
```

`scripts/temple-run.py` types a tiny in-memory JIT daemon into
`adam_task` over the QEMU monitor (`sendkey`), then streams each `.ZC`
file as raw bytes through a COM2 chardev socket. The daemon calls
`ExePutS(buf)` on each chunk — JIT-compiles in memory, no disk write
or `#include`. Top-level `PASS` / `ASSERT_EQ` calls in test files
execute during the push; output streams back over COM1 and lands in
`build/serial-temple.log`.

The dev loop differs from ZealOS in three load-bearing ways:

1. **Machine: `pc` not `q35`.** Terry's 2017 distro has no AHCI driver.
   `boot-temple.sh` uses i440FX + plain IDE.
2. **No shuttle / payload disk.** TempleOS reads FAT32 from the
   secondary IDE slot unreliably and refuses to switch `Drv()` to an
   ISO9660 mount (`File System Not Supported`). We push everything
   over COM2 instead.
3. **Daemon runs in `adam_task` directly, not via `Spawn`.** Spawned
   tasks' JIT compile context doesn't reliably see adam's
   `#include`'d symbols (`CommPrint`, `FifoU8Rem`, `comm_ports`) via
   the `hash_table->next` chain. Calling `D()` from adam means adam's
   REPL blocks forever — that's fine, every push goes via COM2 from
   then on.

If your fork has more `src/*.ZC` files that depend on ZealOS-only APIs
(AHCI, ZealOS-renamed FIFO calls), pass `--skip A.ZC,B.ZC` to
`temple-run.py`. If alphabetical push order doesn't satisfy `#include`
deps in your code, pass `--order Foo.ZC,Bar.ZC,Baz.ZC` to override.

ZealOS↔TempleOS API renames are auto-substituted on push (see
`COMPAT_SUBS` in `temple-run.py`): `MessageGet→GetMsg`,
`MESSAGE_KEY_DOWN/UP→MSG_KEY_DOWN/UP`,
`WIG_USER_TASK_DEFAULT→WIG_USER_TASK_DFT`, `mouse→ms`,
`tS → (cnts.jiffies(F64)/1000.0)`. Add to that list if your fork hits
more.

### Launching an interactive viewer (`make launch-temple`)

The test path keeps the daemon running forever (adam blocks in `D()`).
For an interactive tool — an editor, a viewer, anything WM-tile based —
you need adam's REPL back. The daemon picks up a `_D_exit=TRUE;` push
and falls out of the loop, letting adam continue. `--launch[=CMD]`
automates that:

```sh
# push src files, exit the daemon, sendkey CMD into adam's prompt:
make launch-temple CMD='YourViewer(2, 8000);'

# or no CMD — just exits the daemon, leaves you at adam's REPL:
make launch-temple
```

Why we go through adam instead of `Spawn`'ing the viewer in its own
task: Spawn'd tasks' JIT compile context doesn't reliably resolve
adam's `ExePutS`'d symbols via the `hash_table->next` chain, and
spawned tasks lack the `UserStartUp` setup (CDoc, `display_flags`)
that a real user-task gets — `WinMax` / `Fs->draw_it` silently no-op.
Running in adam means full WM chrome (resize handles, drag, [X]) and
all the symbols just work.

Quirks worth knowing if you hit a panic:

- Original TempleOS uses `FifoU8Rem`/`FifoU8Ins`; ZealOS renamed them
  to `FifoU8Remove`/`FifoU8Insert`.
- `#include "::/Doc/Comm";` (no extension) appends `.HC.Z` and works.
  Writing `Comm.HC` literally looks for an uncompressed file that
  doesn't exist.
- The earlier `FileWrite` + `ExeFile` design churned the RedSea FS
  hard enough to panic Adam after ~10 chunks. `ExePutS` sidesteps this.
- QEMU `sendkey` defaults to a 100 ms hold; tighter pacing without
  matching `hold_ms=30` and a post-flush wait silently drops keys
  mid-stream. `scripts/send.py` handles this.

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
