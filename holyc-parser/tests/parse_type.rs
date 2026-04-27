//! Tests for `parse::type_::parse_type` (PrsType, parse-spec §4.2).

use holyc_parser::lex::lex;
use holyc_parser::parse::ast::{PrimType, TypeRef};
use holyc_parser::parse::parser::{ParseConfig, Parser};
use holyc_parser::parse::type_::{parse_type, parse_type_allow_named};

fn parse_one(src: &str) -> (Option<TypeRef>, Vec<String>) {
    let (toks, _diags) = lex("test", src);
    let mut p = Parser::new("test", toks, ParseConfig::default());
    let ty = parse_type(&mut p);
    let rules: Vec<String> = p.diags.into_iter().map(|d| d.rule.to_string()).collect();
    (ty, rules)
}

fn parse_one_named(src: &str) -> (Option<TypeRef>, Vec<String>) {
    let (toks, _diags) = lex("test", src);
    let mut p = Parser::new("test", toks, ParseConfig::default());
    let ty = parse_type_allow_named(&mut p);
    let rules: Vec<String> = p.diags.into_iter().map(|d| d.rule.to_string()).collect();
    (ty, rules)
}

#[test]
fn primitive_i64() {
    let (t, _) = parse_one("I64");
    assert_eq!(t, Some(TypeRef::Prim { ty: PrimType::I64, pointer_depth: 0 }));
}

#[test]
fn primitive_f64() {
    let (t, _) = parse_one("F64");
    assert_eq!(t, Some(TypeRef::Prim { ty: PrimType::F64, pointer_depth: 0 }));
}

#[test]
fn primitive_u0_pointer() {
    let (t, _) = parse_one("U0 *");
    assert_eq!(t, Some(TypeRef::Prim { ty: PrimType::U0, pointer_depth: 1 }));
}

#[test]
fn primitive_double_star() {
    let (t, _) = parse_one("U8 **");
    assert_eq!(t, Some(TypeRef::Prim { ty: PrimType::U8, pointer_depth: 2 }));
}

#[test]
fn primitive_too_many_stars() {
    let (t, rules) = parse_one("I64 *****");
    assert!(t.is_some());
    assert!(rules.iter().any(|r| r == "too-many-stars"));
}

#[test]
fn unknown_ident_rejects_without_named_flag() {
    let (t, rules) = parse_one("Foo");
    assert!(t.is_none());
    assert!(rules.iter().any(|r| r == "expecting-type"));
}

#[test]
fn named_class_with_allow_named() {
    let (t, _) = parse_one_named("Foo");
    assert_eq!(t, Some(TypeRef::Named { name: "Foo".to_string(), pointer_depth: 0 }));
}

#[test]
fn named_class_pointer() {
    let (t, _) = parse_one_named("Foo *");
    assert_eq!(t, Some(TypeRef::Named { name: "Foo".to_string(), pointer_depth: 1 }));
}

#[test]
fn function_pointer_simple() {
    // `F64 (*)()` — return F64, no params, ptr-depth 1.
    let (t, _) = parse_one("F64 (*)()");
    match t {
        Some(TypeRef::Func { ret, params, pointer_depth }) => {
            assert_eq!(*ret, TypeRef::Prim { ty: PrimType::F64, pointer_depth: 0 });
            assert!(params.is_empty());
            assert_eq!(pointer_depth, 1);
        }
        other => panic!("expected Func, got {:?}", other),
    }
}

#[test]
fn function_pointer_named_arg() {
    let (t, _) = parse_one("U0 (*fn)(I64 x)");
    match t {
        Some(TypeRef::Func { ret, params, pointer_depth }) => {
            assert_eq!(*ret, TypeRef::Prim { ty: PrimType::U0, pointer_depth: 0 });
            assert_eq!(params.len(), 1);
            assert_eq!(params[0].name.as_deref(), Some("x"));
            assert_eq!(pointer_depth, 1);
        }
        other => panic!("expected Func, got {:?}", other),
    }
}

#[test]
fn fnptr_with_paren_no_star_falls_back() {
    // `F64 ()` — no `*` after `(`. Should NOT parse as fun-ptr;
    // the `(` is left for the outer parser. parse_type returns the
    // F64 base, leaving `(` ungobbled.
    let (toks, _) = lex("test", "F64 ()");
    let mut p = Parser::new("test", toks, ParseConfig::default());
    let t = parse_type(&mut p);
    assert_eq!(t, Some(TypeRef::Prim { ty: PrimType::F64, pointer_depth: 0 }));
    assert!(matches!(p.peek(), holyc_parser::lex::TokenKind::LParen));
}
