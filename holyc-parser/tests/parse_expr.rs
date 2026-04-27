//! Expression-parser correctness tests. See parse-spec §2 for the
//! grammar and §5 for the bug-compat behaviours exercised here.

use holyc_parser::lex::lex;
use holyc_parser::parse::ast::{
    BinOp, Expr, ExprKind, PostfixOp, PrefixOp, PrimType, SizeofArg, TypeRef,
};
use holyc_parser::parse::expr::parse_expression;
use holyc_parser::parse::parser::{ParseConfig, Parser};

// ---------------- harness ----------------

fn parse(src: &str) -> (Option<Expr>, Vec<String>) {
    parse_with(src, ParseConfig::default())
}

fn parse_with(src: &str, config: ParseConfig) -> (Option<Expr>, Vec<String>) {
    let (toks, _lex_diags) = lex("test", src);
    let mut p = Parser::new("test", toks, config);
    let e = parse_expression(&mut p);
    let rules: Vec<String> = p.diags.iter().map(|d| d.rule.to_string()).collect();
    (e, rules)
}

fn parse_ok(src: &str) -> Expr {
    let (e, rules) = parse(src);
    assert!(rules.is_empty(), "unexpected diags for `{src}`: {rules:?}");
    e.unwrap_or_else(|| panic!("parser returned None for `{src}`"))
}

fn kind(e: &Expr) -> &ExprKind {
    &e.kind
}

// ---------------- atoms ----------------

#[test]
fn atom_int_lit() {
    assert_eq!(kind(&parse_ok("42")), &ExprKind::IntLit(42));
}

#[test]
fn atom_float_lit() {
    match kind(&parse_ok("3.14")) {
        ExprKind::FloatLit(v) => assert!((*v - 3.14).abs() < 1e-9),
        k => panic!("expected FloatLit, got {k:?}"),
    }
}

#[test]
fn atom_str_lit() {
    match kind(&parse_ok("\"hi\"")) {
        ExprKind::StrLit(b) => assert_eq!(b.as_slice(), b"hi"),
        k => panic!("expected StrLit, got {k:?}"),
    }
}

#[test]
fn atom_char_lit() {
    match kind(&parse_ok("'A'")) {
        ExprKind::CharLit(v) => assert_eq!(*v, 'A' as i64),
        k => panic!("expected CharLit, got {k:?}"),
    }
}

#[test]
fn atom_ident() {
    assert_eq!(kind(&parse_ok("foo")), &ExprKind::Ident("foo".into()));
}

// ---------------- prefix unary ----------------

#[test]
fn prefix_neg() {
    match kind(&parse_ok("-x")) {
        ExprKind::Prefix(PrefixOp::Minus, inner) => {
            assert_eq!(inner.kind, ExprKind::Ident("x".into()));
        }
        k => panic!("expected Prefix(Minus,_), got {k:?}"),
    }
}

#[test]
fn prefix_lognot() {
    match kind(&parse_ok("!x")) {
        ExprKind::Prefix(PrefixOp::LogNot, _) => {}
        k => panic!("expected LogNot, got {k:?}"),
    }
}

#[test]
fn prefix_bitnot() {
    match kind(&parse_ok("~x")) {
        ExprKind::Prefix(PrefixOp::BitNot, _) => {}
        k => panic!("expected BitNot, got {k:?}"),
    }
}

#[test]
fn prefix_deref() {
    match kind(&parse_ok("*p")) {
        ExprKind::Prefix(PrefixOp::Deref, _) => {}
        k => panic!("expected Deref, got {k:?}"),
    }
}

#[test]
fn prefix_addr() {
    match kind(&parse_ok("&x")) {
        ExprKind::Prefix(PrefixOp::AddrOf, _) => {}
        k => panic!("expected AddrOf, got {k:?}"),
    }
}

#[test]
fn prefix_preinc() {
    match kind(&parse_ok("++i")) {
        ExprKind::Prefix(PrefixOp::PreInc, _) => {}
        k => panic!("expected PreInc, got {k:?}"),
    }
}

#[test]
fn prefix_predec() {
    match kind(&parse_ok("--i")) {
        ExprKind::Prefix(PrefixOp::PreDec, _) => {}
        k => panic!("expected PreDec, got {k:?}"),
    }
}

// ---------------- postfix ----------------

#[test]
fn postfix_inc() {
    match kind(&parse_ok("i++")) {
        ExprKind::Postfix(PostfixOp::Inc, _) => {}
        k => panic!("expected Postfix(Inc), got {k:?}"),
    }
}

#[test]
fn postfix_dec() {
    match kind(&parse_ok("i--")) {
        ExprKind::Postfix(PostfixOp::Dec, _) => {}
        k => panic!("expected Postfix(Dec), got {k:?}"),
    }
}

#[test]
fn postfix_index() {
    match kind(&parse_ok("a[i]")) {
        ExprKind::Index(arr, idx) => {
            assert_eq!(arr.kind, ExprKind::Ident("a".into()));
            assert_eq!(idx.kind, ExprKind::Ident("i".into()));
        }
        k => panic!("expected Index, got {k:?}"),
    }
}

#[test]
fn postfix_member_dot() {
    match kind(&parse_ok("a.b")) {
        ExprKind::Member(o, m) => {
            assert_eq!(o.kind, ExprKind::Ident("a".into()));
            assert_eq!(m, "b");
        }
        k => panic!("expected Member, got {k:?}"),
    }
}

#[test]
fn postfix_arrow() {
    match kind(&parse_ok("a->b")) {
        ExprKind::Arrow(o, m) => {
            assert_eq!(o.kind, ExprKind::Ident("a".into()));
            assert_eq!(m, "b");
        }
        k => panic!("expected Arrow, got {k:?}"),
    }
}

#[test]
fn postfix_call_args() {
    match kind(&parse_ok("f(1, 2)")) {
        ExprKind::Call(callee, args) => {
            assert_eq!(callee.kind, ExprKind::Ident("f".into()));
            assert_eq!(args.len(), 2);
            assert_eq!(args[0].kind, ExprKind::IntLit(1));
            assert_eq!(args[1].kind, ExprKind::IntLit(2));
        }
        k => panic!("expected Call, got {k:?}"),
    }
}

#[test]
fn postfix_call_no_args() {
    match kind(&parse_ok("f()")) {
        ExprKind::Call(_, args) => assert!(args.is_empty()),
        k => panic!("expected Call, got {k:?}"),
    }
}

#[test]
fn postfix_typecast_holyc() {
    // `expr (F64)` postfix typecast (parse-spec §2.3).
    match kind(&parse_ok("x (F64)")) {
        ExprKind::HolycCast(inner, ty) => {
            assert_eq!(inner.kind, ExprKind::Ident("x".into()));
            assert!(matches!(ty, TypeRef::Prim { ty: PrimType::F64, pointer_depth: 0 }));
        }
        k => panic!("expected HolycCast, got {k:?}"),
    }
}

#[test]
fn postfix_typecast_pointer() {
    match kind(&parse_ok("p (U8*)")) {
        ExprKind::HolycCast(_, ty) => {
            assert!(matches!(ty, TypeRef::Prim { ty: PrimType::U8, pointer_depth: 1 }));
        }
        k => panic!("expected HolycCast, got {k:?}"),
    }
}

// ---------------- binary precedence ----------------

#[test]
fn precedence_add_mul() {
    // 1 + 2 * 3 == 1 + (2*3)
    match kind(&parse_ok("1 + 2 * 3")) {
        ExprKind::Binary(BinOp::Add, l, r) => {
            assert_eq!(l.kind, ExprKind::IntLit(1));
            match &r.kind {
                ExprKind::Binary(BinOp::Mul, ll, rr) => {
                    assert_eq!(ll.kind, ExprKind::IntLit(2));
                    assert_eq!(rr.kind, ExprKind::IntLit(3));
                }
                k => panic!("expected Mul on RHS, got {k:?}"),
            }
        }
        k => panic!("expected Add at root, got {k:?}"),
    }
}

#[test]
fn precedence_shift_at_mul_level() {
    // HolyC quirk: `<<` is at PREC_MUL, tighter than `+`.
    // `1 << 2 + 3` parses as `(1 << 2) + 3` because `<<` is tighter
    // than `+`. (TempleOS PrsExp §2.2.)
    match kind(&parse_ok("1 << 2 + 3")) {
        ExprKind::Binary(BinOp::Add, l, r) => {
            match &l.kind {
                ExprKind::Binary(BinOp::Shl, ll, rr) => {
                    assert_eq!(ll.kind, ExprKind::IntLit(1));
                    assert_eq!(rr.kind, ExprKind::IntLit(2));
                }
                k => panic!("expected Shl on LHS, got {k:?}"),
            }
            assert_eq!(r.kind, ExprKind::IntLit(3));
        }
        k => panic!("expected Add at root, got {k:?}"),
    }
}

#[test]
fn precedence_shift_left_assoc() {
    // `a << 4 << 5` — left-assoc within MUL → ((a<<4)<<5).
    match kind(&parse_ok("a << 4 << 5")) {
        ExprKind::Binary(BinOp::Shl, l, r) => {
            assert_eq!(r.kind, ExprKind::IntLit(5));
            match &l.kind {
                ExprKind::Binary(BinOp::Shl, _, _) => {}
                k => panic!("expected nested Shl LHS, got {k:?}"),
            }
        }
        k => panic!("expected Shl at root, got {k:?}"),
    }
}

#[test]
fn logical_and_or_precedence() {
    // a && b || c == (a && b) || c
    match kind(&parse_ok("a && b || c")) {
        ExprKind::Binary(BinOp::LogOr, l, r) => {
            assert!(matches!(l.kind, ExprKind::Binary(BinOp::LogAnd, _, _)));
            assert_eq!(r.kind, ExprKind::Ident("c".into()));
        }
        k => panic!("expected LogOr at root, got {k:?}"),
    }
}

#[test]
fn logical_xor_holyc_only() {
    // ^^ exists in HolyC.
    match kind(&parse_ok("a ^^ b")) {
        ExprKind::Binary(BinOp::LogXor, _, _) => {}
        k => panic!("expected LogXor, got {k:?}"),
    }
}

#[test]
fn power_right_assoc() {
    // 2 `3 `2 == 2 ^ (3 ^ 2)
    match kind(&parse_ok("2 `3 `2")) {
        ExprKind::Binary(BinOp::Power, l, r) => {
            assert_eq!(l.kind, ExprKind::IntLit(2));
            match &r.kind {
                ExprKind::Binary(BinOp::Power, ll, rr) => {
                    assert_eq!(ll.kind, ExprKind::IntLit(3));
                    assert_eq!(rr.kind, ExprKind::IntLit(2));
                }
                k => panic!("expected Power on RHS, got {k:?}"),
            }
        }
        k => panic!("expected Power at root, got {k:?}"),
    }
}

#[test]
fn power_simple() {
    match kind(&parse_ok("2 `3")) {
        ExprKind::Binary(BinOp::Power, l, r) => {
            assert_eq!(l.kind, ExprKind::IntLit(2));
            assert_eq!(r.kind, ExprKind::IntLit(3));
        }
        k => panic!("expected Power, got {k:?}"),
    }
}

#[test]
fn power_tighter_than_mul() {
    // 2 * 3 `4 == 2 * (3`4)
    match kind(&parse_ok("2 * 3 `4")) {
        ExprKind::Binary(BinOp::Mul, _, r) => {
            assert!(matches!(r.kind, ExprKind::Binary(BinOp::Power, _, _)));
        }
        k => panic!("expected Mul at root, got {k:?}"),
    }
}

#[test]
fn compare_chain_left_assoc() {
    // `a < b == c` — left-assoc through CMP/CMP2 (CMP is tighter).
    match kind(&parse_ok("a < b == c")) {
        ExprKind::Binary(BinOp::Eq, l, r) => {
            assert!(matches!(l.kind, ExprKind::Binary(BinOp::Lt, _, _)));
            assert_eq!(r.kind, ExprKind::Ident("c".into()));
        }
        k => panic!("expected Eq at root, got {k:?}"),
    }
}

// ---------------- assignment ----------------

#[test]
fn assign_simple() {
    match kind(&parse_ok("x = 1")) {
        ExprKind::Binary(BinOp::Assign, l, r) => {
            assert_eq!(l.kind, ExprKind::Ident("x".into()));
            assert_eq!(r.kind, ExprKind::IntLit(1));
        }
        k => panic!("expected Assign, got {k:?}"),
    }
}

#[test]
fn assign_compound_add() {
    match kind(&parse_ok("x += 1")) {
        ExprKind::Binary(BinOp::AddAssign, _, _) => {}
        k => panic!("expected AddAssign, got {k:?}"),
    }
}

#[test]
fn assign_compound_shl() {
    match kind(&parse_ok("x <<= 2")) {
        ExprKind::Binary(BinOp::ShlAssign, _, _) => {}
        k => panic!("expected ShlAssign, got {k:?}"),
    }
}

#[test]
fn assign_right_assoc() {
    // a = b = 1 → a = (b = 1)
    match kind(&parse_ok("a = b = 1")) {
        ExprKind::Binary(BinOp::Assign, _, r) => {
            assert!(matches!(r.kind, ExprKind::Binary(BinOp::Assign, _, _)));
        }
        k => panic!("expected Assign, got {k:?}"),
    }
}

// ---------------- builtins ----------------

#[test]
fn sizeof_type() {
    match kind(&parse_ok("sizeof(F64)")) {
        ExprKind::Sizeof(SizeofArg::Type(TypeRef::Prim { ty: PrimType::F64, pointer_depth: 0 })) => {}
        k => panic!("expected Sizeof(Type F64), got {k:?}"),
    }
}

#[test]
fn sizeof_expr() {
    match kind(&parse_ok("sizeof(x)")) {
        ExprKind::Sizeof(SizeofArg::Expr(e)) => {
            assert_eq!(e.kind, ExprKind::Ident("x".into()));
        }
        k => panic!("expected Sizeof(Expr), got {k:?}"),
    }
}

#[test]
fn offset_chain() {
    match kind(&parse_ok("offset(Foo.bar)")) {
        ExprKind::OffsetOf(parts) => {
            assert_eq!(parts, &vec!["Foo".to_string(), "bar".to_string()]);
        }
        k => panic!("expected OffsetOf, got {k:?}"),
    }
}

#[test]
fn defined_macro() {
    match kind(&parse_ok("defined(MACRO)")) {
        ExprKind::Defined(name) => assert_eq!(name, "MACRO"),
        k => panic!("expected Defined, got {k:?}"),
    }
}

// ---------------- parens ----------------

#[test]
fn paren_expr() {
    match kind(&parse_ok("(1 + 2)")) {
        ExprKind::Paren(inner) => {
            assert!(matches!(inner.kind, ExprKind::Binary(BinOp::Add, _, _)));
        }
        k => panic!("expected Paren, got {k:?}"),
    }
}

#[test]
fn paren_overrides_precedence() {
    // (1 + 2) * 3 — multiplication of a paren'd add.
    match kind(&parse_ok("(1 + 2) * 3")) {
        ExprKind::Binary(BinOp::Mul, l, r) => {
            assert!(matches!(l.kind, ExprKind::Paren(_)));
            assert_eq!(r.kind, ExprKind::IntLit(3));
        }
        k => panic!("expected Mul, got {k:?}"),
    }
}

// ---------------- bug-compat ----------------

#[test]
fn c_style_cast_rejected_default() {
    // `(F64)x` — C-style prefix cast. Must error with the documented
    // rule (parse-spec §5.9).
    let (_e, rules) = parse("(F64)x");
    assert!(
        rules.iter().any(|r| r == "compiler-c-style-cast"),
        "expected compiler-c-style-cast rule, got {rules:?}"
    );
}

#[test]
fn c_style_cast_allowed_with_flag() {
    let cfg = ParseConfig {
        allow_c_style_cast: true,
        ..Default::default()
    };
    let (e, rules) = parse_with("(F64)x", cfg);
    assert!(rules.is_empty(), "unexpected diags: {rules:?}");
    let e = e.expect("expected an expression");
    assert!(matches!(e.kind, ExprKind::HolycCast(_, _)));
}

#[test]
fn no_ternary() {
    // HolyC has no `?:`. We emit `expr-no-ternary`.
    let (_e, rules) = parse("a ? b : c");
    assert!(
        rules.iter().any(|r| r == "expr-no-ternary"),
        "expected expr-no-ternary rule, got {rules:?}"
    );
}

#[test]
fn float_with_negative_exponent_still_parses() {
    // `1e-9` is a lex-quirk in HolyC (TempleOS rejects neg exponents),
    // but the lexer still emits a FloatLit token. The expression parser
    // must accept it without compounding the diagnostic.
    let (toks, _diags) = lex("test", "1e-9");
    let mut p = Parser::new("test", toks, ParseConfig::default());
    let e = parse_expression(&mut p);
    let parser_rules: Vec<String> = p.diags.iter().map(|d| d.rule.to_string()).collect();
    assert!(
        parser_rules.is_empty(),
        "expression parser produced spurious diags: {parser_rules:?}"
    );
    let e = e.expect("expected an expression");
    assert!(matches!(e.kind, ExprKind::FloatLit(_)));
}

#[test]
fn bt_parses_as_function_call() {
    // Bug-compat §5.8: `Bt`/`Bts`/`Btr` are NOT operators, just calls.
    match kind(&parse_ok("Bt(&v, 3)")) {
        ExprKind::Call(callee, args) => {
            assert_eq!(callee.kind, ExprKind::Ident("Bt".into()));
            assert_eq!(args.len(), 2);
        }
        k => panic!("expected Call, got {k:?}"),
    }
}

#[test]
fn nested_call_member_chain() {
    // f(x).y[0]++ — exercise full postfix chain.
    let e = parse_ok("f(x).y[0]++");
    match kind(&e) {
        ExprKind::Postfix(PostfixOp::Inc, inner) => match &inner.kind {
            ExprKind::Index(target, _) => match &target.kind {
                ExprKind::Member(_, m) => assert_eq!(m, "y"),
                k => panic!("expected Member, got {k:?}"),
            },
            k => panic!("expected Index, got {k:?}"),
        },
        k => panic!("expected Postfix Inc at root, got {k:?}"),
    }
}

#[test]
fn unary_minus_with_power_associativity() {
    // -2 `2 — TempleOS treats `-x``y` as `-(x``y)` (parse-spec §2.2).
    // Our AST encodes this naturally because power is tighter than
    // unary in our impl: prefix unary descends into another unary
    // term, which then parses 2 as the LHS of `\``. So the result is
    // Prefix(Minus, Power(2, 2)) — the documented semantics.
    let e = parse_ok("-2 `2");
    match &e.kind {
        ExprKind::Prefix(PrefixOp::Minus, inner) => {
            assert!(matches!(inner.kind, ExprKind::Binary(BinOp::Power, _, _)));
        }
        k => panic!("expected Prefix(Minus, Power), got {k:?}"),
    }
}

#[test]
fn bitwise_precedence() {
    // a & b | c == (a & b) | c
    match kind(&parse_ok("a & b | c")) {
        ExprKind::Binary(BinOp::BitOr, l, _) => {
            assert!(matches!(l.kind, ExprKind::Binary(BinOp::BitAnd, _, _)));
        }
        k => panic!("expected BitOr at root, got {k:?}"),
    }
}

#[test]
fn comma_in_call_args_only() {
    // We don't implement the comma operator at expression level; commas
    // inside `f(a, b)` must still be argument separators.
    match kind(&parse_ok("f(a, b, c)")) {
        ExprKind::Call(_, args) => assert_eq!(args.len(), 3),
        k => panic!("expected Call, got {k:?}"),
    }
}
