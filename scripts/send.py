#!/usr/bin/env python3
"""Send a string to the running QEMU VM as keystrokes via the monitor socket.

Usage:
    scripts/send.py 'Dir("B:/");' --enter
    scripts/send.py --enter           # just press Enter
    scripts/send.py --key ret         # send a single named key

The monitor socket path is build/qemu.sock relative to repo root.
"""
import argparse
import socket
import sys
import time
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
SOCK = REPO / "build" / "qemu.sock"

# Map ASCII char -> QEMU sendkey arg.
# QEMU keynames: https://qemu.readthedocs.io/en/latest/system/keys.html
KEYMAP = {
    " ": "spc",
    "\n": "ret",
    "\t": "tab",
    "`": "grave_accent",
    "-": "minus",
    "=": "equal",
    "[": "bracket_left",
    "]": "bracket_right",
    "\\": "backslash",
    ";": "semicolon",
    "'": "apostrophe",
    ",": "comma",
    ".": "dot",
    "/": "slash",
}
SHIFT_MAP = {
    "~": "grave_accent",
    "!": "1",
    "@": "2",
    "#": "3",
    "$": "4",
    "%": "5",
    "^": "6",
    "&": "7",
    "*": "8",
    "(": "9",
    ")": "0",
    "_": "minus",
    "+": "equal",
    "{": "bracket_left",
    "}": "bracket_right",
    "|": "backslash",
    ":": "semicolon",
    '"': "apostrophe",
    "<": "comma",
    ">": "dot",
    "?": "slash",
}


def char_to_sendkey(ch: str) -> str:
    if ch.isalpha():
        if ch.isupper():
            return f"shift-{ch.lower()}"
        return ch
    if ch.isdigit():
        return ch
    if ch in KEYMAP:
        return KEYMAP[ch]
    if ch in SHIFT_MAP:
        return f"shift-{SHIFT_MAP[ch]}"
    raise ValueError(f"unmappable character: {ch!r}")


def send_lines(lines: list[str], delay: float) -> str:
    if not SOCK.exists():
        sys.exit(f"error: {SOCK} not found — is QEMU running with -monitor?")
    with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as s:
        s.connect(str(SOCK))
        s.settimeout(0.5)
        try:
            s.recv(8192)  # drain banner
        except socket.timeout:
            pass
        # Throwaway "shift" sendkey first — the very first scancode after
        # connecting to the monitor is intermittently dropped (banner-drain
        # race). A modifier-only press is benign on the ZealOS side: it
        # doesn't generate a typed character.
        s.sendall(b"sendkey shift\n")
        time.sleep(delay)
        # Send each sendkey command, with a short inter-key delay so the
        # guest scancode buffer doesn't drop keys. Don't block on recv
        # between sends — QEMU's monitor will echo prompts but we don't
        # care; we just need the keys delivered.
        for line in lines:
            s.sendall((line + "\n").encode())
            time.sleep(delay)
        # Drain whatever's pending so the next caller starts clean.
        time.sleep(0.1)
        out = []
        try:
            while True:
                chunk = s.recv(8192)
                if not chunk:
                    break
                out.append(chunk.decode("utf-8", errors="replace"))
        except socket.timeout:
            pass
        return "".join(out)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("text", nargs="?", default="", help="text to type")
    ap.add_argument("--enter", action="store_true", help="press Enter at end")
    ap.add_argument("--key", action="append", default=[], help="send a named key (e.g. ret, esc, f1)")
    ap.add_argument("--delay", type=float, default=0.05, help="seconds between keys")
    args = ap.parse_args()

    lines = []
    for ch in args.text:
        lines.append(f"sendkey {char_to_sendkey(ch)}")
    for k in args.key:
        lines.append(f"sendkey {k}")
    if args.enter:
        lines.append("sendkey ret")

    if not lines:
        sys.exit("error: nothing to send")

    print(f"==> sending {len(lines)} keystroke(s) at {args.delay}s spacing")
    out = send_lines(lines, args.delay)
    # Filter monitor noise; show only meaningful lines.
    for line in out.splitlines():
        line = line.strip()
        if line and not line.startswith("(qemu)") and "sendkey" not in line:
            print(line)


if __name__ == "__main__":
    main()
