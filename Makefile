.PHONY: setup disk shuttle install boot boot-disk dev repl wire-makehome test test-fast lint watch clean clean-disk help \
	setup-temple disk-temple install-temple boot-temple-disk dev-temple test-temple launch-temple \
	vm-status vm-screen vm-logs vm-reset vm-down

# ---- Original TempleOS (Terry's 2017 Distro) — side-by-side compat target ----
# No shuttle / FAT image / payload disk: the host pushes the entire test
# battery over COM2 via scripts/temple-run.py once the VM is up.
TEMPLEOS_URL  := https://templeos.org/Downloads/TempleOS.ISO
TEMPLE_ISO    := vendor/templeos/templeos.iso
TEMPLE_DISK   := vendor/templeos/disk.qcow2

setup-temple: $(TEMPLE_ISO)
$(TEMPLE_ISO):
	mkdir -p $(dir $@)
	curl -sSL -o $@ "$(TEMPLEOS_URL)"
	@ls -lh $@

disk-temple: $(TEMPLE_DISK)
$(TEMPLE_DISK):
	mkdir -p $(dir $@)
	qemu-img create -f qcow2 $@ $(DISK_SIZE)

install-temple: $(TEMPLE_ISO) $(TEMPLE_DISK)
	bash scripts/boot-temple.sh install

boot-temple-disk: $(TEMPLE_DISK)
	bash scripts/boot-temple.sh disk

dev-temple: $(TEMPLE_DISK)
	bash scripts/boot-temple.sh dev

# Push the battery to a running dev-temple VM (boot-temple.sh dev must
# be up; pick Drive C in the bootloader and skip the Once.HC tour with
# 'n', then run this). Use T= to filter test files.
test-temple:
	T="$(T)" python3 scripts/temple-run.py --filter="$(T)"

# Push src files (skip tests), exit the daemon, and sendkey CMD into
# adam's REPL — adam now owns the foreground task. Pattern for taking
# over the WM tile with an interactive viewer your fork ships in
# src/. Pass CMD as a HolyC statement; with empty CMD just leaves
# adam at its prompt for manual driving.
#
#   make launch-temple CMD='YourViewer(2,8000);'
#   make launch-temple              # no command — manual REPL drive
launch-temple:
	python3 scripts/temple-run.py --launch="$(CMD)"


ZEALOS_URL := https://github.com/Zeal-Operating-System/ZealOS/releases/download/latest/ZealOS-PublicDomain-BIOS-2025-11-10-02_56_42.iso
ISO        := vendor/zealos/zealos.iso
DISK       := vendor/zealos/disk.qcow2
DISK_SIZE  := 4G
SHUTTLE    := build/shuttle.img

# Test run timeout — kill the VM if it takes longer than this.
TEST_TIMEOUT := 300

help:
	@echo "make setup          fetch ZealOS BIOS ISO into vendor/zealos/"
	@echo "make disk           create a fresh 4G qcow2 install disk"
	@echo "make install        boot CD + disk for one-time interactive install"
	@echo "make boot           boot the live CD (no disk, no shuttle)"
	@echo "make boot-disk      boot installed qcow2 only (no shuttle)"
	@echo "make shuttle        build build/shuttle.img from src/ and tests/"
	@echo "make dev            boot installed disk + shuttle — main dev loop"
	@echo "make repl           dev mode + REPL daemon on com2.sock — use scripts/zpush.sh"
	@echo "make wire-makehome  one-time: sendkey Setup.ZC to wire ~/MakeHome.ZC"
	@echo "make test           build shuttle, boot dev, parse serial.log, exit 0/1"
	@echo "make test T=Hello   only run tests whose filename contains 'Hello'"
	@echo "make test-fast      push tests to running 'make repl' VM (sub-second)"
	@echo "make lint           static-lint src/ and tests/ — boot-phase quirks, balance"
	@echo "make watch          auto-run test cycle on save (T=, WARM=, PUSH_TIMEOUT= pass-through)"
	@echo "make clean          remove build artifacts (keeps ISO and disk)"
	@echo "make clean-disk     wipe the installed disk (forces a fresh install)"

setup: $(ISO)

$(ISO):
	mkdir -p $(dir $@)
	curl -sSL -o $@ "$(ZEALOS_URL)"
	@ls -lh $@

disk: $(DISK)

$(DISK):
	mkdir -p $(dir $@)
	qemu-img create -f qcow2 $@ $(DISK_SIZE)

shuttle:
	T="$(T)" bash scripts/build-shuttle.sh

install: $(ISO) $(DISK)
	bash scripts/boot.sh install

boot: $(ISO)
	bash scripts/boot.sh cd

boot-disk: $(DISK)
	bash scripts/boot.sh disk

dev: $(DISK) shuttle
	bash scripts/boot.sh dev

# Dev mode + the live-REPL daemon. The shuttle's Boot.ZC queues RunDaemon
# via Sys() so it runs post-boot. Push code at the VM with:
#   echo 'Print("hi\n");' | scripts/zpush.sh
# All daemon trace lands in build/serial.log.
repl: $(DISK)
	DAEMON=1 T="$(T)" bash scripts/build-shuttle.sh
	bash scripts/boot.sh dev

wire-makehome:
	bash scripts/wire-makehome.sh

# `make test` runs the dev loop with a timeout. Boot.ZC tries ACPI
# shutdown on common QEMU ports, but q35 may not respond, so we also
# watch the serial log from the host and `quit` via the monitor socket
# the moment TEST_RUN_END appears. Belt and suspenders.
test: $(DISK) shuttle
	@echo "==> running tests (timeout $(TEST_TIMEOUT)s)"
	@bash scripts/run-tests.sh
	@bash scripts/check-tests.sh

# Push current src/ + tests/ to the running daemon (started by `make repl`
# in another terminal). Sub-second iteration vs ~30s cold boot. T= filters.
test-fast:
	@T="$(T)" python3 scripts/test-fast.py

# Host-side static lint of HolyC sources. Catches the boot-phase quirks
# documented in NOTES.md plus balance / unterminated-string hazards.
# Approximate (regex/token, not the real parser) — for ground truth use
# `make repl` + scripts/zpush.sh. Exits 1 on any error-level diagnostic;
# warnings don't fail the build.
lint:
	@python3 scripts/holyc-lint.py src $$(find tests -name '*.HC' -o -name '*.ZC' -o -name '*.HH' | grep -v '^tests/lint/bad-')
	@bash tests/lint/run.sh

# Re-run the test cycle on every save under src/ or tests/. Auto-detects
# fswatch (macOS) or inotifywait (Linux). Coalesces save bursts via a
# short debounce. Ctrl-C to stop.
#
# Pairs with the snapshot fast-path (vm-warmup):
#   make dev-temple           # cold boot once
#   make vm-warmup            # save snapshot once
#   make watch T=Foo WARM=1   # ~10s feedback per save
#
# T= filters the test battery; WARM=1 uses the snapshot fast-path;
# PUSH_TIMEOUT= forwards to temple-run.py.
watch:
	@bash scripts/watch.sh "$(T)" "$(WARM)" "$(PUSH_TIMEOUT)"

clean:
	rm -rf build

clean-disk:
	rm -f $(DISK)
	@echo "disk wiped — run 'make install' to reinstall"

# ---- VM lifecycle ergonomics (TempleOS dev-temple VM) ----
# Quick inspect + control commands for the running VM. Each is
# idempotent and self-contained — no harm running twice.

TEMPLE_MON   := build/qemu-temple.sock
TEMPLE_COM2  := build/com2-temple.sock
TEMPLE_LOG   := build/serial-temple.log
TEMPLE_PNG   := build/screen-temple.png

# vm-status — is the VM alive? Reports pid/uptime, sockets, log tail.
vm-status:
	@pid=$$(pgrep -f 'qemu-system-x86.*qemu-temple\.sock' | head -1); \
	if [ -n "$$pid" ]; then \
	  etime=$$(ps -o etime= -p $$pid 2>/dev/null | tr -d ' '); \
	  echo "VM up   pid=$$pid  uptime=$$etime"; \
	else \
	  echo "VM down (no qemu process matching qemu-temple.sock)"; \
	fi
	@if [ -S "$(TEMPLE_MON)" ]; then echo "monitor sock: $(TEMPLE_MON) (ok)"; else echo "monitor sock: missing"; fi
	@if [ -S "$(TEMPLE_COM2)" ]; then echo "com2 sock:    $(TEMPLE_COM2) (ok)"; else echo "com2 sock:    missing"; fi
	@if [ -f "$(TEMPLE_LOG)" ]; then \
	  echo "--- last 3 lines of $(TEMPLE_LOG) ---"; \
	  tail -n 3 "$(TEMPLE_LOG)" || true; \
	  echo "--- last D_OK / D2_OK / D_EXIT markers ---"; \
	  grep -nE 'D_OK|D2_OK|D_EXIT' "$(TEMPLE_LOG)" | tail -n 3 || echo "(none seen yet)"; \
	else \
	  echo "serial log:   $(TEMPLE_LOG) missing"; \
	fi

# vm-screen — capture current QEMU framebuffer to build/screen-temple.png.
vm-screen:
	@if [ ! -S "$(TEMPLE_MON)" ]; then \
	  echo "error: $(TEMPLE_MON) not found — VM not running?" >&2; exit 1; \
	fi
	@QEMU_SOCK="$(TEMPLE_MON)" SCREEN_PPM="build/screen-temple.ppm" SCREEN_PNG="$(TEMPLE_PNG)" \
	  bash scripts/screenshot.sh
	@echo "==> wrote $(TEMPLE_PNG) — open it with: open $(TEMPLE_PNG)"

# vm-logs — follow build/serial-temple.log (or fixed-line tail with N=20).
vm-logs:
	@if [ ! -f "$(TEMPLE_LOG)" ]; then \
	  echo "error: $(TEMPLE_LOG) missing — VM not running?" >&2; exit 1; \
	fi
	@if [ -n "$(N)" ]; then \
	  tail -n "$(N)" "$(TEMPLE_LOG)"; \
	else \
	  echo "==> tailing $(TEMPLE_LOG) (Ctrl-C to stop)"; \
	  tail -f "$(TEMPLE_LOG)"; \
	fi

# vm-reset — kill any wedged QEMU, clean sockets+log, relaunch dev-temple.
vm-reset:
	@echo "==> killing any qemu-system-x86 processes"
	@pkill -f 'qemu-system-x86' 2>/dev/null || true
	@sleep 2
	@echo "==> cleaning sockets and serial log"
	@rm -f "$(TEMPLE_MON)" "$(TEMPLE_COM2)" "$(TEMPLE_LOG)"
	@echo "==> relaunching dev-temple in background"
	@nohup bash scripts/boot-temple.sh dev >build/vm-reset.out 2>&1 &
	@echo "==> waiting up to 90s for VM to boot (monitor sock + autopress)"
	@i=0; while [ $$i -lt 90 ]; do \
	  if [ -S "$(TEMPLE_MON)" ]; then break; fi; \
	  sleep 1; i=$$((i+1)); \
	done
	@if [ -S "$(TEMPLE_MON)" ]; then \
	  echo "==> VM responsive — monitor sock present"; \
	  sleep 15; \
	  $(MAKE) --no-print-directory vm-status; \
	else \
	  echo "error: VM did not come up within 90s — check build/vm-reset.out" >&2; \
	  exit 1; \
	fi

# vm-down — kill the QEMU VM and remove its sockets. Idempotent.
vm-down:
	@pkill -f 'qemu-system-x86' 2>/dev/null && echo "==> killed qemu" || echo "==> no qemu process to kill"
	@rm -f "$(TEMPLE_MON)" "$(TEMPLE_COM2)"
	@echo "==> sockets cleaned"
