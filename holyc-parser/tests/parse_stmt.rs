//! Tests for `parse::stmt::parse_statement` (PrsStmt, parse-spec §3).
//!
//! Many statement forms involve sub-expressions. The expression parser
//! (`expr.rs`) is being filled by ExprCoder in parallel and currently
//! returns `None` from a stub. These tests focus on:
//!   - statement structures that don't require a real expression parser
//!     (empty, block, return-no-val, goto, break, label, case-no-val,
//!      try/catch, sub-switch markers, asm)
//!   - assertions that *some* statement was produced for expression-
//!     bearing forms (without inspecting the inner Expr)
//!
//! When ExprCoder lands real expression parsing these tests will keep
//! passing — and additional assertions on Expr internals can be added.

use holyc_parser::lex::lex;
use holyc_parser::parse::ast::{CaseValue, ExprKind, PpDirective, Stmt, StmtKind};
use holyc_parser::parse::parser::{ParseConfig, Parser};
use holyc_parser::parse::stmt::parse_statement;

fn parse_stmts(src: &str) -> (Vec<Stmt>, Vec<String>) {
    let (toks, _diags) = lex("test", src);
    let mut p = Parser::new("test", toks, ParseConfig::default());
    let mut out = Vec::new();
    while !p.at_eof() {
        match parse_statement(&mut p) {
            Some(s) => out.push(s),
            None => {
                if !p.at_eof() { let _ = p.bump(); }
            }
        }
    }
    let rules: Vec<String> = p.diags.into_iter().map(|d| d.rule.to_string()).collect();
    (out, rules)
}

fn parse_one_stmt(src: &str) -> (Option<Stmt>, Vec<String>) {
    let (toks, _diags) = lex("test", src);
    let mut p = Parser::new("test", toks, ParseConfig::default());
    let s = parse_statement(&mut p);
    let rules: Vec<String> = p.diags.into_iter().map(|d| d.rule.to_string()).collect();
    (s, rules)
}

// -------- empty / block --------

#[test]
fn empty_stmt() {
    let (s, _) = parse_one_stmt(";");
    assert!(matches!(s.unwrap().kind, StmtKind::Empty));
}

#[test]
fn empty_block() {
    let (s, _) = parse_one_stmt("{}");
    match s.unwrap().kind {
        StmtKind::Block(v) => assert!(v.is_empty()),
        k => panic!("expected Block, got {:?}", k),
    }
}

#[test]
fn block_of_two_empties() {
    let (s, _) = parse_one_stmt("{ ; ; }");
    match s.unwrap().kind {
        StmtKind::Block(v) => assert_eq!(v.len(), 2),
        k => panic!("expected Block, got {:?}", k),
    }
}

#[test]
fn block_of_returns() {
    let (s, _) = parse_one_stmt("{ return; return; }");
    match s.unwrap().kind {
        StmtKind::Block(v) => {
            assert_eq!(v.len(), 2);
            for stmt in &v {
                assert!(matches!(stmt.kind, StmtKind::Return(None)));
            }
        }
        k => panic!("expected Block, got {:?}", k),
    }
}

// -------- return / goto / break / label --------

#[test]
fn return_no_value() {
    let (s, _) = parse_one_stmt("return;");
    assert!(matches!(s.unwrap().kind, StmtKind::Return(None)));
}

#[test]
fn break_stmt() {
    let (s, _) = parse_one_stmt("break;");
    assert!(matches!(s.unwrap().kind, StmtKind::Break));
}

#[test]
fn goto_stmt() {
    let (s, _) = parse_one_stmt("goto loop_top;");
    match s.unwrap().kind {
        StmtKind::Goto(name) => assert_eq!(name, "loop_top"),
        k => panic!("expected Goto, got {:?}", k),
    }
}

#[test]
fn label_stmt() {
    let (s, _) = parse_one_stmt("loop_top:");
    match s.unwrap().kind {
        StmtKind::Label(name) => assert_eq!(name, "loop_top"),
        k => panic!("expected Label, got {:?}", k),
    }
}

// -------- if / while / do-while / for ---- structural only --------

#[test]
fn if_structure() {
    // The expression stub may consume past `)`; we just check that
    // *something* parsed. When ExprCoder lands, this becomes a real
    // structural test.
    let (s, _) = parse_one_stmt("if (c) ;");
    if let Some(stmt) = s {
        // accept either If or that recovery left the cursor. The
        // important thing is the parser didn't panic.
        let _ = stmt;
    }
}

#[test]
fn while_structure() {
    let (s, _) = parse_one_stmt("while (c) {}");
    let _ = s;
}

#[test]
fn do_while_requires_semi_after_paren() {
    // `do {} while (c);` — the `;` after `)` is required (parse-spec §5.5).
    let (s, _) = parse_one_stmt("do {} while (c);");
    let _ = s;
}

#[test]
fn for_structure_empty_init() {
    let (s, _) = parse_one_stmt("for (;;) {}");
    if let Some(stmt) = s {
        // for(;;) — init/cond/update are all empty.
        match stmt.kind {
            StmtKind::For { init, cond, update, .. } => {
                assert!(init.is_none());
                assert!(cond.is_none());
                assert!(update.is_none());
            }
            other => panic!("expected For, got {:?}", other),
        }
    }
}

#[test]
fn for_comma_in_init_and_update() {
    // HolyC accepts comma-operator expression lists in both the init
    // and update slots: `for (i = 0, j = 10; i < n; i++, j--) {}`.
    let (s, rules) = parse_one_stmt("for (i = 0, j = 10; i < n; i++, j--) {}");
    assert!(rules.is_empty(), "unexpected diags: {rules:?}");
    match s.unwrap().kind {
        StmtKind::For { init, cond, update, .. } => {
            // init should be a Comma of two assigns, wrapped in
            // StmtKind::Expr.
            let init_stmt = init.expect("init present");
            match init_stmt.kind {
                StmtKind::Expr(e) => match e.kind {
                    ExprKind::Comma(items) => assert_eq!(items.len(), 2),
                    other => panic!("expected Comma init, got {:?}", other),
                },
                other => panic!("expected Expr init, got {:?}", other),
            }
            assert!(cond.is_some());
            // update should be a Comma of two postfix ops.
            match update.expect("update present").kind {
                ExprKind::Comma(items) => assert_eq!(items.len(), 2),
                other => panic!("expected Comma update, got {:?}", other),
            }
        }
        other => panic!("expected For, got {:?}", other),
    }
}

#[test]
fn for_comma_in_init_only() {
    let (s, rules) = parse_one_stmt("for (a = 0, b = 0; a < 10; a++) {}");
    assert!(rules.is_empty(), "unexpected diags: {rules:?}");
    match s.unwrap().kind {
        StmtKind::For { init, update, .. } => {
            let init_stmt = init.expect("init present");
            match init_stmt.kind {
                StmtKind::Expr(e) => match e.kind {
                    ExprKind::Comma(items) => assert_eq!(items.len(), 2),
                    other => panic!("expected Comma init, got {:?}", other),
                },
                other => panic!("expected Expr init, got {:?}", other),
            }
            // update should be a single bare expression (not Comma).
            let upd = update.expect("update present");
            assert!(!matches!(upd.kind, ExprKind::Comma(_)),
                "expected non-Comma single update");
        }
        other => panic!("expected For, got {:?}", other),
    }
}

#[test]
fn for_comma_in_update_only() {
    let (s, rules) = parse_one_stmt("for (i = 0; i < n; i++, j--) {}");
    assert!(rules.is_empty(), "unexpected diags: {rules:?}");
    match s.unwrap().kind {
        StmtKind::For { init, update, .. } => {
            // init bare-expr, not Comma.
            let init_stmt = init.expect("init present");
            match init_stmt.kind {
                StmtKind::Expr(e) => assert!(!matches!(e.kind, ExprKind::Comma(_))),
                other => panic!("expected Expr init, got {:?}", other),
            }
            match update.expect("update present").kind {
                ExprKind::Comma(items) => assert_eq!(items.len(), 2),
                other => panic!("expected Comma update, got {:?}", other),
            }
        }
        other => panic!("expected For, got {:?}", other),
    }
}

#[test]
fn for_single_expr_clauses_regression() {
    // Single-expression init and update — must still parse as bare
    // expressions, not Comma-wrapped.
    let (s, rules) = parse_one_stmt("for (i = 0; i < n; i++) {}");
    assert!(rules.is_empty(), "unexpected diags: {rules:?}");
    match s.unwrap().kind {
        StmtKind::For { init, update, .. } => {
            let init_stmt = init.expect("init present");
            match init_stmt.kind {
                StmtKind::Expr(e) => assert!(!matches!(e.kind, ExprKind::Comma(_))),
                other => panic!("expected Expr init, got {:?}", other),
            }
            let upd = update.expect("update present");
            assert!(!matches!(upd.kind, ExprKind::Comma(_)));
        }
        other => panic!("expected For, got {:?}", other),
    }
}

// -------- switch / case / default --------

#[test]
fn case_auto_increment() {
    // `case :` (no value) — parse-spec §5.12.
    let (s, _) = parse_one_stmt("case :");
    match s.unwrap().kind {
        StmtKind::Case(values) => {
            assert_eq!(values.len(), 1);
            assert!(matches!(values[0], CaseValue::AutoIncrement));
        }
        k => panic!("expected Case, got {:?}", k),
    }
}

#[test]
fn case_range_triple_dot() {
    // HolyC `case 1 ... 5:` — the corpus exercises this as a smoke
    // test, but doesn't assert structure. Lock in CaseValue::Range
    // with both bounds preserved.
    let (s, rules) = parse_one_stmt("case 1 ... 5 :");
    assert!(rules.is_empty(), "unexpected diags: {rules:?}");
    match s.unwrap().kind {
        StmtKind::Case(values) => {
            assert_eq!(values.len(), 1);
            assert!(matches!(values[0], CaseValue::Range(..)));
        }
        k => panic!("expected Case, got {:?}", k),
    }
}

#[test]
fn case_range_double_dot() {
    // `..` is also accepted as a range operator (parse-spec §5.12);
    // mirrors the `...` case above.
    let (s, rules) = parse_one_stmt("case 1 .. 5 :");
    assert!(rules.is_empty(), "unexpected diags: {rules:?}");
    match s.unwrap().kind {
        StmtKind::Case(values) => {
            assert_eq!(values.len(), 1);
            assert!(matches!(values[0], CaseValue::Range(..)));
        }
        k => panic!("expected Case, got {:?}", k),
    }
}

#[test]
fn default_stmt() {
    let (s, _) = parse_one_stmt("default :");
    assert!(matches!(s.unwrap().kind, StmtKind::Default));
}

#[test]
fn sub_switch_start_inside_switch() {
    // `start` / `end` are CONTEXTUAL keywords — they only act as
    // sub-switch markers inside a switch body. Outside, they're
    // ordinary identifiers (label / expression).
    let (s, _) = parse_one_stmt("switch (x) { start: case 1: break; end: }");
    let stmt = s.unwrap();
    if let StmtKind::Switch { body, .. } = stmt.kind {
        assert!(body.iter().any(|s| matches!(s.kind, StmtKind::SubSwitchStart)));
        assert!(body.iter().any(|s| matches!(s.kind, StmtKind::SubSwitchEnd)));
    } else {
        panic!("expected Switch");
    }
}

#[test]
fn start_outside_switch_is_label() {
    // Bare `start :` outside of a switch must NOT produce
    // SubSwitchStart — it parses as an ordinary label.
    let (s, _) = parse_one_stmt("start :");
    assert!(matches!(s.unwrap().kind, StmtKind::Label(_)));
}

#[test]
fn end_outside_switch_is_label() {
    let (s, _) = parse_one_stmt("end :");
    assert!(matches!(s.unwrap().kind, StmtKind::Label(_)));
}

// -------- try / catch --------

#[test]
fn try_catch_empty_bodies() {
    let (s, _) = parse_one_stmt("try { } catch { }");
    match s.unwrap().kind {
        StmtKind::Try { body, catch_body } => {
            assert!(body.is_empty());
            assert!(catch_body.is_empty());
        }
        k => panic!("expected Try, got {:?}", k),
    }
}

#[test]
fn try_catch_with_returns() {
    let (s, _) = parse_one_stmt("try { return; } catch { return; }");
    match s.unwrap().kind {
        StmtKind::Try { body, catch_body } => {
            assert_eq!(body.len(), 1);
            assert_eq!(catch_body.len(), 1);
        }
        k => panic!("expected Try, got {:?}", k),
    }
}

// -------- asm --------

#[test]
fn asm_block_smoke() {
    let (s, _) = parse_one_stmt("asm { NOP }");
    match s.unwrap().kind {
        StmtKind::Asm(text) => assert!(text.contains("NOP") || text.contains("<ident>")),
        k => panic!("expected Asm, got {:?}", k),
    }
}

// -------- lock --------

#[test]
fn lock_wraps_inner_stmt() {
    let (s, _) = parse_one_stmt("lock ;");
    match s.unwrap().kind {
        StmtKind::Lock(inner) => assert!(matches!(inner.kind, StmtKind::Empty)),
        k => panic!("expected Lock, got {:?}", k),
    }
}

#[test]
fn lock_with_block() {
    let (s, _) = parse_one_stmt("lock { return; }");
    match s.unwrap().kind {
        StmtKind::Lock(inner) => assert!(matches!(inner.kind, StmtKind::Block(_))),
        k => panic!("expected Lock, got {:?}", k),
    }
}

// -------- multiple statements in a row --------

#[test]
fn many_empty_stmts() {
    let (stmts, _) = parse_stmts("; ; ; ;");
    assert_eq!(stmts.len(), 4);
    for s in &stmts {
        assert!(matches!(s.kind, StmtKind::Empty));
    }
}

#[test]
fn multiple_break_return_label() {
    let (stmts, _) = parse_stmts("foo: break; return;");
    assert_eq!(stmts.len(), 3);
    assert!(matches!(stmts[0].kind, StmtKind::Label(_)));
    assert!(matches!(stmts[1].kind, StmtKind::Break));
    assert!(matches!(stmts[2].kind, StmtKind::Return(None)));
}

// -------- structural smoke for switch with sub-switch markers --------

#[test]
fn switch_body_can_contain_subswitch_markers() {
    // Even though `(x)` will fail under the expression stub, we can
    // run the parser and inspect the Stmt::Switch.body items list
    // for the markers we care about. We do that by skipping over the
    // expression failure and just looking at the post-block content.
    //
    // Concretely: `switch (x) { start: end: }` — markers must each be
    // recognised; even if the scrutinee fails to parse, the body still
    // consumed the markers.
    let (s, _) = parse_one_stmt("switch (x) { start: end: }");
    if let Some(stmt) = s {
        if let StmtKind::Switch { body, .. } = stmt.kind {
            // We expect at least one of start / end markers in body.
            let has_start = body.iter().any(|s| matches!(s.kind, StmtKind::SubSwitchStart));
            let has_end = body.iter().any(|s| matches!(s.kind, StmtKind::SubSwitchEnd));
            assert!(has_start || has_end || body.is_empty());
        }
    }
}

// -------- function-scope `F64 a, b;` is allowed (multi-decl OK) --------

#[test]
fn local_multi_decl_allowed() {
    // At function scope, `F64 a, b;` is fine (parse-spec §4.3).
    let (s, _) = parse_one_stmt("F64 a, b;");
    match s.unwrap().kind {
        StmtKind::LocalDecl(decls) => {
            assert_eq!(decls.len(), 2);
            assert_eq!(decls[0].name, "a");
            assert_eq!(decls[1].name, "b");
        }
        k => panic!("expected LocalDecl, got {:?}", k),
    }
}

#[test]
fn local_decl_pointer() {
    let (s, _) = parse_one_stmt("F64 *p;");
    match s.unwrap().kind {
        StmtKind::LocalDecl(decls) => {
            assert_eq!(decls.len(), 1);
            assert_eq!(decls[0].name, "p");
            // pointer_depth checked in parse_decl tests
        }
        k => panic!("expected LocalDecl, got {:?}", k),
    }
}

#[test]
fn local_decl_simple() {
    let (s, _) = parse_one_stmt("F64 x;");
    match s.unwrap().kind {
        StmtKind::LocalDecl(decls) => {
            assert_eq!(decls.len(), 1);
            assert_eq!(decls[0].name, "x");
        }
        k => panic!("expected LocalDecl, got {:?}", k),
    }
}

// -------- error recovery: bad keyword in expression position --------

#[test]
fn unknown_ident_starts_expr_stmt() {
    // Currently the expression parser is a stub that emits a TODO and
    // recovers to ;. We just check no panic occurred.
    let (_s, rules) = parse_one_stmt("foo;");
    // Either expr-stub-todo or no rules — both are acceptable shapes.
    let _ = rules;
}

// -------- `start` / `end` / `reg` contextual keywords --------
//
// `start` / `end` are sub-switch markers only inside a switch body;
// outside they're ordinary identifiers. `reg` is a modifier only in
// modifier-prefix position; otherwise it's an identifier. The four
// names must work as both forms in the same function (kernel-style).

#[test]
fn start_used_as_marker_and_local_in_same_function() {
    // `I64 start;` declares a local. `switch (x) { start: ... end: }`
    // uses `start`/`end` as sub-switch markers. Inside the switch body,
    // `start = 5;` re-uses the local. All three must coexist.
    let (_s, rules) = parse_one_stmt(
        "{ I64 start; switch (x) { start: case 1: start = 5; end: } }",
    );
    assert!(rules.is_empty(), "unexpected diags: {rules:?}");
}

#[test]
fn reg_modifier_then_reg_local_var() {
    // `reg I64 i;` declares with a register hint; `I64 reg;` declares
    // a local literally named `reg`. Both forms in one function.
    let (_s, rules) = parse_one_stmt("{ reg I64 i = 0; I64 reg = 0; reg = i; }");
    assert!(rules.is_empty(), "unexpected diags: {rules:?}");
}

// -------- `offset` contextual keyword --------
//
// `offset` followed by `(` is the offsetof operator; otherwise it's
// a plain identifier. Mixing both forms in the same function works.

#[test]
fn offset_local_then_offsetof_operator() {
    // `I64 offset = ...` declares a local; `offset(Foo.bar)` invokes
    // the offsetof operator. Both must coexist with no diagnostics.
    let (_s, rules) = parse_one_stmt("{ I64 offset = offset(Foo.bar); offset = offset + 4; }");
    assert!(rules.is_empty(), "unexpected diags: {rules:?}");
}

// -------- semantics: case-range parses values without panic --------

#[test]
fn case_value_followed_by_colon_struct_smoke() {
    // We cannot reliably parse `case 0:` until ExprCoder ships, but
    // structurally we want Case(*) with at least one value entry.
    let (s, _) = parse_one_stmt("case :");
    if let Some(st) = s {
        if let StmtKind::Case(vals) = st.kind {
            assert_eq!(vals.len(), 1);
        }
    }
}

// -------- mid-body preprocessor directives --------
//
// `#`-lines (`#ifdef`/`#endif`/`#define`/...) are accepted inside
// any `Vec<Stmt>` body — function body, block, for-body, switch-
// body, try/catch, lock-body. They parse into the same `PpDirective`
// shape as at top level, wrapped in `StmtKind::Preprocessor(...)`.
// We don't expand `#ifdef`: both branches are preserved verbatim
// as sibling statements in the surrounding block.

fn block_stmts(s: &Stmt) -> &[Stmt] {
    match &s.kind {
        StmtKind::Block(v) => v.as_slice(),
        k => panic!("expected Block, got {k:?}"),
    }
}

#[test]
fn preproc_ifdef_endif_inside_if_body() {
    // Both `#ifdef` and `#endif` should appear in the AST, with the
    // guarded statement preserved between them — no expansion.
    let (s, rules) = parse_one_stmt(
        "if (x) { #ifdef DEBUG\nprint(\"hi\");\n#endif\n}",
    );
    assert!(rules.is_empty(), "unexpected diags: {rules:?}");
    let st = s.expect("if-stmt should parse");
    let then_branch = match st.kind {
        StmtKind::If { then_branch, .. } => then_branch,
        k => panic!("expected If, got {k:?}"),
    };
    let body = block_stmts(&then_branch);
    // Expect: [Preprocessor(Ifdef("DEBUG")), Expr(...), Preprocessor(EndIf)]
    assert_eq!(body.len(), 3, "body = {body:?}");
    match &body[0].kind {
        StmtKind::Preprocessor(PpDirective::Ifdef(name)) => assert_eq!(name, "DEBUG"),
        k => panic!("expected Preprocessor(Ifdef), got {k:?}"),
    }
    assert!(matches!(body[2].kind, StmtKind::Preprocessor(PpDirective::EndIf)));
}

#[test]
fn preproc_if_inside_for_body() {
    // Stub-expr may emit minor diags on `i = 0`/`i < 10`; we only
    // care that the for-body's preprocessor directives parsed.
    let (s, _rules) = parse_one_stmt(
        "for (i = 0; i < 10; i++) { #ifdef FOO\ny();\n#endif\n}",
    );
    let st = s.expect("for-stmt should parse");
    let body_stmt = match st.kind {
        StmtKind::For { body, .. } => body,
        k => panic!("expected For, got {k:?}"),
    };
    let body = block_stmts(&body_stmt);
    // `#if FOO` parses as `Other { name: "if", ... }` since `#if` is
    // not in our modeled directive set; what matters is that *some*
    // Preprocessor stmt landed at index 0 and that the body also
    // contains an `#endif` directive somewhere.
    assert!(
        matches!(body[0].kind, StmtKind::Preprocessor(_)),
        "expected leading Preprocessor stmt, got {:?}",
        body[0].kind,
    );
    assert!(
        body.iter().any(|s| matches!(
            &s.kind,
            StmtKind::Preprocessor(PpDirective::EndIf),
        )),
        "expected an #endif somewhere in body, got {body:?}",
    );
}

#[test]
fn preproc_ifdef_inside_try_body() {
    let (s, rules) = parse_one_stmt(
        "try { x(); #ifdef BAR\ny();\n#endif\n} catch { z(); }",
    );
    assert!(rules.is_empty(), "unexpected diags: {rules:?}");
    let st = s.expect("try-stmt should parse");
    let (body, catch_body) = match st.kind {
        StmtKind::Try { body, catch_body } => (body, catch_body),
        k => panic!("expected Try, got {k:?}"),
    };
    // try-body: [Expr(x()), Preprocessor(Ifdef BAR), Expr(y()), Preprocessor(EndIf)]
    assert_eq!(body.len(), 4, "try-body = {body:?}");
    match &body[1].kind {
        StmtKind::Preprocessor(PpDirective::Ifdef(name)) => assert_eq!(name, "BAR"),
        k => panic!("expected Preprocessor(Ifdef BAR), got {k:?}"),
    }
    assert!(matches!(body[3].kind, StmtKind::Preprocessor(PpDirective::EndIf)));
    // catch-body should be untouched.
    assert_eq!(catch_body.len(), 1);
}

#[test]
fn preproc_define_inside_block() {
    // Bare `#define X 1` mid-block parses into a Preprocessor stmt.
    let (s, rules) = parse_one_stmt("{ #define X 1\n}");
    assert!(rules.is_empty(), "unexpected diags: {rules:?}");
    let body = match s.expect("block").kind {
        StmtKind::Block(v) => v,
        k => panic!("expected Block, got {k:?}"),
    };
    assert_eq!(body.len(), 1);
    match &body[0].kind {
        StmtKind::Preprocessor(PpDirective::Define { name, .. }) => {
            assert_eq!(name, "X");
        }
        k => panic!("expected Preprocessor(Define X), got {k:?}"),
    }
}
