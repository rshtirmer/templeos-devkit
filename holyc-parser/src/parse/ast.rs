//! HolyC AST. Designed to be filled by `expr.rs`, `stmt.rs`, `decl.rs`,
//! `type_.rs`. Spans use lexer `Pos` so diagnostics line up with the
//! lexer-emitted ones.
//!
//! Naming follows TempleOS where reasonable; deviations are flagged.

use crate::lex::Pos;

pub type Span = (Pos, Pos);

// ============================================================
// Types
// ============================================================

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PrimType {
    U0, I0, U8, I8, Bool, U16, I16, U32, I32, U64, I64, F64,
}

#[derive(Clone, Debug, PartialEq)]
pub enum TypeRef {
    /// `U0` / `I64` / `F64` / etc. Arbitrary star count is encoded
    /// here via `pointer_depth`.
    Prim { ty: PrimType, pointer_depth: u32 },
    /// User-defined class or union. Pointer depth tracks `Class*`.
    Named { name: String, pointer_depth: u32 },
    /// Function type used in function-pointer decls.
    Func { ret: Box<TypeRef>, params: Vec<Param>, pointer_depth: u32 },
}

// ============================================================
// Expressions
// ============================================================

#[derive(Clone, Debug, PartialEq)]
pub struct Expr {
    pub kind: ExprKind,
    pub span: Span,
}

#[derive(Clone, Debug, PartialEq)]
pub enum ExprKind {
    // -------- atoms --------
    IntLit(i64),
    FloatLit(f64),
    CharLit(i64),
    StrLit(Vec<u8>),
    Ident(String),
    /// `$$` — TempleOS class-relative offset / RIP-relative depending
    /// on context. Emitted as opaque atom; resolution is for typeck.
    DolDol,

    // -------- unary --------
    /// Prefix: `+x`, `-x`, `!x`, `~x`, `*x`, `&x`, `++x`, `--x`.
    Prefix(PrefixOp, Box<Expr>),
    /// Postfix: `x++`, `x--`.
    Postfix(PostfixOp, Box<Expr>),

    // -------- binary --------
    Binary(BinOp, Box<Expr>, Box<Expr>),

    // -------- postfix structures --------
    Index(Box<Expr>, Box<Expr>),
    Member(Box<Expr>, String),  // a.b
    Arrow(Box<Expr>, String),   // a->b
    Call(Box<Expr>, Vec<Expr>),
    /// Empty slot in a call's argument list — `f(a, , c)` or `f(a,)`.
    /// HolyC allows omitting an argument when the callee declares a
    /// default value for that parameter; the empty slot means "use
    /// the default." Represented as its own expression so the args
    /// vector preserves positional alignment with the callee's
    /// parameter list. Only ever appears as a direct child of
    /// `ExprKind::Call`'s args.
    DefaultArgSlot,

    /// HolyC postfix typecast: `expr (TYPE)`.
    HolycCast(Box<Expr>, TypeRef),

    // -------- term-level builtins --------
    Sizeof(SizeofArg),
    /// `offset(class.member.member...)` chain.
    OffsetOf(Vec<String>),
    Defined(String),

    /// Parenthesised group (kept so spans + roundtripping stay clean).
    Paren(Box<Expr>),

    /// Comma operator `a, b` — left in tree for now even though
    /// PrsExpression's actual handling treats it as expression-list
    /// flattening at call sites. The parser may rewrite into a list.
    Comma(Vec<Expr>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PrefixOp {
    Plus, Minus, LogNot, BitNot, Deref, AddrOf, PreInc, PreDec,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PostfixOp {
    Inc, Dec,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BinOp {
    // arithmetic
    Power,  // backtick (right-assoc, PREC_EXP)
    Mul, Div, Mod, Shl, Shr,  // PREC_MUL = 14
    Add, Sub,                  // PREC_ADD = 16
    BitAnd,                    // PREC_AND = 18
    BitXor,                    // PREC_XOR = 1C
    BitOr,                     // PREC_OR  = 20
    // compares
    Lt, LtEq, Gt, GtEq,
    Eq, NotEq,
    // logical short-circuit
    LogAnd, LogXor, LogOr,
    // assignment family
    Assign,
    AddAssign, SubAssign, MulAssign, DivAssign, ModAssign,
    AndAssign, OrAssign, XorAssign, ShlAssign, ShrAssign,
}

#[derive(Clone, Debug, PartialEq)]
pub enum SizeofArg {
    Type(TypeRef),
    Expr(Box<Expr>),
}

// ============================================================
// Statements
// ============================================================

#[derive(Clone, Debug, PartialEq)]
pub struct Stmt {
    pub kind: StmtKind,
    pub span: Span,
}

#[derive(Clone, Debug, PartialEq)]
pub enum StmtKind {
    Empty,
    Block(Vec<Stmt>),
    Expr(Expr),
    If {
        cond: Expr,
        then_branch: Box<Stmt>,
        else_branch: Option<Box<Stmt>>,
    },
    While { cond: Expr, body: Box<Stmt> },
    DoWhile { body: Box<Stmt>, cond: Expr },
    For {
        init: Option<Box<Stmt>>,
        cond: Option<Expr>,
        update: Option<Expr>,
        body: Box<Stmt>,
    },
    Switch {
        scrutinee: Expr,
        body: Vec<Stmt>,
        /// `[scrutinee]` form (square brackets) per parse-spec §3.
        square_brackets: bool,
    },
    Break,
    Return(Option<Expr>),
    Goto(String),
    Label(String),
    Try { body: Vec<Stmt>, catch_body: Vec<Stmt> },
    Asm(String),
    /// `lock stmt` — wraps a sub-statement with locking semantics.
    Lock(Box<Stmt>),
    NoWarn(Vec<String>),
    /// Local variable decl(s). Multi-decl is allowed at function scope
    /// (parse-spec §4.3 — bug-compat at file scope).
    LocalDecl(Vec<VarDecl>),
    /// Sub-switch markers — appear inside `switch` bodies.
    Case(Vec<CaseValue>),
    Default,
    SubSwitchStart,
    SubSwitchEnd,
}

#[derive(Clone, Debug, PartialEq)]
pub enum CaseValue {
    Single(Expr),
    Range(Expr, Expr),
    /// Bare `case :` auto-increments from previous (parse-spec §5.12).
    AutoIncrement,
}

// ============================================================
// Declarations
// ============================================================

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Modifier {
    Static, Public, Interrupt,
    Extern, ExternUnderscore, Import, ImportUnderscore,
    Lock, Lastclass,
    Noreg, Reg, // Reg may carry a register name; encode via decl.reg_name.
    Argpop, Noargpop, Nostkchk,
    Intern,
}

#[derive(Clone, Debug, PartialEq)]
pub struct VarDecl {
    pub modifiers: Vec<Modifier>,
    pub ty: TypeRef,
    pub name: String,
    /// Array dimensions (each may be unsized via `[]`).
    pub array_dims: Vec<Option<Expr>>,
    pub init: Option<Initializer>,
    /// If `Modifier::Reg` was specified with a name, this carries it.
    pub reg_name: Option<String>,
    pub span: Span,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Initializer {
    Single(Expr),
    Aggregate(Vec<Initializer>),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Param {
    pub ty: TypeRef,
    pub name: Option<String>,
    pub default: Option<Expr>,
    /// True if this is the trailing `...` varargs marker (no other
    /// fields meaningful in that case).
    pub variadic: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct FunctionDef {
    pub modifiers: Vec<Modifier>,
    pub ret_type: TypeRef,
    pub name: String,
    pub params: Vec<Param>,
    /// `None` = forward declaration / prototype only.
    pub body: Option<Vec<Stmt>>,
    pub span: Span,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ClassDef {
    pub name: String,
    pub base: Option<String>,
    pub members: Vec<VarDecl>,
    pub span: Span,
    pub is_union: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub enum PpDirective {
    Include(String),
    Define { name: String, body: String },
    Ifdef(String),
    Ifndef(String),
    IfAot,
    IfJit,
    Else,
    EndIf,
    Assert(Expr),
    HelpIndex(String),
    HelpFile(String),
    /// Catch-all for directives we tokenize but don't yet model
    /// semantically (e.g. `#exe { ... }`).
    Other { name: String, body: String },
}

// ============================================================
// Top-level
// ============================================================

#[derive(Clone, Debug, PartialEq)]
pub struct Module {
    pub items: Vec<TopItem>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum TopItem {
    Function(FunctionDef),
    Variable(VarDecl),
    GlobalDeclList(Vec<VarDecl>),
    Class(ClassDef),
    Stmt(Stmt),
    Preprocessor(PpDirective),
    Asm(String),
    Empty,
}
