//! Expression parser. Implements the HolyC `PrsExpression` /
//! `PrsExpression2` flow from `PrsExp.HC` per `docs/parse-spec.md` §2.
//!
//! Architecture (parse-spec §8.1, §8.2):
//! - Atoms / unary prefixes are parsed by [`parse_unary_term`].
//! - Postfix modifiers (`[]`, `.`, `->`, `(args)`, `(TYPE)` postfix
//!   typecast, `++`, `--`) are parsed by [`parse_postfix`].
//! - Binary operators are climbed via [`parse_binary_climb`] using a
//!   data-driven precedence table.
//!
//! Bug-compat highlights (parse-spec §5):
//! - `(TYPE)expr` C-style casts are rejected (§5.9, gated by
//!   `config.allow_c_style_cast`).
//! - `<<` / `>>` live at MUL precedence, so `a + b << c` is `a + (b<<c)`
//!   (§2.2 quirks). Tested.
//! - Backtick `` ` `` is the right-associative power operator at
//!   `PREC_EXP` (between `*` and unary).
//! - `^^` is HolyC's short-circuit logical XOR (no C analogue).
//! - HolyC has NO ternary `?:` — encountering `?` errors out.
//! - `Bt`/`Bts`/`Btr`/etc. are normal idents that resolve to function
//!   calls — we do nothing special at parse time (§5.8).
//!
//! Notes:
//! - We do not yet have a real `parse_type` (StmtDeclCoder owns it), so
//!   typecast / `sizeof(TYPE)` parses a **minimal** type form inline:
//!   primitive keyword or user-defined ident, followed by zero or more
//!   `*`s. That covers TempleOS's runtime usage. When the full type
//!   parser lands, swap [`parse_simple_type`] for `type_::parse_type`.

use crate::lex::{Keyword, TokenKind, lookup_keyword};
use crate::parse::ast::{
    BinOp, Expr, ExprKind, PostfixOp, PrefixOp, PrimType, SizeofArg, Span, TypeRef,
};
use crate::parse::parser::Parser;

// ============================================================
// Public entry points
// ============================================================

/// Parse a full expression at HolyC's expression-statement level.
/// Returns `None` if the parser couldn't recover an expression at all
/// (in which case a diagnostic was already emitted).
pub fn parse_expression(p: &mut Parser) -> Option<Expr> {
    parse_assign(p)
}

/// Parse a "trailing expression" (the body of an `if`/`while`/`for`
/// without the trailing `)`). For now identical to `parse_expression`.
pub fn parse_expression_no_terminator(p: &mut Parser) -> Option<Expr> {
    parse_expression(p)
}

// ============================================================
// Precedence ladder (parse-spec §0.4)
// ============================================================

const PREC_ASSIGN: u8 = 0x3C;
const PREC_OR_OR: u8 = 0x38;
const PREC_XOR_XOR: u8 = 0x34;
const PREC_AND_AND: u8 = 0x30;
const PREC_CMP2: u8 = 0x2C;
const PREC_CMP: u8 = 0x28;
const PREC_ADD: u8 = 0x24;
const PREC_OR: u8 = 0x20;
const PREC_XOR: u8 = 0x1C;
const PREC_AND: u8 = 0x18;
const PREC_MUL: u8 = 0x14;
const PREC_EXP: u8 = 0x10;

#[derive(Clone, Copy, Debug)]
enum Assoc {
    Left,
    Right,
}

#[derive(Clone, Copy, Debug)]
struct OpEntry {
    op: BinOp,
    prec: u8,
    assoc: Assoc,
}

/// Map a token kind to its binary-operator descriptor. Returns `None`
/// for tokens that aren't binary operators (or that would terminate
/// an expression).
fn binary_op_for(tok: &TokenKind) -> Option<OpEntry> {
    use TokenKind::*;
    let (op, prec, assoc) = match tok {
        // Power — right-associative, tighter than `*`.
        Backtick => (BinOp::Power, PREC_EXP, Assoc::Right),

        // PREC_MUL — note shifts live here, not at PREC_ADD!
        Star => (BinOp::Mul, PREC_MUL, Assoc::Left),
        Slash => (BinOp::Div, PREC_MUL, Assoc::Left),
        Percent => (BinOp::Mod, PREC_MUL, Assoc::Left),
        Shl => (BinOp::Shl, PREC_MUL, Assoc::Left),
        Shr => (BinOp::Shr, PREC_MUL, Assoc::Left),

        // PREC_AND / XOR / OR (bitwise)
        Amp => (BinOp::BitAnd, PREC_AND, Assoc::Left),
        Caret => (BinOp::BitXor, PREC_XOR, Assoc::Left),
        Pipe => (BinOp::BitOr, PREC_OR, Assoc::Left),

        // PREC_ADD
        Plus => (BinOp::Add, PREC_ADD, Assoc::Left),
        Minus => (BinOp::Sub, PREC_ADD, Assoc::Left),

        // PREC_CMP / CMP2
        Lt => (BinOp::Lt, PREC_CMP, Assoc::Left),
        LtEq => (BinOp::LtEq, PREC_CMP, Assoc::Left),
        Gt => (BinOp::Gt, PREC_CMP, Assoc::Left),
        GtEq => (BinOp::GtEq, PREC_CMP, Assoc::Left),
        EqEq => (BinOp::Eq, PREC_CMP2, Assoc::Left),
        BangEq => (BinOp::NotEq, PREC_CMP2, Assoc::Left),

        // Logical short-circuit
        AmpAmp => (BinOp::LogAnd, PREC_AND_AND, Assoc::Left),
        CaretCaret => (BinOp::LogXor, PREC_XOR_XOR, Assoc::Left),
        PipePipe => (BinOp::LogOr, PREC_OR_OR, Assoc::Left),

        // Assignment family — right-associative
        Eq => (BinOp::Assign, PREC_ASSIGN, Assoc::Right),
        PlusEq => (BinOp::AddAssign, PREC_ASSIGN, Assoc::Right),
        MinusEq => (BinOp::SubAssign, PREC_ASSIGN, Assoc::Right),
        StarEq => (BinOp::MulAssign, PREC_ASSIGN, Assoc::Right),
        SlashEq => (BinOp::DivAssign, PREC_ASSIGN, Assoc::Right),
        PercentEq => (BinOp::ModAssign, PREC_ASSIGN, Assoc::Right),
        AmpEq => (BinOp::AndAssign, PREC_ASSIGN, Assoc::Right),
        PipeEq => (BinOp::OrAssign, PREC_ASSIGN, Assoc::Right),
        CaretEq => (BinOp::XorAssign, PREC_ASSIGN, Assoc::Right),
        ShlEq => (BinOp::ShlAssign, PREC_ASSIGN, Assoc::Right),
        ShrEq => (BinOp::ShrAssign, PREC_ASSIGN, Assoc::Right),

        _ => return None,
    };
    Some(OpEntry { op, prec, assoc })
}

// ============================================================
// Top-level: assignment / climb
// ============================================================

fn parse_assign(p: &mut Parser) -> Option<Expr> {
    parse_binary_climb(p, PREC_ASSIGN + 1)
}

/// Operator-precedence climbing. `min_prec` is exclusive of the *limit*
/// — only operators with `entry.prec < min_prec` (when comparing as
/// "tighter binds first" the way TempleOS PREC_* values decrease) are
/// consumed at this level. Internally we use the convention "smaller
/// PREC value = binds tighter" matching parse-spec §0.4.
fn parse_binary_climb(p: &mut Parser, min_prec: u8) -> Option<Expr> {
    let mut lhs = parse_unary_term(p)?;
    lhs = parse_postfix(p, lhs)?;

    loop {
        // Reject HolyC's missing ternary explicitly so the user gets a
        // clean message rather than a misleading "expected `;`".
        if matches!(p.peek(), TokenKind::Question) {
            let pos = p.current_pos();
            p.error_at(
                pos,
                "expr-no-ternary",
                "HolyC has no `?:` ternary operator; rewrite using `if`/`else`",
            );
            // Consume `?` plus what's after up to `:` so we don't loop
            // forever; mid-expression recovery is best-effort.
            p.bump();
            // try to skip over the "true" branch and the colon
            let _ = parse_assign(p);
            if matches!(p.peek(), TokenKind::Colon) {
                p.bump();
                let _ = parse_assign(p);
            }
            return Some(lhs);
        }

        let entry = match binary_op_for(p.peek()) {
            Some(e) => e,
            None => break,
        };
        if entry.prec >= min_prec {
            break;
        }

        p.bump();

        // Right-associative: same level still allowed on RHS.
        // Left-associative: tighter levels only on RHS.
        let next_min = match entry.assoc {
            Assoc::Left => entry.prec,
            Assoc::Right => entry.prec + 1,
        };
        let rhs = match parse_binary_climb(p, next_min) {
            Some(e) => e,
            None => return Some(lhs),
        };
        let span: Span = (lhs.span.0, rhs.span.1);
        lhs = Expr {
            kind: ExprKind::Binary(entry.op, Box::new(lhs), Box::new(rhs)),
            span,
        };
    }
    Some(lhs)
}

// ============================================================
// Unary terms (atoms + prefix unary)
// ============================================================

fn parse_unary_term(p: &mut Parser) -> Option<Expr> {
    let start = p.current_pos();

    // Keyword-based atoms first (they are emitted as Ident tokens).
    if p.at_keyword(Keyword::Sizeof) {
        return parse_sizeof(p);
    }
    if p.at_keyword(Keyword::Offset) {
        return parse_offsetof(p);
    }
    if p.at_keyword(Keyword::Defined) {
        return parse_defined(p);
    }

    match p.peek().clone() {
        // -------- prefix unary --------
        TokenKind::Plus => {
            p.bump();
            let inner = parse_unary_term(p)?;
            let span = (start, inner.span.1);
            Some(Expr {
                kind: ExprKind::Prefix(PrefixOp::Plus, Box::new(inner)),
                span,
            })
        }
        TokenKind::Minus => {
            p.bump();
            let inner = parse_unary_term(p)?;
            // TempleOS quirk (parse-spec §2.2 / PrsExp.HC:139-147):
            // `-x ` y` parses as `-(x ` y)` — Power binds tighter
            // than unary minus. We splice Power in here before
            // wrapping with the prefix-minus.
            let mut inner = parse_postfix(p, inner)?;
            while matches!(p.peek(), TokenKind::Backtick) {
                p.bump();
                let rhs = match parse_binary_climb(p, PREC_EXP + 1) {
                    Some(e) => e,
                    None => break,
                };
                let span = (inner.span.0, rhs.span.1);
                inner = Expr {
                    kind: ExprKind::Binary(BinOp::Power, Box::new(inner), Box::new(rhs)),
                    span,
                };
            }
            let span = (start, inner.span.1);
            Some(Expr {
                kind: ExprKind::Prefix(PrefixOp::Minus, Box::new(inner)),
                span,
            })
        }
        TokenKind::Bang => {
            p.bump();
            let inner = parse_unary_term(p)?;
            let span = (start, inner.span.1);
            Some(Expr {
                kind: ExprKind::Prefix(PrefixOp::LogNot, Box::new(inner)),
                span,
            })
        }
        TokenKind::Tilde => {
            p.bump();
            let inner = parse_unary_term(p)?;
            let span = (start, inner.span.1);
            Some(Expr {
                kind: ExprKind::Prefix(PrefixOp::BitNot, Box::new(inner)),
                span,
            })
        }
        TokenKind::Star => {
            p.bump();
            let inner = parse_unary_term(p)?;
            let span = (start, inner.span.1);
            Some(Expr {
                kind: ExprKind::Prefix(PrefixOp::Deref, Box::new(inner)),
                span,
            })
        }
        TokenKind::Amp => {
            p.bump();
            // TODO(InternalFun): once the symbol table lands, reject
            //   `&InternalFunName` per parse-spec §5.7. For now we
            //   accept any unary-term operand uniformly.
            let inner = parse_unary_term(p)?;
            let span = (start, inner.span.1);
            Some(Expr {
                kind: ExprKind::Prefix(PrefixOp::AddrOf, Box::new(inner)),
                span,
            })
        }
        TokenKind::PlusPlus => {
            p.bump();
            let inner = parse_unary_term(p)?;
            let span = (start, inner.span.1);
            Some(Expr {
                kind: ExprKind::Prefix(PrefixOp::PreInc, Box::new(inner)),
                span,
            })
        }
        TokenKind::MinusMinus => {
            p.bump();
            let inner = parse_unary_term(p)?;
            let span = (start, inner.span.1);
            Some(Expr {
                kind: ExprKind::Prefix(PrefixOp::PreDec, Box::new(inner)),
                span,
            })
        }

        // -------- atoms --------
        TokenKind::IntLit(v) => {
            let t = p.bump();
            Some(Expr {
                kind: ExprKind::IntLit(v),
                span: (t.start, t.end),
            })
        }
        TokenKind::FloatLit(v) => {
            let t = p.bump();
            Some(Expr {
                kind: ExprKind::FloatLit(v),
                span: (t.start, t.end),
            })
        }
        TokenKind::CharLit(v) => {
            let t = p.bump();
            Some(Expr {
                kind: ExprKind::CharLit(v),
                span: (t.start, t.end),
            })
        }
        TokenKind::StrLit(bs) => {
            let t = p.bump();
            Some(Expr {
                kind: ExprKind::StrLit(bs),
                span: (t.start, t.end),
            })
        }
        TokenKind::Ident(name) => {
            // Type-name idents are *not* expressions on their own —
            // they only appear in casts / sizeof / decls. Bug-compat:
            // we still accept `(TYPE)expr` lookahead-rejection inside
            // `(`-handling below, so a bare type ident here means a
            // user wrote something like `F64;` which TempleOS would
            // also reject. We let it parse as an Ident expression and
            // allow downstream layers to flag it.
            let t = p.bump();
            Some(Expr {
                kind: ExprKind::Ident(name),
                span: (t.start, t.end),
            })
        }

        // -------- ( expr ) or  (TYPE)expr  C-cast (rejected) --------
        TokenKind::LParen => parse_paren_or_c_cast(p),

        // -------- bug-compat: bad token --------
        TokenKind::Question => {
            // Caught by climber too, but if we hit it as a unary-term
            // it means there is no LHS at all.
            let pos = p.current_pos();
            p.error_at(
                pos,
                "expr-no-ternary",
                "HolyC has no `?:` ternary operator",
            );
            p.bump();
            None
        }
        _ => {
            let pos = p.current_pos();
            let actual = p.peek().spelling().to_string();
            p.error_at(
                pos,
                "expr-unexpected-token",
                format!("unexpected token `{}` at start of expression", actual),
            );
            // Bump one token to make progress without devouring the
            // entire statement; outer recovery may then bail us out.
            if !p.at_eof() {
                p.bump();
            }
            None
        }
    }
}

/// `(` was the current token. Decide between a parenthesised expr and
/// a C-style prefix cast (which we reject per parse-spec §5.9 unless
/// `config.allow_c_style_cast` is set).
fn parse_paren_or_c_cast(p: &mut Parser) -> Option<Expr> {
    let start = p.current_pos();
    p.bump(); // consume '('

    // Lookahead to detect `(TYPE)`. A type starts with either a primitive
    // type keyword or a user-defined ident, followed by zero or more `*`,
    // and is closed by `)`.
    if looks_like_type(p) {
        let cp = p.checkpoint();
        if let Some(ty) = parse_simple_type(p) {
            if matches!(p.peek(), TokenKind::RParen) {
                // `(TYPE)` followed by an expression — C-style cast.
                if !p.config.allow_c_style_cast {
                    p.error_at(
                        start,
                        "compiler-c-style-cast",
                        "Use TempleOS postfix typecasting at",
                    );
                    // recover: consume `)` and parse the operand so we
                    // make progress, then return the operand as-is.
                    p.bump(); // ')'
                    return parse_unary_term(p);
                } else {
                    p.bump(); // ')'
                    let inner = parse_unary_term(p)?;
                    let span = (start, inner.span.1);
                    return Some(Expr {
                        kind: ExprKind::HolycCast(Box::new(inner), ty),
                        span,
                    });
                }
            }
            // Looked like a type but not closed by `)` — backtrack and
            // re-parse as a normal parenthesised expr.
            p.restore(cp);
        } else {
            p.restore(cp);
        }
    }

    // Plain `(expr)`.
    let inner = parse_assign(p)?;
    let end_pos = p.current_pos();
    if !p.eat(&TokenKind::RParen) {
        p.error_at(end_pos, "expr-missing-rparen", "expected `)`");
    }
    let span = (start, end_pos);
    Some(Expr {
        kind: ExprKind::Paren(Box::new(inner)),
        span,
    })
}

// ============================================================
// Postfix ([ ] . -> ( ) (TYPE) ++ --)
// ============================================================

fn parse_postfix(p: &mut Parser, mut lhs: Expr) -> Option<Expr> {
    loop {
        match p.peek().clone() {
            TokenKind::PlusPlus => {
                let t = p.bump();
                let span = (lhs.span.0, t.end);
                lhs = Expr {
                    kind: ExprKind::Postfix(PostfixOp::Inc, Box::new(lhs)),
                    span,
                };
            }
            TokenKind::MinusMinus => {
                let t = p.bump();
                let span = (lhs.span.0, t.end);
                lhs = Expr {
                    kind: ExprKind::Postfix(PostfixOp::Dec, Box::new(lhs)),
                    span,
                };
            }
            TokenKind::LBracket => {
                p.bump();
                let idx = parse_assign(p)?;
                let end_pos = p.current_pos();
                if !p.eat(&TokenKind::RBracket) {
                    p.error_at(end_pos, "expr-missing-rbracket", "expected `]`");
                }
                let span = (lhs.span.0, end_pos);
                lhs = Expr {
                    kind: ExprKind::Index(Box::new(lhs), Box::new(idx)),
                    span,
                };
            }
            TokenKind::Dot => {
                p.bump();
                let (name, end) = match p.peek().clone() {
                    TokenKind::Ident(s) => {
                        let t = p.bump();
                        (s, t.end)
                    }
                    _ => {
                        let pos = p.current_pos();
                        p.error_at(pos, "expr-expected-member", "expected member name after `.`");
                        return Some(lhs);
                    }
                };
                let span = (lhs.span.0, end);
                lhs = Expr {
                    kind: ExprKind::Member(Box::new(lhs), name),
                    span,
                };
            }
            TokenKind::Arrow => {
                p.bump();
                let (name, end) = match p.peek().clone() {
                    TokenKind::Ident(s) => {
                        let t = p.bump();
                        (s, t.end)
                    }
                    _ => {
                        let pos = p.current_pos();
                        p.error_at(pos, "expr-expected-member", "expected member name after `->`");
                        return Some(lhs);
                    }
                };
                let span = (lhs.span.0, end);
                lhs = Expr {
                    kind: ExprKind::Arrow(Box::new(lhs), name),
                    span,
                };
            }
            TokenKind::LParen => {
                // Disambiguate: `(TYPE)` postfix typecast vs `(args)`
                // function-call. Per parse-spec §2.3 / §5.6: try-type
                // first; if not a type, parse as call.
                let cp = p.checkpoint();
                p.bump(); // '('
                let parsed_cast = if looks_like_type(p) {
                    let cp2 = p.checkpoint();
                    if let Some(ty) = parse_simple_type(p) {
                        if matches!(p.peek(), TokenKind::RParen) {
                            let end_tok = p.bump();
                            let span = (lhs.span.0, end_tok.end);
                            Some(Expr {
                                kind: ExprKind::HolycCast(Box::new(lhs.clone()), ty),
                                span,
                            })
                        } else {
                            p.restore(cp2);
                            None
                        }
                    } else {
                        p.restore(cp2);
                        None
                    }
                } else {
                    None
                };
                match parsed_cast {
                    Some(e) => {
                        lhs = e;
                    }
                    None => {
                        // Not a typecast — restore and parse as call.
                        p.restore(cp);
                        p.bump(); // re-consume '('
                        let mut args = Vec::new();
                        if !matches!(p.peek(), TokenKind::RParen) {
                            loop {
                                let arg = parse_assign(p)?;
                                args.push(arg);
                                if matches!(p.peek(), TokenKind::Comma) {
                                    p.bump();
                                    continue;
                                }
                                break;
                            }
                        }
                        let end_pos = p.current_pos();
                        if !p.eat(&TokenKind::RParen) {
                            p.error_at(end_pos, "expr-missing-rparen", "expected `)` after call args");
                        }
                        let span = (lhs.span.0, end_pos);
                        lhs = Expr {
                            kind: ExprKind::Call(Box::new(lhs), args),
                            span,
                        };
                    }
                }
            }
            _ => break,
        }
    }
    Some(lhs)
}

// ============================================================
// sizeof / offset / defined
// ============================================================

fn parse_sizeof(p: &mut Parser) -> Option<Expr> {
    let start = p.current_pos();
    p.bump(); // sizeof

    let needs_paren = matches!(p.peek(), TokenKind::LParen);
    if needs_paren {
        p.bump();
    }

    // Try a type first; if that fails, fall back to an expression.
    let arg = if looks_like_type(p) {
        let cp = p.checkpoint();
        match parse_simple_type(p) {
            Some(ty) => SizeofArg::Type(ty),
            None => {
                p.restore(cp);
                SizeofArg::Expr(Box::new(parse_assign(p)?))
            }
        }
    } else {
        SizeofArg::Expr(Box::new(parse_assign(p)?))
    };

    let end_pos = p.current_pos();
    if needs_paren && !p.eat(&TokenKind::RParen) {
        p.error_at(end_pos, "expr-missing-rparen", "expected `)` after sizeof");
    }
    Some(Expr {
        kind: ExprKind::Sizeof(arg),
        span: (start, end_pos),
    })
}

fn parse_offsetof(p: &mut Parser) -> Option<Expr> {
    let start = p.current_pos();
    p.bump(); // offset

    let needs_paren = matches!(p.peek(), TokenKind::LParen);
    if needs_paren {
        p.bump();
    }

    let mut chain = Vec::new();
    match p.peek().clone() {
        TokenKind::Ident(s) => {
            p.bump();
            chain.push(s);
        }
        _ => {
            let pos = p.current_pos();
            p.error_at(pos, "expr-offset-class", "expected class name after `offset`");
            return None;
        }
    }
    while matches!(p.peek(), TokenKind::Dot) {
        p.bump();
        match p.peek().clone() {
            TokenKind::Ident(s) => {
                p.bump();
                chain.push(s);
            }
            _ => {
                let pos = p.current_pos();
                p.error_at(pos, "expr-offset-member", "expected member name in `offset(...)` chain");
                break;
            }
        }
    }

    let end_pos = p.current_pos();
    if needs_paren && !p.eat(&TokenKind::RParen) {
        p.error_at(end_pos, "expr-missing-rparen", "expected `)` after offset");
    }
    Some(Expr {
        kind: ExprKind::OffsetOf(chain),
        span: (start, end_pos),
    })
}

fn parse_defined(p: &mut Parser) -> Option<Expr> {
    let start = p.current_pos();
    p.bump(); // defined

    let needs_paren = matches!(p.peek(), TokenKind::LParen);
    if needs_paren {
        p.bump();
    }

    let name = match p.peek().clone() {
        TokenKind::Ident(s) => {
            p.bump();
            s
        }
        _ => {
            let pos = p.current_pos();
            p.error_at(pos, "expr-defined-arg", "expected identifier after `defined`");
            return None;
        }
    };

    let end_pos = p.current_pos();
    if needs_paren && !p.eat(&TokenKind::RParen) {
        p.error_at(end_pos, "expr-missing-rparen", "expected `)` after defined");
    }
    Some(Expr {
        kind: ExprKind::Defined(name),
        span: (start, end_pos),
    })
}

// ============================================================
// Minimal type recognition (until type_::parse_type lands)
// ============================================================

/// Cheap lookahead: does the current cursor *plausibly* start a type?
fn looks_like_type(p: &Parser) -> bool {
    if let TokenKind::Ident(s) = p.peek() {
        if let Some(kw) = lookup_keyword(s) {
            return matches!(
                kw,
                Keyword::U0
                    | Keyword::I0
                    | Keyword::U8
                    | Keyword::I8
                    | Keyword::Bool
                    | Keyword::U16
                    | Keyword::I16
                    | Keyword::U32
                    | Keyword::I32
                    | Keyword::U64
                    | Keyword::I64
                    | Keyword::F64
            );
        }
        // User-defined type names cannot be distinguished without the
        // symbol table. We conservatively treat *capitalised* idents
        // followed by `*`/`)` as plausible types in the casting hot
        // paths (typecast / sizeof). This is a heuristic — when the
        // symbol table exists we'll switch to a real lookup.
        let is_cap = s.chars().next().map(|c| c.is_ascii_uppercase()).unwrap_or(false);
        if !is_cap {
            return false;
        }
        // Peek: if followed by `*` or `)` it's almost certainly a type
        // in this context. Otherwise we treat it as an ident.
        let next = p.peek_at(1);
        return matches!(next, TokenKind::Star | TokenKind::RParen);
    }
    false
}

/// Parse the minimal type form: PRIM_OR_NAMED ('*' | '**' | ...). Used
/// only inside expression-level constructs (`sizeof`, casts).
fn parse_simple_type(p: &mut Parser) -> Option<TypeRef> {
    let prim = match p.peek().clone() {
        TokenKind::Ident(s) => match lookup_keyword(&s) {
            Some(Keyword::U0) => Some((PrimType::U0, s)),
            Some(Keyword::I0) => Some((PrimType::I0, s)),
            Some(Keyword::U8) => Some((PrimType::U8, s)),
            Some(Keyword::I8) => Some((PrimType::I8, s)),
            Some(Keyword::Bool) => Some((PrimType::Bool, s)),
            Some(Keyword::U16) => Some((PrimType::U16, s)),
            Some(Keyword::I16) => Some((PrimType::I16, s)),
            Some(Keyword::U32) => Some((PrimType::U32, s)),
            Some(Keyword::I32) => Some((PrimType::I32, s)),
            Some(Keyword::U64) => Some((PrimType::U64, s)),
            Some(Keyword::I64) => Some((PrimType::I64, s)),
            Some(Keyword::F64) => Some((PrimType::F64, s)),
            _ => None,
        },
        _ => None,
    };

    let ty = if let Some((pty, _)) = prim {
        p.bump();
        let mut depth = 0u32;
        while matches!(p.peek(), TokenKind::Star) {
            p.bump();
            depth += 1;
        }
        TypeRef::Prim {
            ty: pty,
            pointer_depth: depth,
        }
    } else if let TokenKind::Ident(name) = p.peek().clone() {
        // User-defined type — caller has already gated this with
        // `looks_like_type` so we accept and consume.
        p.bump();
        let mut depth = 0u32;
        while matches!(p.peek(), TokenKind::Star) {
            p.bump();
            depth += 1;
        }
        TypeRef::Named {
            name,
            pointer_depth: depth,
        }
    } else {
        return None;
    };

    Some(ty)
}
