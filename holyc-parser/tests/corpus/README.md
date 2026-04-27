# HolyC parser test corpus

Ground-truth corpus of HolyC source snippets, validated by JIT-compiling
each one inside a live TempleOS VM. Used by the Rust parser to
cross-check its own diagnostics.

## Layout

```
corpus/
  passing/   NNN-CATEGORY-shortdesc.hc      # snippet
             NNN-CATEGORY-shortdesc.hc.expected   # always "OK\n"
  failing/   NNN-CATEGORY-shortdesc.hc
             NNN-CATEGORY-shortdesc.hc.expected   # error text from VM
  README.md
  surprises.md      # cases where TempleOS disagreed with parse-spec
```

The Rust port should walk these dirs and, for each `.hc`, run the
parser. For files in `passing/` the parser must produce no diagnostic.
For files in `failing/` the parser must produce a diagnostic whose text
**substring-matches** the `.expected` content (byte-for-byte parity is
not required — TempleOS prepends `&LexExcept` call-stack noise that we
do not reproduce).

## Naming

`NNN-CATEGORY-shortdesc.hc`

- `NNN` is a zero-padded 3-digit index per directory.
- `CATEGORY` is one of:
  - `atom`     — primary expressions (literals, idents, scalar types)
  - `expr`     — operators, precedence, sizeof/offset/defined, calls
  - `stmt`     — statement forms (if/for/while/switch/try/asm/lock/...)
  - `decl`     — declarations (var, fn, class, union, modifiers)
  - `bug-compat`— intentional reproductions of `parse-spec.md` §5
  - `errors`   — generic syntax/semantic errors
  - `surprises`— cases where VM behaviour diverged from the parse-spec
                  prediction; see `surprises.md`
- `shortdesc` is lowercase-with-hyphens.

## Generation pipeline

1. Boot the live TempleOS VM:
   ```
   bash scripts/boot-temple.sh dev
   ```
   The boot script auto-dismisses the bootloader menu (`AUTO_BOOT=1`)
   and the post-boot Take-Tour prompt.

2. Run the daemon bootstrap once per VM lifetime:
   ```
   python3 scripts/corpus-run.py bootstrap
   ```
   This types the COM2-fed JIT daemon into adam's REPL and upgrades to
   stage-2 (the `D2` daemon, which captures compile errors over COM1
   between `COMPILE_ERR_BEGIN` / `COMPILE_ERR_END` markers — see PR #8
   `feat/com1-error-capture`).

3. Generate snippet `.hc` files from the source-of-truth:
   ```
   python3 scripts/gen-corpus.py
   ```

4. Validate. Each `.hc` is pushed over COM2, JIT-compiled by `ExePutS`,
   and the result (`COMPILE_OK` or the framed error text) is captured:
   ```
   python3 scripts/corpus-run.py validate --write-expected \
       holyc-parser/tests/corpus/passing
   python3 scripts/corpus-run.py validate --write-expected \
       holyc-parser/tests/corpus/failing
   ```
   With `--write-expected`, the captured text is also stored as the
   sibling `.expected` file. Without it, only `.actual` is written
   (useful for sanity-checking after a change).

## `.expected` format

- `OK\n` — the snippet compiled cleanly.
- Otherwise: the captured COM1 error block, stripped of the
  `COMPILE_ERR_BEGIN` / `COMPILE_ERR_END` framing. Each line begins
  with `&LexExcept …` (the call-stack tag) and includes the message
  (e.g. `ERROR: Missing ';' at"…"`). Multiple lines are possible when
  one snippet triggers cascading throws (see e.g.
  `failing/006-bug-compat-bug59-c-style-cast.hc.expected`).

The Rust parser cross-comparison should use **substring matching** on
the human-readable message portion (`ERROR: …`), not byte-for-byte
matching, since the parser doesn't replicate TempleOS's internal call
chain.

## TempleOS version

- Disk image: `vendor/templeos/disk.qcow2`
  - md5: `ca6c7ce361ef5a186531c66e33effb48`
  - Source: copied from `temple-quake/devkit/vendor/templeos/disk.qcow2`,
    a fresh install of Terry's 2017 distro from
    https://templeos.org/Downloads/TempleOS.ISO performed in early 2026.
  - On-disk OS: `TempleOS V5.03` (visible in the boot screen banner).
- Devkit branch the corpus was validated against:
  `feat/parser-corpus` (HEAD `d430751` at corpus authoring time),
  which merges `feat/holyc-parser` into `feat/com1-error-capture` and
  adds the boot-script auto-dismiss patches.

## §5 (parse-spec) bug-compat coverage map

| §    | Behaviour                                           | Snippet |
| ---- | --------------------------------------------------- | ------- |
| 5.1  | `return` at file scope                              | `failing/001-bug-compat-bug51-toplevel-return.hc` |
| 5.1  | `LABEL:` at file scope                              | `failing/002-bug-compat-bug51-toplevel-label.hc` |
| 5.2  | `continue` is not a keyword                         | `failing/007-bug-compat-bug52-continue-keyword.hc` |
| 5.3  | `if (cond) static I64 x;` at file scope             | `passing/.../bug53-if-static-decl.hc` (parses; static is unconditional — see surprises.md note) |
| 5.4  | `for(I64 i; …)` at file scope                       | `failing/003-bug-compat-bug54-for-decl-filescope.hc` |
| 5.5  | `do { } while (…)` missing trailing `;`             | `failing/004-bug-compat-bug55-dowhile-missing-semi.hc` |
| 5.6  | Nested grouping-paren typecast `((F64)x)`           | `failing/005-bug-compat-bug56-nested-typecast.hc` |
| 5.7  | `&Internal_Fun` (e.g. `&Sqr`)                       | `failing/008-bug-compat-bug57-addr-of-internal.hc` |
| 5.8  | `Bt(&v, n)` parses as a fun call                    | `passing/.../bug58-bt-as-call.hc` |
| 5.9  | C-style prefix cast `(I64)x`                        | `failing/006-bug-compat-bug59-c-style-cast.hc` |
| 5.10 | Default-arg expression evaluated once at parse time | `passing/.../bug510-default-arg-once.hc` |
| 5.11 | Switch range > 0xFFFF                               | `failing/024-bug-compat-bug511-switch-range-huge.hc` |
| 5.12 | `case :` (no expression) auto-increments            | `passing/.../stmt-switch-case-auto.hc` |
| 5.13 | Implicit `Print` ends arg list at `;` not `)`       | `passing/.../bug513-implicit-print.hc` |

## Distribution

| Category    | Passing | Failing |
| ----------- | ------- | ------- |
| atom        | 42      | 0       |
| expr        | 69      | 0       |
| stmt        | 34      | 0       |
| decl        | 32      | 0       |
| bug-compat  | 4       | 10      |
| errors      | 1       | 13      |
| surprises   | 3       | 2       |
| **total**   | **185** | **25**  |
| **all**     | **210** |         |
