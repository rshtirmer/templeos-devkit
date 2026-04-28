//! Symbol resolver. Walks parsed `Module`s, registers declarations,
//! then validates every identifier-in-expression against the
//! registered set + the kernel built-ins manifest.
//!
//! Multi-file mode (now): callers register-then-check per file in
//! input order. Each file only sees decls from itself + earlier files
//! (plus locals + the built-in manifest). This matches
//! `temple-run.py`'s push semantics: every `src/*.HC` is JIT-compiled
//! in alphabetical order, so a use in `Cmd.HC` of `Cvar_FindVar`
//! (defined in `Cvar.HC`, which sorts AFTER `Cmd.HC`) is genuinely
//! unresolved at push time — and we now report it.
//!
//! Forward references INSIDE a single file are fine: register_module
//! is called once for the whole file before check_module runs, so
//! function bodies can reference any decl in the same file regardless
//! of source order.

use std::collections::HashMap;

use crate::diag::{Diag, Severity};
use crate::parse::ast::{
    Expr, ExprKind, Module, Param, SizeofArg, Stmt, StmtKind, TopItem, VarDecl,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SymbolKind {
    Function,
    Variable,
    Class,
    Param,
    Local,
}

#[derive(Clone, Debug)]
pub struct Symbol {
    pub name: String,
    pub kind: SymbolKind,
    pub file: String,
    pub line: u32,
}

#[derive(Default)]
pub struct Resolver {
    /// Global symbols installed by file-scope decls.
    globals: HashMap<String, Symbol>,
}

impl Resolver {
    pub fn new() -> Self {
        Self::default()
    }

    /// Register every top-level declaration in a module.
    pub fn register_module(&mut self, file: &str, m: &Module) {
        for item in &m.items {
            match item {
                TopItem::Function(f) => {
                    self.globals.insert(
                        f.name.clone(),
                        Symbol {
                            name: f.name.clone(),
                            kind: SymbolKind::Function,
                            file: file.to_string(),
                            line: f.span.0.line,
                        },
                    );
                }
                TopItem::Variable(v) => {
                    self.register_var(file, v);
                }
                TopItem::GlobalDeclList(vs) => {
                    for v in vs {
                        self.register_var(file, v);
                    }
                }
                TopItem::Class(c) => {
                    self.globals.insert(
                        c.name.clone(),
                        Symbol {
                            name: c.name.clone(),
                            kind: SymbolKind::Class,
                            file: file.to_string(),
                            line: c.span.0.line,
                        },
                    );
                }
                TopItem::Preprocessor(pp) => {
                    // `#define NAME body` installs NAME in the hash
                    // table — treat as a symbol for resolution.
                    if let crate::parse::ast::PpDirective::Define { name, .. } = pp {
                        self.globals.insert(
                            name.clone(),
                            Symbol {
                                name: name.clone(),
                                kind: SymbolKind::Variable,
                                file: file.to_string(),
                                line: 0,
                            },
                        );
                    }
                }
                TopItem::Stmt(_) | TopItem::Asm(_) | TopItem::Empty => {}
            }
        }
    }

    fn register_var(&mut self, file: &str, v: &VarDecl) {
        self.globals.insert(
            v.name.clone(),
            Symbol {
                name: v.name.clone(),
                kind: SymbolKind::Variable,
                file: file.to_string(),
                line: v.span.0.line,
            },
        );
    }

    /// Validate uses across a module. `file` is the diagnostic label.
    pub fn check_module(&self, file: &str, m: &Module) -> Vec<Diag> {
        let mut diags = Vec::new();
        for item in &m.items {
            self.check_top_item(file, item, &mut diags);
        }
        diags
    }

    fn check_top_item(&self, file: &str, item: &TopItem, diags: &mut Vec<Diag>) {
        match item {
            TopItem::Function(f) => {
                let mut local = LocalScope::new(self);
                for p in &f.params {
                    local.add_param(p);
                }
                if let Some(body) = &f.body {
                    for s in body {
                        self.check_stmt(file, s, &mut local, diags);
                    }
                }
            }
            TopItem::Variable(v) => self.check_var(file, v, &mut LocalScope::new(self), diags),
            TopItem::GlobalDeclList(vs) => {
                for v in vs {
                    self.check_var(file, v, &mut LocalScope::new(self), diags);
                }
            }
            TopItem::Class(_) => {} // class-body init checking is future work
            TopItem::Stmt(s) => self.check_stmt(file, s, &mut LocalScope::new(self), diags),
            TopItem::Preprocessor(_) | TopItem::Asm(_) | TopItem::Empty => {}
        }
    }

    fn check_var(
        &self,
        file: &str,
        v: &VarDecl,
        scope: &mut LocalScope,
        diags: &mut Vec<Diag>,
    ) {
        for dim in &v.array_dims {
            if let Some(e) = dim {
                self.check_expr(file, e, scope, diags);
            }
        }
        if let Some(init) = &v.init {
            self.check_initializer(file, init, scope, diags);
        }
        scope.add_local(&v.name);
    }

    fn check_initializer(
        &self,
        file: &str,
        init: &crate::parse::ast::Initializer,
        scope: &mut LocalScope,
        diags: &mut Vec<Diag>,
    ) {
        use crate::parse::ast::Initializer;
        match init {
            Initializer::Single(e) => self.check_expr(file, e, scope, diags),
            Initializer::Aggregate(items) => {
                for i in items {
                    self.check_initializer(file, i, scope, diags);
                }
            }
        }
    }

    fn check_stmt(
        &self,
        file: &str,
        s: &Stmt,
        scope: &mut LocalScope,
        diags: &mut Vec<Diag>,
    ) {
        match &s.kind {
            StmtKind::Empty => {}
            StmtKind::Block(body) => {
                let mut inner = scope.child();
                for s2 in body {
                    self.check_stmt(file, s2, &mut inner, diags);
                }
            }
            StmtKind::Expr(e) => self.check_expr(file, e, scope, diags),
            StmtKind::If { cond, then_branch, else_branch } => {
                self.check_expr(file, cond, scope, diags);
                self.check_stmt(file, then_branch, scope, diags);
                if let Some(eb) = else_branch {
                    self.check_stmt(file, eb, scope, diags);
                }
            }
            StmtKind::While { cond, body } | StmtKind::DoWhile { cond, body } => {
                self.check_expr(file, cond, scope, diags);
                self.check_stmt(file, body, scope, diags);
            }
            StmtKind::For { init, cond, update, body } => {
                let mut inner = scope.child();
                if let Some(i) = init {
                    self.check_stmt(file, i, &mut inner, diags);
                }
                if let Some(c) = cond {
                    self.check_expr(file, c, &mut inner, diags);
                }
                if let Some(u) = update {
                    self.check_expr(file, u, &mut inner, diags);
                }
                self.check_stmt(file, body, &mut inner, diags);
            }
            StmtKind::Switch { scrutinee, body, .. } => {
                self.check_expr(file, scrutinee, scope, diags);
                for s2 in body {
                    self.check_stmt(file, s2, scope, diags);
                }
            }
            StmtKind::Break | StmtKind::SubSwitchStart | StmtKind::SubSwitchEnd | StmtKind::Default => {}
            StmtKind::Return(opt) => {
                if let Some(e) = opt {
                    self.check_expr(file, e, scope, diags);
                }
            }
            StmtKind::Goto(_) | StmtKind::Label(_) | StmtKind::Asm(_) | StmtKind::NoWarn(_) => {}
            // Mid-body preprocessor directives — names inside `#ifdef`
            // bodies, `#define` substitutions, etc. aren't validated
            // (mirrors how `TopItem::Preprocessor` is handled).
            StmtKind::Preprocessor(_) => {}
            StmtKind::Try { body, catch_body } => {
                let mut inner = scope.child();
                for s2 in body {
                    self.check_stmt(file, s2, &mut inner, diags);
                }
                let mut inner2 = scope.child();
                for s2 in catch_body {
                    self.check_stmt(file, s2, &mut inner2, diags);
                }
            }
            StmtKind::Lock(inner) => self.check_stmt(file, inner, scope, diags),
            StmtKind::LocalDecl(vs) => {
                for v in vs {
                    self.check_var(file, v, scope, diags);
                }
            }
            StmtKind::Case(values) => {
                use crate::parse::ast::CaseValue;
                for v in values {
                    match v {
                        CaseValue::Single(e) => self.check_expr(file, e, scope, diags),
                        CaseValue::Range(a, b) => {
                            self.check_expr(file, a, scope, diags);
                            self.check_expr(file, b, scope, diags);
                        }
                        CaseValue::AutoIncrement => {}
                    }
                }
            }
        }
    }

    fn check_expr(
        &self,
        file: &str,
        e: &Expr,
        scope: &LocalScope,
        diags: &mut Vec<Diag>,
    ) {
        match &e.kind {
            ExprKind::Ident(name) => {
                if !self.is_known(name, scope) {
                    diags.push(Diag {
                        file: file.to_string(),
                        line: e.span.0.line,
                        col: e.span.0.col,
                        severity: Severity::Error,
                        rule: "unresolved-identifier",
                        message: format!(
                            "`{name}` is not declared in any input file and is \
                             not a known TempleOS built-in. Either define it or \
                             add it to the built-ins manifest."
                        ),
                    });
                }
            }
            ExprKind::IntLit(_) | ExprKind::FloatLit(_) | ExprKind::CharLit(_)
            | ExprKind::StrLit(_) | ExprKind::DolDol | ExprKind::DefaultArgSlot => {}
            ExprKind::Prefix(_, x) | ExprKind::Postfix(_, x) | ExprKind::Paren(x) => {
                self.check_expr(file, x, scope, diags);
            }
            ExprKind::Binary(_, l, r) => {
                self.check_expr(file, l, scope, diags);
                self.check_expr(file, r, scope, diags);
            }
            ExprKind::Index(a, b) => {
                self.check_expr(file, a, scope, diags);
                self.check_expr(file, b, scope, diags);
            }
            ExprKind::Member(x, _) | ExprKind::Arrow(x, _) => {
                self.check_expr(file, x, scope, diags);
                // member-name resolution requires a class symbol table — skip.
            }
            ExprKind::Call(callee, args) => {
                self.check_expr(file, callee, scope, diags);
                for a in args {
                    self.check_expr(file, a, scope, diags);
                }
            }
            ExprKind::HolycCast(x, _) => self.check_expr(file, x, scope, diags),
            ExprKind::Sizeof(arg) => {
                if let SizeofArg::Expr(x) = arg {
                    self.check_expr(file, x, scope, diags);
                }
            }
            ExprKind::OffsetOf(_) | ExprKind::Defined(_) => {}
            ExprKind::Comma(items) => {
                for x in items {
                    self.check_expr(file, x, scope, diags);
                }
            }
        }
    }

    fn is_known(&self, name: &str, scope: &LocalScope) -> bool {
        if scope.has(name) {
            return true;
        }
        if self.globals.contains_key(name) {
            return true;
        }
        super::is_builtin(name)
    }
}

/// Scope chain for parameters + locals during a single function body
/// walk. We don't model true lexical scoping with shadow hiding —
/// `has` returns true if the name is in any active scope.
struct LocalScope<'r> {
    parent: Option<&'r Resolver>,
    names: Vec<String>,
    inherited: Vec<String>,
}

impl<'r> LocalScope<'r> {
    fn new(parent: &'r Resolver) -> Self {
        Self { parent: Some(parent), names: Vec::new(), inherited: Vec::new() }
    }

    fn child(&self) -> Self {
        let mut all = self.inherited.clone();
        all.extend(self.names.iter().cloned());
        Self { parent: self.parent, names: Vec::new(), inherited: all }
    }

    fn add_param(&mut self, p: &Param) {
        if p.variadic {
            return;
        }
        if let Some(name) = &p.name {
            self.names.push(name.clone());
        }
    }

    fn add_local(&mut self, name: &str) {
        self.names.push(name.to_string());
    }

    fn has(&self, name: &str) -> bool {
        let _ = self.parent;
        self.names.iter().any(|n| n == name)
            || self.inherited.iter().any(|n| n == name)
    }
}
