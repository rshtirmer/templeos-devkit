//! Regression coverage for the rules that moved from `holyc-lint.py`
//! to `holycc` during the lint/parser specialization. If any of these
//! drop their diagnostic, the migration silently regressed.

use holyc_parser::diag::Severity;
use holyc_parser::parse::{parse_module, ParseConfig};

fn rules(src: &str) -> Vec<&'static str> {
    let (_m, diags) = parse_module("test", src, ParseConfig::default());
    diags.into_iter()
        .filter(|d| matches!(d.severity, Severity::Error))
        .map(|d| d.rule)
        .collect()
}

#[test]
fn boot_phase_return_at_file_scope() {
    assert!(rules("return;").contains(&"return-at-file-scope"));
}

#[test]
fn boot_phase_label_at_file_scope() {
    assert!(rules("foo: U0 X(){}").contains(&"no-global-labels"));
}

#[test]
fn boot_phase_for_with_decl_at_file_scope() {
    assert!(rules("for (I64 i = 0; i < 10; i++) {}").contains(&"for-decl-top-level"));
}

#[test]
fn exponent_float_literal_negative_exp() {
    // `1e-9` is the canonical case — TempleOS's Pow10I64(-9) is
    // undefined and trips the parser at the live VM. Our lexer
    // emits the diagnostic on negative exponents.
    let r = rules("F64 x = 1e-9;");
    assert!(r.contains(&"exponent-float-literal"), "got {r:?}");
}

#[test]
fn f32_reference_as_type_position() {
    let r = rules("F32 x = 1.0;");
    assert!(r.contains(&"f32-reference"), "got {r:?}");
}

#[test]
fn reserved_name_collision_eps_param() {
    let r = rules("U0 Near(F64 actual, F64 expected, F64 eps) { Print(\"x\"); }");
    assert!(r.contains(&"reserved-name-collision"), "got {r:?}");
}

#[test]
fn reserved_name_collision_pi_local() {
    let r = rules("U0 F() { F64 pi = 3.14; Print(\"x\"); }");
    assert!(r.contains(&"reserved-name-collision"), "got {r:?}");
}

#[test]
fn reserved_name_collision_inf_param() {
    let r = rules("U0 F(F64 inf) { Print(\"x\"); }");
    assert!(r.contains(&"reserved-name-collision"), "got {r:?}");
}

#[test]
fn keyword_as_decl_name_global() {
    let r = rules("I64 end = 42;");
    assert!(r.contains(&"keyword-as-name"), "got {r:?}");
}

#[test]
fn keyword_as_decl_name_local() {
    let r = rules("U0 F() { I64 end = 42; }");
    assert!(r.contains(&"keyword-as-name"), "got {r:?}");
}

#[test]
fn keyword_as_function_name() {
    let r = rules("U0 end() { Print(\"x\"); }");
    assert!(r.contains(&"keyword-as-name"), "got {r:?}");
}

#[test]
fn legitimate_use_of_pi_constant_does_not_collide() {
    // Reading `pi` inside a function body is fine — the rule only
    // fires when `pi` is used as a parameter or local NAME.
    let r = rules("U0 F() { F64 area = pi * 2.0; Print(\"x\"); }");
    assert!(!r.contains(&"reserved-name-collision"), "got {r:?}");
}

#[test]
fn comma_decl_list_default_accepted_via_corpus() {
    // VM corpus says ExePutS accepts `F64 a, b;` at top scope; we
    // followed that in the parser default. This test pins the
    // behavior so a future tightening doesn't silently regress
    // past the corpus baseline.
    assert!(!rules("F64 a, b;").contains(&"compiler-multi-decl-global"));
}
