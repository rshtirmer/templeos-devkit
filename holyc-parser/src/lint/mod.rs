//! AST-level lint rules. Runs after parsing and (optionally) name
//! resolution. Each rule walks the AST and emits diagnostics.
//!
//! Rules in this module are heuristic — they rely on syntactic
//! patterns rather than full type information. If a check needs
//! type information it belongs in a future `types` pass.
//!
//! Currently shipped:
//!   - `switch-case-shared-scope`  (warning)
//!   - `f64-bitwise`               (warning)

use crate::diag::{Diag, Severity};
use crate::parse::ast::{
    BinOp, Expr, ExprKind, Module, Stmt, StmtKind, TopItem, VarDecl,
};

/// Lint a parsed module. Returns all rule diagnostics.
pub fn lint_module(file: &str, m: &Module) -> Vec<Diag> {
    let mut out = Vec::new();
    for item in &m.items {
        lint_top_item(file, item, &mut out);
    }
    out
}

fn lint_top_item(file: &str, item: &TopItem, out: &mut Vec<Diag>) {
    match item {
        TopItem::Function(f) => {
            if let Some(body) = &f.body {
                for s in body {
                    lint_stmt(file, s, out);
                }
            }
        }
        TopItem::Variable(v) => lint_var_decl_init(file, v, out),
        TopItem::GlobalDeclList(vs) => {
            for v in vs {
                lint_var_decl_init(file, v, out);
            }
        }
        TopItem::Stmt(s) => lint_stmt(file, s, out),
        TopItem::Class(_) | TopItem::Preprocessor(_) | TopItem::Asm(_) | TopItem::Empty => {}
    }
}

fn lint_var_decl_init(file: &str, v: &VarDecl, out: &mut Vec<Diag>) {
    if let Some(init) = &v.init {
        lint_initializer(file, init, out);
    }
}

fn lint_initializer(file: &str, init: &crate::parse::ast::Initializer, out: &mut Vec<Diag>) {
    use crate::parse::ast::Initializer;
    match init {
        Initializer::Single(e) => lint_expr(file, e, out),
        Initializer::Aggregate(items) => {
            for i in items {
                lint_initializer(file, i, out);
            }
        }
    }
}

fn lint_stmt(file: &str, s: &Stmt, out: &mut Vec<Diag>) {
    match &s.kind {
        StmtKind::Empty | StmtKind::Break | StmtKind::Default
        | StmtKind::SubSwitchStart | StmtKind::SubSwitchEnd
        | StmtKind::Goto(_) | StmtKind::Label(_) | StmtKind::Asm(_)
        | StmtKind::NoWarn(_) => {}
        StmtKind::Block(body) => {
            for s2 in body {
                lint_stmt(file, s2, out);
            }
        }
        StmtKind::Expr(e) => lint_expr(file, e, out),
        StmtKind::If { cond, then_branch, else_branch } => {
            lint_expr(file, cond, out);
            lint_stmt(file, then_branch, out);
            if let Some(eb) = else_branch {
                lint_stmt(file, eb, out);
            }
        }
        StmtKind::While { cond, body } | StmtKind::DoWhile { cond, body } => {
            lint_expr(file, cond, out);
            lint_stmt(file, body, out);
        }
        StmtKind::For { init, cond, update, body } => {
            if let Some(i) = init { lint_stmt(file, i, out); }
            if let Some(c) = cond { lint_expr(file, c, out); }
            if let Some(u) = update { lint_expr(file, u, out); }
            lint_stmt(file, body, out);
        }
        StmtKind::Switch { scrutinee, body, .. } => {
            lint_expr(file, scrutinee, out);
            // Per-rule: walk the switch body for patterns that
            // depend on switch context.
            check_switch_case_shared_scope(file, body, out);
            for s2 in body {
                lint_stmt(file, s2, out);
            }
        }
        StmtKind::Return(opt) => {
            if let Some(e) = opt { lint_expr(file, e, out); }
        }
        StmtKind::Try { body, catch_body } => {
            for s2 in body { lint_stmt(file, s2, out); }
            for s2 in catch_body { lint_stmt(file, s2, out); }
        }
        StmtKind::Lock(inner) => lint_stmt(file, inner, out),
        StmtKind::LocalDecl(vs) => {
            for v in vs {
                lint_var_decl_init(file, v, out);
            }
        }
        StmtKind::Case(values) => {
            use crate::parse::ast::CaseValue;
            for v in values {
                match v {
                    CaseValue::Single(e) => lint_expr(file, e, out),
                    CaseValue::Range(a, b) => {
                        lint_expr(file, a, out);
                        lint_expr(file, b, out);
                    }
                    CaseValue::AutoIncrement => {}
                }
            }
        }
    }
}

fn lint_expr(file: &str, e: &Expr, out: &mut Vec<Diag>) {
    match &e.kind {
        ExprKind::IntLit(_) | ExprKind::FloatLit(_) | ExprKind::CharLit(_)
        | ExprKind::StrLit(_) | ExprKind::Ident(_) | ExprKind::DolDol => {}
        ExprKind::Prefix(_, x) | ExprKind::Postfix(_, x) | ExprKind::Paren(x)
        | ExprKind::HolycCast(x, _) => lint_expr(file, x, out),
        ExprKind::Binary(op, l, r) => {
            // ---- Rule: f64-bitwise (warning, heuristic) ----
            //
            // `&` / `|` / `^` between two operands that aren't
            // syntactically integer-shaped. In HolyC, bitwise ops on
            // F64 act on the IEEE-754 bit pattern — almost always a
            // porting bug from C. The fix is to truncate to I64 first
            // (assignment to I64 local, postfix `(I64)` cast, or
            // explicit constant).
            //
            // Heuristic: this rule fires when neither operand looks
            // integer-shaped. With no type information from the
            // parser, bare `Ident`s and direct function calls always
            // look "non-integer-shaped" — even when the variable was
            // declared `I64 x;` or the function returns I64. Those
            // are FALSE POSITIVES; the user should read the warning
            // and silence by adding an explicit `(I64)` cast on at
            // least one side, or wait for the type pass to land.
            //
            // We accept the noise because the cost of MISSING a
            // genuine F64-bitwise bug (silent miscomputation) is
            // higher than the cost of a one-line cast added to a
            // legitimate integer expression.
            if matches!(op, BinOp::BitAnd | BinOp::BitOr | BinOp::BitXor) {
                // Warn only when NEITHER side is integer-shaped —
                // `x & 0xFF` (one literal) or `(I64)x & y` (one cast)
                // is the common idiom and shouldn't fire.
                if !looks_integer_shaped(l) && !looks_integer_shaped(r) {
                    out.push(Diag {
                        file: file.to_string(),
                        line: e.span.0.line,
                        col: e.span.0.col,
                        severity: Severity::Warning,
                        rule: "f64-bitwise",
                        message: format!(
                            "bitwise `{}` between non-integer-shaped operands; \
                             HolyC's bitwise ops act on IEEE-754 bit patterns \
                             when operands are F64. Cast each side to I64 if \
                             you mean integer truncation",
                            op_name(*op)
                        ),
                    });
                }
            }
            lint_expr(file, l, out);
            lint_expr(file, r, out);
        }
        ExprKind::Index(a, b) => {
            lint_expr(file, a, out);
            lint_expr(file, b, out);
        }
        ExprKind::Member(x, _) | ExprKind::Arrow(x, _) => lint_expr(file, x, out),
        ExprKind::Call(callee, args) => {
            lint_expr(file, callee, out);
            for a in args { lint_expr(file, a, out); }
        }
        ExprKind::Sizeof(_) | ExprKind::OffsetOf(_) | ExprKind::Defined(_) => {}
        ExprKind::Comma(items) => {
            for x in items { lint_expr(file, x, out); }
        }
    }
}

fn op_name(op: BinOp) -> &'static str {
    match op {
        BinOp::BitAnd => "&",
        BinOp::BitOr => "|",
        BinOp::BitXor => "^",
        _ => "?",
    }
}

/// Heuristic: an expression "looks integer-shaped" if it's a
/// literal, an int-cast, a bitwise / shift result, a `~` unary, a
/// signed integer literal (`-256`), or any of the above wrapped in
/// parens. We can't see static types from the AST, so calls to
/// integer-returning functions still false-positive — those need
/// the type pass.
fn looks_integer_shaped(e: &Expr) -> bool {
    use crate::parse::ast::{PrefixOp, PrimType, TypeRef};
    match &e.kind {
        ExprKind::IntLit(_) | ExprKind::CharLit(_) => true,
        ExprKind::HolycCast(_, ty) => matches!(
            ty,
            TypeRef::Prim {
                ty: PrimType::U0
                | PrimType::I0
                | PrimType::U8
                | PrimType::I8
                | PrimType::Bool
                | PrimType::U16
                | PrimType::I16
                | PrimType::U32
                | PrimType::I32
                | PrimType::U64
                | PrimType::I64,
                ..
            }
        ),
        ExprKind::Prefix(PrefixOp::BitNot, _) => true,
        // `-256` and `+42` — signed integer literals.
        ExprKind::Prefix(PrefixOp::Minus | PrefixOp::Plus, inner) => {
            matches!(inner.kind, ExprKind::IntLit(_) | ExprKind::CharLit(_))
        }
        // Bitwise and shift results are always integer.
        ExprKind::Binary(
            BinOp::BitAnd | BinOp::BitOr | BinOp::BitXor | BinOp::Shl | BinOp::Shr,
            _,
            _,
        ) => true,
        ExprKind::Paren(inner) => looks_integer_shaped(inner),
        _ => false,
    }
}

// ============================================================
// Rule: switch-case-shared-scope
// ============================================================
//
// HolyC's `switch { case A: { ... } case B: { ... } }` shares the
// outer switch scope across case arms — declaring a local of the
// same name in two arms is rejected by PrsType as "Duplicate
// member". The fix is to hoist the declaration above the switch
// or rename per case. This rule walks a switch body, tracks
// LocalDecl names per case arm, and warns on collisions.

fn check_switch_case_shared_scope(file: &str, body: &[Stmt], out: &mut Vec<Diag>) {
    // The body is a flat sequence — Case markers split the arms.
    // Within each arm we collect VarDecl names. After the arm ends
    // (next Case / Default / SubSwitchEnd / end of body), compare
    // against the running global "ever-declared in this switch" set.
    use std::collections::HashMap;
    let mut seen: HashMap<String, (u32, u32)> = HashMap::new();
    let mut current_arm: Vec<(String, u32, u32)> = Vec::new();

    let flush = |arm: &mut Vec<(String, u32, u32)>,
                 seen: &mut HashMap<String, (u32, u32)>,
                 out: &mut Vec<Diag>| {
        for (name, line, col) in arm.drain(..) {
            if let Some(&(p_line, _p_col)) = seen.get(&name) {
                out.push(Diag {
                    file: file.to_string(),
                    line,
                    col,
                    severity: Severity::Warning,
                    rule: "switch-case-shared-scope",
                    message: format!(
                        "`{name}` is also declared in another case body of \
                         this switch (first at line {p_line}). HolyC's case \
                         arms share the surrounding switch scope, so PrsType \
                         will reject the second declaration as `Duplicate \
                         member`. Hoist the variable above the switch, or \
                         use distinct names per arm"
                    ),
                });
            } else {
                seen.insert(name, (line, col));
            }
        }
    };

    let mut iter = body.iter().peekable();
    while let Some(s) = iter.next() {
        match &s.kind {
            StmtKind::Case(_) | StmtKind::Default | StmtKind::SubSwitchStart | StmtKind::SubSwitchEnd => {
                let mut arm = current_arm.split_off(0);
                flush(&mut arm, &mut seen, out);
            }
            StmtKind::LocalDecl(vs) => {
                for v in vs {
                    current_arm.push((v.name.clone(), v.span.0.line, v.span.0.col));
                }
            }
            StmtKind::Block(inner) => {
                // Walk into nested blocks within the same case arm.
                collect_decls(inner, &mut current_arm);
            }
            _ => {}
        }
    }
    let mut arm = current_arm.split_off(0);
    flush(&mut arm, &mut seen, out);
}

fn collect_decls(body: &[Stmt], out: &mut Vec<(String, u32, u32)>) {
    for s in body {
        match &s.kind {
            StmtKind::LocalDecl(vs) => {
                for v in vs {
                    out.push((v.name.clone(), v.span.0.line, v.span.0.col));
                }
            }
            StmtKind::Block(inner) => collect_decls(inner, out),
            _ => {}
        }
    }
}
