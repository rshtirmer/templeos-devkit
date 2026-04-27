# HolyC Parser Specification (Rust port reference)

> Source of truth: TempleOS `Compiler/PrsExp.HC`, `Compiler/PrsStmt.HC`,
> `Compiler/PrsVar.HC`, `Compiler/PrsLib.HC`, `Compiler/CompilerA.HH`,
> `Compiler/CompilerB.HH` (cia-foundation/TempleOS, branch `archive`).
>
> Citations are in the form `PrsXxx.HC:LINE` referring to the upstream files.
> Behaviour described here is what the live TempleOS parser does — bug
> compatibility takes precedence over standard C semantics. **Read the
> "Bug-compatibility list" section before claiming the parser is "wrong".**
>
> Audience: a Rust engineer porting the parser to a host-side recursive
> descent implementation. The Rust port consumes the same source text the
> TempleOS lexer would consume, then emits the same shape of decisions
> (declaration vs statement, expression precedence collapses, error points)
> so downstream tools can reason about HolyC sources the way TempleOS does.

---

## 0. Cross-cutting concepts

### 0.1 The `CCmpCtrl` (cc) state object

Every parser entry point takes a `CCmpCtrl *cc`. Conceptually:

```rust
pub struct CmpCtrl {
    pub token: Token,                 // current token; updated by Lex()
    pub cur_str: Option<String>,      // identifier / string lexeme
    pub cur_i64: i64,                 // int / char lexeme
    pub cur_f64: f64,                 // f64 lexeme
    pub flags: CcFlags,               // CCF_* (see below)
    pub htc:   HashTableContext,      // global / local / define hashes
    pub coc:   CodeCtrl,              // intermediate code being emitted
    pub fun:   Option<FunHandle>,     // current function (None at file scope)
    pub lb_leave: Option<LabelHandle>,// function epilogue label
    pub lock_cnt: i32,                // `lock { ... }` nesting
    pub aot_depth: i32,               // AOT compile recursion
    pub abs_cnts: AbsCounts,          // absolute-address bookkeeping
    pub aotc: Option<AotCtrl>,        // AOT module state
    pub asm_undef_hash: ...,
    pub local_var_entry: Option<MemberLst>, // set by Lex when ident is a local
    pub hash_entry: Option<HashEntry>,      // set by Lex when ident is a hash hit
    pub last_U16: u16,                // previous char for line-tracking
    pub min_line: i32, pub max_line: i32,
    pub lex_include_stk: ...,         // include / #define expansion stack
    pub fun_lex_file: ...,            // file the current fn was opened in
    pub class_dol_offset: i64,        // value of `$$` inside a class body
}
```

The Rust port must preserve the side-effect-on-lex pattern: `Lex(cc)`
**both advances and resolves** the next token. When the lexer sees an
identifier it looks it up in `htc` and stashes the hit in
`cc.hash_entry` (global) and/or `cc.local_var_entry` (local). The parser
peeks at those fields **without re-looking them up** to decide what kind
of statement / expression term it is parsing. Equivalent in Rust:

```rust
fn lex(&mut self) -> Token {
    let tk = self.lexer.next();
    self.token = tk;
    if tk == Token::Ident {
        self.local_var_entry = self.htc.local.lookup(&self.cur_str);
        self.hash_entry      = self.htc.global.lookup(&self.cur_str);
    } else {
        self.local_var_entry = None;
        self.hash_entry      = None;
    }
    tk
}
```

### 0.2 The intermediate code (IC) stream

Both the expression parser and the statement parser emit IC ops as side
effects (`ICAdd(cc, IC_*, arg, class, flags)`, see `PrsLib.HC:79`).
A faithful Rust port can emit a host AST instead, but it must run the
expression parser as a **stack machine over precedences** (see §2) so
it produces the same operator-grouping decisions TempleOS does.

The IC opcode table lives in `CompilerA.HH:20-237`. The opcodes that the
expression parser directly produces (and that the spec below references)
are reproduced inline in §2.4.

### 0.3 Class type system (pointer encoding)

`PrsClassNew()` (`PrsLib.HC:40-60`) allocates **`PTR_STARS_NUM+1`
contiguous `CHashClass` records** for every class. `T*` is represented
as `&class[1]`, `T**` as `&class[2]`, etc. So `tmpc++` = "add a star",
`tmpc--` = "remove a star". The Rust port should mirror this with a
`(ClassId, ptr_stars: u8)` pair rather than five separate records, but
must reproduce the saturation behaviour at `PTR_STARS_NUM` levels
(`PrsVar.HC:306` raises `Too many *'s at`).

### 0.4 Precedence levels

From `CompilerA.HH:339-356`:

| Level             | Value | Meaning                                  |
| ----------------- | ----- | ---------------------------------------- |
| `PREC_NULL`       | 0x00  | sentinel — empty stack                   |
| `PREC_TERM`       | 0x04  | atomic terms (literals, ident, parens)   |
| `PREC_UNARY_POST` | 0x08  | postfix `++`, `--`, `[]`, `.`, `->`, `()`|
| `PREC_UNARY_PRE`  | 0x0C  | prefix `~ ! - * & ++ --`                 |
| `PREC_EXP`        | 0x10  | exponent `` ` ``                         |
| `PREC_MUL`        | 0x14  | `* / %` `<< >>`                          |
| `PREC_AND`        | 0x18  | bitwise `&`                              |
| `PREC_XOR`        | 0x1C  | bitwise `^`                              |
| `PREC_OR`         | 0x20  | bitwise `\|`                             |
| `PREC_ADD`        | 0x24  | `+ -`                                    |
| `PREC_CMP`        | 0x28  | `< <= > >=`                              |
| `PREC_CMP2`       | 0x2C  | `== !=`                                  |
| `PREC_AND_AND`    | 0x30  | `&&`                                     |
| `PREC_XOR_XOR`    | 0x34  | `^^`  (HolyC-only logical XOR)           |
| `PREC_OR_OR`      | 0x38  | `\|\|`                                   |
| `PREC_ASSIGN`     | 0x3C  | `=` and all compound-assign forms        |
| `PREC_MAX`        | 0x40  | sentinel                                 |

Associativity is encoded in the **low 2 bits** of the precedence byte
via `ASSOCF_LEFT=1`, `ASSOCF_RIGHT=2`, `ASSOC_MASK=3` (CompilerA.HH:335).
The expression engine uses `prec & ~ASSOC_MASK` to compare precedence
levels and `prec & ASSOC_MASK` to break ties.

### 0.5 Flags consulted by the parser

`CCF_*` (subset; full list in `CompilerB.HH` plus other headers):

| Flag                  | Set when …                                   |
| --------------------- | -------------------------------------------- |
| `CCF_PAREN`           | last value came from a parenthesised expr    |
| `CCF_PREINC/PREDEC`   | currently parsing `++x`/`--x`                |
| `CCF_POSTINC/POSTDEC` | currently parsing `x++`/`x--`                |
| `CCF_FUN_EXP`         | last term is a function-pointer expression   |
| `CCF_RAX`             | last term lives in RAX (typecast etc.)       |
| `CCF_ARRAY`           | last term is an array (not yet collapsed)    |
| `CCF_ASM_EXPRESSIONS` | inside `asm { }` (relaxed lookup rules)      |
| `CCF_AOT_COMPILE`     | building an AOT module (no JIT eval)         |
| `CCF_EXE_BLK`         | inside a `streamblk { }` (`PrsStmt.HC:805`)  |
| `CCF_NOT_CONST`       | last expression had non-const side effect    |
| `CCF_HAS_RETURN`      | function body has emitted a `return`         |
| `CCF_HAS_MISC_DATA`   | last expression embedded a string/data const|
| `CCF_DONT_MAKE_RES`   | inside a var-decl init that wants no result  |
| `CCF_NO_REG_OPT`      | function body has try/catch — disable regopt |
| `CCF_CLASS_DOL_OFFSET`| inside class body — `$$` is class offset     |

The Rust port should keep the same names; downstream code we already
have keys off them.

---

## 1. Top-level grammar

HolyC's "top level" is unusual: it is **executed as it is parsed**.
`ExePutS` / `ExeFile` (`CompilerB.HH:7`) feed source into `PrsStmt`
which, at file scope (`cc.fun == None`), parses one statement, then if
that statement was a runnable expression, JIT-compiles it and runs it
immediately. Function bodies, class bodies and pre-processor directives
are *not* runnable; they install hash entries.

### 1.1 EBNF (file scope)

```ebnf
file              ::= top_item* EOF

top_item          ::= ';'                              (* skipped *)
                    | preprocessor_directive
                    | extern_decl
                    | _extern_decl
                    | _intern_decl
                    | _import_decl
                    | import_decl
                    | storage_modifier* fun_or_var_decl
                    | class_decl
                    | union_decl
                    | asm_block
                    | top_level_stmt                   (* runs immediately *)
                    | label ':'                        (* REJECTED at file scope *)

storage_modifier  ::= 'static' | 'public' | 'interrupt'
                    | 'haserrcode' | 'argpop' | 'noargpop'

fun_or_var_decl   ::= type_spec declarator
                       ( '(' fun_args ')' fun_body              (* function def *)
                       | ('=' initializer)? (',' declarator ('=' init)?)* ';' )

class_decl        ::= 'class' IDENT (':' IDENT)? '{' member_lst '}' ';'
union_decl        ::= 'union' IDENT '{' member_lst '}' ';'

extern_decl       ::= 'extern'  type_spec  declarator_list ';'
                    | 'extern'  ('class' | 'union') IDENT (':' IDENT)? '{' members '}' ';'
_extern_decl      ::= '_extern' IDENT type_spec declarator_list ';'
_intern_decl      ::= '_intern' const_expr type_spec declarator_list ';'
import_decl       ::= 'import'  type_spec declarator_list ';'   (* AOT only *)
_import_decl      ::= '_import' IDENT type_spec declarator_list ';'

top_level_stmt    ::= expression_stmt                  (* runs *)
                    | compound_stmt                    (* runs *)
                    | string_or_charlit (',' expr)* ';' (* implicit Print/PutChars *)
                    | 'try' '{' stmt* '}' 'catch' '{' stmt* '}'
                    | 'if' '(' expr ')' stmt ('else' stmt)?
                    | 'while' '(' expr ')' stmt
                    | 'do' stmt 'while' '(' expr ')' ';'
                    | 'for' '(' init? ';' cond? ';' update? ')' stmt
                    | 'switch' ('(' | '[') expr (')' | ']') '{' switch_body '}'
                    | 'lock' stmt
                    | 'no_warn' IDENT (',' IDENT)* ';'

preprocessor_directive ::=
       '#include'   STR
     | '#define'    IDENT TOKEN_LIST EOL
     | '#ifdef'     IDENT
     | '#ifndef'    IDENT
     | '#ifaot'   | '#ifjit' | '#endif'
     | '#assert'    const_expr
     | '#help_index' STR
     | '#help_file'  STR
     | '#exe' '{' stream_blk '}'
```

### 1.2 EBNF (function scope)

```ebnf
fun_body          ::= '{' stmt* '}'

stmt              ::= ';'
                    | compound_stmt
                    | expression_stmt
                    | if_stmt | while_stmt | do_while_stmt | for_stmt
                    | switch_stmt | break_stmt | return_stmt | goto_stmt
                    | try_stmt
                    | local_var_decl
                    | label                            (* IDENT ':'  *)
                    | asm_stmt
                    | 'lock' stmt
                    | 'no_warn' IDENT (',' IDENT)* ';'

compound_stmt     ::= '{' stmt* '}'
expression_stmt   ::= expression ';'
local_var_decl    ::= ('static' | 'reg' [REG] | 'noreg')*
                      type_spec declarator ('=' init)?
                      (',' declarator ('=' init)?)* ';'
```

### 1.3 What's allowed at file scope vs function scope

| Construct                          | File scope                              | Function scope                          |
| ---------------------------------- | --------------------------------------- | --------------------------------------- |
| Variable declaration               | yes — global                            | yes — local                             |
| Function definition                | yes                                     | **no** — nested funs not allowed        |
| Function declaration (header only) | yes                                     | no (fun args are introduced by `(...)`) |
| `extern` / `_extern` / `import`    | yes                                     | **no** — `LexExcept "Not allowed in fun"` (`PrsStmt.HC:993`) |
| `class` / `union`                  | yes                                     | yes (rare; introduces new type)         |
| `static` / `public` / `interrupt`  | yes                                     | `static` yes for static locals; others no |
| `if` / `while` / `for` / `switch`  | yes — runs at parse time                | yes                                     |
| `do { } while ( );`                | yes                                     | yes                                     |
| `try` / `catch`                    | yes                                     | yes (forces `CCF_NO_REG_OPT`)           |
| `return`                           | **NO** — `LexExcept "Not in fun. Can't return a val"` (`PrsStmt.HC:1090`) | yes                |
| `goto LABEL` / `LABEL:`            | `goto` parses but emits a JMP into a label that must be defined; defining a label at file scope hits `LexExcept "No global labels at "` (`PrsStmt.HC:1198`) | yes |
| `break` / `continue`               | only inside an enclosing `for`/`while`/`switch`; otherwise `LexExcept "'break' not allowed"` (`PrsStmt.HC:1135`); **HolyC does not have `continue`** as a keyword (see §5) | same |
| `asm { }`                          | yes — emitted as JIT or AOT chunk       | yes — embedded as `IC_ASM`              |
| String / `'X'` as a statement      | yes — calls `Print` / `PutChars`        | yes                                     |
| Labels alone (`LABEL:`)            | rejected (see above)                    | yes                                     |

Top-level statements run during `ExePutS`, so they observe definitions
made earlier *in the same file* (forward references must be hoisted by
the user via header decls).

---

## 2. Expression grammar with precedence table

The expression parser is `PrsExpression` / `PrsExpression2`
(`PrsExp.HC:264, 65`). It is a **stack-driven shunting-yard** with a
state machine of 13 states (`PE_UNARY_TERM1 … PE_POP_ALL2`,
`PrsExp.HC:1-13`). The states cycle as follows:

```
              ┌────────────────────────┐
              ▼                        │
  UNARY_TERM1 → UNARY_TERM2 → MAYBE_MODIFIERS → UNARY_MODIFIERS
                                  │                    │
                                  ▼                    ▼
                              CHECK_BIN_OPS1 ──── DEREFERENCE
                                  │                    │
                                  ▼                    ▼
                              CHECK_BIN_OPS2 ───► DO_UNARY_OP
                                  │
                                  ▼
                              DO_BINARY_OP ─► POP_HIGHER ─► PUSH_LOWER
                                  │
                                  ▼
                              POP_ALL1 → POP_ALL2 → done
```

Each state returns the **next state**. A `LexExcept` thrown anywhere
inside is caught by `PrsExpression` (`PrsExp.HC:281-286`), which clears
the pending stack and returns `false`.

### 2.1 Operator-dispatch tables

The lexer fills two tables, `cmp.binary_ops[token]` and the unary maps
inside `PrsUnaryTerm`, mapping a token byte to a **packed 4-byte word**:

```
u16[0] = IC_*            (intermediate-code opcode)
u16[1] = precedence | associativity
u8[3]  = ECF_* extra flags (e.g. ECF_HAS_PUSH_CMP for chained compares)
```

The Rust port should mirror this with a static table:

```rust
struct OpEntry { ic: Ic, prec: u8, ecf: u8 }
const BINARY_OPS: [Option<OpEntry>; 256] = { /* keyed by Token byte */ };
```

### 2.2 Precedence + associativity table

| Prec class    | Form                | Operator(s)            | Assoc    | IC opcode             | Handler                              | Notes |
| ------------- | ------------------- | ---------------------- | -------- | --------------------- | ------------------------------------ | ----- |
| TERM (4)      | atom                | literals / ident       | n/a      | `IC_IMM_I64/F64/STR_CONST/ABS_ADDR/RBP/RIP` | `PrsUnaryTerm` | `cur_i64<0` ⇒ U64 type promotion (PrsExp.HC:682) |
| TERM (4)      | atom                | `(expr)`               | n/a      | recursive             | `PrsUnaryTerm` `case '('`            | rejects `(TYPE)` C-style cast (HolyC uses postfix typecast) |
| TERM (4)      | atom                | `$$`                   | n/a      | `IC_RIP` or `IC_IMM_I64` | `PrsUnaryTerm` `case '$$'`        | inside class: `cc.class_dol_offset` |
| TERM (4)      | atom                | `sizeof(...)`          | n/a      | `IC_IMM_I64`          | `PrsSizeOf` (`PrsExp.HC:303`)        | accepts type, var, fun, member chain `.foo.bar`; trailing `*`s set size to `sizeof(U8*)` |
| TERM (4)      | atom                | `offset(class.member)` | n/a      | `IC_IMM_I64`          | `PrsOffsetOf` (`PrsExp.HC:353`)      | requires `.`, walks `.member` chain |
| TERM (4)      | atom                | `defined(IDENT)`       | n/a      | `IC_IMM_I64`          | `PrsUnaryTerm` `KW_DEFINED`          | parens optional but balanced |
| TERM (4)      | atom                | `&IDENT`               | n/a      | `IC_IMM_I64` / `IC_ABS_ADDR` | `PrsUnaryTerm` `case '&'`     | special-cases extern fun, internal fun, sys sym (PrsExp.HC:621-672) |
| UNARY_POST(8) | postfix             | `x++`                  | left     | `IC__PP`              | `PrsUnaryModifier` (`PrsExp.HC:960`) | actually fires from `PE_DEREFERENCE` |
| UNARY_POST(8) | postfix             | `x--`                  | left     | `IC__MM`              | same                                 | |
| UNARY_POST(8) | postfix             | `a[i]`                 | left     | `IC_MUL`+`IC_ADD` ladder | `PrsUnaryModifier` `case '['`     | multi-dim arrays use `tmpad` chain (PrsExp.HC:1069-1085) |
| UNARY_POST(8) | postfix             | `a.member`             | left     | `IC_IMM_I64`+`IC_ADD` | `PrsUnaryModifier` `case '.'`        | class lookup; reads `MemberFind` |
| UNARY_POST(8) | postfix             | `a->member`            | left     | `IC_DEREF`+`IC_ADD`   | `PrsUnaryModifier` `case TK_DEREFERENCE` | TempleOS lexer emits `TK_DEREFERENCE` for `->` |
| UNARY_POST(8) | postfix             | `f(args)`              | left     | `IC_CALL_*`           | `PrsFunCall` (`PrsExp.HC:383`)       | applies to fun-ptrs and direct funs; default-arg, `lastclass` and `...` handled here |
| UNARY_POST(8) | postfix typecast    | `expr (TYPE)`          | left     | `IC_HOLYC_TYPECAST`   | `PrsUnaryModifier` `case '('`        | **HolyC-specific**: cast goes after the value |
| UNARY_PRE (C) | prefix              | `~x`                   | right    | `IC_COM`              | `PrsUnaryTerm` `~`                   | |
| UNARY_PRE (C) | prefix              | `!x`                   | right    | `IC_NOT`              | `PrsUnaryTerm` `!`                   | |
| UNARY_PRE (C) | prefix              | `-x`                   | right    | `IC_UNARY_MINUS`      | `PrsUnaryTerm` `-`                   | |
| UNARY_PRE (C) | prefix              | `+x`                   | right    | (none, drops)         | `PrsUnaryTerm` `+`                   | |
| UNARY_PRE (C) | prefix              | `*x`                   | right    | `IC_DEREF`            | `PrsUnaryTerm` `*`                   | |
| UNARY_PRE (C) | prefix              | `&x`                   | right    | `IC_ADDR`             | `PrsUnaryTerm` `&`                   | (with sub-cases above) |
| UNARY_PRE (C) | prefix              | `++x`                  | right    | `IC_PP_`              | `PrsUnaryTerm` `TK_PLUS_PLUS`        | sets `CCF_PREINC`, materialised at `PE_DEREFERENCE` |
| UNARY_PRE (C) | prefix              | `--x`                  | right    | `IC_MM_`              | same                                 | sets `CCF_PREDEC` |
| EXP (10)      | binary              | `` x `y ``             | **right**| `IC_POWER`            | `PrsAddOp`                           | `` ` `` is the HolyC exponent op; bound tighter than `*` |
| MUL (14)      | binary              | `x * y`                | left     | `IC_MUL`              | `PrsAddOp`                           | |
| MUL (14)      | binary              | `x / y`                | left     | `IC_DIV`              | `PrsAddOp`                           | |
| MUL (14)      | binary              | `x % y`                | left     | `IC_MOD`              | `PrsAddOp`                           | |
| MUL (14)      | binary              | `x << y`               | left     | `IC_SHL`              | `PrsAddOp`                           | precedence is **`*`-class**, not `+`-class |
| MUL (14)      | binary              | `x >> y`               | left     | `IC_SHR`              | `PrsAddOp`                           | same |
| AND (18)      | binary              | `x & y`                | left     | `IC_AND`              | `PrsAddOp`                           | |
| XOR (1C)      | binary              | `x ^ y`                | left     | `IC_XOR`              | `PrsAddOp`                           | |
| OR  (20)      | binary              | `x \| y`               | left     | `IC_OR`               | `PrsAddOp`                           | |
| ADD (24)      | binary              | `x + y`                | left     | `IC_ADD`              | `PrsAddOp`                           | pointer arith multiplied by `IC_SIZEOF` automatically (PrsExp.HC:21) |
| ADD (24)      | binary              | `x - y`                | left     | `IC_SUB`              | `PrsAddOp`                           | ptr-ptr divides by sizeof (PrsExp.HC:32) |
| CMP (28)      | binary              | `< <= > >=`            | left     | `IC_LESS/...`         | `PrsAddOp`                           | chained compare (`a<b<c`) ⇒ `IC_PUSH_CMP`/`IC_AND_AND`+`ICF_POP_CMP` (PrsExp.HC:225-232) |
| CMP2 (2C)     | binary              | `== !=`                | left     | `IC_EQU_EQU`/`IC_NOT_EQU`| `PrsAddOp`                        | participates in chained-compare same way |
| ANDAND (30)   | binary              | `x && y`               | left     | `IC_AND_AND`          | `PrsAddOp`                           | inserts `IC_NOP1` before for short-circuit hook |
| XORXOR (34)   | binary              | `x ^^ y`               | left     | `IC_XOR_XOR`          | `PrsAddOp`                           | **HolyC-only logical XOR** |
| OROR  (38)    | binary              | `x \|\| y`             | left     | `IC_OR_OR`            | `PrsAddOp`                           | inserts `IC_NOP1` |
| ASSIGN (3C)   | binary              | `x = y`                | **right**| `IC_ASSIGN`           | `PrsAddOp` w/ IST_ASSIGN guard       | LHS must be IST_DEREF; otherwise `LexExcept "Invalid lval at "` (PrsExp.HC:206) |
| ASSIGN (3C)   | binary              | `x += y`               | right    | `IC_ADD_EQU`          | same                                 | ptr += int multiplies sizeof (PrsExp.HC:40) |
| ASSIGN (3C)   | binary              | `x -= y`               | right    | `IC_SUB_EQU`          | same                                 | |
| ASSIGN (3C)   | binary              | `x *= y`               | right    | `IC_MUL_EQU`          | same                                 | |
| ASSIGN (3C)   | binary              | `x /= y`               | right    | `IC_DIV_EQU`          | same                                 | |
| ASSIGN (3C)   | binary              | `x %= y`               | right    | `IC_MOD_EQU`          | same                                 | |
| ASSIGN (3C)   | binary              | `x &= y`               | right    | `IC_AND_EQU`          | same                                 | |
| ASSIGN (3C)   | binary              | `x \|= y`              | right    | `IC_OR_EQU`           | same                                 | |
| ASSIGN (3C)   | binary              | `x ^= y`               | right    | `IC_XOR_EQU`          | same                                 | |
| ASSIGN (3C)   | binary              | `x <<= y`              | right    | `IC_SHL_EQU`          | same                                 | HolyC has both `<<=` and `>>=` |
| ASSIGN (3C)   | binary              | `x >>= y`              | right    | `IC_SHR_EQU`          | same                                 | |
| n/a (call-form, NOT operator) | bit-test  | `Bt(&v, n)` etc.   | n/a      | `IC_BT/BTS/BTR/BTC/LBTS/LBTR/LBTC/BSF/BSR` | resolved as ordinary fun calls; the IC code is selected by the matching `Ff_INTERNAL` template fun (CompilerB.HH/CompilerA.HH:162-170) | TempleOS treats these as **template intrinsics** — they look like calls, not operators |

#### Notes on operator precedence quirks

- HolyC has **no `?:` ternary**. There is no `IC_TERNARY` and no parsing
  for it. A Rust port that wants to surface a friendlier error should
  detect `'?'` in `PrsExpression2` and emit "ternary not supported in
  HolyC" instead of the generic `Missing ')'`-style cascade.
- Shift operators `<<` / `>>` sit at `PREC_MUL`, not between `+` and
  `<`. `a + b << c` parses as `a + (b<<c)` — opposite of C.
- `^^` is a **logical XOR** at its own level between `&&` and `||`
  (`PREC_XOR_XOR` 0x34). C does not have this.
- Power `` ` `` is right-associative and tighter than `*`. `2`3`4 ==
  2^(3^4) == 2^81`. There is a special-case for `-x``y`: when the parser
  sees `IC_POWER` on top of `IC_UNARY_MINUS` it skips the `op` and
  re-pushes both — i.e. `-2`2 == -(2`2)`, not `(-2)`2` (PrsExp.HC:139-147).
- The "paren warning" machinery (`ParenWarning`) flags redundant `()`
  when both sides have known precedence (`PrsExp.HC:122-128, 192-197,
  245-248`). The Rust port should preserve a `WARN_REDUNDANT_PAREN`
  diagnostic at the same spots — TempleOS regression tests rely on it.

### 2.3 Postfix typecast (HolyC-specific)

```
expr ::= ... '(' TYPE_NAME ')'
```

is parsed by `PrsUnaryModifier` (`PrsExp.HC:1016`). When in a unary-modifier
state and the next token is `'('`, the parser **requires** the inner
token to be `TK_IDENT` and resolves it as a type via `PrsType`. The
resulting class (with `*`-stars and `[]`-dims) is attached as the type
of the most recent IC node, then `IC_HOLYC_TYPECAST` is emitted.

C-style prefix casts `(int)x` are explicitly rejected at
`PrsExp.HC:728-731`: `LexExcept(cc,"Use TempleOS postfix typecasting at ")`.
A Rust port must not silently accept the C form.

### 2.4 IC opcodes the parser emits

For reference (full table CompilerA.HH:20-237):

`IC_IMM_I64, IC_IMM_F64, IC_STR_CONST, IC_ABS_ADDR, IC_RBP, IC_RIP,
IC_DEREF, IC_ADDR, IC_COM, IC_NOT, IC_UNARY_MINUS, IC_PP_, IC_MM_,
IC__PP, IC__MM, IC_HOLYC_TYPECAST, IC_SIZEOF, IC_POWER, IC_MUL,
IC_DIV, IC_MOD, IC_SHL, IC_SHR, IC_AND, IC_XOR, IC_OR, IC_ADD,
IC_SUB, IC_EQU_EQU, IC_NOT_EQU, IC_LESS, IC_LESS_EQU, IC_GREATER,
IC_GREATER_EQU, IC_AND_AND, IC_XOR_XOR, IC_OR_OR, IC_ASSIGN,
IC_*_EQU, IC_PUSH_CMP, IC_NOP1, IC_NOP2, IC_END_EXP, IC_LABEL,
IC_BR_ZERO, IC_BR_NOT_ZERO, IC_JMP, IC_RETURN_VAL, IC_LEAVE,
IC_ENTER, IC_RET, IC_CALL_START, IC_CALL_END, IC_CALL,
IC_CALL_INDIRECT, IC_CALL_INDIRECT2, IC_CALL_IMPORT,
IC_CALL_EXTERN, IC_ADD_RSP, IC_ADD_RSP1, IC_PUSH_REGS,
IC_GET_LABEL, IC_SUB_CALL, IC_SWITCH, IC_NOBOUND_SWITCH,
IC_HEAP_GLBL, IC_ADDR_IMPORT, IC_ASM, IC_TYPE.`

A pure Rust AST port can elide all of these and just produce a HolyC
AST; the IC list is reproduced here so emit-points can be cross-checked
against TempleOS when tests differ.

### 2.5 The expression-stack invariants

`CPrsStk` has two stacks: `stk` (operands/operators) and `stk2`
(function-pointer cookies). `PrsExpression` pushes two zeroes as
sentinels (`PrsExp.HC:276-277`) and after the parse asserts
`ps->ptr == 0` else raises `Compiler Parse Error at` (`PrsExp.HC:288`).
Mirror this assertion in the Rust port — it catches stack-imbalance
bugs immediately.

### 2.6 PrsFunCall details (callable forms)

`PrsFunCall` (`PrsExp.HC:383`):

1. If invoked with `tmpf == NULL`, the parser is materialising an
   **implicit Print/PutChars** call (a top-level string literal or
   `'X'` char-const used as a statement). It picks `Print` for `TK_STR`
   and `PutChars` for `TK_CHAR_CONST`.
2. `(` is optional in two cases: when the function is `Print`/`PutChars`
   (used as a `"fmt", a, b;` form) and when called from a context that
   already consumed the lparen (e.g., `&fn`).
3. Default args (`MLF_DFT_AVAILABLE`) are filled if the caller writes
   `,` or `)` early. `MLF_LASTCLASS` substitutes the *last classname
   encountered in the arg list*.
4. Variadic (`...`) functions use a hidden `argc` arg; the parser pushes
   each remaining expression and patches `argc` into the count once it
   sees `;` (Print form) or `)` (general form) (`PrsExp.HC:491-531`).
5. Pre-resolved internal "template funs" (`Ff_INTERNAL`,
   `IC_SQR..IC_ATAN`) are inlined as IC ops rather than CALL.

In the Rust port, function-call parsing is **interleaved with expression
parsing** — the call returns `PE_UNARY_MODIFIERS` so subsequent `.`,
`->`, `[]`, `()` continue against the result type.

---

## 3. Statement grammar

`PrsStmt` (`PrsStmt.HC:904`) is one big switch. Entry parameters:

| Param        | Meaning                                                    |
| ------------ | ---------------------------------------------------------- |
| `try_cnt`    | depth of enclosing `try{}` blocks (for `return` unwinding) |
| `lb_break`   | label to jump to on `break`; `NULL` ⇒ no enclosing loop    |
| `cmp_flags`  | `CMPF_PRS_SEMICOLON` = consume the trailing `;`; `CMPF_ONE_ASM_INS` = parse a single asm op |

Each statement form below names the dispatcher. **All `LexExcept` calls
are immediate aborts** caught by the next-higher `try` (see §6).

### 3.1 Statement table

| Form                         | Required tokens                                | Optional       | Handler / line                                  | Notes |
| ---------------------------- | ---------------------------------------------- | -------------- | ----------------------------------------------- | ----- |
| `;`                          | `;`                                            | -              | `PrsStmt.HC:930`                                | empty stmt; consumed iff `CMPF_PRS_SEMICOLON` |
| `{ stmt* }`                  | `{` `}`                                        | trailing `,`   | `PrsStmt.HC:923-929`                            | iterates `PrsStmt` until `}` or `TK_EOF`. After `}` if next is `,` it loops; this is what enables comma-separated stmt-blocks at the same nesting level |
| `expr ;`                     | `expr` `;`                                     | -              | falls through to `sm_prs_exp` `PrsStmt.HC:1205` | implicit-print form for `TK_STR`/`TK_CHAR_CONST` (PrsStmt.HC:1201) |
| `if ( cond ) stmt`           | `if` `(` `cond` `)` `stmt`                     | `else stmt`    | `PrsIf` (`PrsStmt.HC:459`)                      | one-line untrailed forms (`if(c) goto L;`) **work** for most `stmt`s but trip when the body is a `continue`-style construct (see §5.3) |
| `while ( cond ) stmt`        | `while` `(` `cond` `)` `stmt`                  | -              | `PrsWhile` (`PrsStmt.HC:486`)                   | |
| `do stmt while ( cond ) ;`   | `do` `stmt` `while` `(` `cond` `)` `;`         | -              | `PrsDoWhile` (`PrsStmt.HC:506`)                 | the trailing `;` is consumed inside `PrsDoWhile`, **not** by `PrsStmt`'s outer driver — passing `CMPF_PRS_SEMICOLON=0` to a wrapping caller is therefore unsafe for `do/while` |
| `for ( init ; cond ; step ) stmt` | `for` `(` `init` `;` `cond` `;` `step` `)` `stmt` | empty `init`/`cond`/`step` (init via `;` only, step via `)` only) | `PrsFor` (`PrsStmt.HC:529`) | `cond` is **mandatory** — empty `for(;;)` will `LexExcept "Missing ';' at"` from `PrsExpression`. `init` is parsed via `PrsStmt(cc, try_cnt)` with no `lb_break`, so it can be a *type-decl* — but see §5.4 for the file-scope bug |
| `switch ( expr ) { ... }`    | `switch` `(`/`[` `expr` `)`/`]` `{` body `}`   | -              | `PrsSwitch` (`PrsStmt.HC:578`)                  | `[` form sets `IC_NOBOUND_SWITCH` (no range check) |
| `case k :`                   | `case` `:`                                     | -              | inside `PrsSwitch`                              | `case :` (no expression) auto-increments from previous case (PrsStmt.HC:681-685) |
| `case k ... m :`             | `case` `k` `...` `m` `:`                       | -              | inside `PrsSwitch`                              | range-case; expanded into `m-k+1` `CSwitchCase` records |
| `start: ... end:`            | `start` `:` `...` `end` `:`                    | -              | inside `PrsSwitch`                              | **HolyC sub-switch**: groups statements into a callable subroutine. Multiple `start:`s nest; on `end:` the parser emits `IC_RET` and falls through to the next case (PrsStmt.HC:621-762). Sub-switches *implicitly* `IC_SUB_CALL` themselves at every `case:`/`default:` so the body always runs once |
| `default :` / `dft :`        | `dft` `:`                                      | -              | inside `PrsSwitch`                              | the keyword is `dft`, not `default` (KW_DFT, CompilerA.HH:263) |
| `break ;`                    | `break` `;`                                    | -              | `PrsStmt.HC:1133`                               | uses `lb_break`; throws if NULL |
| `return ;` / `return expr ;` | `return` (`expr`)? `;`                         | -              | `PrsStmt.HC:1089`                               | rejected at file scope; emits `SysUntry` calls for each enclosing try |
| `goto LABEL ;`               | `goto` `IDENT` `;`                             | -              | `PrsStmt.HC:1121`                               | label may be a forward reference; resolved at end of fun |
| `LABEL :`                    | `IDENT` `:`                                    | -              | `PrsStmt.HC:1185-1199`                          | rejected at file scope (`No global labels at`); duplicates rejected (`Duplicate goto label at`) |
| `try { ... } catch { ... }`  | `try` `{...}` `catch` `{...}`                  | -              | `PrsTryBlk` (`PrsStmt.HC:842`)                  | introduces `SysTry`/`SysUntry` runtime calls; sets `CCF_NO_REG_OPT` |
| `lock stmt`                  | `lock` `stmt`                                  | -              | `PrsStmt.HC:965`                                | bumps `cc.lock_cnt`; ICs emitted in body get `ICF_LOCK` |
| `no_warn IDENT,...;`         | `no_warn` `IDENT` `(`,`IDENT`)*` `;`           | -              | `PrsNoWarn` (`PrsStmt.HC:791`)                  | suppresses unused-var warnings on listed locals |
| `asm { ... }` / `asm INS;`   | `asm` `{` … `}`  or one-instruction inline     | -              | `PrsStmt.HC:944-961`, `PrsAsmBlk`               | inside fun body: emitted as `IC_ASM`. At file scope or inside AOT: passed to `CmpJoin` with `CMPF_ASM_BLK` |
| `streamblk { ... }` / `#exe { ... }` | `{ ... }`                              | -              | `PrsStreamBlk` (`PrsStmt.HC:805`)               | runs body in a fresh hash-table context with `CCF_EXE_BLK`; the body's accumulated text is then **re-fed into the lexer** as a generated source segment |
| `static type ident ...;`     | `static` …                                     | -              | combined as a member-decl prefix                | `static` outside a fun creates a public-suppressed global; inside a fun creates `MLF_STATIC` local with rip-relative storage |
| `interrupt`/`haserrcode`/`argpop`/`noargpop` | applied to following fun decl | -              | `PrsStmt.HC:1066-1085`                          | accumulated into `fsp_flags`; reset to `FSF_ASM` after consumption |
| Implicit Print               | `TK_STR` / `TK_CHAR_CONST` ... `;`             | -              | `PrsStmt.HC:1201-1203`                          | top-level `"fmt", a, b;` becomes `Print("fmt",a,b)` |

### 3.2 Sub-switch (`start { } end`) details

This is unique to HolyC and has surprised every other parser:

```holyc
switch (x) {
  case 1: ...; break;
  start:
    case 2: ...; break;
    case 3: ...; break;
  end:
  case 4: ...; break;
}
```

Inside `case 1`/`case 4` the engine treats `start:`…`end:` as **a tiny
subroutine** `IC_SUB_CALL`-d at every entry into the surrounding cases.
On `end:` it emits `IC_RET` so each "case body" terminates cleanly.

Multiple sub-switches stack via `CSubSwitch` (`PrsStmt.HC:566`). Each
`case:` inside a sub-switch synthesises an `IC_SUB_CALL` for *every*
already-open sub-switch, so fall-through is preserved (PrsStmt.HC:672-680).

The Rust port can model this as a stack of `SubSwitch { lb_start,
lb_break }`. Mirror the `OptFree(tmpi_jmp)` of the leading `IC_JMP` when
no statements appeared between `start:` and the next case
(PrsStmt.HC:649-657) — TempleOS depends on it for a clean code layout.

### 3.3 `try` / `catch` semantics

`PrsTryBlk` (`PrsStmt.HC:842`) requires that `SysTry` and `SysUntry`
exist in the hash table — without them it `LexExcept`s. The parser
emits:

```
IC_CALL_START
IC_GET_LABEL untry      ; pushed
IC_GET_LABEL catch      ; pushed
IC_CALL SysTry
IC_ADD_RSP 16
IC_CALL_END
IC_END_EXP
... body ...
IC_LABEL untry
IC_CALL SysUntry
IC_JMP done
IC_LABEL catch
... catch body ...
IC_RET
IC_LABEL done
```

Every nested `return` walks the `try_cnt` and emits an `IC_CALL SysUntry`
per level (`PrsStmt.HC:1093-1108`).

### 3.4 `do { } while ( cond );`

Identical syntactically to C. Two differences worth flagging in the port:

1. `PrsDoWhile` consumes the trailing `;` itself
   (`PrsStmt.HC:524-526`), so a calling `PrsStmt` *must not* also
   consume it. The dispatcher in `PrsStmt` does not set
   `CMPF_PRS_SEMICOLON` on the `KW_DO` path, which is why this works
   accidentally — port faithfully.
2. The `do` body is parsed via `PrsStmt`, so it accepts a single
   un-braced statement (e.g. `do x++; while(cond);`).

### 3.5 Goto-label scoping

`COCGoToLabelFind` (`PrsLib.HC:152`) walks `cc.coc.coc_next_misc`. Labels
are scoped to the **function** (each fn pushes a fresh COC). A label
defined inside a sub-block is visible everywhere in the same function,
**including before its declaration** (forward `goto` works). Forward
labels left undefined at end of fn are caught by `COCDel`
(`PrsLib.HC:222-226`) which throws `Undefined goto label at`.

---

## 4. Declaration grammar

### 4.1 Top-level dispatch

The keyword routing in `PrsStmt` decides:

```
TK_IDENT
  ├── hash_entry is HTT_KEYWORD
  │     ├── KW_EXTERN/_EXTERN/_INTERN/_IMPORT/IMPORT → PrsGlblVarLst
  │     ├── KW_STATIC/PUBLIC/INTERRUPT/HASERRCODE/ARGPOP/NOARGPOP → set fsp_flags, loop
  │     ├── KW_CLASS/UNION → PrsClass + maybe PrsGlblVarLst
  │     └── KW_RETURN/GOTO/BREAK/IF/.../TRY/LOCK/NO_WARN → statement
  ├── hash_entry is HTT_CLASS / HTT_INTERNAL_TYPE
  │     ├── inside fun       → PrsVarLst (local, optionally MLF_STATIC)
  │     └── outside fun       → PrsGlblVarLst
  ├── hash_entry is HTT_OPCODE/ASM_KEYWORD → asm one-liner
  └── otherwise (label or expression)
TK_STR / TK_CHAR_CONST → implicit Print/PutChars
```

### 4.2 `PrsType`

`PrsType` (`PrsVar.HC:285`) reads a type starting from a class identifier
(already in `cc.hash_entry` or `*_tmpc1`). Steps:

1. Consume `*`s (≤ `PTR_STARS_NUM`), advancing the class pointer.
2. If next is `class` / `union`, recurse via `PrsClass` to introduce a
   fresh anonymous class.
3. If next is `(`, expect `*…*` and treat as a **function pointer**
   (`int (*fp)(args)` form). HolyC requires `(*` immediately — the
   `*` is what disambiguates fun-ptr from grouping.
4. Read identifier into `*_ident` (or `_anon_` if mode says it's allowed
   to be anonymous, e.g. an ellipsis-style variadic).
5. If fun-ptr, expect `) (` and recurse via `PrsFunJoin` to read args.
6. Read array dims via `PrsArrayDims`.

`PrsArrayDims` (`PrsVar.HC:247`) reads zero or more `[expr]` sequences;
`[]` is allowed only as the **outermost** dim and only for globals
(triggers undef-array-size flow). Inside fun args, `[]` is rejected
with `LexExcept "No arrays in fun args at"` (`PrsVar.HC:257`).

### 4.3 Multi-variable declarations

**This is a major footgun.** `PrsVarLst` (`PrsVar.HC:408`) handles
multi-decls but the loop is built around *one PrsType per identifier*.
Specifically (`PrsVar.HC:691-714`):

```c
case ',':
  if (mode.u8[1]==PRS1B_FUN_ARG && !(mode&PRSF_UNION))
    Lex(cc);                  // fun args use ',' to separate types
  else {
    first=FALSE;
    goto pvl_restart2;        // re-parse type for this decl
  }
```

And at `pvl_restart2` (`PrsVar.HC:486-489`):

```c
tmpc1=tmph;                   // reset to base type
LexPush(cc);
Lex(cc);                      // skip type or ','
```

So a multi-decl re-walks `PrsType` for each variable, **starting from
the same `tmph` token** stashed in `LexPush`/`LexPop`. The lex-restore
machinery is what makes `F64 a, b;` *should* work. In practice, the
combination of:

- the `LexPush`/`LexPopNoRestore` push/pop pairs in `PrsType` when
  seeing `*` (`PrsVar.HC:300-303`)
- the array-dim push/pop inside `PrsArrayDims`
- the static/local init paths at `PrsVar.HC:556-617`

means **certain combinations break**. From observed failures:

| Pattern                       | Result                                                                  |
| ----------------------------- | ----------------------------------------------------------------------- |
| `F64 a, b;` (top-level)       | **Trips** in JIT/top-level mode — second decl re-enters `PrsType` but the token cursor is already past the type for the second var. (PrsVar.HC:486-489 reuses `tmph` correctly *inside* a fn but the file-scope path goes through `PrsGlblVarLst` which itself re-runs `PrsType` per declarator at PrsVar.HC:222) |
| `F64 a, b;` (inside fn)       | Works                                                                   |
| `F64 m[9], im[9];` (top-level) | **Trips** — same reason, plus `PrsArrayDims` mutates `tmpad` chain and the second decl inherits `dim.next` |
| `F64 m[9], im[9];` (inside fn) | Works                                                                   |
| `I64 *p, *q;`                 | Works (each `*` is fresh on the second decl)                            |
| `I64 *p, q;`                  | Works — `q` gets the base type, not `I64*`                              |
| `I64 a = 1, b = 2;`           | Works                                                                   |

Conclusion: `PrsGlblVarLst`'s re-run of `PrsType` per declarator
**re-reads tokens** rather than re-using a pushed lex state. The Rust
port should:

- For globals, accept the multi-decl form by **caching the base type +
  storage modifiers**, then for each `, IDENT (...)` re-issue declarator
  parsing without re-reading the type word. (i.e. fix the bug in the
  port, but document it as a deviation.) *Or* mirror it bug-for-bug
  with a `parser-bug-compat: 'multi-decl'` flag.

### 4.4 Initializers

`PrsVarInit2` (`PrsVar.HC:123`) and `PrsVarInit` (`PrsVar.HC:1`) handle:

- Scalar: any expression. Compiled with `LexExpression2Bin`,
  immediately `Call`-ed if non-AOT, and the resulting `r` is
  `MemCpy`'d into `dst`.
- Array of `I8`/`U8` with string literal: `char s[] = "abc"` style;
  also fills undefined size from string length (`PrsVar.HC:131-152`).
- Aggregate `{ ... }`: walks members recursively. Trailing comma
  permitted before `}`. Designated initialisers are **not** supported
  (no `.field =` syntax).
- Struct/class init: same `{ ... }` form, member-order-only.
- Undefined array `[]`: queues each subarray, counts at the end, sets
  `total_cnt`, then copies into a freshly malloc'd block
  (`PrsVar.HC:159-189`). The first pass (`pass==1`) only sizes; the
  second pass (`pass==2`) actually writes. Two-pass to support
  forward refs in const-folded init expressions.
- F64 / I64 coercion happens via `r(F64)=r` etc.
  (`PrsVar.HC:101-106`).

### 4.5 Storage / function modifiers

| Modifier      | Where legal                                | Sets               | Effect                                |
| ------------- | ------------------------------------------ | ------------------ | ------------------------------------- |
| `public`      | global decl                                | `FSF_PUBLIC`       | `tmpf.type \|= HTF_PUBLIC`            |
| `static`      | global or local                            | `FSF_STATIC`       | local: `MLF_STATIC` (rip-relative)    |
| `interrupt`   | global fun                                 | `FSF_INTERRUPT`+`NOARGPOP` | tagged for `IRET`               |
| `haserrcode`  | global fun                                 | `FSF_HASERRCODE`   | adjusts entry frame                   |
| `argpop`      | global fun                                 | `FSF_ARGPOP`       | callee pops args                      |
| `noargpop`    | global fun                                 | `FSF_NOARGPOP`     | callee does not pop args              |
| `_extern X`   | global only                                | mode = `PRS0__EXTERN` | binds to absolute system symbol     |
| `_intern N`   | global only                                | `PRS0__INTERN`     | binds to compile-time numeric address |
| `_import "n"` / `import` | global only (AOT)                | `PRS0_*IMPORT`     | reference resolved by linker          |
| `extern`      | global only                                | `PRS0_EXTERN`      | forward decl                          |
| `reg [REG]`   | local var or fun arg (after type or before)| `tmpm.reg`         | hint to register allocator            |
| `noreg`       | local var or fun arg                       | `tmpm.reg=REG_NONE`| force memory                          |
| `lock`        | as a *statement* prefix                    | `cc.lock_cnt++`    | wraps each emitted IC with `ICF_LOCK` |
| `lastclass`   | only as a default-arg value in fun decl    | `MLF_LASTCLASS`    | substitutes last classname seen       |

Modifier placement (order) is loose: `static reg I64 x;` and
`reg static I64 x;` both work because the parser loops on
`PrsKeyWord` until it sees a non-modifier (`PrsVar.HC:451-466,
498-519`). The Rust port should accept any order for modifiers that
appear in this loop, and require the `_extern`/`_intern`/etc. forms
to lead the declaration.

### 4.6 Class / Union

`PrsClass` (`PrsVar.HC:1`):

```ebnf
class_decl ::= 'class' IDENT (':' IDENT)? '{' member_lst '}' ';'
union_decl ::= 'union' IDENT '{' member_lst '}' ';'
```

- Single-base inheritance only (`,` after base ⇒ `LexExcept "Only one
  base class allowed"`).
- The body is parsed by `PrsVarLst` with `PRS1_CLASS` (`PRSF_UNION`
  added for unions).
- `$$` inside the body equals the **current offset**; it can be
  assigned via `$$=expr;` to skip / reposition.
  (PrsVar.HC:430-444)
- Per-member metadata (`IDENT STR` or `IDENT expr` after the
  declarator) is stashed in `CMemberLstMeta` (PrsVar.HC:675-687) — the
  HolyC reflection feature.

### 4.7 Function declarations / definitions

`PrsFunJoin` (`PrsVar.HC:62`) reads `'(' arg_list ')'`. Default args
use `= expr` with the expression compiled and immediately `Call`ed to
get a constant (PrsVar.HC:629-657). `lastclass` is recognised as a
default value placeholder. Variadic `...` is the *last* parameter and
synthesises hidden `argc`/`argv` locals (`PrsDotDotDot`,
`PrsVar.HC:373`).

Function body parsing is in `PrsFun` (`PrsVar.HC:140`); it sets up a
fresh COC, emits `IC_ENTER`, calls `PrsStmt(cc,,,0)` (no
`CMPF_PRS_SEMICOLON`), checks return-class consistency, emits the
leave-label, `IC_LEAVE`, then JIT- or AOT-compiles.

---

## 5. Bug-compatibility list

Each item here is a behaviour the Rust port should **reproduce** so
existing TempleOS source files continue to parse the same way. Where the
port can be opt-in stricter, gate the fix behind a flag.

### 5.1 `return` / `goto LABEL` / `LABEL:` rejected at file scope

- `return` outside a fn:
  `LexExcept "Not in fun. Can't return a val"` (`PrsStmt.HC:1090`).
- `LABEL:` outside a fn: `LexExcept "No global labels at"`
  (`PrsStmt.HC:1198`).
- `goto LABEL;` outside a fn: parses successfully but the JIT
  immediately tries to compile the resulting code-block; since no
  matching label has been or will be defined in the same COC,
  `COCDel` later trips `Undefined goto label at` (`PrsLib.HC:222`).
  Net effect to user: `goto` at top level is unusable.

### 5.2 No `continue` keyword

Inspect `CompilerA.HH:239-313`: the keyword table contains `KW_BREAK`
but no `KW_CONTINUE`. Writing `continue` at top level matches no hash
entry, so it is treated as an **identifier** — followed by anything
else it becomes either a label-decl (`continue:`) or a free-standing
identifier expression (which then fails because it's not a callable /
not an lvalue). Common manifestation: `if (cond) continue;` reports
"Missing ';' at " or "Invalid lval at " depending on what follows.

The Rust port should detect the bare identifier `continue` in a
statement context and emit a clear diagnostic ("HolyC has no `continue`
keyword; rewrite using `goto next_iter;`") **without** changing
behaviour vs. TempleOS (still a parse error, but a friendlier one).

### 5.3 One-line untrailed-stmt forms break for some bodies

`PrsIf` calls `PrsStmt(cc, try_cnt, lb_break)` (PrsStmt.HC:473) which
allows any single statement. *But*: `PrsStmt`'s outer dispatcher's
`while(TRUE)` loop continues if the next token is `,`
(PrsStmt.HC:920-933), which most call sites avoid. The combination of:

- A one-line if-body that is itself a keyword-form (e.g. `if (c)
  return;`, `if (c) break;`)
- `KW_RETURN` / `KW_BREAK` / `KW_GOTO` doing `goto sm_semicolon`
  *before* the outer loop checks for `,` (PrsStmt.HC:1120, 1132, 1138)

means the trailing `;` is consumed and control returns to `PrsIf`
correctly. Forms that **do** trip:

- `if (cond) continue;` — fails because `continue` isn't a keyword
  (see §5.2).
- `if (cond) static I64 x;` — `static` is a modifier, so the body
  becomes `static I64 x;` which would parse as a local decl; but at
  file scope (no `cc.htc.fun`), `PrsVarLst` is not chosen — instead
  `PrsGlblVarLst` runs, *and* it executes, so the `static` global
  appears unconditionally, regardless of `cond`. The "if" is silently
  ineffective.
- `if (cond) for (...) ...;` — works, but the trailing `;` belongs to
  the inner-stmt of the `for`, not to the `if`. Visually confusing.

### 5.4 `for` / `while` with type-decl in init

`PrsFor` (`PrsStmt.HC:529`) calls `PrsStmt(cc, try_cnt)` for the init
position. That dispatcher *does* recognise type identifiers and route
them to `PrsVarLst` (function scope) or `PrsGlblVarLst` (file scope).
So `for (I64 i = 0; i < 10; i++) ...;` *should* be parseable inside a
function. **At file scope** the init runs through `PrsGlblVarLst`,
which terminates on `;` only after consuming the **first** semicolon
of the `for` header — so the `cond` token is then mis-interpreted as
a follow-on decl, blowing up with "Expecting type at" or similar.

Same for `while`: `while` has no init slot, so the question doesn't
arise — but `for(I64 i; ...; ...)` at top level is the only place this
manifests.

### 5.5 `do { } while (...)` extra semicolon

`PrsDoWhile` consumes its own trailing `;`. If a caller wraps the
`do/while` and *also* expects a `;` (e.g. via `CMPF_PRS_SEMICOLON`),
the extra `;` will be eaten as an empty statement at the next iteration
of `PrsStmt`. Net effect: `do {} while(0);;` is parsed as one valid
do-while plus one empty statement — fine. But `do {} while(0)`
(no `;`) is a **hard error** (`Missing ';' at`).

### 5.6 Postfix typecast inside another typecast

`expr (T1) (T2)` is parsed left-to-right. The outer `(T2)` re-enters
`PrsUnaryModifier case '('` which requires the inner token to be
`TK_IDENT`-resolving-to-type. If `T2` is itself parenthesised, the
parser bails. Effect: nested typecasts must be *flat*, not chained
through grouping parens. C programmers will reach for `((int)x)`
which becomes a "Use TempleOS postfix typecasting at" because of the
prefix `(` → `(` → `IDENT` lookahead.

### 5.7 `&Internal_Fun` is rejected

`PrsUnaryTerm` `case '&'` at `PrsExp.HC:621-655` checks
`Bt(&tmpf->flags, Ff_INTERNAL)` and **returns to the normal address-of
path** only for non-internal funs. For internal funs (template
intrinsics like `Sqr`, `Abs`, `RDTSC`, the `Bt`/`Bts` family), taking
the address is silently impossible; the parser falls through to the
generic `IC_ADDR` path which then fails downstream. Document this:
**you cannot take the address of a HolyC built-in template fun**.

### 5.8 `Bt`/`Bts`/`Btr`/etc. parse as fun calls, not operators

These are recognised by name lookup, not by token. Source files that
re-define `Bt` as a different function will hijack bit-test
expressions. The Rust port should special-case the standard names *or*
faithfully resolve them via a hash lookup (preferred). Either way,
emit the same `IC_BT/BTS/BTR/BTC/LBTS/LBTR/LBTC/BSF/BSR` opcode the
matching template fun specifies.

### 5.9 `(type)expr` (C-style cast) is rejected at parse time

`PrsExp.HC:728-731`. Always: `LexExcept "Use TempleOS postfix
typecasting at"`. No fallback. The Rust port should emit the same
message verbatim — diagnostic-test golden files in TempleOS check
against it.

### 5.10 Default arg evaluation runs at parse time

`PrsVar.HC:637-651`. A function declared as `U0 F(I64 x = SomeFun());`
**runs `SomeFun()` once, at the moment the header is parsed**, and
stores the result as the default. Subsequent calls without the arg get
the cached value. This is observable as a difference vs. C++ and is
relied on by some TempleOS code.

### 5.11 Switch range encoding silently truncates large ranges

`PrsStmt.HC:767-771`: `if (lo>hi || !(0<range<=0xFFFF)) LexExcept`.
Switches over more than 64K distinct values throw at parse time. Less
than 16 entries starting above zero are normalised by `if (0<lo<=16)
lo=0;` (PrsStmt.HC:767) which builds the jump table from 0 instead.

### 5.12 `case` without expression auto-increments

`PrsStmt.HC:681-685`. Two adjacent `case :` with no value reuse the
last expression `+1`. The first `case :` (no prior value) defaults to
`0`. Common idiom in TempleOS:

```c
switch (x) {
  case 1: ...;  break;
  case  : ...;  break;   // == case 2
  case  : ...;  break;   // == case 3
}
```

Document explicitly; do not "fix" to a syntax error.

### 5.13 Implicit Print at top level changes argument parsing

`PrsFunCall` with `tmpf == NULL` (PrsExp.HC:394-409) handles the
**bare-string-as-statement** form. Its argument list is **terminated
by `;` instead of `)`** (PrsExp.HC:441-453, 495-511). This is the
*only* call site where `;` ends an arg list. Any expression-internal
`;` therefore terminates the implicit Print early — sometimes
silently truncating output.

---

## 6. Error recovery strategy

TempleOS has **two** levels of try/catch in the parser:

### 6.1 Expression-level

`PrsExpression` (`PrsExp.HC:278-286`) wraps `PrsExpression2` in
`try { } catch { if (Fs->except_ch=='Compiler') ... }`. A
`LexExcept`/`throw('Compiler')` raised anywhere inside expression
parsing is caught here, the function returns `false`, and the caller
chooses what to do (most callers immediately `throw('Compiler')`
again to escalate).

A subtle behaviour: inside `PrsExpression`, the parser-stack `ps` is
**not** torn down on catch — the caller-supplied `_ps` may still hold
operator/operand entries. Subsequent expression calls can therefore
reuse the same `ps` to continue (e.g. inside `PrsFunCall` arg parsing).
The Rust port should keep this semantics: a per-call stack is
threaded through, and on error the *outer* call decides whether to
clear it.

### 6.2 Statement-level

`PrsStmt` does **not** wrap in `try/catch`. A `throw('Compiler')` from
inside it propagates up to whichever caller installed a catch. Common
catch points:

- `Cmp` / `ExeFile` / `ExePutS` (top-level driver) catches `'Compiler'`
  and prints the error, then continues with the next statement at file
  scope (recovery point: next `;` or `}`).
- `PrsTryBlk` does not catch — runtime `try` is a different mechanism.

### 6.3 Recovery points

The driver's recovery uses `LexSkipEol` and a "scan to next `}` or `;`"
loop. The Rust port can mirror this with:

```rust
fn recover_to_stmt_boundary(&mut self) {
    let mut depth = 0;
    loop {
        match self.token {
            Token::LBrace => depth += 1,
            Token::RBrace if depth == 0 => return,
            Token::RBrace => depth -= 1,
            Token::Semi if depth == 0 => { self.lex(); return; }
            Token::Eof => return,
            _ => {}
        }
        self.lex();
    }
}
```

…and call it from a wrapping driver:

```rust
loop {
    match catch_unwind(AssertUnwindSafe(|| self.parse_stmt())) {
        Ok(_) => continue,
        Err(ParseError::Compiler(_)) => self.recover_to_stmt_boundary(),
    }
}
```

(The actual port should use a `Result<(), ParseError>` rather than
unwinding, of course — TempleOS uses `try/catch` because HolyC's
`throw` is value-only and lightweight.)

### 6.4 `LexExcept` call sites by category

A non-exhaustive map from message → file:line so the port can build a
deterministic-error table:

| Message                                       | File:Line               |
| --------------------------------------------- | ----------------------- |
| `Invalid lval at `                            | PrsExp.HC:206, 812      |
| `Invalid class at `                           | PrsExp.HC:310, 320, 332, 365, 995, PrsVar.HC:46, 296 |
| `Invalid member at `                          | PrsExp.HC:336, 374, 998 |
| `Missing ')' at `                             | PrsExp.HC:744, 919, 932, 951, 1052, PrsVar.HC:352 |
| `Missing ']' at `                             | PrsExp.HC:1077, 1093, PrsVar.HC:280, PrsStmt.HC:611 |
| `Missing '}' at `                             | PrsVar.HC:29, PrsStmt.HC:750, 824 |
| `Missing ';' at`                              | PrsStmt.HC:1213, 547, 525, 442, PrsVar.HC:714 |
| `Expecting ',' at `                           | PrsExp.HC:445, 451, 502, 519, PrsVar.HC:801 |
| `Expecting type at `                          | PrsStmt.HC:1008, 1022, 1041, 1050, 1058, PrsVar.HC:477, 484 |
| `Expecting identifier at `                    | PrsVar.HC:5, 346, PrsStmt.HC:1123 |
| `Expecting '*' at `                           | PrsVar.HC:323            |
| `Expecting '(' at `                           | PrsStmt.HC:464, 489, 516, 535, PrsVar.HC:354 |
| `Expecting '{' at `                           | PrsStmt.HC:618, PrsVar.HC:16 |
| `Expecting ':' at `                           | PrsStmt.HC:631, 715, 729, 761 |
| `Expecting '=' at `                           | PrsVar.HC:432            |
| `Expecting label with underscore at `         | PrsStmt.HC:1003, 1017    |
| `Expecting system sym at `                    | PrsStmt.HC:1001, 1015    |
| `Expecting '.' at `                           | PrsExp.HC:370            |
| `Use TempleOS postfix typecasting at `        | PrsExp.HC:730            |
| `Must be address, not value `                 | PrsExp.HC:991            |
| `Not array or ptr `                           | PrsExp.HC:1061, 1065     |
| `Missing 'while' at`                          | PrsStmt.HC:514           |
| `Missing 'catch' at`                          | PrsStmt.HC:894           |
| `Missing 'end' at `                           | PrsStmt.HC:757           |
| `'break' not allowed`                         | PrsStmt.HC:1136          |
| `Not in fun.  Can't return a val `            | PrsStmt.HC:1090          |
| `No global labels at `                        | PrsStmt.HC:1198          |
| `Undefined identifier at `                    | PrsStmt.HC:1196          |
| `Duplicate goto label at `                    | PrsStmt.HC:1190          |
| `Undefined goto label at `                    | PrsLib.HC:225            |
| `Compiler Parse Error at `                    | PrsExp.HC:289            |
| `Missing expression at `                      | PrsExp.HC:957            |
| `Missing header for SysTry() and SysUntry()`  | PrsStmt.HC:850           |
| `Missing header for Print() and PutChars()`   | PrsExp.HC:396, 404       |
| `Compiler Optimization Error at `             | PrsLib.HC:294            |
| `Too many *'s at `                            | PrsVar.HC:307, 328       |
| `No arrays in fun args at `                   | PrsVar.HC:257            |
| `Static unions are not implemented `          | PrsVar.HC:541            |
| `Feature not implemented `                    | PrsVar.HC:348, PrsStmt.HC:879 |
| `import not needed at `                       | PrsStmt.HC:269, 314      |
| `Only one base class allowed at this time at `| PrsVar.HC:49             |
| `switch range error at `                      | PrsStmt.HC:771           |
| `Duplicate case at `                          | PrsStmt.HC:780           |
| `Invalid array size at `                      | PrsVar.HC:263            |
| `Can't init glbl var on data heap in AOT module `| PrsVar.HC:338         |
| `Can't take addr of extern fun`               | PrsExp.HC:636            |
| `Illegal fwd ref at `                         | PrsExp.HC:839            |
| `Use Asm Blk at `                             | PrsStmt.HC:1175          |
| `Not allowed in fun`                          | PrsStmt.HC:994           |
| `Expecting local var at `                     | PrsStmt.HC:796           |
| `Size not defined at `                        | PrsExp.HC:323            |

The Rust port should produce strings character-identical to these for
all messages so existing TempleOS tooling that greps parser output
(error-line scrapers, doc test runners) continues to work.

---

## 7. Open questions (need VM verification)

1. **Multi-decl at fn scope vs file scope.** The TempleOS parser source
   suggests `F64 a, b;` *should* work at both scopes but observation
   says it trips at file scope. Verify by:
   - running `F64 a, b;` at the boot prompt (file scope) and inside
     `U0 Test() { F64 a, b; }`
   - and confirming whether the failure is
     `"Missing ';'"` or `"Expecting type at"`.
   The exact error pinpoints which `PrsType` re-entry is at fault.

2. **`for (I64 i=0; i<10; i++) ...;` at top level.** Confirm whether
   it produces `"Expecting type at"` (initial decl runs to next `;`
   and then the cond is misread) or `"Missing ';'"`.

3. **Implicit Print arg termination.** Is `;` *the* terminator, or
   are `}` / `EOF` also accepted? PrsExp.HC:496 only checks `;`. Test
   `"foo", x` (no `;`) at end of a file — observed behaviour vs spec.

4. **`continue` as identifier.** With `continue` not being a keyword:
   is it usable as a *variable name* (`I64 continue=1;`)? The lexer
   will tokenise it as `TK_IDENT`; the hash table has no entry; the
   parser allows it. Confirm and document.

5. **`reg` after type vs before type.** `PrsVar.HC:451-466` handles
   the leading position; `PrsVar.HC:497-519` handles after-type. Do
   the two paths agree on `reg RAX I64 x;`? Or only one of them?

6. **Sub-switch inside sub-switch.** `CSubSwitch` is a queue, not a
   stack. Multiple nested `start:`...`end:` blocks: does each `case:`
   inside the innermost emit `IC_SUB_CALL` for *all* enclosing
   sub-switches in queue order, or only the immediate one? Read of
   PrsStmt.HC:672-680 says "all"; verify with a 3-level test.

7. **Empty `switch { }`.** Does the parser accept a switch with no
   cases? PrsStmt.HC:621 loops until `}`; the jump-table build
   (PrsStmt.HC:767-771) requires `range > 0`, which fails when
   `lo==I64_MAX, hi==I64_MIN`. Probable error: `switch range error at `.
   Verify.

8. **`$$=expr;` inside a class body that's also a union.** PrsVar.HC:437-440
   handles the union case but it's not clear whether `$$` is reset to
   `union_base` between members. Test:
   ```
   union U {
     I64 a;
     $$ = 32;
     I64 b;
   };
   ```

9. **Default arg with side effects.** Confirm §5.10: does the side
   effect run once (at parse time) or once per call? Suspected: once
   at parse time, value cached.

10. **`extern class Foo;` (forward declaration).** `PrsClass` with
    `is_extern=TRUE` does not parse a body but expects only an ident
    + `;`. PrsStmt.HC:1033-1038 handles this only if `extern` precedes
    `class`. Verify whether `class Foo;` (no body, no extern) is
    rejected.

---

## 8. Rust port suggestions

### 8.1 Architecture

Recursive descent, mirroring the HolyC structure:

```
parser/
├── lexer.rs       // tokens; same TK_* shape
├── cmp_ctrl.rs    // CmpCtrl state object
├── prec_table.rs  // static OPERATOR table (data-driven)
├── prs_exp.rs     // expression parser (state machine)
├── prs_stmt.rs    // statement dispatcher
├── prs_var.rs     // type, declarator, var-list
├── prs_type.rs    // PrsType + PrsArrayDims + PrsClass
├── prs_init.rs    // initializer walker
├── prs_lib.rs     // PrsStk, PrsKeyWord, hash helpers
└── ast.rs         // host AST (replaces IC stream)
```

The expression engine should be data-driven, exactly like TempleOS's
`cmp.binary_ops[token]`:

```rust
struct OpDesc {
    ic: Option<Ic>,
    prec: u8,
    assoc: Assoc,
    flags: OpFlags,
}
static BINARY_OPS: [Option<OpDesc>; 256] = build_binary_ops();
static UNARY_PRE_OPS: [Option<OpDesc>; 256] = build_unary_pre_ops();
static UNARY_POST_OPS: [Option<OpDesc>; 256] = build_unary_post_ops();
```

`build_*` are `const fn` builders that initialise from a small
`(Token, Op)` literal table — Rust's match-in-const lets us mirror
TempleOS's "fill at boot" pattern at compile time.

### 8.2 Implementing the precedence climb

The TempleOS algorithm in `PrsExpression2` is essentially a hand-rolled
shunting yard. A direct Rust port:

```rust
fn parse_expression(&mut self, end_exp: bool) -> Result<MaxPrec> {
    let mut max_prec = PREC_NULL;
    let mut stk: Vec<StkEntry> = Vec::with_capacity(16);
    stk.push(SENTINEL);
    self.parse_exp_inner(&mut stk, &mut max_prec)?;
    if stk.len() != 1 { return Err(ParseError::stack_imbalance()); }
    if end_exp { self.emit(Ic::EndExp); }
    Ok(max_prec)
}
```

The state machine in TempleOS can be encoded as recursive helpers
(`parse_unary_term`, `parse_unary_modifier`, `pop_higher`,
`push_lower`) rather than a giant `loop { match state { ... } }`,
**but** keep the names the same so cross-references to the HolyC
sources stay readable.

### 8.3 Lex push/pop

`LexPush` / `LexPopRestore` / `LexPopNoRestore` (`PrsLib`-adjacent)
implement a *checkpoint stack* for the lexer. Used heavily by
`PrsType` and `PrsVarLst` for backtracking when multi-decls require
re-walking a type.

Rust port:

```rust
pub struct LexCheckpoint { /* token, cur_str clone, file pos, line, col, ... */ }
impl Lexer {
    pub fn push_checkpoint(&mut self) -> LexCheckpoint { ... }
    pub fn pop_restore(&mut self, c: LexCheckpoint) { ... }
    pub fn pop_no_restore(&mut self, c: LexCheckpoint) { drop(c); }
}
```

The "stack of checkpoints" can live in the lexer; calls map 1:1 to
TempleOS naming.

### 8.4 Hash entries vs AST handles

TempleOS uses raw `CHashClass*` / `CHashFun*` / `CHashGlblVar*`. The
Rust port should box those into typed handles:

```rust
pub enum HashEntry {
    Class(ClassId),
    InternalType(InternalTypeId),
    Fun(FunId),
    GlblVar(GlblId),
    ExportSysSym(SysSymId),
    Keyword(Keyword),
    Opcode(Opcode),
    AsmKeyword(AsmKeyword),
    Define(DefineId),
}
```

Type encoding (`(class, ptr_stars)`) replaces TempleOS's
`tmpc++/tmpc--` star-counting. Convert at the PrsLib boundary.

### 8.5 Bug-compat flag

```rust
pub struct ParseConfig {
    pub allow_multi_decl_globals: bool,    // fix §4.3 #1
    pub allow_continue_keyword: bool,       // §5.2
    pub allow_c_style_cast: bool,           // §5.9
    pub allow_for_decl_top_level: bool,     // §5.4
    pub strict_paren_warnings: bool,        // §2.2
    // ... default = "bug-compatible with TempleOS"
}
```

Default: all `false` (faithful). Fixes are opt-in and emit a
`Diagnostic::Lint` rather than altering the resulting AST shape.

### 8.6 Error model

Single error type with location:

```rust
#[derive(Debug, Clone)]
pub enum ParseError {
    Compiler { msg: &'static str, loc: SourceLoc, fatal: bool },
    Lex(LexError),
}
```

Use the `&'static str` set from §6.4 as the `msg` so existing tooling
matches verbatim.

### 8.7 Testing strategy

1. **Golden-error tests.** For every `LexExcept` site in §6.4, write a
   minimal HolyC source that triggers it. The Rust port must produce
   the same message at the same byte offset.
2. **Bug-compat tests.** §5.1–§5.13 each get a small fixture; baseline
   captured from a TempleOS VM, then the Rust port asserts match.
3. **Cross-comparison.** A `holy-parse-diff` tool: feeds source into
   both TempleOS (via VM serial) and the Rust port, dumps the produced
   AST/IC sequence, and diffs. Differences are bugs in the port (or
   newly discovered TempleOS bugs).

### 8.8 What to *not* port

- The IC optimiser (`OptPass*`). The Rust port stops at AST.
- The COC machinery (intermediate code accumulator). Replace with a
  direct AST builder.
- `LexExpression2Bin` / `Call(machine_code)` — the Rust port cannot
  JIT-eval init expressions. Use a const-folder that handles only the
  HolyC-supported forms (literals, sizeof/offset, basic arithmetic on
  consts, identifier-as-known-const). Anything that would have JIT
  in TempleOS becomes either a deferred runtime evaluation (host
  decides) or a `ParseError::NotConstFoldable`.
- Stream-block (`#exe`, `streamblk`) bodies that produce dynamically
  generated source. The port can record the inner text and surface a
  hook for the host to feed it back through the lexer.

### 8.9 Static tables to mirror verbatim

These are TempleOS data tables the port should literally copy:

| Source table                   | Rust mirror                               |
| ------------------------------ | ----------------------------------------- |
| `cmp.binary_ops[256]`          | `BINARY_OPS: [Option<OpDesc>; 256]`       |
| `cmp.internal_types[RT_*]`     | `INTERNAL_TYPES: [ClassId; RT_COUNT]`     |
| `intermediate_code_table[]`    | `IC_TABLE: [IcDesc; IC_COUNT]` (only `.type` field needed for parser: IST_DEREF / IST_ASSIGN / IST_CMP) |
| Keyword table (HTT_KEYWORD)    | `KEYWORDS: phf::Map<&'static str, KW>`    |
| `PREC_*` constants             | `pub const PREC_*: u8 = ...` (CompilerA.HH:339) |
| Modifier sets `FSF_*`/`FSG_*`  | bitflags!                                 |
| `PRS0_*` / `PRS1_*` mode bytes | enum + flags for `PrsType`/`PrsVarLst`    |

### 8.10 Suggested public API

```rust
pub fn parse_file(src: &str, cfg: ParseConfig) -> (Module, Vec<Diagnostic>);
pub fn parse_expression(src: &str, cfg: ParseConfig) -> (Expr, Vec<Diagnostic>);
pub fn parse_stmt(src: &str, cfg: ParseConfig) -> (Stmt, Vec<Diagnostic>);
pub fn parse_type(src: &str, cfg: ParseConfig) -> (TypeRef, Vec<Diagnostic>);
```

Plus a streaming parser that yields one top-level item at a time —
needed to mirror TempleOS's "parse one stmt, run it, parse the next"
file-scope semantics:

```rust
pub struct Parser<'a> { /* ... */ }
impl<'a> Parser<'a> {
    pub fn new(src: &'a str, cfg: ParseConfig) -> Self;
    pub fn next_top_item(&mut self) -> Option<Result<TopItem, ParseError>>;
}
```

The host (devkit) drives this in lockstep with its own evaluator,
matching TempleOS's `ExePutS` loop.

---

## Appendix A. Quick map: TempleOS function → Rust module/function

| TempleOS                | Rust port                                    |
| ----------------------- | -------------------------------------------- |
| `PrsExpression`         | `prs_exp::parse_expression`                  |
| `PrsExpression2`        | `prs_exp::parse_expression_inner`            |
| `PrsAddOp`              | `prs_exp::add_op`                            |
| `PrsUnaryTerm`          | `prs_exp::parse_unary_term`                  |
| `PrsUnaryModifier`      | `prs_exp::parse_unary_modifier`              |
| `PrsFunCall`            | `prs_exp::parse_fun_call`                    |
| `PrsSizeOf`             | `prs_exp::parse_sizeof`                      |
| `PrsOffsetOf`           | `prs_exp::parse_offsetof`                    |
| `PrsStmt`               | `prs_stmt::parse_stmt`                       |
| `PrsIf`/`While`/`DoWhile`/`For`/`Switch` | `prs_stmt::parse_*`              |
| `PrsTryBlk`             | `prs_stmt::parse_try`                        |
| `PrsStreamBlk`          | `prs_stmt::parse_stream_blk`                 |
| `PrsNoWarn`             | `prs_stmt::parse_no_warn`                    |
| `PrsClass`              | `prs_type::parse_class`                      |
| `PrsType`               | `prs_type::parse_type`                       |
| `PrsArrayDims`          | `prs_type::parse_array_dims`                 |
| `PrsVarLst`             | `prs_var::parse_var_lst`                     |
| `PrsGlblVarLst`         | `prs_var::parse_glbl_var_lst`                |
| `PrsFunJoin`/`PrsFun`   | `prs_var::parse_fun_join`/`parse_fun_body`   |
| `PrsVarInit`/`PrsVarInit2`/`PrsGlblInit`/`PrsStaticInit` | `prs_init::*` |
| `PrsDotDotDot`          | `prs_var::parse_ellipsis`                    |
| `PrsKeyWord`            | `prs_lib::peek_keyword`                      |
| `PrsPush`/`PrsPop`/`PrsPopDeref` | `prs_lib::Stk` methods               |
| `ICAdd`                 | (replaced by AST builder)                    |
| `COCMiscNew`/`COCGoToLabelFind` | (replaced by AST label table)        |
| `LexExcept`/`LexWarn`   | `Diagnostics::error/warn`                    |
| `LexPush`/`LexPopRestore`/`LexPopNoRestore` | `Lexer::checkpoint/restore/discard` |
| `OptClassFwd`           | `types::strip_fwd_class`                     |
| `MemberFind`            | `types::find_member`                         |
| `HashFind`              | `htc::lookup`                                |

---

## Appendix B. Reading the IC stream as an AST shortcut

For a Rust port that wants to skip emitting IC and go straight to AST,
the mapping is:

| IC pattern                                       | AST node                          |
| ------------------------------------------------ | --------------------------------- |
| `IC_IMM_I64 N`                                   | `Expr::IntLit(N)`                 |
| `IC_IMM_F64 F`                                   | `Expr::FloatLit(F)`               |
| `IC_STR_CONST cm`                                | `Expr::StrLit(cm.str)`            |
| `IC_RBP, IC_IMM_I64 off, IC_ADD`                 | `Expr::Local { offset: off }` (resolved to ident via member-list lookup) |
| `IC_ABS_ADDR x` / `IC_IMM_I64 x` (addr of glbl)  | `Expr::Global(x)`                 |
| `IC_DEREF`                                       | `Expr::Deref(child)`              |
| `IC_ADDR`                                        | `Expr::AddrOf(child)`             |
| `IC_<binop>` after two operands                  | `Expr::BinOp { op, lhs, rhs }`    |
| `IC_HOLYC_TYPECAST` after one operand            | `Expr::Cast { to, expr }`         |
| `IC_CALL_START ... IC_CALL fn ... IC_CALL_END`   | `Expr::Call { callee, args }`     |
| `IC_BR_ZERO lb`                                  | (control flow — reconstructed as `if/while/for`) |
| `IC_LABEL lb`                                    | (control flow target)             |
| `IC_SWITCH mc_jt`                                | `Stmt::Switch { ... }`            |

Most of the IC stream is reducible because TempleOS emits in source
order; a topological pass can recover the original `Stmt`/`Expr` tree.
But it's strictly easier to **build the AST in the parser itself** and
not bother with IC reconstruction.

---

## Appendix C. Key constants reference

```text
PTR_STARS_NUM       = 4              (max '*'s on a type)
PREC_*              0x00..0x40       (CompilerA.HH:339)
ASSOCF_LEFT  = 1, ASSOCF_RIGHT = 2, ASSOC_MASK = 3
CMPF_ASM_BLK        = 1
CMPF_ONE_ASM_INS    = 2
CMPF_LEX_FIRST      = 4
CMPF_PRS_SEMICOLON  = 8
FSF_PUBLIC=1, FSF_ASM=2, FSF_STATIC=4, FSF__=8
FSF_INTERRUPT=(1<<Ff_INTERRUPT)
PRS0_NULL=0, PRS0__EXTERN=1, PRS0__INTERN=2, PRS0__IMPORT=3,
PRS0_EXTERN=4, PRS0_IMPORT=5, PRS0_TYPECAST=6
PRS1B_NULL=0, PRS1B_LOCAL_VAR=1, PRS1B_FUN_ARG=2, PRS1B_CLASS=3,
PRS1B_STATIC_LOCAL_VAR=4
PRSF_UNION=0x10000
KW_*                0..47           (CompilerA.HH:239-287)
AKW_*               64..88          (asm-only keywords)
KW_KWS_NUM          = 89
```

---

## Appendix D. Glossary

- **COC** — *Code Output Context*. The accumulator the parser emits IC
  into. Pushed/popped to reorder code (e.g. fn args evaluated before
  the call IC).
- **HTC** — *Hash Table Context*. The triple of (define table, global
  table, local table) the lexer searches for an identifier.
- **CmpCtrl** — *Compiler Controller*. The mutable parser state.
- **Member list** — TempleOS's name for the declaration list inside a
  class or function-arg list. Each entry is a `CMemberLst`.
- **Sub-switch** — A `start: ... end:` group inside a `switch` body.
  Behaves as a callable scoped sub-block inside the surrounding
  `switch`.
- **Template fun** — A built-in HolyC intrinsic compiled to a single IC
  opcode, rather than a real function call. E.g. `Sqr`, `Abs`, `Sin`,
  `Bt`, `Bts`. Identified by `Bt(&fun.flags, Ff_INTERNAL)` and
  `IC_SQR <= exe_addr <= IC_ATAN`.

---

End of spec.
