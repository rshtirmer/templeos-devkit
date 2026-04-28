//! Preprocessor-directive parsing.
//!
//! `holyc-parser` deliberately does NOT expand macros — TempleOS's
//! parametrized `#define` is unreliable across ExePutS chunks, so we
//! recognize directives and pass them through as opaque AST nodes.
//! `#ifdef` / `#endif` do not gate parsing of the surrounding code:
//! both branches are preserved verbatim. Object-like `#define` names
//! are visible to the symbol resolver and (when the body parses as an
//! integer literal) feed into the type-aware lint context.

use holyc_parser::lex::lex;
use holyc_parser::lint::lint_module;
use holyc_parser::parse::ast::{PpDirective, TopItem};
use holyc_parser::parse::parser::{ParseConfig, Parser};
use holyc_parser::parse::{decl, parse_module};

fn parse_top(src: &str) -> Vec<TopItem> {
    let (toks, _diags) = lex("test", src);
    let mut p = Parser::new("test", toks, ParseConfig::default());
    let mut items = Vec::new();
    while !p.at_eof() {
        match decl::parse_top_item(&mut p) {
            Some(it) => items.push(it),
            None => {
                if !p.at_eof() {
                    let _ = p.bump();
                }
            }
        }
    }
    items
}

fn directives(src: &str) -> Vec<PpDirective> {
    parse_top(src)
        .into_iter()
        .filter_map(|it| match it {
            TopItem::Preprocessor(d) => Some(d),
            _ => None,
        })
        .collect()
}

// ---------- #include ----------

#[test]
fn include_path_preserved() {
    let d = directives("#include \"::/Kernel/KMain\"\n");
    assert_eq!(d.len(), 1);
    match &d[0] {
        PpDirective::Include(p) => assert_eq!(p, "::/Kernel/KMain"),
        other => panic!("expected Include, got {:?}", other),
    }
}

// ---------- object-like #define ----------

#[test]
fn define_object_like_recorded() {
    let d = directives("#define MAX_VAL 0xFFFF\n");
    assert_eq!(d.len(), 1);
    match &d[0] {
        PpDirective::Define { name, .. } => assert_eq!(name, "MAX_VAL"),
        other => panic!("expected Define, got {:?}", other),
    }
}

#[test]
fn define_int_const_silences_f64_bitwise_lint() {
    // The type context records `#define X <int-literal>` as I64-typed,
    // so `x & MASK` shouldn't fire f64-bitwise. Regression for PR #27.
    let (m, _) = parse_module(
        "test",
        r#"
#define MASK 0xFF
U0 F() {
  I64 x = 5;
  I64 y = x & MASK;
}
"#,
        ParseConfig::default(),
    );
    let rules: Vec<&str> = lint_module("test", &m).iter().map(|d| d.rule).collect();
    assert!(
        !rules.iter().any(|r| *r == "f64-bitwise"),
        "f64-bitwise false positive on int #define: {rules:?}"
    );
}

// (Note: the `parametrized-define` warning lives in `holyc-lint.py`,
// not the Rust parser — per the project's lint/parser split, the
// Python lint owns cheap regex-level heuristics. The parser merely
// records the directive verbatim.)

// ---------- #ifdef / #endif don't gate parsing ----------

#[test]
fn ifdef_endif_preserves_both_branches() {
    // Both branches are parsed and the directives appear in-order in
    // the AST. Conditional skipping would require macro evaluation
    // we deliberately don't do.
    let items = parse_top(
        r#"
#ifdef DEBUG
U0 DebugLog() {}
#else
U0 ReleaseLog() {}
#endif
"#,
    );
    let names: Vec<String> = items
        .iter()
        .filter_map(|it| match it {
            TopItem::Function(f) => Some(f.name.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(names, vec!["DebugLog", "ReleaseLog"]);
    let directive_kinds: Vec<&'static str> = items
        .iter()
        .filter_map(|it| match it {
            TopItem::Preprocessor(PpDirective::Ifdef(_)) => Some("ifdef"),
            TopItem::Preprocessor(PpDirective::Else) => Some("else"),
            TopItem::Preprocessor(PpDirective::EndIf) => Some("endif"),
            _ => None,
        })
        .collect();
    assert_eq!(directive_kinds, vec!["ifdef", "else", "endif"]);
}

// ---------- #ifndef recognized ----------

#[test]
fn ifndef_recognized() {
    let d = directives("#ifndef GUARD\n#endif\n");
    assert!(matches!(d[0], PpDirective::Ifndef(_)));
    assert!(matches!(d[1], PpDirective::EndIf));
}

// ---------- #ifaot / #ifjit (TempleOS dual-path conditionals) ----------

#[test]
fn ifaot_ifjit_recognized() {
    let d = directives("#ifaot\n#endif\n#ifjit\n#endif\n");
    assert!(matches!(d[0], PpDirective::IfAot));
    assert!(matches!(d[2], PpDirective::IfJit));
}

// ---------- unknown directives become PpDirective::Other ----------

#[test]
fn unknown_directive_caught_as_other() {
    let d = directives("#exe { Print(\"hi\"); }\n");
    assert_eq!(d.len(), 1);
    match &d[0] {
        PpDirective::Other { name, .. } => assert_eq!(name, "exe"),
        other => panic!("expected Other, got {:?}", other),
    }
}
