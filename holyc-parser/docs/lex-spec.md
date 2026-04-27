# HolyC Lexer Spec (TempleOS-faithful)

Source: `Compiler/Lex.HC`, `Compiler/LexLib.HC`, `Compiler/CompilerA.HH`,
`Compiler/CompilerB.HH`, and the dual-token / binary-op tables in
`Compiler/CInit.HC` (function `CmpFillTables`).

This document is a **read-only spec** distilled from those files. It is the
contract a Rust port must satisfy for bug-compat with the TempleOS HolyC
compiler. It does not propose Rust code, only types, behaviors, and quirks.

---

## 0. High-level shape

The TempleOS lexer is a **streaming, character-driven, single-token-at-a-time**
lexer. It is *not* a tokenize-the-whole-file-up-front design.

Key design facts:

1. The single entry point is `I64 Lex(CCmpCtrl *cc)`. It advances the stream
   by exactly one token and stuffs the result into fields of `cc`:
   - `cc->token`         — the token kind (a small int / one of `TK_*`)
   - `cc->cur_i64`       — value for `TK_I64`, `TK_CHAR_CONST`, `TK_INS_BIN_SIZE`
   - `cc->cur_f64`       — value for `TK_F64`
   - `cc->cur_str`       — heap-allocated UTF-8 (really 1252-ish) bytes for
     `TK_IDENT`, `TK_STR`, `TK_INS_BIN`
   - `cc->cur_str_len`   — length of `cur_str` *including* trailing NUL for
     idents (`buf[i++]=0; cur_str_len=i;`) but for `TK_STR` the length is
     bytes including NUL terminator (see below).
   - `cc->hash_entry`    — if the ident matched a hash-table entry, this is
     the hash entry pointer. The lexer does identifier→keyword resolution
     *via the global hash table*, not via a static keyword list.
2. One-char put-back is supported via `CCF_USE_LAST_U16` flag + `cc->last_U16`.
   Many lexer rules speculatively read one char then "unread" it.
3. Whitespace, line comments (`//`), block comments (`/* */`, **nestable**),
   `$$...$$` DolDoc escapes, and preprocessor branching (`#if/#ifdef/#ifndef/
   #ifaot/#ifjit/#else/#endif`) are all handled inside `Lex()`. The caller
   never sees them.
4. Macro expansion (`#define`) happens inline: when an ident resolves to a
   `HTT_DEFINE_STR` hash entry, the lexer pushes a synthetic `CLexFile` with
   the macro body and re-enters its main loop. So **`Lex()` is recursive
   through `LexIncludeStr`** when expanding macros and `#include`s.
5. The character source `LexGetChar()` itself can yield *synthetic chars*
   from a DolDoc-formatted document: in particular `TK_INS_BIN`,
   `TK_INS_BIN_SIZE`, `TK_SUPERSCRIPT`, `TK_SUBSCRIPT`, `TK_NORMALSCRIPT`.
   These are integer values >255 returned through the same channel as ASCII.
6. Backslash-newline does **not** behave like C's continuation. There is no
   general line-splicing. The only place backslash-newline is consumed is
   inside `#define` bodies (see §2.4).

---

## 1. Token enum (Rust-friendly)

TempleOS uses a single I64 "token" namespace shared by raw ASCII chars and
named `TK_*` constants. ASCII-range chars (`;`, `{`, `(`, …) are returned
*as their byte value*. Multi-char and special tokens use `TK_*` values
above the ASCII range (and above 255 to coexist with the synthetic chars).

The Rust port should flatten this into one `enum TokenKind` whose variants
fall into the categories below. The proposed naming uses `PascalCase` and
groups operator variants as their TempleOS `TK_*` name with the `TK_`
prefix dropped.

### 1.1 Pseudo-tokens / control

| TempleOS         | Rust variant      | Notes                                              |
|------------------|-------------------|----------------------------------------------------|
| `TK_EOF` (0)     | `Eof`             | End of all streams.                                |
| `TK_IDENT`       | `Ident(String)`   | Body kept in `cur_str`. Length-limited to STR_LEN. |
| `TK_I64`         | `IntLit(i64)`     | Decimal, hex (`0x`), binary (`0b`). No octal.      |
| `TK_F64`         | `FloatLit(f64)`   | See §2.5 — `i*Pow10I64(...)`-based, lossy.         |
| `TK_CHAR_CONST`  | `CharLit(i64)`    | 1–8 bytes packed little-endian into i64.           |
| `TK_STR`         | `StrLit(Vec<u8>)` | NUL-terminated, can be split across `\` continuations via `LexExtStr`. |
| `TK_DOT_DOT`     | `DotDot`          | `..` token.                                        |
| `TK_ELLIPSIS`    | `Ellipsis`        | `...` token.                                       |
| `TK_INS_BIN`     | `InsBin(Vec<u8>)` | DolDoc-embedded binary blob (synthetic).           |
| `TK_INS_BIN_SIZE`| `InsBinSize(i64)` | Size of preceding `INS_BIN`.                       |
| `TK_SUPERSCRIPT` | (folded to `>`)   | DolDoc layout marker; `Lex` rewrites to `'>'`.      |
| `TK_SUBSCRIPT`   | (folded to `<`)   | Rewrites to `'<'`.                                 |
| `TK_NORMALSCRIPT`| (folded to `=`)   | Rewrites to `'='`.                                 |
| `TK_TKS_NUM`     | (sentinel)        | Marks the high end of the TK_ enum; never emitted. |

### 1.2 Single-char punctuation/operators (returned as their byte value)

| Char | Rust variant   | Notes |
|------|----------------|-------|
| `;`  | `Semicolon`    | |
| `,`  | `Comma`        | |
| `:`  | `Colon`        | (also part of `::` → `TK_DBL_COLON`) |
| `?`  | `Question`     | Ternary. |
| `(`  | `LParen`       | |
| `)`  | `RParen`       | |
| `{`  | `LBrace`       | Used for compound stmts AND HolyC array initializers. |
| `}`  | `RBrace`       | |
| `[`  | `LBracket`     | |
| `]`  | `RBracket`     | |
| `~`  | `Tilde`        | Bitwise NOT. |
| `!`  | `Bang`         | Logical NOT. (Has dual-form `!=`.) |
| `.`  | `Dot`          | Member access. Also see `..` and `...` below, plus float-leading `.5`. |
| `@`  | `At`           | Only when `CCF_KEEP_AT_SIGN` flag is set; otherwise `@` is part of an ident. |
| `#`  | `Hash`         | Only when `CCF_KEEP_SIGN_NUM` flag is set; otherwise `#` triggers preprocessor. |
| `+`  | `Plus`         | |
| `-`  | `Minus`        | |
| `*`  | `Star`         | Multiply / pointer / deref. |
| `/`  | `Slash`        | Divide / start of `//` or `/*`. |
| `%`  | `Percent`      | |
| `&`  | `Amp`          | |
| `\|` | `Pipe`         | |
| `^`  | `Caret`        | |
| `=`  | `Eq`           | |
| `<`  | `Lt`           | |
| `>`  | `Gt`           | |
| `` ` ``| `Backtick`   | **Power operator** (`a\`b` is a^b). PREC_EXP, right-assoc. |
| `'`  | (handled inside char-const lexer, never returned as a bare token) |
| `"`  | (handled inside string lexer, never returned as a bare token) |
| `\`  | (handled inside string-escape and `#define` continuation, never returned as a bare token) |
| `\n` | `Newline`      | Only emitted when `CCF_KEEP_NEW_LINES` flag set; otherwise eaten. |

Notes:
- `$` (single dollar) cannot be lexed: TempleOS replaces single `$` byte with
  `$$` everywhere internally. The lexer only ever sees `$$` (the double-dollar
  escape that toggles DolDoc formatting / dollar-pair-string mode). See §1.4.
- A single `$$` outside any context becomes a literal `'$$'` token (kept only
  for DolDoc roundtripping); when followed by another `$$` it's emitted as
  `'$$'`. Inside a `$$ ... $$` pair (the formatting escape) the body is
  *skipped entirely*.

### 1.3 Multi-char operator tokens (`TK_*`)

These are produced by the dual-token tables `dual_U16_tokens1/2/3` set up
in `CmpFillTables` (`CInit.HC`). The mechanism: when the lexer reads char
`c1` that has a row in `dual_U16_tokens1`, it peeks `c2`. If `c2` matches
the table's expected second char, the token is the upper-half-word value;
otherwise it falls through to `dual_U16_tokens2` (a second alternative)
and finally `dual_U16_tokens3` (a third alternative). Order matters and
encodes "longest match wins, but with a fixed branching set per leading
char."

Table rows (from `CmpFillTables`):

| Lead | Tier 1                  | Tier 2                  | Tier 3            |
|------|-------------------------|-------------------------|-------------------|
| `!`  | `!=` → `TK_NOT_EQU`     | —                       | —                 |
| `&`  | `&&` → `TK_AND_AND`     | `&=` → `TK_AND_EQU`     | —                 |
| `*`  | `*=` → `TK_MUL_EQU`     | —                       | —                 |
| `+`  | `++` → `TK_PLUS_PLUS`   | `+=` → `TK_ADD_EQU`     | —                 |
| `-`  | `->` → `TK_DEREFERENCE` | `--` → `TK_MINUS_MINUS` | `-=` → `TK_SUB_EQU` |
| `/`  | `/*` (block comment)    | `//` (line comment)     | `/=` → `TK_DIV_EQU` |
| `:`  | `::` → `TK_DBL_COLON`   | —                       | —                 |
| `<`  | `<=` → `TK_LESS_EQU`    | `<<` → `TK_SHL`         | —                 |
| `=`  | `==` → `TK_EQU_EQU`     | —                       | —                 |
| `>`  | `>=` → `TK_GREATER_EQU` | `>>` → `TK_SHR`         | —                 |
| `^`  | `^=` → `TK_XOR_EQU`     | `^^` → `TK_XOR_XOR`     | —                 |
| `\|` | `\|\|` → `TK_OR_OR`     | `\|=` → `TK_OR_EQU`     | —                 |
| `%`  | `%=` → `TK_MOD_EQU`     | —                       | —                 |

Two further special-cases live inline (not in the tables) inside `Lex()`:

- After `<<` or `>>`, the lexer **peeks one more char**: if it is `=`, the
  token becomes `TK_SHL_EQU` / `TK_SHR_EQU`, otherwise the `=` is pushed
  back. Therefore: `<<=` and `>>=` exist as distinct tokens.
- For `..`, after the second `.` the lexer peeks one more char: if it is
  `.`, the token becomes `TK_ELLIPSIS`, else `TK_DOT_DOT`.

Full TK list (operator/punctuation `TK_*`):

| TempleOS         | Rust variant   | Spelling | Cat       |
|------------------|----------------|----------|-----------|
| `TK_NOT_EQU`     | `BangEq`       | `!=`     | cmp       |
| `TK_EQU_EQU`     | `EqEq`         | `==`     | cmp       |
| `TK_LESS_EQU`    | `LtEq`         | `<=`     | cmp       |
| `TK_GREATER_EQU` | `GtEq`         | `>=`     | cmp       |
| `TK_AND_AND`     | `AmpAmp`       | `&&`     | logical   |
| `TK_OR_OR`       | `PipePipe`     | `\|\|`   | logical   |
| `TK_XOR_XOR`     | `CaretCaret`   | `^^`     | logical-xor (HolyC short-circuit XOR) |
| `TK_PLUS_PLUS`   | `PlusPlus`     | `++`     | unary inc |
| `TK_MINUS_MINUS` | `MinusMinus`   | `--`     | unary dec |
| `TK_SHL`         | `Shl`          | `<<`     | shift     |
| `TK_SHR`         | `Shr`          | `>>`     | shift     |
| `TK_SHL_EQU`     | `ShlEq`        | `<<=`    | assign    |
| `TK_SHR_EQU`     | `ShrEq`        | `>>=`    | assign    |
| `TK_MUL_EQU`     | `StarEq`       | `*=`     | assign    |
| `TK_DIV_EQU`     | `SlashEq`      | `/=`     | assign    |
| `TK_MOD_EQU`     | `PercentEq`    | `%=`     | assign    |
| `TK_ADD_EQU`     | `PlusEq`       | `+=`     | assign    |
| `TK_SUB_EQU`     | `MinusEq`      | `-=`     | assign    |
| `TK_AND_EQU`     | `AmpEq`        | `&=`     | assign    |
| `TK_OR_EQU`      | `PipeEq`       | `\|=`    | assign    |
| `TK_XOR_EQU`     | `CaretEq`      | `^=`     | assign    |
| `TK_DEREFERENCE` | `Arrow`        | `->`     | member    |
| `TK_DBL_COLON`   | `ColonColon`   | `::`     | scope     |
| `TK_DOT_DOT`     | `DotDot`       | `..`     | range     |
| `TK_ELLIPSIS`    | `Ellipsis`     | `...`    | varargs   |

There is **no** `TK_` for `?:`; `?` and `:` are both bare ASCII. There is
**no** `TK_` for `,`; comma is bare ASCII. Right-assoc and precedence are
encoded in `cmp.binary_ops` (see §3).

### 1.4 Literals — detail

#### 1.4.1 Integer literals

`Lex.HC` lines 517–562:

- Leading digit is consumed, then **uppercase** of the next char is checked:
  - `'X'` → hex mode: read while `Bt(char_bmp_hex_numeric, ch)`, accumulate
    `i = i<<4 + (ch<='9' ? ch-'0' : ch-'A'+10)`. **Note**: due to HolyC operator
    precedence (`<<` has the same precedence as `*` in HolyC! See §3.) the
    expression `i<<4+ch-'0'` is `i << (4+ch-'0')`. **This is a real TempleOS
    bug-compat point** — but in practice the result is identical for valid hex
    digits because the source `i` is being shifted by enough to clear the
    relevant bits anyway. The Rust port should still implement integer literal
    parsing as `i = i*16 + digit` (the *intended* semantics) unless a corpus
    test shows TempleOS observably differs.
  - `'B'` → binary: read `0`/`1`, accumulate `i = i<<1 + bit`.
  - else → decimal: keep accumulating `i*10 + digit` until a non-digit, `.`,
    `e`, or `E` appears.
- **Octal does not exist.** A leading `0` followed by digits is plain
  decimal — `017` parses as decimal 17, not octal 15.
- After the integer body, if `ch=='.'` or `ch=='e'/'E'`, falls through into
  the float path. Otherwise `cc->cur_i64 = i; token = TK_I64; goto lex_end;`
  with the char put back via `CCF_USE_LAST_U16`.

#### 1.4.2 Float literals

After an int prefix and a `.`:

- The lexer reads more digits into `i` while incrementing a counter `k`.
- On a non-digit non-`e`/`E`, value is `cc->cur_f64 = i * Pow10I64(-k)`.
- On `e`/`E`, optionally `-`, then digits into `j`. Final value is
  `i * Pow10I64( (neg_e ? -j : j) - k )`.
- A leading-dot float (`.5`) is supported via the `'.'` arm of `Lex()`:
  if the char after `.` is in `'0'..'9'`, it jumps to `lex_float_start`
  with `i=0`. So `.5` lexes as a TK_F64 of `5 * 10^-1`.

**Critical bug-compat:** the float is built from a 64-bit integer mantissa
multiplied by a power of 10. There is **no** `strtod`. Implications:

- Mantissas with more than ~18 significant decimal digits silently wrap
  (i64 overflow on `i*10+digit`). Anything past digit 19 is garbage.
- The exponent path uses `Pow10I64(-j-k)` — this is `Pow10I64` of a *negative
  argument*. If TempleOS's `Pow10I64` only accepts non-negative arguments
  (likely, given the name `I64`), then `1e-9` would compute `Pow10I64(-9)`
  and either return 0/1 or crash. **This is the bug we hit.** Treat
  `Pow10I64(neg)` as undefined / returning a small wrong value. Do not use
  Rust `f64::powi` here as a "fix" — the spec is "TempleOS bug-for-bug."
  The Rust port should reproduce the formula and document the bug, with a
  test asserting parity with TempleOS output.
- Trailing exponent with no digits (`1e`) has undefined behavior: the inner
  `while` reads digits forever until a non-digit appears, then falls through
  with `j=0`, so `1e` parses as `1 * Pow10I64(-k) = 1` and the next token
  starts at the char that followed `e`. Bug-compat means we accept `1e`
  silently as `1.0`.

#### 1.4.3 Char constants `'...'`

`Lex.HC` lines 634–685. **Up to 8 bytes packed little-endian into an i64**
(via `k.u8[j]`). Behavior:

- Loop `j = 0..7` reading chars.
- Stops on terminating `'` or NUL.
- Backslash escapes inside char consts (recognised set, full list):
  `\0` `\'` `\\`` `\"` `\\` `\d` `\n` `\r` `\t` `\xHH` `\XHH`. Anything else
  emits a literal `\` and pushes the following char back. Note `\d` emits
  the `$$` byte (literal dollar) — this is HolyC's "escape a dollar."
- `$$` inside a char-const reads a second char: if it's also `$$`, the
  byte `'$$'` is stored; otherwise the second char is unread and a single
  `'$$'` is stored. (Same DolDoc-pair rule as strings.)
- After the loop, if `j` reached 8 without a closing `'`, the lexer reads
  one more char; if that isn't `'`, throws `LexExcept(... "Char const limited
  to 8 chars at ")`. So the maximum is exactly 8 bytes.
- The packed value goes into `cur_i64`. So `'AB'` is `0x4241` (little-endian),
  `'ABCD'` is `0x44434241`, `'ABCDEFGH'` is `0x4847464544434241`. Single-char
  `'A'` is `0x41`. Empty `''` is `0`. **Padding bytes for shorter literals
  are zero** because `k` was zero-initialized.

#### 1.4.4 String literals `"..."`

`Lex.HC` lines 367–439 (`LexInStr`) plus the `'"'` arm in `Lex()`.

- Reads until unescaped `"` or NUL.
- Same escape set as char-const: `\0 \' \` \" \\ \d \n \r \t \xHH`.
- Strings can be **arbitrarily long**; they're built up by repeated
  `LexInStr` calls into a fixed `STR_LEN`-sized scratch buffer with growing
  `MAlloc`/`MemCpy` concatenation.
- The terminating NUL is **stored** in `cur_str_len` (`buf[i++]=0; return i;`).
  So `cc->cur_str_len` for `"abc"` is 4 (`a b c \0`).
- **DolDoc pairs**: an unescaped `$$` inside a string toggles a counter
  (`cc->dollar_cnt`). When `dollar_cnt > 0`, subsequent `$$` are stored as
  literal `'$$'` bytes. When `dollar_cnt == 0`, a `$$` followed by anything
  other than another `$$` causes the *next* `$$` to be required (it sets
  `dollar_cnt=1` and pushes back the byte that wasn't a `$$`). In practice:
  `"foo$$bar$$baz"` stores `foo`, `$$`, `bar`, `$$`, `baz`. The DolDoc
  formatting commands inside the `$$...$$` pair are *not* interpreted at
  this layer — they are kept as raw bytes in the resulting string.
- **String concatenation across newlines**: see `LexExtStr` in `LexLib.HC`.
  After lexing one `TK_STR`, if the very next char is `\` (or if `lex_next`
  is true), the lexer continues into the next token; if that next token
  is also `TK_STR`, the two are merged with the inner NULs collapsed.
  Equivalent to C's adjacent string-literal concatenation, but **opt-in via
  `\` continuation** rather than implicit.
- **No max length**, but a single un-terminated string returns when `LexInStr`
  hits NUL (EOF), so a missing closing `"` quietly truncates the string at
  EOF.

### 1.5 Identifiers

`Lex.HC` lines 467–516.

- Start chars: `A..Z`, `a..z`, `_`, `128..255`. `@` is included only when
  `CCF_KEEP_AT_SIGN` is *not* set (the default).
- Body chars: anything in `cc->char_bmp_alpha_numeric` (a 256-bit char map).
  By default this is `char_bmp_alpha_numeric`; with `CCF_KEEP_AT_SIGN` it is
  `char_bmp_alpha_numeric_no_at`. The bitmap is global state defined in the
  Kernel; the spec is: ASCII letters, digits, `_`, `@`, and high-bit bytes.
- The DolDoc `TK_SUPERSCRIPT/SUBSCRIPT/NORMALSCRIPT` synthetic chars are
  re-mapped to `>`, `<`, `=` and **kept as ident body chars** — so an
  identifier like `foo<sub>bar` (in DolDoc) lexes as `foo<bar` ident.
  Surprising; document for the Rust port but unlikely to come up in plain
  text.
- Length capped at `STR_LEN` chars (TempleOS const, typically 144). Going
  over throws `LexExcept(... "Ident limited to STR_LEN chars at ")`.
- After collecting the ident, the lexer **looks up** the name first in the
  current local-var list (`cc->htc.local_var_lst`) via `MemberFind`, then
  in the hash chain (`cc->htc.hash_table_lst`) via `HashFind`. If the entry
  is `HTT_DEFINE_STR` (a `#define` macro) and `CCF_NO_DEFINES` is not set,
  the lexer **expands the macro inline** by pushing a synthetic file with
  the macro body onto the include stack and looping (no token emitted yet).
  Otherwise the token is `TK_IDENT` with `cc->hash_entry` set (or NULL).

So **the lexer alone cannot tell you "this is a keyword" vs "this is a
user-defined name"** — that distinction lives in the hash table. For the
Rust port, the practical answer is: the lexer always emits `Ident`, and a
post-pass (or the parser) converts keyword idents into keyword tokens by
consulting a hash-table. See §1.6.

### 1.6 Keywords (resolved through the hash table)

The full set of HolyC keywords is the `KW_*` enumeration in `CompilerB.HH`
(lines 239–313 of that file). They're installed as hash entries with
`type & HTT_KEYWORD` set, with `user_data0 = KW_*`. The lexer looks them
up *only* for `#`-directives (the `case '#'` arm of `Lex()` does
`tmph(CHashGeneric *)->user_data0` switching on `KW_*` values). All other
"keywords" (`if`, `else`, `for`, `class`, `U64`, …) are returned as plain
`TK_IDENT` and resolved by the parser (`PrsKeyWord` in `LexLib.HC` proxies
to the same lookup).

Full keyword list, with proposed Rust mapping. Categorized for readability;
the lexer itself does not categorize.

#### 1.6.1 Control flow

| KW           | Spelling   | Rust variant   |
|--------------|------------|----------------|
| `KW_IF`      | `if`       | `KwIf`         |
| `KW_ELSE`    | `else`     | `KwElse`       |
| `KW_FOR`     | `for`      | `KwFor`        |
| `KW_WHILE`   | `while`    | `KwWhile`      |
| `KW_DO`      | `do`       | `KwDo`         |
| `KW_SWITCH`  | `switch`   | `KwSwitch`     |
| `KW_CASE`    | `case`     | `KwCase`       |
| `KW_DFT`     | `default`  | `KwDefault`    |
| `KW_BREAK`   | `break`    | `KwBreak`      |
| `KW_RETURN`  | `return`   | `KwReturn`     |
| `KW_GOTO`    | `goto`     | `KwGoto`       |
| `KW_TRY`     | `try`      | `KwTry`        |
| `KW_CATCH`   | `catch`    | `KwCatch`      |
| `KW_START`   | `start`    | `KwStart`      | (range marker, switch context)
| `KW_END`     | `end`      | `KwEnd`        |
| `KW_SIZEOF`  | `sizeof`   | `KwSizeof`     |
| `KW_OFFSET`  | `offset`   | `KwOffset`     |
| `KW_DEFINED` | `defined`  | `KwDefined`    |
| `KW_ASM`     | `asm`      | `KwAsm`        |

(Note: HolyC has no `continue`. There is no `KW_CONTINUE`.)

#### 1.6.2 Type keywords

The integer/float types are installed by `internal_types_table` in `CInit.HC`:
`I0 I0i U0 U0i I8 I8i U8 U8i Bool I16i U16i I32i U32i I64i U64i F64 F64i`.
Note `Bool` is an `I8` alias, and the `*i` variants are "info-only" types
used by the symbol tables. There is **no** `F32`, `String`, etc. as a true
type; those exist only as `ST_RAW_TYPES` define-list entries.

| Spelling   | Rust variant |
|------------|--------------|
| `U0`       | `TyU0`       |
| `I0`       | `TyI0`       |
| `U8`       | `TyU8`       |
| `I8`       | `TyI8`       |
| `Bool`     | `TyBool`     |
| `U16`      | `TyU16` (via `U16i` info entry) |
| `I16`      | `TyI16`      |
| `U32`      | `TyU32`      |
| `I32`      | `TyI32`      |
| `U64`      | `TyU64`      |
| `I64`      | `TyI64`      |
| `F64`      | `TyF64`      |

#### 1.6.3 Storage / linkage modifiers

| KW             | Spelling     | Rust variant   |
|----------------|--------------|----------------|
| `KW_EXTERN`    | `extern`     | `KwExtern`     |
| `KW__EXTERN`   | `_extern`    | `KwExternBare` |
| `KW_IMPORT`    | `import`     | `KwImport`     |
| `KW__IMPORT`   | `_import`    | `KwImportBare` |
| `KW__INTERN`   | `_intern`    | `KwIntern`     |
| `KW_PUBLIC`    | `public`     | `KwPublic`     |
| `KW_STATIC`    | `static`     | `KwStatic`     |
| `KW_LOCK`      | `lock`       | `KwLock`       |
| `KW_INTERRUPT` | `interrupt`  | `KwInterrupt`  |
| `KW_HASERRCODE`| `haserrcode` | `KwHasErrCode` |
| `KW_LASTCLASS` | `lastclass`  | `KwLastClass`  |
| `KW_NOREG`     | `noreg`      | `KwNoreg`      |
| `KW_REG`       | `reg`        | `KwReg`        |
| `KW_ARGPOP`    | `argpop`     | `KwArgPop`     |
| `KW_NOARGPOP`  | `noargpop`   | `KwNoArgPop`   |
| `KW_NO_WARN`   | `no_warn`    | `KwNoWarn`     |

#### 1.6.4 Class / aggregate

| KW             | Spelling | Rust variant |
|----------------|----------|--------------|
| `KW_CLASS`     | `class`  | `KwClass`    |
| `KW_UNION`     | `union`  | `KwUnion`    |

`intern`, `argpop`, `noargpop`, `nostkchk` show up as KW_* but are storage
modifiers, listed above. `nostkchk` does not appear in `CompilerB.HH`'s
KW_ list — it is a parser-side flag, not a lexer keyword. Confirm in
parser sources (open question §4).

#### 1.6.5 Preprocessor (handled inline by `Lex()`)

| KW             | Spelling     | Lexer behavior |
|----------------|--------------|----------------|
| `KW_INCLUDE`   | `#include`   | Reads next `TK_STR`, opens the file (`LexAttachDoc` if running with DolDoc enabled, else `LexIncludeStr`). |
| `KW_DEFINE`    | `#define`    | Reads ident name, then reads body until newline (with `\`-newline continuation). Stores as `HTT_DEFINE_STR`. |
| `KW_IF`        | `#if`        | Reads expression; if non-zero, falls through to body; else skips to matching `#else`/`#endif`. Nesting handled. |
| `KW_IFDEF`     | `#ifdef`     | Reads ident; takes branch if in hash table. |
| `KW_IFNDEF`    | `#ifndef`    | Inverse of `#ifdef`. |
| `KW_IFAOT`     | `#ifaot`     | Branch if compiling AOT (`CCF_AOT_COMPILE`). |
| `KW_IFJIT`     | `#ifjit`     | Branch if NOT AOT (JIT). |
| `KW_ELSE`      | `#else`      | When seen at top-level (not inside `CCF_IN_IF`), skips to matching `#endif`. |
| `KW_ENDIF`     | `#endif`     | Resolves the current branch. |
| `KW_ASSERT`    | `#assert`    | Lex-time assert; warns if expression is false. |
| `KW_EXE`       | `#exe`       | Reads a stream-block; runs it at compile-time. |
| `KW_HELP_INDEX`| `#help_index`| Sets help index for following definitions. |
| `KW_HELP_FILE` | `#help_file` | Sets help file for following definitions. |

There is **no `#pragma`**. `#error`, `#warning`, `#line` do not exist in
TempleOS. There is **no `#undef`**.

#### 1.6.6 Assembler keywords (`AKW_*`)

These are recognized only inside `asm { ... }` blocks. The lexer itself
does not switch modes; rather, the `asm { }` parser uses these via
`PrsKeyWord` lookup. Listed for completeness:

`ALIGN ORG I0 I8 I16 I32 I64 U0 U8 U16 U32 U64 F64 DU8 DU16 DU32 DU64 DUP
USE16 USE32 USE64 IMPORT LIST NOLIST BINFILE`

The Rust port can map them to `AsmKw*` variants but the lexer doesn't need
to distinguish them — they come back as `TK_IDENT` with a hash-entry
pointing at an `HTT_ASM_KEYWORD`-tagged entry.

### 1.7 DolDoc embedding (`$$ ... $$`)

A `$$` outside a string and outside `CCF_KEEP_SIGN_NUM` puts the lexer
into a "swallow until the matching `$$`" mode (Lex.HC lines 1097–1114):

```
case '$$':
  ch = LexGetChar();
  if (ch == '$$') { token = '$$'; goto lex_end; }   // empty pair
  else if (ch)    { do ch = LexGetChar(); while (ch && ch!='$$');
                    if (!ch) { token = TK_EOF; goto lex_end; }
                    else     goto lex_cont; }       // skip whole pair
  else            { CCF_USE_LAST_U16; token='$$'; goto lex_end; }
```

So **outside strings, `$$ ... $$` is a comment-like region**: the body is
discarded, lexing resumes after the closing `$$`. An empty `$$$$` lexes as
a single `'$$'` punctuation token (same as `'.'`-fallthrough).

**Inside string literals**, the body of `$$ ... $$` is *kept* verbatim as
bytes (because `LexInStr` only swallows when the *opening* quote was inside
a doc and `LexDollar` activated; for plain source, the doc-context branch
is not entered). The pair-counter (`cc->dollar_cnt`) provides a
quasi-balanced tracker.

For the Rust port:

- Implement `$$...$$` outside strings as a *skip*, not a comment-with-text.
- Inside strings, keep the bytes; the parser never sees DolDoc semantics.
- Document the asymmetry as a known quirk (§3).

---

## 2. Lexer state machine sketch

```
fn lex(cc) -> TokenKind:
  loop:                                          # outer "lex_cont" loop
    ch = lex_get_char(cc)
    match ch:
      0:                  return Eof
      TK_SUPERSCRIPT:     ch='>'; goto ident
      TK_SUBSCRIPT:       ch='<'; goto ident
      TK_NORMALSCRIPT:    ch='='; goto ident
      '@' (and !KEEP_AT): return punct(ch)
      [A-Za-z_\x80-\xff]: goto ident
      [0-9]:              goto number
      '"':                goto string
      '\'':               goto char_const
      '#':                if KEEP_SIGN_NUM { return punct('#') }
                          else            { goto preprocessor }
      '\n':               if KEEP_NEW_LINES { return Newline } else continue
      TK_INS_BIN | TK_INS_BIN_SIZE: return synthetic(ch)
      '.':                if KEEP_DOT { return punct('.') }
                          ch1 = lex_get_char(cc)
                          if ch1 in '0'..'9': i=0; goto float_start
                          if ch1 == '.':       goto dot_dot
                          unread; return punct('.')
      ('!' | '$$'..'&' | '('..'-' | '/' | ':'..'?' | '[' | ']'..'^' |
       '{'..'~' | '`'):
                          goto dual_token
      TK_TKS_NUM:         break (sentinel; never happens)

  ident:
    buf = [ch]
    loop:
      if buf.len >= STR_LEN: throw "Ident limited"
      c = lex_get_char(cc)
      if c == 0: break
      if alphanum_bmp[c]: buf.push(c); continue
      if c == TK_SUPERSCRIPT: buf.push('>'); continue
      if c == TK_SUBSCRIPT:   buf.push('<'); continue
      if c == TK_NORMALSCRIPT:buf.push('='); continue
      unread; break
    h = lookup_local(buf) ?? lookup_hash(buf)
    if h is HTT_DEFINE_STR and not NO_DEFINES:
      push_synthetic_file(h.body); goto outer-loop  # macro expansion
    cc.cur_str = buf; cc.hash_entry = h; return Ident

  number:
    i = ch - '0'
    c = upper(lex_get_char(cc))
    if c == 'X':           # hex
      loop: c = upper(lex_get_char()); if hex_bmp[c] { i = i<<4 + digit } else break
      unread c; cc.cur_i64 = i; return Int
    if c == 'B':           # binary
      loop: c = lex_get_char(); if c=='0' i<<=1; elif c=='1' i=i<<1+1; else break
      unread c; cc.cur_i64 = i; return Int
    # decimal
    while dec_bmp[c]: i = i*10 + digit; c = lex_get_char()
    if c not in {'.', 'e', 'E'}:
      unread c; cc.cur_i64 = i; return Int
    if c == '.':
      c = lex_get_char()
      if c == '.':            # "1.." is int-then-dotdot
        flag CCF_LAST_WAS_DOT; unread (somewhat); cc.cur_i64=i; return Int
    float_start:
      k = 0
      while dec_bmp[c]: i = i*10 + digit; k++; c = lex_get_char()
      if c not in {'e','E'}:
        unread c; cc.cur_f64 = i * Pow10I64(-k); return Float
    # exponent
    c = lex_get_char()
    neg_e = (c == '-'); if neg_e: c = lex_get_char()
    j = 0
    while dec_bmp[c]: j = j*10 + digit; c = lex_get_char()
    unread c
    cc.cur_f64 = i * Pow10I64( (neg_e ? -j : j) - k )
    return Float

  char_const:
    if NO_CHAR_CONST: break (fall through; '\'' becomes plain punct? — no,
                             actually 'break' here exits the switch arm and
                             continues outer loop, swallowing the apostrophe)
    k = 0   # i64; use k.u8[0..7]
    for j in 0..8:
      c = lex_get_char()
      if !c or c == '\'': break
      if c == '\\': c = lex_get_char(); k.u8[j] = decode_esc(c)  # see escape table
      elif c == '$$': c2 = lex_get_char(); k.u8[j] = '$$';
                     if c2 != '$$': unread c2
      else: k.u8[j] = c
    if c != '\'' and (c=lex_get_char()) and c != '\'':
      throw "Char const limited to 8 chars"
    cc.cur_i64 = k; return Char

  string:
    flag CCF_IN_QUOTES
    repeatedly call LexInStr(buf, STR_LEN, &done) into a growing heap buffer
    until done. NUL-terminate. clear CCF_IN_QUOTES. return Str

  preprocessor:                # case '#'
    next = lex(cc)             # recursive, expecting an ident
    if next != Ident or !hash_entry or !(hash_entry.type & HTT_KEYWORD):
      return next              # bare '#'-followed-by-non-keyword falls through
    switch hash_entry.user_data0:
      KW_INCLUDE:    expect Str; LexIncludeStr or LexAttachDoc; continue
      KW_DEFINE:     read define body; install HTT_DEFINE_STR; continue
      KW_IF:         lex_if   (cf. §1.6.5)
      KW_IFDEF:      lex_ifdef
      KW_IFNDEF:     lex_ifndef
      KW_IFAOT:      lex_ifaot
      KW_IFJIT:      lex_ifjit
      KW_ELSE:       lex_else
      KW_ENDIF:      ...
      KW_ASSERT:     lex_expr; warn if false
      KW_EXE:        PrsStreamBlk
      KW_HELP_INDEX: read Str; store
      KW_HELP_FILE:  read Str; store
    continue       # most directives don't emit a token

  dot_dot:
    cc.token = DotDot
    if lex_get_char() == '.': cc.token = Ellipsis
    else: unread
    return cc.token

  dual_token:                  # see §1.3 tables
    i = dual_U16_tokens1[ch]
    if i == 0:                 # plain single-char punct
      if ch == '$$': handle DolDoc skip (§1.7)
      else: return punct(ch)
    j = lex_get_char()
    if low16(i) == j:
      tok = high16(i)
      if tok == 0:             # "/*" — start of nestable block comment
        depth = 1
        loop:
          c = lex_get_char()
          if c == 0: return Eof
          if c == '*':
            c = lex_get_char()
            if c == 0: return Eof
            if c == '/': depth--; if depth == 0 break
            else continue scanning with c
          elif c == '/':
            c = lex_get_char()
            if c == 0: return Eof
            if c == '*': depth++; continue
            else continue scanning with c
        continue outer loop
      else:                    # actual dual token
        if tok in {Shl, Shr}:    # peek third char for <<= / >>=
          c = lex_get_char()
          if c == '=': tok = (tok==Shl ? ShlEq : ShrEq)
          else: unread c
        return tok
    # fall through to tier 2
    i2 = dual_U16_tokens2[ch]
    if i2 != 0 and low16(i2) == j:
      tok = high16(i2)
      if tok == 0:             # "//" — line comment
        skip until non-eol; if KEEP_NEW_LINES return Newline else continue
      return tok
    # fall through to tier 3
    i3 = dual_U16_tokens3[ch]
    if i3 != 0 and low16(i3) == j:
      return high16(i3)
    # nope — give back j, return single-char ch
    unread j
    return punct(ch)
```

### 2.1 Whitespace and comments

- Spaces, tabs, vertical tabs, etc. are filtered at the **`LexGetChar`**
  layer for things like `CH_SHIFT_SPACE → CH_SPACE`. Plain spaces and tabs
  fall through `Lex()`'s switch as no-op `case TK_TKS_NUM: break;` (the
  default arm at line 1187), which simply re-runs the outer loop.
  *Implication:* whitespace classification in the Rust port should produce
  no token; just continue.
- `//` line comment: consumes until end-of-line (`Bt(char_bmp_non_eol, ch)`).
  If `CCF_KEEP_NEW_LINES` is set, emits `Newline` token; else continues.
- `/* */` block comment: **nestable**. Tracks `j = depth` and increments on
  embedded `/*`, decrements on `*/`. Unclosed → `TK_EOF`.
- DolDoc `$$ ... $$` outside strings: skip body, no token.

### 2.2 Newlines and continuation

- A bare `\n` is consumed silently by default. With `CCF_KEEP_NEW_LINES`,
  it becomes a `Newline` token.
- **No backslash-newline line continuation in source code generally.** The
  C-style `\<newline>` is meaningless outside specific contexts.
- Inside `#define` bodies only: `\` followed by `\n` (or `\r\n`) continues
  the body onto the next line. `\` followed by `"` inside the body is kept
  as `\"` (so the macro can contain a string).
- Inside string literals, `\` is the standard escape character (no
  newline-splicing).

### 2.3 Multi-char char-literal packing

`'A'`         → 0x0000000000000041
`'AB'`        → 0x0000000000004241  (little-endian byte order)
`'ABCD'`      → 0x0000000044434241
`'ABCDEFGH'`  → 0x4847464544434241
`''`          → 0
`'\n'`        → 0x0A
`'\xFF\x00A'` → 0x000000000041_00_FF (`A` at byte 2; bytes 1 and 3+ zero)

Maximum 8 bytes; 9th non-`'` char throws.

### 2.4 `#define` body parsing

The most complex non-token-emitting section of `Lex()` (lines 710–806).
Salient quirks:

- Skip whitespace (non-EOL) between the macro name and the start of the body.
- Read until `\n`. A `\` at top level swallows the next char; if that next
  char is `\n` or `\r\n`, continuation; otherwise the `\` and the char are
  both kept literally.
- Inside the body, `//` starts a line comment that is **stripped from the
  macro body** (only when `in_str` is false).
- `"` toggles `in_str`. Inside `in_str`, `\"` is preserved; other `\X`
  sequences split awkwardly (the inner loop treats `\X` where `X != "` as
  "keep both, push X back").
- Body buffer grows in `STR_LEN-4` chunks (header reserves 4 bytes for
  `\\`, `ch`, `ch`, NUL).

**Bug-compat concerns** (§3):

- A `//` inside a `"..."` inside a macro body is kept (good).
- A `/` not followed by `/` inside the macro body inserts the `/` and pushes
  the next char back. So `a/b` in a macro body parses fine.
- An odd `\` at end-of-file inside a `#define` is silently appended.

### 2.5 Number parsing — full quirks list

- No `0o` prefix (irrelevant; HolyC has no octal anyway).
- No digit separators (`1_000`).
- Suffixes (`L`, `U`, `f`, `0xFFu`) **do not exist**; an unrecognized char
  ends the number and is pushed back.
- A bare `.` adjacent to a `..` becomes `TK_DOT_DOT` (set via
  `CCF_LAST_WAS_DOT`).
- Leading-dot floats: `.5` works; `.` alone is the member-access op.
- The hex/bin/dec accumulator is a single `i64`; overflow is silent
  truncation of upper bits (not an error).
- The float exponent path uses `Pow10I64(neg-arg)` for `e-N` cases — known
  to misbehave (see §3).

---

## 3. Quirks to bug-compatibly preserve

Numbered for traceability in tests.

**Q1. Nested block comments.** `/* /* */ */` works; standard C does not.
*Behavior to copy:* maintain a depth counter, not a flag.

**Q2. No octal.** `017` is `17`, not `15`. Integer literal regex starts
`[0-9]` and dispatches to hex/bin only on the *second* char being `X`/`B`.

**Q3. Float exponent uses `Pow10I64(i64)` with possibly-negative arg.**
`1e-9` triggers `Pow10I64(-9)`. Whatever TempleOS returns, we must return.
*Action:* in the Rust port, faithfully replicate `Pow10I64` (likely a table
of `10^0..10^18` and unspecified behavior outside that range). Test with
TempleOS-known-values corpus.

**Q4. Float mantissa builds in i64.** A literal with >18 significant digits
(e.g. `1.234567890123456789012`) will silently overflow. Rust port must
mirror the i64 accumulation semantic, not call `f64::from_str`.

**Q5. `i<<4 + ch - '0'` parens bug.** HolyC operator precedence gives `<<`
the same precedence as `*` (higher than `+`), so this parses as
`i << (4 + ch - '0')`. *In practice*, when `ch` is a hex digit `0..9` or
`A..F`, this still produces the right hex value as long as `i` had no
significant bits in that range (which it normally doesn't because each
digit shifts by 4 fresh). But for very large hex literals near i64-MAX, the
exact overflow pattern can differ from `i*16 + digit`. *Action:* Implement
faithfully if a corpus test fails; otherwise document and use the C-style
`i*16 + digit` formulation.

**Q6. 8-byte char-literal packing.** `'ABCDEFGH'` is a valid i64. Up to 8
bytes; 9 throws; 0 is fine (`''` == 0). Endian is little (byte 0 = LSB).

**Q7. `\d` is the dollar escape.** `'\d'` and `"\d"` both produce the byte
`'$$'`. Standard C has no such escape; `\d` in C is implementation-defined
or an error. **Don't normalize this away.**

**Q8. `\x` reads up to 2 hex digits and stops on the first non-hex.**
`"\x4"` is one byte (`0x04`); `"\x4G"` is one byte plus `G`. No 4-digit `\u`,
no `\u{...}`, no `\U`. Same for `\X`.

**Q9. Unknown escape `\Z` keeps the `\` and pushes `Z` back.** Standard C
is undefined; ANSI C says it's implementation-defined; many compilers
silently strip the `\`. TempleOS keeps both bytes by re-emitting `\` and
re-lexing `Z`.

**Q10. Single-quote on EOF / 9th-char overflow throws.** `LexExcept` is a
hard failure; the lexer doesn't recover.

**Q11. Identifiers can include `@` by default.** `foo@bar` is one ident
unless `CCF_KEEP_AT_SIGN` is set. Same for high-bit bytes (`128..255`),
which are valid ident body chars — meaning latin-1 names like `pâté` are
identifiers, but the byte sequence is taken raw (no UTF-8 decoding).

**Q12. Identifiers can include `>`, `<`, `=`** when those came from DolDoc
`TK_SUPERSCRIPT/SUBSCRIPT/NORMALSCRIPT` synthetic chars. In a plain text
file this never triggers, but if you feed a `.DD` (DolDoc) file the lexer,
super/subscripts inside an ident embed those chars literally.

**Q13. `$$` outside strings is a region-skip, not a comment-with-content.**
It's not a token, not a remark, just a swallowed range. `$$$$` (empty pair)
emits a literal `'$$'` punct token.

**Q14. DolDoc strings round-trip with `$$` pair counting.** The lexer
preserves `$$` bytes inside string literals and tracks them with
`cc->dollar_cnt`. A Rust port must keep the bytes and not auto-format.

**Q15. `//` does NOT skip the trailing newline by default.** It calls
`LexSkipEol`, which consumes everything until (and excluding) `\n`. The
`\n` is then handled by the next `LexGetChar`, which by default discards
it but with `CCF_KEEP_NEW_LINES` emits a token. Subtle but important for
line-counting.

**Q16. Macro bodies are stored as strings and re-lexed on use.** This
means **`#define`'d expressions inherit the lexer state of the call site**,
not the definition site. E.g. inside an `#ifdef` (with `CCF_NO_DEFINES`
set), the `#define` itself runs with macro-expansion *disabled* (good),
but if a body contains another macro reference, that reference is resolved
at use-time.

**Q17. `#include` filename gets `HC.Z` extension defaulted.** `ExtDft(name,
"HC.Z")` — so `#include "Foo"` becomes `#include "Foo.HC.Z"` if no extension
given. The `.Z` is TempleOS's native compressed-file convention.

**Q18. No `#undef`.** Once a `#define` is in the hash table, it stays.
(Removal happens via `HashDel` in C-side teardown only.)

**Q19. `'\0'` is a special escape that puts byte zero.** Not "octal zero,
maybe followed by more octal digits" as in C. Just a literal NUL byte.

**Q20. Identifier max length is `STR_LEN` (typically 144) and overflow
throws.** Not silent truncation.

**Q21. Strings have no max length but a missing closing `"` truncates at
EOF without error** (LexInStr returns done=TRUE on NUL).

**Q22. `'.'` after a number is consumed greedily.** `1.` is a TK_F64 of
`1.0` (because `i*Pow10I64(0) = 1` with k=0 after the dot path that read
no fractional digits). `1..2` is `Int(1)`, `DotDot`, `Int(2)` (via
`CCF_LAST_WAS_DOT`).

**Q23. Hex/binary literals produce signed i64 values, but accumulation is
always wrapping i64.** A hex literal with the high bit set yields a
negative `cur_i64`. Whether downstream code treats it as signed or unsigned
is the parser's problem.

---

## 4. Open questions (need VM testing)

**O1.** Exact behavior of `Pow10I64(negative)`: does it return 0, an
unspecified large value, or trap? Run TempleOS in QEMU, compile a one-line
program with a known float literal, and dump the IR / immediate.

**O2.** Does `'X'` (hex prefix) accept both upper and lowercase digits
without `ToUpper`? The code does `ToUpper(LexGetChar)` so yes — but
confirm `0xff` and `0xFF` produce identical bytes.

**O3.** What happens with `0x` followed by no hex digit? The loop hits
the `else` arm immediately, so `cur_i64 = 0`, the next token starts with
the original next char. Confirm a corpus example.

**O4.** Is there a `KW_NOSTKCHK`? It's mentioned in `CompilerB.HH`
preprocessor flags / `FSF_*` but not in `KW_*`. Likely lives in the parser
as a parsed-after-the-fact attribute. Need to grep `PrsLib.HC` /
`PrsStmt.HC`.

**O5.** Is `continue` truly absent? Spot-check the kernel for any use; if
present it's likely a `goto`.

**O6.** Macro hygiene / argument expansion: the `#define` we read parses
a body string, but does TempleOS support `#define FOO(x,y) x+y` macro
*arguments*? The body-parsing loop in `Lex.HC` does not appear to special-
case parens. Suspicion: TempleOS `#define` is **object-like only** — there
are no function-like macros. Confirm with corpus / `Demo` directory.

**O7.** Dollar-pair behavior inside char constants — the code does `if
(ch=='$$') { ch = LexGetChar(); k.u8[j]='$$'; if (ch!='$$') CCF_USE_LAST_U16; }`.
So `'$$ '` (single dollar + space) packs `'$$'` then unreads space, giving
a 1-byte char of `'$$'`. Confirm.

**O8.** What does `#include <stdio.h>`-style angle-bracket syntax do? The
code only checks `Lex(cc)!=TK_STR`; angle brackets would lex as `<` `stdio`
`.` `h` `>` and fail. So **only quoted `#include "foo"` works.**

**O9.** Behavior of CR (`\r`) at line ends: not in `char_bmp_non_eol`?
Need to check the bitmap. Suspected: `\r` is treated as ordinary whitespace.

**O10.** The lexer flag `CCF_LAST_WAS_DOT` lifetime: it's set in the int
path on seeing `..` and consumed by the `'.'` arm. Race between this and
`CCF_USE_LAST_U16`?

---

## 5. Implementation notes for the Rust port

**Crate stance.** Stdlib only. No `nom`, no `logos`, no `regex`. The
TempleOS lexer is a hand-rolled DFA-ish loop and the bug-compat surface
demands we mirror that exactly. A library would impose foreign idioms.

**Suggested types.**

```text
struct LexCtx<'src> {
  // Input stack (mirrors CLexFile linked list)
  files: Vec<LexFile<'src>>,
  // One-char put-back
  last_ch: Option<u32>,
  use_last: bool,
  // Flags (mirror CCF_*)
  flags: LexFlags,
  // Current token output
  token: TokenKind,
  cur_i64: i64,
  cur_f64: f64,
  cur_str: Vec<u8>,
  // Hash table for keyword + #define resolution
  hash: Rc<RefCell<HashTable>>,
  // DolDoc pair counter
  dollar_cnt: u32,
  // ...
}

enum TokenKind {
  Eof, Ident(String), IntLit(i64), FloatLit(f64),
  CharLit(i64), StrLit(Vec<u8>),
  // Single-char punct (returned as ASCII byte)
  Punct(u8),
  // Multi-char operators
  BangEq, EqEq, LtEq, GtEq, AmpAmp, PipePipe, CaretCaret,
  PlusPlus, MinusMinus, Shl, Shr, ShlEq, ShrEq,
  StarEq, SlashEq, PercentEq, PlusEq, MinusEq,
  AmpEq, PipeEq, CaretEq, Arrow, ColonColon, DotDot, Ellipsis,
  // DolDoc-synthetic
  InsBin(Vec<u8>), InsBinSize(i64),
  Newline,
}
```

Keywords *should not* live in `TokenKind`. Emit `Ident` and let the parser
do hash-lookup. This matches TempleOS exactly and keeps the lexer pure.

**Bitmaps.** Reproduce `char_bmp_alpha_numeric`, `char_bmp_alpha_numeric_no_at`,
`char_bmp_dec_numeric`, `char_bmp_hex_numeric`, `char_bmp_non_eol`, and
`char_bmp_non_eol_white_space` as `[u64; 4]` arrays (256 bits) at module
init. The `Bt` macro is `(bmp[i>>6] >> (i&63)) & 1`.

**Dual-token tables.** Hardcode the three `dual_U16_tokens{1,2,3}` tables
as `[u32; 256]` (or a small phf map) per the table in §1.3. **Order
matters**: tier-1 wins, tier-2 fallback, tier-3 fallback.

**One-char put-back.** Keep an `Option<u32>` "peeked" char. On consume,
clear it. On unread, set it.

**File / include stack.** `Vec<LexFile>`. Each entry has a buffer + cursor
+ line number + flags. `#include` pushes; EOF on a non-bottom file pops.
Macros are pushed as synthetic files with `LFSF_DEFINE` flag.

**Macro expansion recursion.** A macro body that itself references other
macros recursively pushes more files. There is **no recursion guard** in
TempleOS — a self-referential macro will infinite-loop. The Rust port
should add an optional depth cap (256?) for sanity, but a flag should
disable the cap for bug-compat tests.

**Where bug-compat matters most.**

1. Float literal value: must use the `i*Pow10I64(...)` formulation, not
   `f64::from_str`. Implement `Pow10I64` with the same domain quirks.
2. Char-literal packing: little-endian, exactly 8 bytes max, padded with
   zeros from MSB end.
3. `\d` escape, `\x` 2-digit limit, unknown-escape "keep `\` + push char."
4. Nested `/* */`.
5. `$$ ... $$` skip outside strings, keep inside.
6. `#define` body line-continuation rules and `//`-stripping inside body.
7. No octal. No `#undef`. No function-like macros (probably; see §4.O6).
8. ASCII operators returned as byte values; multi-char as named tokens.

**Where bug-compat does NOT matter** (we can be saner without breaking
parity):

- Identifier UTF-8 decoding: TempleOS reads raw bytes 128..255 as ident
  chars. The Rust port can keep `Vec<u8>` internally and only convert at
  the API boundary, lossily, for diagnostics.
- Source line numbers: TempleOS's are 1-based, fine to copy.
- Error messages: `LexExcept` strings can be replaced with richer Rust
  error types; the *trigger conditions* are what must match.
- Echo to console (`OPTF_ECHO`): can be a no-op.

**Test harness.** Build a small `lex-fixtures/` corpus of HolyC snippets
with known token streams (extracted from a TempleOS QEMU run that dumps
`cc->token`/`cc->cur_i64`/`cc->cur_f64`/`cc->cur_str` per call). The
fixture format should be one snippet + expected token JSON per file.
Iterate: any divergence is either a new bug-compat finding or a Rust port
bug. Document each compatibility decision in test fixture comments.

**Streaming vs eager.** Mirror TempleOS: `next_token()` returning one
token at a time. Don't precompute. The macro-expansion machinery makes
eager tokenization conceptually awkward.

**Public API sketch.**

```text
impl LexCtx {
  fn new(buf: &[u8], filename: &str, flags: LexFlags) -> Self;
  fn next_token(&mut self) -> Result<TokenKind, LexError>;
  fn line(&self) -> u32;
  fn column(&self) -> u32;
  fn push_include(&mut self, buf: Vec<u8>, filename: String);
  fn push_macro_body(&mut self, name: &str, body: Vec<u8>);
  fn save(&self) -> Bookmark;             // mirror LexPush
  fn restore(&mut self, b: Bookmark);     // mirror LexPopRestore
  fn discard(&mut self, b: Bookmark);     // mirror LexPopNoRestore
}
```

The `save`/`restore`/`discard` triplet mirrors `LexPush`/`LexPopRestore`/
`LexPopNoRestore` and is essential — the parser uses speculative lookahead
in several places.

---

## Appendix A. Operator precedence (from `cmp.binary_ops` in `CInit.HC`)

| Token                         | Prec class       | Assoc  | IC code      |
|-------------------------------|------------------|--------|--------------|
| `` ` ``                       | `PREC_EXP`       | right  | `IC_POWER`   |
| `<<` (`TK_SHL`)               | `PREC_EXP`       | left   | `IC_SHL`     |
| `>>` (`TK_SHR`)               | `PREC_EXP`       | left   | `IC_SHR`     |
| `*`                           | `PREC_MUL`       | (n/a — no ASSOCF set; see code) | `IC_MUL` |
| `/`                           | `PREC_MUL`       | left   | `IC_DIV`     |
| `%`                           | `PREC_MUL`       | left   | `IC_MOD`     |
| `&`                           | `PREC_AND`       | (none) | `IC_AND`     |
| `^`                           | `PREC_XOR`       | (none) | `IC_XOR`     |
| `\|`                          | `PREC_OR`        | (none) | `IC_OR`      |
| `+`                           | `PREC_ADD`       | (none) | `IC_ADD`     |
| `-`                           | `PREC_ADD`       | left   | `IC_SUB`     |
| `<`, `>`, `<=`, `>=`          | `PREC_CMP`       | (none) | `IC_LESS` etc|
| `==`, `!=`                    | `PREC_CMP2`      | (none) | `IC_EQU_EQU` etc|
| `&&`                          | `PREC_AND_AND`   | (none) | `IC_AND_AND` |
| `^^`                          | `PREC_XOR_XOR`   | (none) | `IC_XOR_XOR` |
| `\|\|`                        | `PREC_OR_OR`     | (none) | `IC_OR_OR`   |
| `=`, all `*=` family          | `PREC_ASSIGN`    | right  | various      |

Prec values (numeric; lower = tighter binding):

```
PREC_TERM        = 0x04
PREC_UNARY_POST  = 0x08
PREC_UNARY_PRE   = 0x0C
PREC_EXP         = 0x10
PREC_MUL         = 0x14
PREC_AND         = 0x18
PREC_XOR         = 0x1C
PREC_OR          = 0x20
PREC_ADD         = 0x24
PREC_CMP         = 0x28
PREC_CMP2        = 0x2C
PREC_AND_AND     = 0x30
PREC_XOR_XOR     = 0x34
PREC_OR_OR       = 0x38
PREC_ASSIGN      = 0x3C
PREC_MAX         = 0x40
```

Notable departures from C precedence:

- `&` (bitwise AND) is **tighter** than `+`/`-`. In C it's looser.
- `|` is also tighter than `+`/`-`.
- `^` sits between `&` and `|`, same as C.
- `**` does not exist; the **power operator is `` ` ``** (backtick), at
  `PREC_EXP` with `<<`/`>>`. Right-associative.
- `^^` (logical XOR with short-circuit) exists at its own `PREC_XOR_XOR`
  between `&&` and `||`.

Ranges of subtle parse-difference vs C are easy to construct: `a & b + c`
in HolyC is `(a & b) + c`, not `a & (b + c)`.

---

## Appendix B. Lexer-relevant CCF_* flags (partial)

| Flag                         | Purpose                                              |
|------------------------------|------------------------------------------------------|
| `CCF_USE_LAST_U16`           | Push-back: don't advance, reuse `cc->last_U16`.      |
| `CCF_DONT_FREE_BUF`          | Caller owns the input buffer.                        |
| `CCF_PMT`                    | Stream is an interactive prompt; refill on EOF.      |
| `CCF_KEEP_AT_SIGN`           | `@` is punct, not ident-body.                        |
| `CCF_KEEP_SIGN_NUM`          | `#` is punct, not preprocessor introducer.           |
| `CCF_KEEP_DOT`               | `.` is always punct, no number / dotdot lookahead.   |
| `CCF_KEEP_NEW_LINES`         | `\n` emits `Newline` token.                          |
| `CCF_NO_CHAR_CONST`          | `'` is not a char-const introducer.                  |
| `CCF_NO_DEFINES`             | Idents do NOT trigger macro expansion.               |
| `CCF_IN_QUOTES`              | Internal flag set during string scan.                |
| `CCF_IN_IF`                  | Internal flag set during `#if`/`#ifdef` body.        |
| `CCF_AOT_COMPILE`            | Selects `#ifaot` vs `#ifjit` branch.                 |
| `CCF_LAST_WAS_DOT`           | Internal flag for the `1..2` → `Int DotDot Int` case.|
| `CCF_QUESTION_HELP`          | Prompt-mode: turn `?` into `Help;;`.                 |

---

## Appendix C. File mapping summary

| TempleOS file       | Role                                                |
|---------------------|-----------------------------------------------------|
| `Lex.HC`            | `Lex()`, `LexGetChar()`, `LexInStr()`, `LexDollar()`, `LexAttachDoc()`, file push/pop. The big switch lives here. |
| `LexLib.HC`         | `LexBackupLastChar`, `LexPush`, `LexPopRestore`, `LexPopNoRestore`, `LexExtStr`, member-list helpers. |
| `CompilerA.HH`      | `CIntermediateStruct`, `IC_*` opcodes, `KW_*` and `AKW_*` enum, `PREC_*`, `ASSOCF_*`, function/storage flags. |
| `CompilerB.HH`      | Public extern declarations (`CmpCtrlNew`, `Lex`, `LexExpression`, ...). |
| `CInit.HC`          | `CmpFillTables` — populates `dual_U16_tokens1/2/3` and `binary_ops` precedence/IC table. **Source of truth for operator strings and precedence.** |

---

End of spec.
