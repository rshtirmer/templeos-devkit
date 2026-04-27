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
