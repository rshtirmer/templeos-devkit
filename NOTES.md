# NOTES — research log and dead-ends

The working dev loop is documented in `README.md`. This file logs the things
that *didn't* work (and exactly why), so the next attempt can start from
known ground.

## What works (don't break this)

- `make install` once → `make wire-makehome` once → `make test` autonomously
- Boot path: BIOS → ZealOS Boot Loader (we send `1`) → installed kernel →
  `~/MakeHome.ZC` → mounts shuttle on AHCI port 2 as E: → includes
  `E:/Boot.ZC` → tests print `TEST_PASS:*` / `TEST_RUN_END` to COM1 →
  host-side `run-tests.sh` sees `TEST_RUN_END` in `build/serial.log` and
  sends `quit` via `build/qemu.sock`.
- `~30s` per cycle on Apple Silicon TCG (no x86 hardware accel).

## Live REPL ("zpush" daemon) — WORKING (`make repl`)

Persistent VM, push HolyC source over a Unix socket, observe results in
serial.log. ~800ms round-trip vs ~30s cold boot.

Pieces:

- `src/Daemon.ZC` — `RunDaemon(U8 *data)`. Re-inits COM1+COM2 in its own
  task heap, polls `comm_ports[2].RX_fifo`, writes received bytes to
  `C:/Tmp/Cmd.HC` on EOT (0x04), runs them via `ExeFile`.
- `scripts/zpush.sh` — `nc -U build/com2.sock`, appends EOT.
- `scripts/build-shuttle.sh` (DAEMON=1) — emits a Boot.ZC that
  `#include`s Daemon.ZC at boot phase, then queues
  `Spawn(&RunDaemon, NULL, "Daemon");` for sys_task post-boot via
  `TaskExe(sys_task, Fs, "...", 0)` (see "Findings" below).
- `scripts/boot.sh dev` — wires COM2 as a chardev socket
  (`build/com2.sock`) in addition to COM1=file.

### Findings (the path through three failure modes)

**Mode 1: top-level `RunDaemon;` invocation.** Boot-phase parser quirks
(documented further down) trip "Undefined identifier" on types inside
function bodies invoked at top level. Function definitions parse fine;
invocation is what crosses the boot-phase boundary.

**Mode 2: `Sys("RunDaemon;")` from boot phase.** `Sys()`'s
`Fs == sys_task` branch (which is true during MakeHome execution) calls
`JobsHandler(RFlagsGet)` *synchronously*. JobsHandler runs the queued
job by calling `ExePrint("%s", aux_str)` — which compiles and executes
the source RIGHT NOW, in boot phase. Same parser quirks apply. Sys()
does NOT actually defer to post-boot when invoked from sys_task.

**Mode 3: `TaskExe(sys_task, Fs, "...", 0)` + spinner.** TaskExe with
`flags=0` queues without triggering JobsHandler. Good. But Boot.ZC ended
with `while (TRUE) Sleep(1000);` to "spin" sys_task. That pins sys_task
in the spinner and the queue never drains. The job sits forever.

**What works:** TaskExe with flags=0 to queue, then **let Boot.ZC
return naturally** (no spinner). sys_task moves on through its normal
post-MakeHome init, drains its queue, our `Spawn(&RunDaemon, ...)`
runs, and the daemon comes up. `run-tests.sh` quits qemu via the
monitor socket on `TEST_RUN_END` — no spinner needed for `make test`
either.

### Findings (FIFO heap ownership)

`FifoU8New` (in `src/Kernel/KDataTypes.ZC`) allocates with
`mem_task = Fs` — i.e. the calling task's heap. `CommInit8n1` calls
this for both RX and TX fifos. So when Boot.ZC does
`CommInit8n1(1, 9600)` from sys_task, `comm_ports[1].TX_fifo` lives in
sys_task's heap.

A spawned task that calls `CommPrint(1, ...)` then GP-faults in
`FifoU8Insert` — the cross-task heap reference is unsafe (heap
re-use, lock contention, or both — exact mechanism unverified).
Workaround: re-call `CommInit8n1(1, 9600); CommInit8n1(2, 9600)` from
inside the spawned task so both port FIFOs live in the spawned task's
own heap.

### Earlier dead-ends (kept for the record)

### Findings (HolyC boot-phase parser)

The on-disk parser in ZealOS treats code differently when
`!Bt(&sys_run_level, RLf_SYSTEM_SERVER)` (boot phase). The error path is
in `src/Compiler/CExcept.ZC` — it just prints the *note* "Still in boot
phase" and the actual error follows.

What we hit, with reproducers:

| Construct in MakeHome | Result |
| --- | --- |
| `while (TRUE) { CTCPSocket *_dcl; ... }` at top level | "Undefined identifier" on the type |
| `for (;;) { ... }` at top level | same |
| `goto LOOP_TOP;` with `LOOP_TOP:` at top level | "No global labels" (`src/Compiler/ParseStatement.ZC` checks `cc->htc.fun`) |
| `U0 RunDaemon() { if (...) return; ... }` defined at top level then called | Note: "Can't return" (boot phase forbids `return`) |
| `U0 RunDaemon() { ... while (TRUE) { ... } }` defined inline, then `RunDaemon;` | Parses but body still throws "Undefined identifier" on types |

The cleanest theory is: types loaded by sibling `#include`s (e.g.,
`CTCPSocket` from `::/Home/Net/Start` → `Load.ZC` → `Protocols/TCP/MakeTCP`
→ `TCP.HH`) end up registered in a hash table that *the parser invoked
during boot phase doesn't fully see for code inside loop bodies / function
bodies*. Top-level uses in the same file work; uses inside `while` /
`for` / `goto` blocks fail.

### Workarounds we found that *do* compile

- **`Sys("source")`** (`src/Kernel/Job.ZC`) — queues `source` for execution
  on `sys_task`. The string is parsed when the system task picks it up,
  which is post-boot. Functions, loops, returns, types — all behave
  normally. We didn't get the daemon fully running this way because the
  COM-port plumbing also broke (see below).
- **`Once("source")`** (`src/System/Registry.ZC`) — appends `source` to
  `~/Registry.ZC` Once/User tree. Runs at "first user term" via
  `OnceExe;` in `~/Once.ZC` (or `/Once.ZC`). Same execution context as
  `Sys`. Side effect: persists across reboots until flushed.

### Findings (COM port input)

ZealOS `Doc/Comm.ZC` exposes only **output** functions
(`CommInit8n1`, `CommPutChar`, `CommPutS`, `CommPrint`). The interrupt
handler `CommHandler` *does* receive bytes and pushes them into
`comm_ports[port].RX_fifo`, a `CFifoU8 *`. There is no public read API,
but a HolyC program can poll directly:

```
U8 b;
if (FifoU8Remove(comm_ports[2].RX_fifo, &b)) { ... }
```

`FifoU8Remove(CFifoU8 *f, U8 *out)` returns `Bool` (true if a byte was
removed). Defined in `src/Kernel/KDataTypes.ZC`.

### Why the actual daemon attempt stalled

We tried two channels:

1. **TCP over `pcnet` NIC** with QEMU `hostfwd=tcp::7777-:7777`. Daemon
   reached `DAEMON_LISTEN:7777`, the daemon task was visible in the system
   task list, and host-side `nc 127.0.0.1 7777` connected at TCP level
   (we saw `0x611E` packets in the ZealOS net trace). But
   `TCPSocketAccept` never returned for those connections — the TCP
   handshake didn't complete end-to-end through QEMU user-mode →
   PCnet → ZealOS TCP stack.
2. **Serial socket via QEMU `-chardev socket` on COM1, then COM2.**
   With chardev backend instead of `-serial file:`, the new MakeHome's
   `CommPrint` calls produced no output to either the chardev's
   `logfile=` or to a connected `nc -U` client, even though the system
   was visibly alive. Buffering / chardev-init timing not fully
   understood. `MAKEHOME_BEGIN` printed reliably with the file backend
   but never with the socket backend, suggesting either ZealOS's COM port
   driver state differs based on backend, or QEMU `chardev socket`
   without an active client at boot drops early writes.

### Open angles for next attempt

- `-chardev pty` instead of `socket` — backed by a real PTY the host can
  open via `/dev/tty.qemu0`-style symlinks. Different backend, may flush
  differently.
- `-device virtio-serial-pci -device virtconsole,chardev=...` —
  virtio-serial. No idea if ZealOS recognises it; would need a driver.
- Different NIC model (`virtio-net-pci`, `e1000`, `rtl8139`) for the TCP
  attempt; PCnet was just the wiki-recommended one.
- `Sys()` queuing the daemon while keeping COM1 on the file backend —
  daemon would only need RX, which it could poll on the *file* descriptor
  if QEMU's `file` backend supported bidirectional access (it doesn't —
  it's TX-only, hence the chardev attempt).
- `aarzilli/templestuff` has tooling (FUSE for RedSea, `.Z`
  decompressor) — could let us edit `~/MakeHome.ZC` directly from the
  host, removing the need to bootstrap the qcow2 from inside the VM
  every iteration.
- `EXODUS` (`1fishe2fishe/EXODUS`) — TempleOS as a Linux/BSD process,
  built-in CLI mode. **x86_64 only**, no Apple Silicon support; would
  need a Linux x86 VM or container to host it.

## ZealOS source pointers worth remembering

| What | Where |
| --- | --- |
| Boot phase flag | `Bt(&sys_run_level, RLf_SYSTEM_SERVER)` |
| "Still in boot phase" note | `src/Compiler/CExcept.ZC` |
| "No global labels" check | `src/Compiler/ParseStatement.ZC` (gated on `cc->htc.fun`) |
| "Undefined identifier" emit | `src/Compiler/ParseStatement.ZC` |
| `Sys()` queue | `src/Kernel/Job.ZC` |
| `Once()` / `OnceExe` | `src/System/Registry.ZC` and `src/Once.ZC` |
| Comm output API | `src/Doc/Comm.ZC` |
| Comm RX FIFO | `comm_ports[1..4].RX_fifo` (poll with `FifoU8Remove`) |
| TCP server example | `src/Home/Net/Tests/TCPEchoServer.ZC` |
| Network bring-up | `src/Home/Net/Start.ZC` → `Load.ZC` |
| Network drivers | `src/Home/Net/Drivers/{PCNet,E1000,RTL8139,VirtIONet}.ZC` |

## QEMU notes specific to this setup

- Apple Silicon: `qemu-system-x86_64` runs under TCG (no HVF accel for
  x86 on ARM). Boot: ~10s, install: ~80min.
- Drive layout matters. `BootHDIns` bakes mount info into the kernel.
  Our installed kernel expects: `ahci.0`=CD, `ahci.1`=disk. Booting
  without the CD attached panics with `AHCI Port/BlkDev error`. `dev`
  mode adds `ahci.2`=shuttle and the kernel doesn't auto-mount it —
  `MakeHome.ZC` does the manual `BlkDevNextFreeSlot('E', 2);
  AHCIPortInit(...); BlkDevAdd(bd, 1, 0, 1);` dance.
- Shuttle must be **MBR-partitioned FAT32** (not raw FAT). On macOS use
  `hdiutil create -fs FAT32 -layout MBRSPUD …`. With the default
  `-fs MS-DOS` hdiutil silently picks FAT16 even at 256 MB sizes, which
  ZealOS reads as garbage (the partition entry says type 0x0B = FAT32 but
  the BPB is FAT16).
- `BlkDevAdd` boolean args: at command line the literal identifiers
  `False`/`True` aren't always in scope, but `0`/`1` always work.
- `send.py` drops the first character intermittently — prefix sent
  strings with a leading space (` CBlkDev *bd;`). Cause unknown; possibly
  a QEMU monitor sendkey timing race on the first scancode.
