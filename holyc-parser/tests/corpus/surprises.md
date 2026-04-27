# Surprises — VM-vs-spec divergences

While generating this corpus we encountered five behaviours where the
TempleOS VM disagreed with what `parse-spec.md` predicted (or where the
spec was silent and the result was non-obvious). All are reproduced as
explicit `surprises/` snippets so the Rust port either matches them or
explicitly opts to be stricter.

---

## 1. `for (I64 i = 0; …)` is rejected at **function** scope, not just file scope

**Spec**: §5.4 says
> "`for (I64 i = 0; i < 10; i++) ...;` *should* be parseable inside a
> function. **At file scope** the init runs through `PrsGlblVarLst` …"

implying that at function scope the type-decl init form should work.

**VM**: rejects it at function scope as well, with
`ERROR: Missing ';' at")"`. Trace:
`&LexExcept PrsFor PrsStmt PrsStmt`.

**Snippet**: `failing/000-surprises-surprise-for-decl-fnscope.hc`

**Hypothesis**: the function-scope init does run `PrsVarLst`, but
`PrsVarLst` expects a top-level statement terminator (`;`) and consumes
through to it before returning to `PrsFor`. Since the same `;` is what
`PrsFor` expects to find between init and cond, the parser sees `i<3`
where it expected nothing.

**Recommendation for the port**: if the Rust parser opts to *fix*
this in non-bug-compat mode, gate the fix behind a flag.

---

## 2. `switch (x) { default: break; }` (no `case`) is a parse error

**Spec**: nothing in §3 forbids a switch with only a `default:`.

**VM**: `ERROR: switch range error at "}"` from `PrsSwitch`.

**Snippet**: `failing/001-surprises-surprise-switch-only-default.hc`

**Hypothesis**: §5.11's range-encoding logic computes `lo`/`hi` from
seen `case` expressions; with no cases, the range collapses to
`hi < lo`, tripping `LexExcept "switch range error"`.

**Recommendation**: the port should mirror this rejection — many
TempleOS programs rely on the diagnostic.

---

## 3. Unclosed function body `U0 f() { I64 a=1;` accepted by `ExePutS`

**Spec**: not addressed.

**VM**: parses cleanly. The daemon's `_DRun` calls `ExePutS(buf)` with a
null-terminated buffer ending mid-function. No `'Compiler'` throw is
raised — the partial fn appears to just accumulate without being
finalised.

**Snippet**: `passing/.../surprise-missing-rbrace-fn.hc`

**Caveat**: this is a **JIT-context artefact**. A standalone .HC.Z file
loaded via `Cmp` would still error. The Rust parser, which *does* see
EOF, must reject this — i.e. the corpus entry exists to document that
the VM-pipeline ground truth is sometimes too permissive.

**Recommendation**: the port should treat unclosed top-level decls as
errors. We've classified the snippet as `passing` for
*VM-cross-comparison* purposes only.

---

## 4. `dft:` outside a `switch` is treated as a regular label

**Spec**: §3.1 defines `default:` / `dft:` as a "case form inside
`PrsSwitch`". By implication, outside a switch it should error.

**VM**: parses cleanly — `dft` outside a switch is just an identifier
followed by `:`, which is an ordinary label decl.

**Snippet**: `passing/.../surprise-default-outside-switch.hc`

**Hypothesis**: `dft` *is* tokenised as `KW_DFT`, but `PrsStmt`'s
default dispatcher falls through to the label-decl case for any
`IDENT/KEYWORD : `…` token sequence at function scope. Only `PrsSwitch`
treats the keyword specially.

**Recommendation**: port should mirror — these labels are just unused
identifiers and TempleOS doesn't reserve `dft` as a statement keyword
outside switches.

---

## 5. `class C { I64 x }` (missing `;` after last member) is accepted

**Spec**: §4.6 says class body is `'{' (member ';')* '}'`. The trailing
`;` after the last member should be required.

**VM**: parses cleanly when there is a single member with no trailing
`;` before `}`. Likely the `}` token is enough to terminate the
member-decl loop; the `;` is required between members but optional
after the last.

**Snippet**: `passing/.../surprise-class-missing-semi.hc`

**Recommendation**: port can choose. If matching VM exactly, accept;
otherwise emit a stricter "expected `;` before `}`" diagnostic.

---

## Aggregate

5 surprises across 210 snippets ≈ 2.4 %. None are showstoppers; all are
documented and reproducible.
