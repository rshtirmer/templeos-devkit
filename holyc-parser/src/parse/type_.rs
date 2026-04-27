//! Type parser (PrsType + PrsArrayDims). Implements parse-spec §4.2.
//!
//! `parse_type` reads:
//! 1. an optional storage/modifier prefix is *not* consumed here — the
//!    caller (decl/local-decl) accumulates modifiers.
//! 2. a primitive type keyword (`U0`, `I8`, `F64`, ...) or a
//!    user-defined class name (any `Ident` whose keyword lookup says
//!    "not a keyword"), then
//! 3. zero or more `*` stars (saturated at PTR_STARS_NUM = 4 with a
//!    diagnostic — message "Too many *'s at " per parse-spec §6.4).
//!
//! Function-pointer syntax `RetTy (*)(args)` is detected when, after
//! the base type and any leading stars, the next token is `(`.

use crate::lex::{Keyword, Pos, TokenKind, lookup_keyword};
use crate::parse::ast::{Param, PrimType, TypeRef};
use crate::parse::parser::Parser;

const PTR_STARS_NUM: u32 = 4;

/// TempleOS compile-time constants / globals that resolve to a value
/// during PrsType. Used as parameter or local names, they get
/// resolved to the constant before the slot is allocated, which
/// trips the parser. See the `reserved-name-collision` rule.
const RESERVED_DECL_NAMES: &[&str] = &[
    "eps", "pi", "inf", "nan",   // F64 constants
    "tS",                          // ZealOS seconds-since-boot global
    "ms",                          // mouse global
];

pub fn is_reserved_name(name: &str) -> bool {
    RESERVED_DECL_NAMES.contains(&name)
}

pub fn format_reserved_message(name: &str) -> String {
    format!(
        "`{name}` is a TempleOS reserved compile-time constant; using it \
         as a parameter / local name causes PrsType to resolve it to the \
         constant value and trip 'Expecting *'. Rename to `tol`/`epsilon`/etc."
    )
}

/// Returns `Some(prim)` if `kw` is a primitive type keyword.
fn keyword_to_prim(kw: Keyword) -> Option<PrimType> {
    Some(match kw {
        Keyword::U0 => PrimType::U0,
        Keyword::I0 => PrimType::I0,
        Keyword::U8 => PrimType::U8,
        Keyword::I8 => PrimType::I8,
        Keyword::Bool => PrimType::Bool,
        Keyword::U16 => PrimType::U16,
        Keyword::I16 => PrimType::I16,
        Keyword::U32 => PrimType::U32,
        Keyword::I32 => PrimType::I32,
        Keyword::U64 => PrimType::U64,
        Keyword::I64 => PrimType::I64,
        Keyword::F64 => PrimType::F64,
        _ => return None,
    })
}

/// Peek-only: does the current token *start* a type?
pub fn at_type(p: &Parser) -> bool {
    if let TokenKind::Ident(s) = p.peek() {
        match lookup_keyword(s) {
            Some(kw) => keyword_to_prim(kw).is_some(),
            // A bare ident might be a user-defined class — but we can't
            // tell here without a symbol table, so the caller decides.
            None => false,
        }
    } else {
        false
    }
}

/// Parse a type specifier starting at the current cursor. Returns
/// `None` if the cursor isn't at a type token (and emits a diagnostic).
pub fn parse_type(p: &mut Parser) -> Option<TypeRef> {
    parse_type_inner(p, /*allow_named=*/ false)
}

/// `allow_named = true` — also accept a bare `Ident` as a class name.
/// Used by the decl parser when it has already determined this slot
/// must be a type (e.g. inside a function parameter list).
pub fn parse_type_allow_named(p: &mut Parser) -> Option<TypeRef> {
    parse_type_inner(p, /*allow_named=*/ true)
}

fn parse_type_inner(p: &mut Parser, allow_named: bool) -> Option<TypeRef> {
    let pos = p.current_pos();
    let base: TypeRef = match p.peek().clone() {
        TokenKind::Ident(s) => match lookup_keyword(&s) {
            Some(kw) => match keyword_to_prim(kw) {
                Some(prim) => {
                    p.bump();
                    TypeRef::Prim { ty: prim, pointer_depth: 0 }
                }
                None => {
                    p.error_at(pos, "expecting-type", "Expecting type at ");
                    return None;
                }
            },
            None => {
                // `F32` is a porting hazard — HolyC has only F64. Flag
                // explicitly so the message names the fix instead of
                // letting it slip through as a bogus class type.
                if s == "F32" {
                    p.error_at(
                        pos,
                        "f32-reference",
                        "HolyC has no F32 type — use F64",
                    );
                }
                if allow_named {
                    p.bump();
                    TypeRef::Named { name: s, pointer_depth: 0 }
                } else {
                    p.error_at(pos, "expecting-type", "Expecting type at ");
                    return None;
                }
            }
        },
        _ => {
            p.error_at(pos, "expecting-type", "Expecting type at ");
            return None;
        }
    };

    let mut ty = base;
    let stars = consume_stars(p);
    bump_pointer_depth(&mut ty, stars);

    // Function-pointer form: `Ret (*)(args)`. We detect a `(` after
    // the base type, expect `*`s + `)`, then a `(` parameter list.
    if matches!(p.peek(), TokenKind::LParen) {
        if let Some(fnref) = try_parse_function_pointer(p, ty.clone()) {
            return Some(fnref);
        }
        // Not a function-pointer pattern — leave the `(` alone for the
        // declarator/array parser to handle.
    }
    Some(ty)
}

fn consume_stars(p: &mut Parser) -> u32 {
    let mut n: u32 = 0;
    while matches!(p.peek(), TokenKind::Star) {
        let pos = p.current_pos();
        p.bump();
        n += 1;
        if n > PTR_STARS_NUM {
            p.error_at(pos, "too-many-stars", "Too many *'s at ");
            // Keep counting tokens but cap the depth.
        }
    }
    n.min(PTR_STARS_NUM)
}

fn bump_pointer_depth(ty: &mut TypeRef, n: u32) {
    if n == 0 { return; }
    match ty {
        TypeRef::Prim { pointer_depth, .. }
        | TypeRef::Named { pointer_depth, .. }
        | TypeRef::Func { pointer_depth, .. } => {
            *pointer_depth = (*pointer_depth + n).min(PTR_STARS_NUM);
        }
    }
}

/// Try to parse `(*...) (args)`. The cursor is at the `(`. Returns
/// `None` and rewinds if the pattern doesn't match.
fn try_parse_function_pointer(p: &mut Parser, ret: TypeRef) -> Option<TypeRef> {
    let cp = p.checkpoint();
    if !p.eat(&TokenKind::LParen) { return None; }
    if !matches!(p.peek(), TokenKind::Star) {
        // not a fun-pointer syntax — could be a postfix-typecast or
        // an unrelated paren expression. Restore.
        p.restore(cp);
        return None;
    }
    let stars = consume_stars(p);
    // Optional name in `(*name)` form is not stored on the type; the
    // declarator parser handles names. Accept and ignore an ident here.
    if let TokenKind::Ident(_) = p.peek() {
        p.bump();
    }
    if !p.eat(&TokenKind::RParen) {
        p.error_at(p.current_pos(), "expecting-rparen", "Missing ')' at ");
        return None;
    }
    if !p.eat(&TokenKind::LParen) {
        p.error_at(p.current_pos(), "expecting-lparen", "Expecting '(' at ");
        return None;
    }
    let params = parse_param_list(p);
    if !p.eat(&TokenKind::RParen) {
        p.error_at(p.current_pos(), "expecting-rparen", "Missing ')' at ");
    }
    let mut t = TypeRef::Func {
        ret: Box::new(ret),
        params,
        pointer_depth: 0,
    };
    bump_pointer_depth(&mut t, stars.max(1));
    let _ = Pos::default(); // silence unused import on some configs
    Some(t)
}

/// Parse a comma-separated parameter list, stopping at the closing
/// `)`. Used by both type_.rs (fun-pointer) and decl.rs (real fn decl).
pub fn parse_param_list(p: &mut Parser) -> Vec<Param> {
    let mut params = Vec::new();
    if matches!(p.peek(), TokenKind::RParen) { return params; }
    loop {
        // `...` variadic marker terminates.
        if matches!(p.peek(), TokenKind::Ellipsis) {
            p.bump();
            params.push(Param {
                ty: TypeRef::Prim { ty: PrimType::U0, pointer_depth: 0 },
                name: None,
                default: None,
                variadic: true,
            });
            break;
        }
        // Parse type (allow named class).
        let ty = match parse_type_allow_named(p) {
            Some(t) => t,
            None => break,
        };
        let mut name: Option<String> = None;
        let name_pos = p.current_pos();
        let next_ident: Option<String> = if let TokenKind::Ident(s) = p.peek() {
            if lookup_keyword(s).is_none() { Some(s.clone()) } else { None }
        } else {
            None
        };
        if let Some(s) = next_ident {
            if is_reserved_name(&s) {
                p.error_at(name_pos, "reserved-name-collision",
                    format_reserved_message(&s));
            }
            name = Some(s);
            p.bump();
        }
        // Optional default-arg (= expr) — call into expr parser. We
        // accept None gracefully (ExprCoder may not be ready).
        let mut default = None;
        if p.eat(&TokenKind::Eq) {
            default = crate::parse::expr::parse_expression(p);
        }
        params.push(Param {
            ty,
            name,
            default,
            variadic: false,
        });
        if !p.eat(&TokenKind::Comma) { break; }
    }
    params
}
