#!/usr/bin/env python3
"""holyc-lint.py — host-side HolyC (.ZC / .HC / .HH) linter.

Zero-dep, stdlib only. Catches the boot-phase quirks documented in
NOTES.md plus garden-variety syntax hazards. Output format mirrors
gcc/eslint:

    path:line:col: level: message  [rule]

Exit code 1 if any error-level diagnostic was emitted, else 0.
Warnings don't fail the build.

Usage:
    scripts/holyc-lint.py src tests
    scripts/holyc-lint.py path/to/File.ZC

The rules are conservative — this is a regex/token linter, not the
HolyC parser. The ground-truth path for the boot-phase quirks is still
`make repl` + `scripts/zpush.sh`. This script is the fast, offline
approximation.
"""

from __future__ import annotations

import os
import re
import sys
from dataclasses import dataclass
from pathlib import Path
from typing import Callable, Iterable

# ---------------------------------------------------------------- token model

TYPES = {
    "U0", "U8", "U16", "U32", "U64",
    "I0", "I8", "I16", "I32", "I64",
    "F64", "Bool",
}

_CLASS_TYPE_RE = re.compile(r"^C[A-Z][A-Za-z0-9_]*$")

KEYWORDS = {
    "if", "else", "while", "for", "do",
    "switch", "case", "default", "break", "continue",
    "goto", "return",
    "try", "catch", "throw",
    "start", "end",
    "extern", "_extern", "import", "_import",
    "public", "static",
    "interrupt", "haserrcode", "argpop", "noargpop", "reg", "noreg",
    "no_warn", "lastclass", "lock",
    "class", "union",
    "sizeof", "offset",
    "TRUE", "FALSE", "NULL", "ON", "OFF",
    "asm",
}


@dataclass
class Token:
    kind: str          # comment | preproc | string | char | number | ident | keyword | type | punct | ws
    value: str
    line: int
    col: int


@dataclass
class Diag:
    path: str
    level: str         # error | warning
    line: int
    col: int
    msg: str
    rule: str


def tokenize(src: str, on_error: Callable[[Diag], None] | None = None,
             path: str = "<input>") -> list[Token]:
    """Lex HolyC source. Lossy on whitespace classification but keeps
    enough structure for the rules below.

    Errors during lexing (unterminated string / block comment) get
    reported via on_error and the lexer recovers as best it can.
    """
    out: list[Token] = []
    i = 0
    line = 1
    col = 1
    N = len(src)

    def advance(n: int = 1) -> None:
        nonlocal i, line, col
        for _ in range(n):
            if i >= N:
                return
            if src[i] == "\n":
                line += 1
                col = 1
            else:
                col += 1
            i += 1

    def at_line_start(idx: int) -> bool:
        j = idx - 1
        while j >= 0 and src[j] in " \t":
            j -= 1
        return j < 0 or src[j] == "\n"

    while i < N:
        sl, sc, si = line, col, i
        ch = src[i]
        ch2 = src[i + 1] if i + 1 < N else ""

        # whitespace
        if ch in " \t\r\n":
            while i < N and src[i] in " \t\r\n":
                advance()
            out.append(Token("ws", src[si:i], sl, sc))
            continue

        # line comment
        if ch == "/" and ch2 == "/":
            while i < N and src[i] != "\n":
                advance()
            out.append(Token("comment", src[si:i], sl, sc))
            continue

        # block comment
        if ch == "/" and ch2 == "*":
            advance(2)
            closed = False
            while i < N:
                if src[i] == "*" and i + 1 < N and src[i + 1] == "/":
                    advance(2)
                    closed = True
                    break
                advance()
            if not closed and on_error is not None:
                on_error(Diag(path, "error", sl, sc,
                              "unterminated block comment", "lex"))
            out.append(Token("comment", src[si:i], sl, sc))
            continue

        # preprocessor — only at line start
        if ch == "#" and at_line_start(si):
            while i < N and src[i] != "\n":
                advance()
            out.append(Token("preproc", src[si:i], sl, sc))
            continue

        # string
        if ch == '"':
            advance()
            closed = False
            while i < N:
                if src[i] == "\\" and i + 1 < N:
                    advance(2)
                    continue
                if src[i] == '"':
                    advance()
                    closed = True
                    break
                if src[i] == "\n":
                    break
                advance()
            if not closed and on_error is not None:
                on_error(Diag(path, "error", sl, sc,
                              "unterminated string literal", "lex"))
            out.append(Token("string", src[si:i], sl, sc))
            continue

        # char literal — HolyC permits multi-char ('ABC')
        if ch == "'":
            advance()
            closed = False
            while i < N:
                if src[i] == "\\" and i + 1 < N:
                    advance(2)
                    continue
                if src[i] == "'":
                    advance()
                    closed = True
                    break
                if src[i] == "\n":
                    break
                advance()
            if not closed and on_error is not None:
                on_error(Diag(path, "error", sl, sc,
                              "unterminated char literal", "lex"))
            out.append(Token("char", src[si:i], sl, sc))
            continue

        # numbers
        if ch.isdigit() or (ch == "." and ch2.isdigit()):
            if ch == "0" and ch2 in "xX":
                advance(2)
                while i < N and (src[i].isdigit() or src[i] in "abcdefABCDEF_"):
                    advance()
            else:
                while i < N and (src[i].isdigit() or src[i] == "_"):
                    advance()
                if i < N and src[i] == ".":
                    advance()
                    while i < N and (src[i].isdigit() or src[i] == "_"):
                        advance()
                if i < N and src[i] in "eE":
                    advance()
                    if i < N and src[i] in "+-":
                        advance()
                    while i < N and src[i].isdigit():
                        advance()
            out.append(Token("number", src[si:i], sl, sc))
            continue

        # identifier
        if ch.isalpha() or ch == "_":
            while i < N and (src[i].isalnum() or src[i] == "_"):
                advance()
            word = src[si:i]
            if word in TYPES:
                kind = "type"
            elif word in KEYWORDS:
                kind = "keyword"
            elif _CLASS_TYPE_RE.match(word):
                # ZealOS class convention: C-prefix uppercase
                # (CFifoU8, CBlkDev, CTCPSocket).
                kind = "type"
            else:
                kind = "ident"
            out.append(Token(kind, word, sl, sc))
            continue

        # punctuation — coalesce 2-char operators where useful
        two = src[i:i + 2]
        if two in {"==", "!=", "<=", ">=", "&&", "||", "<<", ">>",
                  "+=", "-=", "*=", "/=", "%=", "&=", "|=", "^=",
                  "++", "--", "->", "..", "::"}:
            advance(2)
            out.append(Token("punct", two, sl, sc))
            continue
        advance()
        out.append(Token("punct", ch, sl, sc))

    return out


# -------------------------------------------------------------------- linter

def lint(path: str, src: str) -> list[Diag]:
    diags: list[Diag] = []

    def push(level: str, line: int, col: int, msg: str, rule: str) -> None:
        diags.append(Diag(path, level, line, col, msg, rule))

    tokens = tokenize(src, on_error=diags.append, path=path)

    # --- balance: { } ( ) [ ]  -----------------------------------------
    opener = {")": "(", "]": "[", "}": "{"}
    stack: list[Token] = []
    for t in tokens:
        if t.kind != "punct":
            continue
        if t.value in {"(", "[", "{"}:
            stack.append(t)
        elif t.value in {")", "]", "}"}:
            want = opener[t.value]
            if not stack:
                push("error", t.line, t.col,
                     f"stray '{t.value}' with no matching '{want}'",
                     "balance")
            elif stack[-1].value != want:
                top = stack[-1]
                push("error", t.line, t.col,
                     f"mismatched '{t.value}' — expected to close "
                     f"'{top.value}' from {top.line}:{top.col}",
                     "balance")
                stack.pop()
            else:
                stack.pop()
    for t in stack:
        push("error", t.line, t.col,
             f"unclosed '{t.value}' opened here", "balance")

    # --- top-level boot-phase hazards (NOTES.md) -----------------------
    code = [t for t in tokens if t.kind not in {"ws", "comment"}]
    depth = 0
    for idx, t in enumerate(code):
        if t.kind == "punct" and t.value == "{":
            depth += 1
            continue
        if t.kind == "punct" and t.value == "}":
            depth = max(0, depth - 1)
            continue
        if depth != 0:
            continue
        if t.kind != "keyword":
            continue

        if t.value == "return":
            push("warning", t.line, t.col,
                 "top-level 'return' — boot-phase parser rejects this; "
                 "see NOTES.md", "boot-phase-return")

        if t.value == "goto":
            push("warning", t.line, t.col,
                 "top-level 'goto' — boot-phase parser rejects global "
                 "labels", "boot-phase-goto")

        if t.value in {"for", "while"}:
            # find body start (skip header parens for `for`/`while`)
            j = idx + 1
            paren = 0
            body_at = -1
            while j < len(code):
                u = code[j]
                if u.kind == "punct" and u.value == "(":
                    paren += 1
                elif u.kind == "punct" and u.value == ")":
                    paren -= 1
                elif u.kind == "punct" and u.value == "{" and paren == 0:
                    body_at = j
                    break
                elif u.kind == "punct" and u.value == ";" and paren == 0:
                    break
                j += 1
            if body_at >= 0:
                bd = 1
                k = body_at + 1
                has_type = False
                while k < len(code) and bd > 0:
                    u = code[k]
                    if u.kind == "punct" and u.value == "{":
                        bd += 1
                    elif u.kind == "punct" and u.value == "}":
                        bd -= 1
                    elif u.kind == "type":
                        has_type = True
                    k += 1
                if has_type:
                    push("warning", t.line, t.col,
                         f"top-level '{t.value}' body declares a type — "
                         "boot-phase parser raises 'Undefined "
                         "identifier'; wrap in a function and Spawn() "
                         "(see NOTES.md)", "boot-phase-loop")

    # --- Sys("InfiniteFn;") deadlock pattern ---------------------------
    # Sys() blocks the caller until the queued source returns. If the
    # body is just `Identifier;` and that identifier is a function with
    # an infinite loop, the caller deadlocks. We only have a heuristic:
    # flag bare-identifier Sys() calls and let the human judge.
    for idx in range(len(code) - 3):
        a, b, s, d = code[idx], code[idx + 1], code[idx + 2], code[idx + 3]
        if (a.kind == "ident" and a.value == "Sys"
                and b.kind == "punct" and b.value == "("
                and s.kind == "string"
                and d.kind == "punct" and d.value == ")"):
            body = s.value[1:-1].strip()
            if re.fullmatch(r"[A-Za-z_][A-Za-z0-9_]*\s*;?", body):
                push("warning", a.line, a.col,
                     f'Sys({s.value}) blocks until the queued source '
                     "returns — if it's an infinite loop, use Spawn() "
                     "instead (see NOTES.md)", "sys-deadlock")

    # --- parametrized #define ------------------------------------------
    # HolyC's parametrized macros are unreliable when JIT-pushed: tokens
    # may not substitute correctly across ExePutS chunks. Prefer an
    # out-of-line function.
    _PARAM_DEFINE_RE = re.compile(r"^(\s*)#define\s+\w+\s*\(")
    for ln_idx, ln in enumerate(src.split("\n")):
        m = _PARAM_DEFINE_RE.match(ln)
        if m:
            push("warning", ln_idx + 1, len(m.group(1)) + 1,
                 "parametrized #define is unreliable in HolyC; prefer "
                 "an out-of-line function",
                 "parametrized-define")

    # --- exponent-form float literals ----------------------------------
    # HolyC's parser rejects `1e-9` style literals ("Missing )"). The
    # tokenizer above happily eats them, so we just inspect number
    # tokens. Hex literals like 0x1e9 are a different shape (start with
    # 0x) and won't match.
    _EXP_FLOAT_RE = re.compile(r"^\d+(\.\d+)?[eE][+-]?\d+$")
    for t in tokens:
        if t.kind != "number":
            continue
        if t.value.startswith("0x") or t.value.startswith("0X"):
            continue
        if _EXP_FLOAT_RE.match(t.value):
            push("error", t.line, t.col,
                 f"exponent-form float literal '{t.value}' not "
                 "supported; use plain decimal e.g. 0.000000001",
                 "exponent-float-literal")

    # --- comma-separated typed decls / multi-array decls ---------------
    # HolyC's PrsVarLst chokes on `F64 a, b;` and `F64 m[9], im[9];`.
    # We only flag these at statement scope — function-parameter lists
    # (inside parentheses) legitimately use commas. We track paren depth
    # over the full token stream so we don't fire inside prototypes or
    # call sites.
    _COMMA_DECL_RE = re.compile(
        r"\b(F64|F32|I0|I8|I16|I32|I64|U0|U8|U16|U32|U64|Bool)"
        r"\s+\w+\s*,\s*\w+"
    )
    _MULTI_ARRAY_RE = re.compile(
        r"\b(F64|F32|I0|I8|I16|I32|I64|U0|U8|U16|U32|U64|Bool)"
        r"\s+\w+\s*\[\s*\d+\s*\]\s*,\s*\w+\s*\[\s*\d+\s*\]"
    )
    # Build a per-character paren-depth array over the raw source so we
    # can tell whether a regex match starts inside a paren group
    # (function params, prototypes, call args). Strings, char literals
    # and comments don't count toward depth.
    depth_at = [0] * (len(src) + 1)
    masked_chars = list(src)
    for t in tokens:
        if t.kind in {"string", "char", "comment"}:
            # locate the token offset by re-walking via line/col is
            # awkward; instead clear those ranges below using a second
            # pass. Skip here.
            pass
    # Walk source character-by-character, honoring string/char/comment
    # to avoid counting parens inside them.
    cur_depth = 0
    j = 0
    Nsrc = len(src)
    in_line_comment = False
    in_block_comment = False
    in_string = False
    string_quote = ""
    while j < Nsrc:
        depth_at[j] = cur_depth
        ch = src[j]
        nxt = src[j + 1] if j + 1 < Nsrc else ""
        if in_line_comment:
            if ch == "\n":
                in_line_comment = False
            j += 1
            continue
        if in_block_comment:
            if ch == "*" and nxt == "/":
                in_block_comment = False
                depth_at[j + 1] = cur_depth
                j += 2
                continue
            j += 1
            continue
        if in_string:
            if ch == "\\" and j + 1 < Nsrc:
                depth_at[j + 1] = cur_depth
                j += 2
                continue
            if ch == string_quote:
                in_string = False
            elif ch == "\n":
                in_string = False
            j += 1
            continue
        if ch == "/" and nxt == "/":
            in_line_comment = True
            j += 2
            continue
        if ch == "/" and nxt == "*":
            in_block_comment = True
            j += 2
            continue
        if ch == '"' or ch == "'":
            in_string = True
            string_quote = ch
            j += 1
            continue
        if ch == "(":
            cur_depth += 1
        elif ch == ")":
            cur_depth = max(0, cur_depth - 1)
        j += 1
    depth_at[Nsrc] = cur_depth

    # Map (line, col) of line-start to absolute offset.
    line_offsets = [0]
    for idx, ch in enumerate(src):
        if ch == "\n":
            line_offsets.append(idx + 1)

    for ln_idx, ln in enumerate(src.split("\n")):
        lineno = ln_idx + 1
        base = line_offsets[ln_idx] if ln_idx < len(line_offsets) else 0
        # strip line comments to avoid false positives
        code_part = ln
        cidx = code_part.find("//")
        if cidx >= 0:
            code_part = code_part[:cidx]
        m = _MULTI_ARRAY_RE.search(code_part)
        if m and depth_at[base + m.start()] == 0:
            push("error", lineno, m.start() + 1,
                 "multiple array declarations in one statement choke "
                 "PrsVarLst; split each array decl onto its own line",
                 "multi-array-decl")
            continue
        m = _COMMA_DECL_RE.search(code_part)
        if m and depth_at[base + m.start()] == 0:
            push("error", lineno, m.start() + 1,
                 "comma-separated typed declarations are rejected by "
                 "PrsVarLst; split into separate declarations: "
                 "`F64 a; F64 b;`",
                 "comma-decl-list")

    # --- F32 references -------------------------------------------------
    # HolyC has only F64. F32 in source is always a porting bug.
    for t in tokens:
        if t.kind in {"comment", "string", "char"}:
            continue
        if t.value == "F32" and (t.kind == "ident" or t.kind == "type"):
            push("error", t.line, t.col,
                 "HolyC has no F32 type — use F64",
                 "f32-reference")

    # --- reserved-name parameter collisions -----------------------------
    # Names like `eps`, `pi`, `inf`, `nan` are TempleOS compile-time F64
    # constants installed in the kernel symbol table. Used as parameter
    # names they get resolved to the constant value DURING PrsType, so
    # the parser sees an INT in a position expecting `*` and trips
    # with: "Expecting '*' at INT:<bits>". Diagnosed in temple-quake's
    # Phase-1 mathlib port (see CLAUDE.md). Catch the common ones at
    # parameter-list and local-decl positions.
    #
    # Match: TYPE_KW [`*` …] NAME — when NAME is a reserved word.
    _RESERVED_NAMES = {
        "eps", "pi", "inf", "nan",   # F64 constants
        "tS",                          # ZealOS only — but harmless to flag
        "ms",                          # mouse global
    }
    _PARAM_DECL_RE = re.compile(
        r"\b("
        r"U0|I0|U8|I8|Bool|U16|I16|U32|I32|U64|I64|F64"
        r")\b\s*\**\s*([A-Za-z_][A-Za-z0-9_]*)"
    )
    for ln_idx, ln in enumerate(src.split("\n")):
        lineno = ln_idx + 1
        # strip line comments
        cidx = ln.find("//")
        code_part = ln if cidx < 0 else ln[:cidx]
        for m in _PARAM_DECL_RE.finditer(code_part):
            name = m.group(2)
            if name in _RESERVED_NAMES:
                push("error", lineno, m.start(2) + 1,
                     f"`{name}` is a TempleOS reserved compile-time constant; "
                     f"using it as a parameter / local name causes PrsType to "
                     f"resolve it to the constant value (e.g. eps = 2.22e-16) "
                     f"and trip 'Expecting *'. Rename to `tol`/`epsilon`/etc.",
                     "reserved-name-collision")

    # --- per-line lints ------------------------------------------------
    for ln_idx, ln in enumerate(src.split("\n")):
        if "\t" in ln:
            push("warning", ln_idx + 1, ln.index("\t") + 1,
                 "tab character — repo style is 2-space indent",
                 "no-tabs")
        stripped = ln.rstrip(" \t")
        if stripped != ln:
            push("warning", ln_idx + 1, len(stripped) + 1,
                 "trailing whitespace", "trailing-whitespace")
        if len(ln) > 100:
            push("warning", ln_idx + 1, 101,
                 f"line is {len(ln)} cols (limit 100)",
                 "max-line-length")

    return diags


# ----------------------------------------------------------------- formatting

RESET = "\x1b[0m"
BOLD = "\x1b[1m"
RED = "\x1b[31m"
YELLOW = "\x1b[33m"
CYAN = "\x1b[36m"
DIM = "\x1b[2m"


def use_color() -> bool:
    return sys.stdout.isatty() and not os.environ.get("NO_COLOR")


def c(code: str, s: str) -> str:
    return code + s + RESET if use_color() else s


def format_diag(d: Diag) -> str:
    lvl = (c(BOLD + RED, "error") if d.level == "error"
           else c(BOLD + YELLOW, "warning"))
    loc = c(BOLD, f"{d.path}:{d.line}:{d.col}")
    rule = c(DIM, f"[{d.rule}]")
    return f"{loc}: {lvl}: {d.msg} {rule}"


# --------------------------------------------------------------------- driver

EXTS = {".ZC", ".HC", ".HH"}


def collect(paths: Iterable[str]) -> list[Path]:
    out: list[Path] = []
    for p in paths:
        path = Path(p)
        if path.is_dir():
            for f in sorted(path.rglob("*")):
                if f.is_file() and f.suffix.upper() in EXTS:
                    out.append(f)
        elif path.is_file():
            out.append(path)
        else:
            print(f"holyc-lint: not found: {p}", file=sys.stderr)
    return out


def main(argv: list[str]) -> int:
    if len(argv) < 2:
        print("usage: holyc-lint.py <file-or-dir> [more...]",
              file=sys.stderr)
        return 2
    files = collect(argv[1:])
    errors = 0
    warnings = 0
    for f in files:
        src = f.read_text(encoding="utf-8")
        diags = lint(str(f), src)
        for d in diags:
            print(format_diag(d))
            if d.level == "error":
                errors += 1
            else:
                warnings += 1
    etag = (c(BOLD + RED, f"{errors} error(s)") if errors > 0
            else c(BOLD + CYAN, "0 errors"))
    wtag = (c(BOLD + YELLOW, f"{warnings} warning(s)") if warnings > 0
            else "0 warnings")
    print(f"{etag}, {wtag} in {len(files)} file(s)", file=sys.stderr)
    return 1 if errors > 0 else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
