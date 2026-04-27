//! Symbol resolver — cross-file declaration / use checks.

use holyc_parser::diag::Severity;
use holyc_parser::parse::{parse_module, ParseConfig};
use holyc_parser::symbol::Resolver;

fn errors_for_files(pairs: &[(&str, &str)]) -> Vec<String> {
    let cfg = ParseConfig::default();
    let mut resolver = Resolver::new();
    let mut modules = Vec::new();
    for (file, src) in pairs {
        let (m, _diags) = parse_module(*file, src, cfg);
        modules.push((file.to_string(), m));
    }
    for (label, m) in &modules {
        resolver.register_module(label, m);
    }
    let mut out = Vec::new();
    for (label, m) in &modules {
        for d in resolver.check_module(label, m) {
            if matches!(d.severity, Severity::Error) {
                out.push(format!("{}:{}:{}: {} [{}]", d.file, d.line, d.col, d.message, d.rule));
            }
        }
    }
    out
}

#[test]
fn unresolved_identifier_caught() {
    let errors = errors_for_files(&[
        ("a.HC", "U0 Foo() { Bar(1); }"),
    ]);
    assert!(
        errors.iter().any(|e| e.contains("`Bar`")),
        "expected unresolved Bar; got {errors:?}"
    );
}

#[test]
fn cross_file_resolution_succeeds() {
    let errors = errors_for_files(&[
        ("bootstrap.HC", "U0 Bar(I64 x) { Print(\"%d\\n\", x); }"),
        ("user.HC",      "U0 Foo() { Bar(1); }"),
    ]);
    assert!(
        errors.is_empty(),
        "expected no errors when defs span files; got {errors:?}"
    );
}

#[test]
fn templeos_builtins_dont_flag() {
    let errors = errors_for_files(&[
        ("a.HC", "U0 Demo() { CommPrint(1, \"hi\\n\"); MAlloc(64); StrLen(\"x\"); }"),
    ]);
    assert!(errors.is_empty(), "builtins must not be flagged: {errors:?}");
}

#[test]
fn local_var_visible_in_body() {
    let errors = errors_for_files(&[
        ("a.HC", "U0 Foo() { I64 x = 1; Print(\"%d\\n\", x); }"),
    ]);
    assert!(errors.is_empty(), "got {errors:?}");
}

#[test]
fn function_param_visible_in_body() {
    let errors = errors_for_files(&[
        ("a.HC", "U0 Foo(I64 y) { Print(\"%d\\n\", y); }"),
    ]);
    assert!(errors.is_empty(), "got {errors:?}");
}

#[test]
fn class_typed_local_var_recognised_as_decl() {
    // `cvar_t *v = ...` — the parser must see this as a local decl
    // (not an expression-stmt) so `v` is registered before its use.
    let errors = errors_for_files(&[
        ("a.HC", r#"
class cvar_t {
  I64 x;
};
U0 Foo() {
  cvar_t *v;
  v = NULL;
}
"#),
    ]);
    assert!(errors.is_empty(), "got {errors:?}");
}

#[test]
fn class_typed_global_recognised_as_decl() {
    let errors = errors_for_files(&[
        ("a.HC", r#"
class cvar_t {
  I64 x;
};
cvar_t my_var;
U0 Foo() {
  my_var.x = 1;
}
"#),
    ]);
    assert!(errors.is_empty(), "got {errors:?}");
}

#[test]
fn class_typed_function_return_recognised() {
    // `cvar_t *Find(...)` — class-pointer return type. Without the
    // looks_like_named_type_decl heuristic, this would fall through
    // to expression-stmt and `Find` wouldn't get registered.
    let errors = errors_for_files(&[
        ("a.HC", r#"
class cvar_t {
  I64 x;
};
cvar_t *Find(U8 *name) {
  return NULL;
}
U0 Use() {
  cvar_t *r = Find("foo");
}
"#),
    ]);
    assert!(errors.is_empty(), "got {errors:?}");
}

#[test]
fn define_macro_name_visible() {
    // `#define X 0xFFFF` — the name X must be treated as known
    // wherever it's referenced.
    let errors = errors_for_files(&[
        ("a.HC", r#"
#define MAX_VAL 0xFFFF
U0 Foo() {
  I64 x = MAX_VAL;
  Print("%d\n", x);
}
"#),
    ]);
    assert!(errors.is_empty(), "got {errors:?}");
}

#[test]
fn define_body_stops_at_newline() {
    // Regression: a `#define` body used to slurp following decls
    // until the next `#`/`;` because newlines weren't a stop. Now
    // the body terminates at end-of-line, so `crctable` below is
    // its own VarDecl item.
    let src = r#"#define X 0xFFFF
U16 crctable[3] = { 1, 2, 3 };
U0 Use() {
  Print("%d\n", crctable[0]);
}
"#;
    let errors = errors_for_files(&[("a.HC", src)]);
    assert!(errors.is_empty(), "crctable should be visible; got {errors:?}");
}
