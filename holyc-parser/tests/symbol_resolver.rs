//! Symbol resolver — cross-file declaration / use checks.

use holyc_parser::diag::Severity;
use holyc_parser::parse::{parse_module, ParseConfig};
use holyc_parser::symbol::Resolver;

fn errors_for_files(pairs: &[(&str, &str)]) -> Vec<String> {
    let cfg = ParseConfig::default();
    let mut resolver = Resolver::new();
    let mut out = Vec::new();
    // Mirror the CLI: register-then-check per file in input order.
    // Each file only sees decls from itself + earlier files + builtins.
    for (file, src) in pairs {
        let (m, _diags) = parse_module(*file, src, cfg);
        let label = file.to_string();
        resolver.register_module(&label, &m);
        for d in resolver.check_module(&label, &m) {
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
fn function_pointer_field_in_class() {
    // `U0 (*function)();` is a fn-pointer field. The parser used to
    // consume `function` inside the type and then expect another
    // ident to follow — yielding a false-positive
    // "expecting-identifier". Now the inline name is plumbed through.
    let errors = errors_for_files(&[
        ("a.HC", r#"
class cmd_function_t {
  cmd_function_t *next;
  U8 *name;
  U0 (*function)();
};
U0 Use(cmd_function_t *c) {
  c->name = NULL;
}
"#),
    ]);
    assert!(errors.is_empty(), "got {errors:?}");
}

#[test]
fn function_pointer_parameter() {
    // `U0 (*fn)()` as a parameter — same fix path. The inline name
    // becomes the parameter name and is visible inside the body.
    let errors = errors_for_files(&[
        ("a.HC", r#"
U0 RegisterCallback(U8 *name, U0 (*fn)()) {
  fn();
  Print("%s\n", name);
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
fn forward_cross_file_use_flagged_as_unresolved() {
    // Push-order: file1 (alphabetically first) uses a symbol from
    // file2 (alphabetically second). The VM JIT-compiles file1
    // before file2 is loaded, so the use fails — the resolver must
    // mirror that order, not silently merge all decls first.
    let errors = errors_for_files(&[
        // 'C' < 'V' alphabetically — Cmd is "pushed" before Cvar.
        ("Cmd.HC",  "U0 RegCmd() { Cvar_VarStr(\"foo\"); }"),
        ("Cvar.HC", "U8 *Cvar_VarStr(U8 *name) { return name; }"),
    ]);
    assert!(
        errors.iter().any(|e| e.contains("`Cvar_VarStr`")),
        "expected unresolved Cvar_VarStr at the use site; got {errors:?}"
    );
}

#[test]
fn backward_cross_file_use_resolves_normally() {
    // Reverse the order: now the consumer comes second. Should
    // resolve cleanly because the dep was registered first.
    let errors = errors_for_files(&[
        ("Cvar.HC", "U8 *Cvar_VarStr(U8 *name) { return name; }"),
        ("Cmd.HC",  "U0 RegCmd() { Cvar_VarStr(\"foo\"); }"),
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
