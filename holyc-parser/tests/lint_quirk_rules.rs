//! AST-level lint rules: switch-case-shared-scope and f64-bitwise.

use holyc_parser::diag::Severity;
use holyc_parser::lint::lint_module;
use holyc_parser::parse::{parse_module, ParseConfig};

fn rules(src: &str) -> Vec<&'static str> {
    let (m, _diags) = parse_module("test", src, ParseConfig::default());
    lint_module("test", &m).into_iter().map(|d| d.rule).collect()
}

fn warnings_only(src: &str) -> Vec<&'static str> {
    let (m, _diags) = parse_module("test", src, ParseConfig::default());
    lint_module("test", &m)
        .into_iter()
        .filter(|d| matches!(d.severity, Severity::Warning))
        .map(|d| d.rule)
        .collect()
}

// ---------- switch-case-shared-scope ----------

#[test]
fn switch_case_shared_scope_collision_flagged() {
    // Two case arms each declare `I64 ia` — would trip TempleOS's
    // PrsType with "Duplicate member at "="". The lint surfaces it
    // before the VM round-trip.
    let r = rules(r#"
U0 F(I64 op) {
  switch (op) {
    case 1: {
      I64 ia = 5;
      Print("%d\n", ia);
    }
    case 2: {
      I64 ia = 7;
      Print("%d\n", ia);
    }
  }
}
"#);
    assert!(
        r.iter().any(|s| *s == "switch-case-shared-scope"),
        "expected switch-case-shared-scope; got {r:?}"
    );
}

#[test]
fn switch_case_shared_scope_distinct_names_clean() {
    let r = rules(r#"
U0 F(I64 op) {
  switch (op) {
    case 1: {
      I64 ia_first = 5;
      Print("%d\n", ia_first);
    }
    case 2: {
      I64 ia_second = 7;
      Print("%d\n", ia_second);
    }
  }
}
"#);
    assert!(
        !r.iter().any(|s| *s == "switch-case-shared-scope"),
        "false positive on distinct names: {r:?}"
    );
}

#[test]
fn switch_case_shared_scope_single_decl_clean() {
    // Variable hoisted above the switch — the recommended fix.
    let r = rules(r#"
U0 F(I64 op) {
  I64 ia;
  switch (op) {
    case 1: {
      ia = 5;
    }
    case 2: {
      ia = 7;
    }
  }
}
"#);
    assert!(
        !r.iter().any(|s| *s == "switch-case-shared-scope"),
        "false positive when var hoisted: {r:?}"
    );
}

// ---------- f64-bitwise ----------

#[test]
fn f64_bitwise_flagged_on_bare_idents() {
    // `x & y` with non-integer-shaped operands → warning.
    let r = warnings_only(r#"
U0 F() {
  F64 x = 3.0;
  F64 y = 5.0;
  F64 z = x & y;
}
"#);
    assert!(r.iter().any(|s| *s == "f64-bitwise"), "got {r:?}");
}

#[test]
fn f64_bitwise_clean_with_explicit_int_cast() {
    // The fix: HolyC postfix typecast on each operand. C-style
    // `(I64)x` is rejected by HolyC's parser (no C-style casts);
    // the canonical form is `x (I64)`.
    let r = warnings_only(r#"
U0 F() {
  F64 x = 3.0;
  F64 y = 5.0;
  I64 z = x(I64) & y(I64);
}
"#);
    assert!(
        !r.iter().any(|s| *s == "f64-bitwise"),
        "false positive with int casts: {r:?}"
    );
}

#[test]
fn f64_bitwise_clean_with_int_literal() {
    // Masking with a literal is the common idiom — must not warn.
    let r = warnings_only(r#"
U0 F() {
  I64 x = 5;
  I64 z = x & 0xFF;
}
"#);
    assert!(
        !r.iter().any(|s| *s == "f64-bitwise"),
        "false positive with literal mask: {r:?}"
    );
}

#[test]
fn f64_bitwise_chain_clean() {
    // `(x & 0xF) | y` — the inner bitand result is integer-shaped.
    let r = warnings_only(r#"
U0 F() {
  I64 x = 5;
  I64 y = 7;
  I64 z = (x & 0xF) | y;
}
"#);
    assert!(
        !r.iter().any(|s| *s == "f64-bitwise"),
        "false positive on bitop chain: {r:?}"
    );
}

#[test]
fn f64_bitwise_xor_also_flagged() {
    let r = warnings_only(r#"
U0 F() {
  F64 a = 3.0;
  F64 b = 5.0;
  F64 c = a ^ b;
}
"#);
    assert!(r.iter().any(|s| *s == "f64-bitwise"), "xor missed: {r:?}");
}

#[test]
fn f64_bitwise_clean_on_int_typed_locals() {
    // The exact pattern that surfaced in real porting work:
    // I64 locals fed into a bitwise op. Without type tracking this
    // would false-positive; the type-aware lookup must silence it.
    let r = warnings_only(r#"
U0 F() {
  I64 a = 15;
  I64 b = 5;
  I64 c = a & b;
}
"#);
    assert!(
        !r.iter().any(|s| *s == "f64-bitwise"),
        "false positive on I64 locals: {r:?}"
    );
}

#[test]
fn f64_bitwise_clean_on_int_typed_params() {
    let r = warnings_only(r#"
U0 F(I64 lo, I64 hi) {
  I64 packed = lo | hi;
}
"#);
    assert!(
        !r.iter().any(|s| *s == "f64-bitwise"),
        "false positive on I64 params: {r:?}"
    );
}

#[test]
fn f64_bitwise_clean_on_int_returning_call() {
    // Calls to a function that returns an integer type should not
    // trigger the warning — the type context records each function's
    // declared return type.
    let r = warnings_only(r#"
I64 GetBits() {
  return 42;
}
U0 F() {
  I64 x = GetBits() & GetBits();
}
"#);
    assert!(
        !r.iter().any(|s| *s == "f64-bitwise"),
        "false positive on int-returning calls: {r:?}"
    );
}

#[test]
fn f64_bitwise_still_flags_on_f64_typed_locals() {
    // Negative case — must keep firing on the actual bug pattern.
    let r = warnings_only(r#"
U0 F() {
  F64 a = 3.0;
  F64 b = 5.0;
  F64 c = a & b;
}
"#);
    assert!(
        r.iter().any(|s| *s == "f64-bitwise"),
        "should flag F64-typed bitwise: {r:?}"
    );
}

#[test]
fn f64_bitwise_clean_on_define_int_const() {
    // `#define X 0xFFFF` — body parses as integer literal, so the
    // type context records the name as I64-typed.
    let r = warnings_only(r#"
#define MAX_VAL 0xFFFF
U0 F() {
  I64 x = 5;
  I64 y = x & MAX_VAL;
}
"#);
    assert!(
        !r.iter().any(|s| *s == "f64-bitwise"),
        "false positive on int-typed #define: {r:?}"
    );
}

// ---------- arity-mismatch ----------

#[test]
fn arity_too_few_flagged() {
    use holyc_parser::diag::Severity;
    let (m, _) = holyc_parser::parse::parse_module(
        "test",
        "U0 Print2(U8 *a, U8 *b) {} U0 F() { Print2(\"x\"); }",
        holyc_parser::parse::ParseConfig::default(),
    );
    let diags = holyc_parser::lint::lint_module("test", &m);
    let errs: Vec<_> = diags
        .iter()
        .filter(|d| matches!(d.severity, Severity::Error))
        .map(|d| d.rule)
        .collect();
    assert!(errs.contains(&"arity-mismatch"), "got {errs:?}");
}

#[test]
fn arity_too_many_flagged() {
    let r = rules(r#"
U0 Print1(U8 *m) {}
U0 F() { Print1("x", "y"); }
"#);
    assert!(r.contains(&"arity-mismatch"), "got {r:?}");
}

#[test]
fn arity_correct_clean() {
    let r = rules(r#"
U0 Print1(U8 *m) {}
U0 F() { Print1("x"); }
"#);
    assert!(!r.contains(&"arity-mismatch"), "false positive: {r:?}");
}

#[test]
fn arity_zero_args_clean() {
    let r = rules(r#"
U0 NoArg() {}
U0 F() { NoArg(); }
"#);
    assert!(!r.contains(&"arity-mismatch"), "false positive: {r:?}");
}

#[test]
fn arity_zero_args_flagged_when_called_with_one() {
    let r = rules(r#"
U0 NoArg() {}
U0 F() { NoArg("oops"); }
"#);
    assert!(r.contains(&"arity-mismatch"), "got {r:?}");
}

#[test]
fn arity_default_arg_slot_counts_as_provided() {
    // `f(a, , c)` — empty slot means "use the declared default."
    // The slot is a provided argument as far as arity goes; the
    // call's len matches the parameter count exactly.
    let r = rules(r#"
U0 Three(U8 *a, U8 *b = "x", U8 *c) {}
U0 F() { Three("a", , "c"); }
"#);
    assert!(!r.contains(&"arity-mismatch"), "default-arg slot must count: {r:?}");
}

#[test]
fn arity_trailing_default_arg_slot_counts_as_provided() {
    let r = rules(r#"
U0 Two(U8 *a, U8 *b = "x") {}
U0 F() { Two("a",); }
"#);
    assert!(!r.contains(&"arity-mismatch"), "trailing default slot must count: {r:?}");
}

#[test]
fn arity_variadic_accepts_extras() {
    // `...` ellipsis at end of param list is variadic — extras OK.
    let r = rules(r#"
U0 Print(U8 *fmt, ...) {}
U0 F() { Print("hi"); Print("hi %d", 42); Print("hi %d %s", 42, "x"); }
"#);
    assert!(!r.contains(&"arity-mismatch"), "variadic should accept any: {r:?}");
}

#[test]
fn arity_variadic_still_requires_fixed_args() {
    let r = rules(r#"
U0 Print(U8 *fmt, ...) {}
U0 F() { Print(); }
"#);
    assert!(r.contains(&"arity-mismatch"), "variadic must require fixed: {r:?}");
}

#[test]
fn arity_unknown_function_skipped() {
    // Calls to functions not declared in any input file aren't
    // arity-checked — they're builtins or yet-unported decls.
    let r = rules(r#"
U0 F() { CommPrint(1, "%s", "hi"); }
"#);
    assert!(!r.contains(&"arity-mismatch"), "unknown call: {r:?}");
}

#[test]
fn arity_local_shadow_skipped() {
    // If a local of the same name shadows a function declaration,
    // we don't know the local's signature, so skip the arity check.
    let r = rules(r#"
U0 Foo(U8 *a) {}
U0 G() {
  U8 *Foo;
  Foo("ignored", "extra");
}
"#);
    // No arity-mismatch — local shadow.
    assert!(!r.contains(&"arity-mismatch"), "local-shadowed: {r:?}");
}

#[test]
fn f64_bitwise_or_also_flagged() {
    let r = warnings_only(r#"
U0 F() {
  F64 a = 3.0;
  F64 b = 5.0;
  F64 c = a | b;
}
"#);
    assert!(r.iter().any(|s| *s == "f64-bitwise"), "or missed: {r:?}");
}

// ---------- f64-bitwise: per-operand check ----------
//
// The pre-2025 formulation used "warn if BOTH operands look
// non-integer," which short-circuited on the very common pattern of
// masking an F64-returning call against a literal. Operand-level
// checks must catch any F64 side regardless of what the LHS
// declaration or the other operand is.

#[test]
fn f64_bitwise_flagged_on_f64_call_with_int_literal_mask() {
    // The reproducer: an F64-returning function on the left, integer
    // literal on the right, assigned to an I64 destination. The
    // assignment target was previously "evidence enough" — but at
    // runtime the bitop happens in IEEE-754 space before the result
    // is widened to the I64 slot. Must warn.
    let r = warnings_only(r#"
F64 PR_GetFloat(I64 i) {
  return 1.5;
}
U0 F() {
  I64 vi = PR_GetFloat(0) & 0xFF;
}
"#);
    assert!(
        r.iter().any(|s| *s == "f64-bitwise"),
        "missed F64-call & int-literal pattern: {r:?}"
    );
}

#[test]
fn f64_bitwise_flagged_on_f64_call_with_i64_var() {
    // Variant: the literal is replaced by a known I64 local. The
    // F64 side still poisons the operation.
    let r = warnings_only(r#"
F64 PR_GetFloat(I64 i) {
  return 1.5;
}
U0 F() {
  I64 mask = 0xFF;
  I64 vi = PR_GetFloat(0) & mask;
}
"#);
    assert!(
        r.iter().any(|s| *s == "f64-bitwise"),
        "missed F64-call & I64-var pattern: {r:?}"
    );
}

#[test]
fn f64_bitwise_flagged_on_f64_var_with_int_literal_mask() {
    // Direct F64 local on one side, int literal on the other. Same
    // shape as a real-world Quake port site.
    let r = warnings_only(r#"
U0 F() {
  F64 v = 3.14;
  I64 vi = v & 0xFF;
}
"#);
    assert!(
        r.iter().any(|s| *s == "f64-bitwise"),
        "missed F64-local & int-literal pattern: {r:?}"
    );
}

#[test]
fn f64_bitwise_clean_on_i64_var_and_int_literal() {
    // Negative case: I64 LHS, int literal RHS. Must not warn.
    let r = warnings_only(r#"
U0 F() {
  I64 v = 5;
  I64 vi = v & 0xFF;
}
"#);
    assert!(
        !r.iter().any(|s| *s == "f64-bitwise"),
        "false positive on I64-var & int-literal: {r:?}"
    );
}

#[test]
fn f64_bitwise_clean_on_explicit_postfix_cast_of_f64() {
    // Negative case: HolyC postfix cast `expr(I64)` of the F64
    // operand should suppress the warning. The cast pulls the
    // bitop back into integer space, which is exactly the documented
    // workaround.
    let r = warnings_only(r#"
U0 F() {
  F64 v = 3.14;
  I64 vi = v(I64) & 0xFF;
}
"#);
    assert!(
        !r.iter().any(|s| *s == "f64-bitwise"),
        "false positive on cast f64 operand: {r:?}"
    );
}

#[test]
fn f64_bitwise_clean_on_explicit_postfix_cast_of_f64_call() {
    // Same as above but the F64 source is a call — verifies the cast
    // wraps a more complex expression too.
    let r = warnings_only(r#"
F64 PR_GetFloat(I64 i) {
  return 1.5;
}
U0 F() {
  I64 vi = PR_GetFloat(0)(I64) & 0xFF;
}
"#);
    assert!(
        !r.iter().any(|s| *s == "f64-bitwise"),
        "false positive on cast f64-call operand: {r:?}"
    );
}

#[test]
fn f64_bitwise_flagged_when_f64_call_is_rhs() {
    // Symmetry: if the F64 operand is on the right and the int side
    // on the left, the warning must still fire.
    let r = warnings_only(r#"
F64 PR_GetFloat(I64 i) {
  return 1.5;
}
U0 F() {
  I64 vi = 0xFF & PR_GetFloat(0);
}
"#);
    assert!(
        r.iter().any(|s| *s == "f64-bitwise"),
        "missed when F64 operand is on RHS: {r:?}"
    );
}
