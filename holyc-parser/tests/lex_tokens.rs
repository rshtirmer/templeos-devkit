//! Lexer correctness — token-stream golden tests.

use holyc_parser::lex::{lex, TokenKind};

fn kinds(src: &str) -> Vec<TokenKind> {
    let (toks, diags) = lex("test", src);
    if !diags.is_empty() {
        for d in &diags { eprintln!("{d}"); }
    }
    toks.into_iter().map(|t| t.kind).collect()
}

fn kinds_with_diags(src: &str) -> (Vec<TokenKind>, Vec<String>) {
    let (toks, diags) = lex("test", src);
    (
        toks.into_iter().map(|t| t.kind).collect(),
        diags.into_iter().map(|d| d.rule.to_string()).collect(),
    )
}

#[test]
fn empty_source_only_eof() {
    assert_eq!(kinds(""), vec![TokenKind::Eof]);
}

#[test]
fn whitespace_eaten() {
    assert_eq!(kinds("   \t\n\n  "), vec![TokenKind::Eof]);
}

#[test]
fn line_comment() {
    assert_eq!(kinds("// hello\n42"), vec![TokenKind::IntLit(42), TokenKind::Eof]);
}

#[test]
fn block_comment_nested() {
    let src = "/* outer /* inner */ outer */ 7";
    assert_eq!(kinds(src), vec![TokenKind::IntLit(7), TokenKind::Eof]);
}

#[test]
fn unterminated_block_comment_is_diag() {
    let (_toks, rules) = kinds_with_diags("/* never closes");
    assert!(rules.iter().any(|r| r == "lex-unterminated-comment"), "rules={rules:?}");
}

// ---------- identifiers ----------

#[test]
fn ident_basic() {
    assert_eq!(kinds("foo"), vec![TokenKind::Ident("foo".into()), TokenKind::Eof]);
}

#[test]
fn ident_with_digits_and_underscores() {
    assert_eq!(
        kinds("_foo123 bar_baz"),
        vec![
            TokenKind::Ident("_foo123".into()),
            TokenKind::Ident("bar_baz".into()),
            TokenKind::Eof
        ]
    );
}

// ---------- integer literals ----------

#[test]
fn int_decimal() {
    assert_eq!(kinds("0 17 1024"), vec![
        TokenKind::IntLit(0),
        TokenKind::IntLit(17),
        TokenKind::IntLit(1024),
        TokenKind::Eof,
    ]);
}

#[test]
fn int_no_octal_means_decimal() {
    // 017 must parse as decimal 17 (TempleOS has no octal — see
    // lex-spec.md §1.4.1 / Q5).
    assert_eq!(kinds("017"), vec![TokenKind::IntLit(17), TokenKind::Eof]);
}

#[test]
fn int_hex() {
    assert_eq!(kinds("0xFF 0xdeadbeef"), vec![
        TokenKind::IntLit(0xFF),
        TokenKind::IntLit(0xdeadbeef),
        TokenKind::Eof,
    ]);
}

#[test]
fn int_binary() {
    assert_eq!(kinds("0b1010"), vec![TokenKind::IntLit(0b1010), TokenKind::Eof]);
}

// ---------- float literals ----------

#[test]
fn float_basic() {
    let toks = kinds("3.14");
    match &toks[0] {
        TokenKind::FloatLit(f) => assert!((f - 3.14).abs() < 1e-12, "got {f}"),
        other => panic!("expected FloatLit, got {other:?}"),
    }
}

#[test]
fn float_leading_dot() {
    let toks = kinds(".5");
    match &toks[0] {
        TokenKind::FloatLit(f) => assert!((f - 0.5).abs() < 1e-12, "got {f}"),
        other => panic!("expected FloatLit, got {other:?}"),
    }
}

#[test]
fn float_exponent_negative_diags() {
    // The `1e-9` literal trips TempleOS's Pow10I64(neg). Our lexer
    // produces a usable f64 host-side value and emits the
    // `exponent-float-literal` diagnostic noting the VM bug.
    let (toks, rules) = kinds_with_diags("1e-9");
    assert!(matches!(toks[0], TokenKind::FloatLit(_)));
    assert!(
        rules.iter().any(|r| r == "exponent-float-literal"),
        "expected exponent-float-literal diag; got {rules:?}"
    );
}

#[test]
fn float_exponent_positive_no_diag() {
    let (toks, rules) = kinds_with_diags("1e9");
    match &toks[0] {
        TokenKind::FloatLit(f) => assert!((f - 1e9).abs() < 1.0, "got {f}"),
        other => panic!("expected FloatLit, got {other:?}"),
    }
    assert!(!rules.iter().any(|r| r == "exponent-float-literal"));
}

#[test]
fn one_dot_dot_is_int_then_dotdot() {
    // `1..` is two tokens: IntLit(1), DotDot. (lex-spec §2 line "1.."
    // is int-then-dotdot.)
    assert_eq!(
        kinds("1..3"),
        vec![
            TokenKind::IntLit(1),
            TokenKind::DotDot,
            TokenKind::IntLit(3),
            TokenKind::Eof
        ]
    );
}

// ---------- char literals (multi-byte packed little-endian) ----------

#[test]
fn char_single() {
    assert_eq!(kinds("'A'"), vec![TokenKind::CharLit(0x41), TokenKind::Eof]);
}

#[test]
fn char_two_byte_packed_little_endian() {
    // 'AB' must be 0x4241 (B=0x42 in slot 1, A=0x41 in slot 0).
    assert_eq!(kinds("'AB'"), vec![TokenKind::CharLit(0x4241), TokenKind::Eof]);
}

#[test]
fn char_eight_byte_packed() {
    let toks = kinds("'ABCDEFGH'");
    let expected = i64::from_le_bytes([b'A', b'B', b'C', b'D', b'E', b'F', b'G', b'H']);
    assert_eq!(toks[0], TokenKind::CharLit(expected));
}

#[test]
fn char_too_many_bytes_diags() {
    let (_toks, rules) = kinds_with_diags("'ABCDEFGHI'");
    assert!(rules.iter().any(|r| r == "lex-char-too-long"), "rules={rules:?}");
}

#[test]
fn char_with_escape_n() {
    assert_eq!(kinds("'\\n'"), vec![TokenKind::CharLit(b'\n' as i64), TokenKind::Eof]);
}

#[test]
fn char_with_escape_d_is_dollar() {
    // HolyC `\d` is the dollar sign (DolDoc escape).
    assert_eq!(kinds("'\\d'"), vec![TokenKind::CharLit(b'$' as i64), TokenKind::Eof]);
}

// ---------- strings ----------

#[test]
fn string_basic() {
    assert_eq!(
        kinds(r#""hello""#),
        vec![TokenKind::StrLit(b"hello".to_vec()), TokenKind::Eof]
    );
}

#[test]
fn string_with_escapes() {
    let toks = kinds(r#""a\nb\tc""#);
    assert_eq!(toks[0], TokenKind::StrLit(b"a\nb\tc".to_vec()));
}

#[test]
fn string_unterminated_diags() {
    let (_toks, rules) = kinds_with_diags("\"runaway");
    assert!(rules.iter().any(|r| r == "lex-unterminated-string"));
}

// ---------- punctuation + multi-char operators ----------

#[test]
fn single_char_punct_basic() {
    assert_eq!(
        kinds("(){}[];,:?~"),
        vec![
            TokenKind::LParen, TokenKind::RParen,
            TokenKind::LBrace, TokenKind::RBrace,
            TokenKind::LBracket, TokenKind::RBracket,
            TokenKind::Semicolon, TokenKind::Comma,
            TokenKind::Colon, TokenKind::Question,
            TokenKind::Tilde,
            TokenKind::Eof,
        ]
    );
}

#[test]
fn dual_eq_family() {
    assert_eq!(
        kinds("== != <= >= && || ^^"),
        vec![
            TokenKind::EqEq, TokenKind::BangEq,
            TokenKind::LtEq, TokenKind::GtEq,
            TokenKind::AmpAmp, TokenKind::PipePipe,
            TokenKind::CaretCaret,
            TokenKind::Eof,
        ]
    );
}

#[test]
fn shifts_and_compound_assigns() {
    assert_eq!(
        kinds("<< >> <<= >>="),
        vec![
            TokenKind::Shl, TokenKind::Shr,
            TokenKind::ShlEq, TokenKind::ShrEq,
            TokenKind::Eof,
        ]
    );
}

#[test]
fn arrow_and_inc_dec() {
    assert_eq!(
        kinds("p->x i++ j--"),
        vec![
            TokenKind::Ident("p".into()), TokenKind::Arrow, TokenKind::Ident("x".into()),
            TokenKind::Ident("i".into()), TokenKind::PlusPlus,
            TokenKind::Ident("j".into()), TokenKind::MinusMinus,
            TokenKind::Eof,
        ]
    );
}

#[test]
fn dotdot_and_ellipsis() {
    assert_eq!(
        kinds(".. ..."),
        vec![TokenKind::DotDot, TokenKind::Ellipsis, TokenKind::Eof]
    );
}

#[test]
fn double_colon_scope() {
    assert_eq!(
        kinds("Foo::Bar"),
        vec![
            TokenKind::Ident("Foo".into()),
            TokenKind::ColonColon,
            TokenKind::Ident("Bar".into()),
            TokenKind::Eof,
        ]
    );
}

#[test]
fn backtick_is_power_operator() {
    assert_eq!(
        kinds("a`b"),
        vec![
            TokenKind::Ident("a".into()),
            TokenKind::Backtick,
            TokenKind::Ident("b".into()),
            TokenKind::Eof,
        ]
    );
}

// ---------- realistic snippet ----------

#[test]
fn a_real_holyc_function_lexes_clean() {
    let src = r#"
F64 _DotProduct(F64 *v1, F64 *v2) {
  return v1[0]*v2[0] + v1[1]*v2[1] + v1[2]*v2[2];
}
"#;
    let (_toks, diags) = lex("dot.HC", src);
    assert!(diags.is_empty(), "expected no diags, got {diags:?}");
}

#[test]
fn keyword_lookup_resolves_after_lexing() {
    use holyc_parser::lex::{lookup_keyword, Keyword, TokenKind};
    let toks = kinds("if while F64 return continue");
    // Map idents through lookup. `continue` is NOT a keyword in HolyC
    // (parse-spec §5) — must come back as None.
    let resolved: Vec<Option<Keyword>> = toks.iter().filter_map(|t| match t {
        TokenKind::Ident(s) => Some(lookup_keyword(s)),
        TokenKind::Eof => None,
        _ => panic!("unexpected token {:?}", t),
    }).collect();
    assert_eq!(
        resolved,
        vec![
            Some(Keyword::If),
            Some(Keyword::While),
            Some(Keyword::F64),
            Some(Keyword::Return),
            None, // `continue` is NOT a HolyC keyword (bug-compat).
        ]
    );
}
