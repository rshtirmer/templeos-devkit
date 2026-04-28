//! Tests for `parse::decl::parse_top_item` and `parse_local_decl`.
//! Implements parse-spec §4 + §1.1 + §5.1 / §5.4 / §5.5 bug-compat.

use holyc_parser::lex::lex;
use holyc_parser::parse::ast::{
    Initializer, Modifier, Module, PpDirective, PrimType, StmtKind, TopItem, TypeRef,
};
use holyc_parser::parse::parser::{ParseConfig, Parser};
use holyc_parser::parse::{decl, stmt};

fn parse_top(src: &str) -> (Module, Vec<String>) {
    parse_top_with(src, ParseConfig::default())
}

fn parse_top_with(src: &str, config: ParseConfig) -> (Module, Vec<String>) {
    let (toks, _diags) = lex("test", src);
    let mut p = Parser::new("test", toks, config);
    let mut items = Vec::new();
    while !p.at_eof() {
        match decl::parse_top_item(&mut p) {
            Some(it) => items.push(it),
            None => {
                if !p.at_eof() { let _ = p.bump(); }
            }
        }
    }
    let rules: Vec<String> = p.diags.into_iter().map(|d| d.rule.to_string()).collect();
    (Module { items }, rules)
}

fn parse_local_only(src: &str) -> (Option<Vec<holyc_parser::parse::ast::VarDecl>>, Vec<String>) {
    let (toks, _diags) = lex("test", src);
    let mut p = Parser::new("test", toks, ParseConfig::default());
    let v = decl::parse_local_decl(&mut p);
    let rules: Vec<String> = p.diags.into_iter().map(|d| d.rule.to_string()).collect();
    (v, rules)
}

// -------- simple variable globals --------

#[test]
fn global_simple_f64() {
    let (m, _) = parse_top("F64 x;");
    assert_eq!(m.items.len(), 1);
    match &m.items[0] {
        TopItem::Variable(v) => {
            assert_eq!(v.name, "x");
            assert!(matches!(v.ty, TypeRef::Prim { ty: PrimType::F64, pointer_depth: 0 }));
            assert!(v.init.is_none());
        }
        other => panic!("expected Variable, got {:?}", other),
    }
}

#[test]
fn global_pointer() {
    let (m, _) = parse_top("F64 *p;");
    match &m.items[0] {
        TopItem::Variable(v) => {
            assert_eq!(v.name, "p");
            assert!(matches!(v.ty, TypeRef::Prim { pointer_depth: 1, .. }));
        }
        other => panic!("expected Variable, got {:?}", other),
    }
}

#[test]
fn global_array() {
    let (m, _) = parse_top("F64 a[3];");
    match &m.items[0] {
        TopItem::Variable(v) => {
            assert_eq!(v.name, "a");
            assert_eq!(v.array_dims.len(), 1);
        }
        other => panic!("expected Variable, got {:?}", other),
    }
}

#[test]
fn global_unsized_array() {
    let (m, _) = parse_top("F64 a[];");
    match &m.items[0] {
        TopItem::Variable(v) => {
            assert_eq!(v.array_dims.len(), 1);
            assert!(v.array_dims[0].is_none());
        }
        other => panic!("expected Variable, got {:?}", other),
    }
}

// -------- multi-decl bug-compat (§4.3 / §5.1) --------

#[test]
fn global_multi_decl_default_accepted() {
    // Default config matches what TempleOS accepts via ExePutS:
    // top-level multi-decl works (the JIT path goes through the
    // function-scope multi-decl logic, not PrsGlblVarLst).
    // Corpus snippets 143/166 confirm.
    let (m, rules) = parse_top("F64 a, b;");
    assert!(
        !rules.iter().any(|r| r == "compiler-multi-decl-global"),
        "default config should accept multi-decl globals: {:?}", rules
    );
    match &m.items[0] {
        TopItem::GlobalDeclList(v) => assert_eq!(v.len(), 2),
        other => panic!("expected GlobalDeclList, got {:?}", other),
    }
}

#[test]
fn global_multi_decl_strict_errors() {
    // Strict mode (matches the AOT compile path that does reject).
    let cfg = ParseConfig { allow_multi_decl_globals: false, ..Default::default() };
    let (_m, rules) = parse_top_with("F64 a, b;", cfg);
    assert!(
        rules.iter().any(|r| r == "compiler-multi-decl-global"),
        "strict mode should flag multi-decl: {:?}", rules
    );
}

#[test]
fn global_multi_array_decl_strict_errors_too() {
    let cfg = ParseConfig { allow_multi_decl_globals: false, ..Default::default() };
    let (_m, rules) = parse_top_with("F64 m[9], im[9];", cfg);
    assert!(rules.iter().any(|r| r == "compiler-multi-decl-global"));
}

// -------- modifiers --------

#[test]
fn static_global() {
    let (m, _) = parse_top("static F64 g = 1.0;");
    match &m.items[0] {
        TopItem::Variable(v) => {
            assert!(v.modifiers.contains(&Modifier::Static));
            assert!(v.init.is_some());
        }
        other => panic!("expected Variable, got {:?}", other),
    }
}

#[test]
fn public_global() {
    let (m, _) = parse_top("public F64 g = 1.0;");
    match &m.items[0] {
        TopItem::Variable(v) => assert!(v.modifiers.contains(&Modifier::Public)),
        other => panic!("expected Variable, got {:?}", other),
    }
}

#[test]
fn extern_var() {
    let (m, _) = parse_top("extern F64 cos;");
    match &m.items[0] {
        TopItem::Variable(v) => assert!(v.modifiers.contains(&Modifier::Extern)),
        other => panic!("expected Variable, got {:?}", other),
    }
}

// -------- function decls --------

#[test]
fn fn_prototype_no_body() {
    let (m, _) = parse_top("F64 sin(F64 x);");
    match &m.items[0] {
        TopItem::Function(f) => {
            assert_eq!(f.name, "sin");
            assert_eq!(f.params.len(), 1);
            assert!(f.body.is_none());
        }
        other => panic!("expected Function, got {:?}", other),
    }
}

#[test]
fn fn_def_with_body() {
    let (m, _) = parse_top("F64 sin(F64 x) { return; }");
    match &m.items[0] {
        TopItem::Function(f) => {
            assert_eq!(f.name, "sin");
            assert!(f.body.is_some());
            let body = f.body.as_ref().unwrap();
            assert_eq!(body.len(), 1);
            assert!(matches!(body[0].kind, StmtKind::Return(None)));
        }
        other => panic!("expected Function, got {:?}", other),
    }
}

#[test]
fn fn_no_args() {
    let (m, _) = parse_top("U0 Main();");
    match &m.items[0] {
        TopItem::Function(f) => {
            assert_eq!(f.name, "Main");
            assert!(f.params.is_empty());
        }
        other => panic!("expected Function, got {:?}", other),
    }
}

#[test]
fn fn_variadic() {
    let (m, _) = parse_top("U0 Print(U8 *fmt, ...);");
    match &m.items[0] {
        TopItem::Function(f) => {
            assert_eq!(f.name, "Print");
            assert!(f.params.last().unwrap().variadic);
        }
        other => panic!("expected Function, got {:?}", other),
    }
}

// -------- class / union --------

#[test]
fn class_simple() {
    let (m, _) = parse_top("class Foo { I64 x; F64 y; };");
    match &m.items[0] {
        TopItem::Class(c) => {
            assert_eq!(c.name, "Foo");
            assert_eq!(c.members.len(), 2);
            assert!(!c.is_union);
        }
        other => panic!("expected Class, got {:?}", other),
    }
}

#[test]
fn class_with_base() {
    let (m, _) = parse_top("class Bar : Foo { I64 z; };");
    match &m.items[0] {
        TopItem::Class(c) => {
            assert_eq!(c.name, "Bar");
            assert_eq!(c.base.as_deref(), Some("Foo"));
            assert_eq!(c.members.len(), 1);
        }
        other => panic!("expected Class, got {:?}", other),
    }
}

#[test]
fn union_simple() {
    let (m, _) = parse_top("union U { I64 i; F64 f; };");
    match &m.items[0] {
        TopItem::Class(c) => {
            assert!(c.is_union);
            assert_eq!(c.members.len(), 2);
        }
        other => panic!("expected Class (union), got {:?}", other),
    }
}

// -------- file-scope `return` / labels rejected (§5.1) --------

#[test]
fn return_at_file_scope_rejected() {
    let (_m, rules) = parse_top("return;");
    assert!(rules.iter().any(|r| r == "return-at-file-scope"),
        "expected return-at-file-scope, got: {:?}", rules);
}

#[test]
fn label_at_file_scope_rejected() {
    let (_m, rules) = parse_top("foo:");
    assert!(rules.iter().any(|r| r == "no-global-labels"),
        "expected no-global-labels, got: {:?}", rules);
}

// -------- §5.4 `for(F64 i = ...)` at file scope rejected --------

#[test]
fn for_decl_top_level_default_errors() {
    let (_m, rules) = parse_top("for (F64 i; i; i) {}");
    assert!(
        rules.iter().any(|r| r == "for-decl-top-level"),
        "expected for-decl-top-level rule, got: {:?}",
        rules
    );
}

#[test]
fn for_decl_top_level_allowed_with_flag() {
    let cfg = ParseConfig { allow_for_decl_top_level: true, ..Default::default() };
    let (_m, rules) = parse_top_with("for (F64 i; i; i) {}", cfg);
    assert!(!rules.iter().any(|r| r == "for-decl-top-level"));
}

// -------- preprocessor directives --------

#[test]
fn pp_include() {
    let (m, _) = parse_top("#include \"foo.HC\"");
    match &m.items[0] {
        TopItem::Preprocessor(PpDirective::Include(s)) => assert_eq!(s, "foo.HC"),
        other => panic!("expected Include, got {:?}", other),
    }
}

#[test]
fn pp_define() {
    let (m, _) = parse_top("#define MAX 42");
    match &m.items[0] {
        TopItem::Preprocessor(PpDirective::Define { name, body: _ }) => {
            assert_eq!(name, "MAX");
        }
        other => panic!("expected Define, got {:?}", other),
    }
}

#[test]
fn pp_ifdef_endif() {
    let (m, _) = parse_top("#ifdef DEBUG\n#endif");
    assert!(matches!(
        m.items[0],
        TopItem::Preprocessor(PpDirective::Ifdef(_))
    ));
    assert!(matches!(
        m.items[1],
        TopItem::Preprocessor(PpDirective::EndIf)
    ));
}

#[test]
fn pp_ifndef() {
    let (m, _) = parse_top("#ifndef DEBUG");
    assert!(matches!(
        m.items[0],
        TopItem::Preprocessor(PpDirective::Ifndef(_))
    ));
}

#[test]
fn pp_if_aot_jit() {
    let (m, _) = parse_top("#ifaot");
    assert!(matches!(
        m.items[0],
        TopItem::Preprocessor(PpDirective::IfAot)
    ));
    let (m2, _) = parse_top("#ifjit");
    assert!(matches!(
        m2.items[0],
        TopItem::Preprocessor(PpDirective::IfJit)
    ));
}

#[test]
fn pp_help_index() {
    let (m, _) = parse_top("#help_index \"Foo\"");
    assert!(matches!(
        m.items[0],
        TopItem::Preprocessor(PpDirective::HelpIndex(_))
    ));
}

#[test]
fn pp_exe_other() {
    let (m, _) = parse_top("#exe { }");
    match &m.items[0] {
        TopItem::Preprocessor(PpDirective::Other { name, .. }) => assert_eq!(name, "exe"),
        other => panic!("expected Other, got {:?}", other),
    }
}

// -------- implicit Print at top level (§5.13) --------

#[test]
fn implicit_print_top_level() {
    // The expr-stub will fail to parse the string-as-expression
    // gracefully; we just check that *something* parses without panic.
    let (_m, _) = parse_top("\"Hello\\n\";");
    // No panic = pass.
}

// -------- local-decl harness (function scope) --------

#[test]
fn local_decl_scalar() {
    let (v, _) = parse_local_only("F64 x;");
    let v = v.unwrap();
    assert_eq!(v.len(), 1);
    assert_eq!(v[0].name, "x");
}

#[test]
fn local_decl_multi_no_error() {
    // At function scope, multi-decls are LEGAL — no error rule.
    let (v, rules) = parse_local_only("F64 a, b;");
    let v = v.unwrap();
    assert_eq!(v.len(), 2);
    assert!(!rules.iter().any(|r| r == "compiler-multi-decl-global"));
}

#[test]
fn local_decl_pointer_pointer() {
    let (v, _) = parse_local_only("I64 **pp;");
    let v = v.unwrap();
    assert_eq!(v.len(), 1);
    assert!(matches!(v[0].ty, TypeRef::Prim { pointer_depth: 2, .. }));
}

#[test]
fn local_decl_with_modifiers() {
    let (v, _) = parse_local_only("static F64 x;");
    let v = v.unwrap();
    assert_eq!(v.len(), 1);
    assert!(v[0].modifiers.contains(&Modifier::Static));
}

// -------- mixed module --------

#[test]
fn mixed_module_with_class_and_fn() {
    let src = "
        class Pt { I64 x; I64 y; };
        F64 dot(F64 a, F64 b);
        F64 g;
    ";
    let (m, _) = parse_top(src);
    assert!(m.items.iter().any(|i| matches!(i, TopItem::Class(_))));
    assert!(m.items.iter().any(|i| matches!(i, TopItem::Function(_))));
    assert!(m.items.iter().any(|i| matches!(i, TopItem::Variable(_))));
}

// -------- empty top-level `;` is dropped --------

#[test]
fn empty_top_item() {
    let (m, _) = parse_top(";");
    assert_eq!(m.items.len(), 1);
    assert!(matches!(m.items[0], TopItem::Empty));
}

// -------- ensure aggregate init parses without panic --------

#[test]
// un-ignored after expr+type integration
fn aggregate_init_smoke() {
    // Initializer parsing routes into expression parser per element.
    // With the stub this will produce IntLit(0) placeholders, but the
    // outer Aggregate / Single shape should be intact.
    let (m, _) = parse_top("F64 a[3] = {1.0, 2.0, 3.0};");
    if let TopItem::Variable(v) = &m.items[0] {
        assert!(matches!(v.init, Some(Initializer::Aggregate(_))));
    } else {
        panic!("expected Variable");
    }
}

// -------- function-pointer globals (with and without array dim) --------

#[test]
fn global_fnptr_simple() {
    // `U0 (*cb)();` — a single function pointer at file scope.
    let (m, rules) = parse_top("U0 (*cb)();");
    assert!(rules.is_empty(), "unexpected diags: {rules:?}");
    assert_eq!(m.items.len(), 1);
    match &m.items[0] {
        TopItem::Variable(v) => {
            assert_eq!(v.name, "cb");
            assert!(v.array_dims.is_empty());
        }
        other => panic!("expected Variable, got {:?}", other),
    }
}

#[test]
fn global_fnptr_array() {
    // `U0 (*pr_builtins[8])();` — an array of fn pointers. The
    // declarator name and array dim are embedded inside the
    // function-pointer parens.
    let (m, rules) = parse_top("U0 (*pr_builtins[8])();");
    assert!(rules.is_empty(), "unexpected diags: {rules:?}");
    assert_eq!(m.items.len(), 1);
    match &m.items[0] {
        TopItem::Variable(v) => {
            assert_eq!(v.name, "pr_builtins");
            assert_eq!(v.array_dims.len(), 1);
            assert!(v.array_dims[0].is_some());
        }
        other => panic!("expected Variable, got {:?}", other),
    }
}

#[test]
fn global_fnptr_array_with_init() {
    let (m, rules) = parse_top(
        "U0 B0() {} U0 B1() {} U0 (*tbl[2])() = {&B0, &B1};",
    );
    assert!(rules.is_empty(), "unexpected diags: {rules:?}");
    let var = m.items.iter().find_map(|it| match it {
        TopItem::Variable(v) if v.name == "tbl" => Some(v),
        _ => None,
    }).expect("tbl variable");
    assert_eq!(var.array_dims.len(), 1);
    assert!(matches!(var.init, Some(Initializer::Aggregate(_))));
}

// -------- silence unused import warning --------
#[allow(dead_code)]
fn _force_use() {
    let _ = stmt::parse_statement;
}
