//! AST-level lint rules. Runs after parsing and (optionally) name
//! resolution. Each rule walks the AST and emits diagnostics.
//!
//! This module is type-aware: it builds a `TypeContext` from the
//! input modules (recording declared types for globals + tracking
//! locals during the walk) so heuristics can ask "is this expression
//! integer-typed?" without false-positiving on `I64`-typed locals.
//!
//! Rules currently shipped:
//!   - `switch-case-shared-scope`  (warning) — pure structural
//!   - `f64-bitwise`               (warning) — type-aware

use std::collections::HashMap;

use crate::diag::{Diag, Severity};
use crate::parse::ast::{
    BinOp, Expr, ExprKind, Initializer, Module, Param, PpDirective, PrefixOp, PrimType,
    Stmt, StmtKind, TopItem, TypeRef, VarDecl,
};

// ============================================================
// Public API
// ============================================================

/// Lint a single parsed module. Builds its own type context from
/// just this module's globals — for cross-file type lookups (calls
/// to functions defined in another module), use `lint_modules`.
pub fn lint_module(file: &str, m: &Module) -> Vec<Diag> {
    let pairs = vec![(file.to_string(), m.clone())];
    lint_modules(&pairs)
        .into_iter()
        .filter(|d| d.file == file)
        .collect()
}

/// Lint a group of modules together. Globals from every module are
/// visible to every other module's lint pass — matches how the rest
/// of `holycc lint` operates and removes false positives on
/// cross-file function calls.
pub fn lint_modules(modules: &[(String, Module)]) -> Vec<Diag> {
    let mut tctx = TypeContext::new();
    for (_, m) in modules {
        tctx.register_module(m);
    }
    let mut out = Vec::new();
    for (file, m) in modules {
        let mut walker = LintWalker::new(file, &tctx, &mut out);
        walker.walk_module(m);
    }
    out
}

// ============================================================
// TypeContext — global symbol → type map
// ============================================================

/// A lightweight type table covering the kinds of declarations the
/// lint pass needs to know about: function return types + signatures,
/// variable types, and `#define` bodies that look like an integer
/// literal. Class declarations are recorded by name (so we can spot
/// user-defined types) but with no further info.
pub struct TypeContext {
    /// global name → declared type (where known)
    globals: HashMap<String, TypeRef>,
    /// function name → callable signature (params + variadic flag).
    /// Lives alongside `globals`; the type entry stores the return
    /// type so a call-as-expression resolves correctly, while this
    /// table is consulted for arity checks.
    functions: HashMap<String, FnSignature>,
    /// declared user-defined classes / unions (name set)
    classes: std::collections::HashSet<String>,
}

#[derive(Clone, Debug)]
struct FnSignature {
    /// Number of fixed (non-variadic) parameters.
    num_params: usize,
    /// True if the parameter list ends in `...`.
    variadic: bool,
}

impl TypeContext {
    pub fn new() -> Self {
        Self {
            globals: HashMap::new(),
            functions: HashMap::new(),
            classes: std::collections::HashSet::new(),
        }
    }

    pub fn register_module(&mut self, m: &Module) {
        for item in &m.items {
            match item {
                TopItem::Function(f) => {
                    // Record the function's RETURN type as the type
                    // of its name (close enough for "what does
                    // calling this give me?").
                    self.globals.insert(f.name.clone(), f.ret_type.clone());
                    let mut variadic = false;
                    let mut num_params = 0;
                    for p in &f.params {
                        if p.variadic {
                            variadic = true;
                        } else {
                            num_params += 1;
                        }
                    }
                    self.functions.insert(
                        f.name.clone(),
                        FnSignature { num_params, variadic },
                    );
                }
                TopItem::Variable(v) => {
                    self.globals.insert(v.name.clone(), v.ty.clone());
                }
                TopItem::GlobalDeclList(vs) => {
                    for v in vs {
                        self.globals.insert(v.name.clone(), v.ty.clone());
                    }
                }
                TopItem::Class(c) => {
                    self.classes.insert(c.name.clone());
                }
                TopItem::Preprocessor(PpDirective::Define { name, body }) => {
                    if let Some(ty) = parse_define_body_type(body) {
                        self.globals.insert(name.clone(), ty);
                    }
                }
                TopItem::Stmt(_)
                | TopItem::Asm(_)
                | TopItem::Empty
                | TopItem::Preprocessor(_) => {}
            }
        }
    }

    fn type_of(&self, name: &str) -> Option<&TypeRef> {
        self.globals.get(name)
    }
}

/// Inspect a `#define` body string and return a type if it's a
/// single integer literal (decimal, hex, binary, optional sign).
/// Anything else returns None.
fn parse_define_body_type(body: &str) -> Option<TypeRef> {
    let s = body.trim();
    if s.is_empty() {
        return None;
    }
    let (negated, rest) = if let Some(stripped) = s.strip_prefix('-') {
        (true, stripped.trim_start())
    } else if let Some(stripped) = s.strip_prefix('+') {
        (false, stripped.trim_start())
    } else {
        (false, s)
    };
    let _ = negated;
    let is_int = if let Some(hex) = rest.strip_prefix("0x").or_else(|| rest.strip_prefix("0X")) {
        !hex.is_empty() && hex.chars().all(|c| c.is_ascii_hexdigit())
    } else if let Some(bin) = rest.strip_prefix("0b").or_else(|| rest.strip_prefix("0B")) {
        !bin.is_empty() && bin.chars().all(|c| c == '0' || c == '1')
    } else {
        !rest.is_empty() && rest.chars().all(|c| c.is_ascii_digit())
    };
    if is_int {
        Some(TypeRef::Prim { ty: PrimType::I64, pointer_depth: 0 })
    } else {
        None
    }
}

// ============================================================
// LintWalker — walks a single module with a local-scope stack
// ============================================================

struct LintWalker<'a> {
    file: &'a str,
    tctx: &'a TypeContext,
    out: &'a mut Vec<Diag>,
    /// Stack of nested scopes. Each scope holds local-name → type.
    /// The walker pushes on function entry, pops on function exit;
    /// locals declared mid-function are added to the top of stack.
    locals: Vec<HashMap<String, TypeRef>>,
}

impl<'a> LintWalker<'a> {
    fn new(file: &'a str, tctx: &'a TypeContext, out: &'a mut Vec<Diag>) -> Self {
        Self { file, tctx, out, locals: Vec::new() }
    }

    fn enter_scope(&mut self) {
        self.locals.push(HashMap::new());
    }

    fn leave_scope(&mut self) {
        self.locals.pop();
    }

    fn add_local(&mut self, name: &str, ty: &TypeRef) {
        if let Some(top) = self.locals.last_mut() {
            top.insert(name.to_string(), ty.clone());
        }
    }

    fn lookup_local(&self, name: &str) -> Option<&TypeRef> {
        for scope in self.locals.iter().rev() {
            if let Some(ty) = scope.get(name) {
                return Some(ty);
            }
        }
        None
    }

    fn type_of(&self, name: &str) -> Option<&TypeRef> {
        self.lookup_local(name).or_else(|| self.tctx.type_of(name))
    }

    fn walk_module(&mut self, m: &Module) {
        for item in &m.items {
            self.walk_top_item(item);
        }
    }

    fn walk_top_item(&mut self, item: &TopItem) {
        match item {
            TopItem::Function(f) => {
                self.enter_scope();
                for p in &f.params {
                    self.add_param(p);
                }
                if let Some(body) = &f.body {
                    for s in body {
                        self.walk_stmt(s);
                    }
                }
                self.leave_scope();
            }
            TopItem::Variable(v) => self.walk_var_decl_init(v),
            TopItem::GlobalDeclList(vs) => {
                for v in vs {
                    self.walk_var_decl_init(v);
                }
            }
            TopItem::Stmt(s) => {
                // Top-level statements run during ExePutS — they have
                // their own implicit scope.
                self.enter_scope();
                self.walk_stmt(s);
                self.leave_scope();
            }
            TopItem::Class(_) | TopItem::Preprocessor(_) | TopItem::Asm(_) | TopItem::Empty => {}
        }
    }

    fn add_param(&mut self, p: &Param) {
        if p.variadic {
            return;
        }
        if let Some(name) = &p.name {
            self.add_local(name, &p.ty);
        }
    }

    fn walk_var_decl_init(&mut self, v: &VarDecl) {
        if let Some(init) = &v.init {
            self.walk_initializer(init);
        }
    }

    fn walk_initializer(&mut self, init: &Initializer) {
        match init {
            Initializer::Single(e) => self.walk_expr(e),
            Initializer::Aggregate(items) => {
                for i in items {
                    self.walk_initializer(i);
                }
            }
        }
    }

    fn walk_stmt(&mut self, s: &Stmt) {
        match &s.kind {
            StmtKind::Empty | StmtKind::Break | StmtKind::Default
            | StmtKind::SubSwitchStart | StmtKind::SubSwitchEnd
            | StmtKind::Goto(_) | StmtKind::Label(_) | StmtKind::Asm(_)
            | StmtKind::NoWarn(_) => {}
            StmtKind::Block(body) => {
                self.enter_scope();
                for s2 in body {
                    self.walk_stmt(s2);
                }
                self.leave_scope();
            }
            StmtKind::Expr(e) => self.walk_expr(e),
            StmtKind::If { cond, then_branch, else_branch } => {
                self.walk_expr(cond);
                self.walk_stmt(then_branch);
                if let Some(eb) = else_branch {
                    self.walk_stmt(eb);
                }
            }
            StmtKind::While { cond, body } | StmtKind::DoWhile { cond, body } => {
                self.walk_expr(cond);
                self.walk_stmt(body);
            }
            StmtKind::For { init, cond, update, body } => {
                self.enter_scope();
                if let Some(i) = init { self.walk_stmt(i); }
                if let Some(c) = cond { self.walk_expr(c); }
                if let Some(u) = update { self.walk_expr(u); }
                self.walk_stmt(body);
                self.leave_scope();
            }
            StmtKind::Switch { scrutinee, body, .. } => {
                self.walk_expr(scrutinee);
                check_switch_case_shared_scope(self.file, body, self.out);
                for s2 in body {
                    self.walk_stmt(s2);
                }
            }
            StmtKind::Return(opt) => {
                if let Some(e) = opt { self.walk_expr(e); }
            }
            StmtKind::Try { body, catch_body } => {
                self.enter_scope();
                for s2 in body { self.walk_stmt(s2); }
                self.leave_scope();
                self.enter_scope();
                for s2 in catch_body { self.walk_stmt(s2); }
                self.leave_scope();
            }
            StmtKind::Lock(inner) => self.walk_stmt(inner),
            StmtKind::LocalDecl(vs) => {
                for v in vs {
                    self.walk_var_decl_init(v);
                    self.add_local(&v.name, &v.ty);
                }
            }
            StmtKind::Case(values) => {
                use crate::parse::ast::CaseValue;
                for v in values {
                    match v {
                        CaseValue::Single(e) => self.walk_expr(e),
                        CaseValue::Range(a, b) => {
                            self.walk_expr(a);
                            self.walk_expr(b);
                        }
                        CaseValue::AutoIncrement => {}
                    }
                }
            }
        }
    }

    fn walk_expr(&mut self, e: &Expr) {
        match &e.kind {
            ExprKind::IntLit(_) | ExprKind::FloatLit(_) | ExprKind::CharLit(_)
            | ExprKind::StrLit(_) | ExprKind::Ident(_) | ExprKind::DolDol
            | ExprKind::DefaultArgSlot => {}
            ExprKind::Prefix(_, x) | ExprKind::Postfix(_, x) | ExprKind::Paren(x)
            | ExprKind::HolycCast(x, _) => self.walk_expr(x),
            ExprKind::Binary(op, l, r) => {
                if matches!(op, BinOp::BitAnd | BinOp::BitOr | BinOp::BitXor) {
                    if !self.looks_integer(l) && !self.looks_integer(r) {
                        self.out.push(Diag {
                            file: self.file.to_string(),
                            line: e.span.0.line,
                            col: e.span.0.col,
                            severity: Severity::Warning,
                            rule: "f64-bitwise",
                            message: format!(
                                "bitwise `{}` between non-integer-typed operands; \
                                 HolyC's bitwise ops act on IEEE-754 bit patterns \
                                 when operands are F64. Cast each side to I64 if \
                                 you mean integer truncation",
                                op_name(*op)
                            ),
                        });
                    }
                }
                self.walk_expr(l);
                self.walk_expr(r);
            }
            ExprKind::Index(a, b) => {
                self.walk_expr(a);
                self.walk_expr(b);
            }
            ExprKind::Member(x, _) | ExprKind::Arrow(x, _) => self.walk_expr(x),
            ExprKind::Call(callee, args) => {
                self.check_arity(callee, args, e.span.0);
                self.walk_expr(callee);
                for a in args { self.walk_expr(a); }
            }
            ExprKind::Sizeof(_) | ExprKind::OffsetOf(_) | ExprKind::Defined(_) => {}
            ExprKind::Comma(items) => {
                for x in items { self.walk_expr(x); }
            }
        }
    }

    /// Arity check: when calling a known global function, the arg
    /// count must match its declared parameter count. Variadic
    /// functions skip the upper-bound check (any extra args are OK)
    /// but still require at least `num_params` fixed args.
    ///
    /// Calls through function pointers, member access, or to
    /// undeclared functions are skipped — we have no signature.
    fn check_arity(&mut self, callee: &Expr, args: &[Expr], call_pos: crate::lex::Pos) {
        let name = match &callee.kind {
            ExprKind::Ident(n) => n,
            _ => return,
        };
        // Resolve: locals can shadow globals (a local of the same
        // name as a function is rare but possible — skip arity if
        // shadowed since we don't know the local's signature).
        if self.lookup_local(name).is_some() {
            return;
        }
        let sig = match self.tctx.functions.get(name) {
            Some(s) => s,
            None => return,
        };
        let provided = args.len();
        if sig.variadic {
            if provided < sig.num_params {
                self.out.push(Diag {
                    file: self.file.to_string(),
                    line: call_pos.line,
                    col: call_pos.col,
                    severity: Severity::Error,
                    rule: "arity-mismatch",
                    message: format!(
                        "`{name}` declared with {n_decl} required parameter{plural} (variadic); \
                         called with {provided}",
                        name = name,
                        n_decl = sig.num_params,
                        plural = if sig.num_params == 1 { "" } else { "s" },
                        provided = provided,
                    ),
                });
            }
        } else if provided != sig.num_params {
            self.out.push(Diag {
                file: self.file.to_string(),
                line: call_pos.line,
                col: call_pos.col,
                severity: Severity::Error,
                rule: "arity-mismatch",
                message: format!(
                    "`{name}` declared with {n_decl} parameter{plural}, called with {provided}",
                    name = name,
                    n_decl = sig.num_params,
                    plural = if sig.num_params == 1 { "" } else { "s" },
                    provided = provided,
                ),
            });
        }
    }

    /// Type-aware check: is `e` an expression whose value is
    /// definitely an integer? Returns true if syntactic shape says
    /// integer, OR if the expression refers to a name whose declared
    /// type is an integer primitive.
    fn looks_integer(&self, e: &Expr) -> bool {
        looks_integer_shape(e) || self.looks_integer_via_types(e)
    }

    fn looks_integer_via_types(&self, e: &Expr) -> bool {
        match &e.kind {
            ExprKind::Ident(name) => self
                .type_of(name)
                .map(is_integer_type)
                .unwrap_or(false),
            ExprKind::Call(callee, _) => {
                // Resolve the callee name (only handle direct calls,
                // i.e. `Ident(args)`). Function-pointer calls fall
                // back to syntactic shape.
                if let ExprKind::Ident(name) = &callee.kind {
                    self.type_of(name).map(is_integer_type).unwrap_or(false)
                } else {
                    false
                }
            }
            ExprKind::Paren(inner) => self.looks_integer(inner),
            ExprKind::Index(arr, _) => {
                // arr[i] of an integer-element array → integer.
                // We model this by inspecting the array's declared type.
                if let ExprKind::Ident(name) = &arr.kind {
                    self.type_of(name).map(is_integer_type).unwrap_or(false)
                } else {
                    false
                }
            }
            // Member / Arrow: would need class field types. Skip.
            _ => false,
        }
    }
}

// ============================================================
// Pure syntactic shape checks (no types needed)
// ============================================================

fn looks_integer_shape(e: &Expr) -> bool {
    match &e.kind {
        ExprKind::IntLit(_) | ExprKind::CharLit(_) => true,
        ExprKind::HolycCast(_, ty) => is_integer_type(ty),
        ExprKind::Prefix(PrefixOp::BitNot, _) => true,
        ExprKind::Prefix(PrefixOp::Minus | PrefixOp::Plus, inner) => matches!(
            inner.kind,
            ExprKind::IntLit(_) | ExprKind::CharLit(_)
        ),
        ExprKind::Binary(
            BinOp::BitAnd | BinOp::BitOr | BinOp::BitXor | BinOp::Shl | BinOp::Shr,
            _,
            _,
        ) => true,
        ExprKind::Paren(inner) => looks_integer_shape(inner),
        _ => false,
    }
}

fn is_integer_type(ty: &TypeRef) -> bool {
    matches!(
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
    )
}

fn op_name(op: BinOp) -> &'static str {
    match op {
        BinOp::BitAnd => "&",
        BinOp::BitOr => "|",
        BinOp::BitXor => "^",
        _ => "?",
    }
}

// ============================================================
// switch-case-shared-scope (no type info needed)
// ============================================================

fn check_switch_case_shared_scope(file: &str, body: &[Stmt], out: &mut Vec<Diag>) {
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

    for s in body {
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
