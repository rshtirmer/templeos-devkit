.PHONY: setup disk shuttle install boot boot-disk dev repl wire-makehome test test-fast lint watch clean clean-disk help \
	setup-temple disk-temple install-temple boot-temple-disk dev-temple test-temple

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
	@echo "make watch          re-run 'make test' on src/ or tests/ change (needs fswatch)"
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
	@python3 scripts/holyc-lint.py src tests

# Re-run the test loop on any change under src/ or tests/. Single shot per
# event; if you save 5 files in 200ms, fswatch coalesces. macOS only;
# `brew install fswatch` if missing.
watch:
	@command -v fswatch >/dev/null || { echo "error: fswatch not installed (brew install fswatch)"; exit 1; }
	@echo "==> watching src/ tests/ — Ctrl-C to stop"
	@fswatch -o src tests | xargs -n1 -I{} $(MAKE) test

clean:
	rm -rf build

clean-disk:
	rm -f $(DISK)
	@echo "disk wiped — run 'make install' to reinstall"
