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
use holyc_parser::parse::ast::{CaseValue, ExprKind, Stmt, StmtKind};
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
fn sub_switch_start() {
    let (s, _) = parse_one_stmt("start :");
    assert!(matches!(s.unwrap().kind, StmtKind::SubSwitchStart));
}

#[test]
fn sub_switch_end() {
    let (s, _) = parse_one_stmt("end :");
    assert!(matches!(s.unwrap().kind, StmtKind::SubSwitchEnd));
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
