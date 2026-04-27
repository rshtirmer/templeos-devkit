//! Declaration parser. Implements parse-spec §4 (PrsVar +
//! PrsGlblVarLst + PrsClass + PrsFunJoin/PrsFun) and §1.1 top-level
//! dispatch.
//!
//! Public entry points:
//!   - `parse_top_item(&mut Parser) -> Option<TopItem>`
//!   - `parse_local_decl(&mut Parser) -> Option<Vec<VarDecl>>`

use crate::lex::{Keyword, Pos, TokenKind, lookup_keyword};
use crate::parse::ast::{
    ClassDef, Expr, FunctionDef, Initializer, Modifier, Param, PpDirective, Stmt, StmtKind,
    TopItem, TypeRef, VarDecl,
};
use crate::parse::parser::Parser;

// ============================================================
// Public top-level dispatcher
// ============================================================

pub fn parse_top_item(p: &mut Parser) -> Option<TopItem> {
    if p.at_eof() { return None; }
    match p.peek().clone() {
        TokenKind::Semicolon => {
            p.bump();
            Some(TopItem::Empty)
        }
        TokenKind::Hash => parse_preprocessor(p).map(TopItem::Preprocessor),
        TokenKind::StrLit(_) | TokenKind::CharLit(_) => {
            // Implicit-Print top-level statement (§5.13).
            let s = crate::parse::stmt::parse_statement_top(p)?;
            Some(TopItem::Stmt(s))
        }
        TokenKind::LBrace => {
            // Free-standing block at top level — runs at parse time.
            let s = crate::parse::stmt::parse_statement_top(p)?;
            Some(TopItem::Stmt(s))
        }
        TokenKind::Ident(name) => match lookup_keyword(&name) {
            Some(Keyword::Class) | Some(Keyword::Union) => {
                parse_class_or_union(p, /*is_extern=*/ false).map(TopItem::Class)
            }
            Some(Keyword::Asm) => {
                let s = crate::parse::stmt::parse_statement_top(p)?;
                if let StmtKind::Asm(text) = s.kind {
                    Some(TopItem::Asm(text))
                } else {
                    Some(TopItem::Stmt(s))
                }
            }
            // Storage / linkage modifiers always lead a declaration.
            Some(Keyword::Static)
            | Some(Keyword::Public)
            | Some(Keyword::Interrupt)
            | Some(Keyword::Extern)
            | Some(Keyword::ExternUnderscore)
            | Some(Keyword::Import)
            | Some(Keyword::ImportUnderscore)
            | Some(Keyword::Intern)
            | Some(Keyword::Argpop)
            | Some(Keyword::Noargpop)
            | Some(Keyword::Nostkchk)
            | Some(Keyword::Lastclass)
            | Some(Keyword::Noreg)
            | Some(Keyword::Reg) => parse_global_decl(p),
            // Statement-introducing keywords.
            Some(Keyword::Return)
            | Some(Keyword::Goto)
            | Some(Keyword::Break)
            | Some(Keyword::If)
            | Some(Keyword::Else)
            | Some(Keyword::While)
            | Some(Keyword::Do)
            | Some(Keyword::For)
            | Some(Keyword::Switch)
            | Some(Keyword::Try)
            | Some(Keyword::Catch)
            | Some(Keyword::Lock)
            | Some(Keyword::Case)
            | Some(Keyword::Default)
            | Some(Keyword::Start)
            | Some(Keyword::End)
            | Some(Keyword::Sizeof)
            | Some(Keyword::Offset)
            | Some(Keyword::Defined) => {
                let s = crate::parse::stmt::parse_statement_top(p)?;
                Some(TopItem::Stmt(s))
            }
            // Primitive type → variable or function decl.
            Some(_) if super::type_::at_type(p) => parse_global_decl(p),
            // Other keywords slip through to expression-stmt.
            Some(_) => {
                let s = crate::parse::stmt::parse_statement_top(p)?;
                Some(TopItem::Stmt(s))
            }
            None => {
                // bare ident at file scope. Could be:
                //   - `IDENT:` — label (rejected at file scope per §5.1)
                //   - `IDENT IDENT …` or `IDENT *…` — decl whose type is
                //     a user-defined class (e.g. `cvar_t v;`,
                //     `cvar_t *Cvar_FindVar(...)`)
                //   - `IDENT(…)` — function-call expression statement
                //   - anything else — expression statement
                if matches!(p.peek_at(1), TokenKind::Colon) {
                    let pos = p.current_pos();
                    p.bump(); p.bump();
                    p.error_at(pos, "no-global-labels", "No global labels at ");
                    return Some(TopItem::Empty);
                }
                if looks_like_named_type_decl(p) {
                    return parse_global_decl(p);
                }
                let s = crate::parse::stmt::parse_statement_top(p)?;
                Some(TopItem::Stmt(s))
            }
        },
        _ => {
            // Anything else: treat as a top-level statement.
            let s = crate::parse::stmt::parse_statement_top(p)?;
            Some(TopItem::Stmt(s))
        }
    }
}

// ============================================================
// Preprocessor
// ============================================================

fn parse_preprocessor(p: &mut Parser) -> Option<PpDirective> {
    let _hash_pos = p.current_pos();
    p.bump(); // #
    let name = match p.peek().clone() {
        TokenKind::Ident(s) => {
            p.bump();
            s
        }
        _ => {
            p.error_at(p.current_pos(), "expecting-ident", "Expecting identifier at ");
            return None;
        }
    };
    let dir = match name.as_str() {
        "include" => {
            let s = take_string_literal(p);
            PpDirective::Include(s)
        }
        "define" => {
            let id_line = p.current_pos().line;
            let id = match p.peek().clone() {
                TokenKind::Ident(s) => { p.bump(); s }
                _ => {
                    p.error_at(p.current_pos(), "expecting-ident", "Expecting identifier at ");
                    String::new()
                }
            };
            // Body: take everything on the same logical line as the
            // `#define` keyword. The lexer drops newlines as tokens
            // but spans carry line numbers — stop when we see a token
            // whose line differs from the directive's own line. Also
            // stop at `#`, `;` and EOF for safety.
            let mut body = String::new();
            while !p.at_eof()
                && !matches!(p.peek(), TokenKind::Hash | TokenKind::Semicolon)
                && p.current_pos().line == id_line
            {
                let t = p.bump();
                if !body.is_empty() { body.push(' '); }
                body.push_str(&token_text(&t.kind));
            }
            PpDirective::Define { name: id, body }
        }
        "ifdef" => {
            let id = take_ident(p);
            PpDirective::Ifdef(id)
        }
        "ifndef" => {
            let id = take_ident(p);
            PpDirective::Ifndef(id)
        }
        "ifaot" => PpDirective::IfAot,
        "ifjit" => PpDirective::IfJit,
        "else" => PpDirective::Else,
        "endif" => PpDirective::EndIf,
        "assert" => {
            // Body left as TODO (types module evaluates it). Capture
            // the expression best-effort.
            let e = crate::parse::expr::parse_expression(p)
                .unwrap_or(Expr {
                    kind: crate::parse::ast::ExprKind::IntLit(0),
                    span: (p.current_pos(), p.current_pos()),
                });
            PpDirective::Assert(e)
        }
        "help_index" => {
            let s = take_string_literal(p);
            PpDirective::HelpIndex(s)
        }
        "help_file" => {
            let s = take_string_literal(p);
            PpDirective::HelpFile(s)
        }
        // #exe { ... } / unhandled directives.
        other => {
            let mut body = String::new();
            if p.eat(&TokenKind::LBrace) {
                let mut depth: i32 = 1;
                while !p.at_eof() {
                    let t = p.bump();
                    match t.kind {
                        TokenKind::LBrace => { depth += 1; body.push('{'); }
                        TokenKind::RBrace => {
                            depth -= 1;
                            if depth == 0 { break; }
                            body.push('}');
                        }
                        _ => {
                            body.push_str(&token_text(&t.kind));
                            body.push(' ');
                        }
                    }
                }
            }
            PpDirective::Other { name: other.to_string(), body }
        }
    };
    Some(dir)
}

fn take_string_literal(p: &mut Parser) -> String {
    if let TokenKind::StrLit(bytes) = p.peek().clone() {
        p.bump();
        String::from_utf8_lossy(&bytes).into_owned()
    } else {
        p.error_at(p.current_pos(), "expecting-string", "Expecting string at ");
        String::new()
    }
}

fn take_ident(p: &mut Parser) -> String {
    if let TokenKind::Ident(s) = p.peek().clone() {
        p.bump();
        s
    } else {
        p.error_at(p.current_pos(), "expecting-ident", "Expecting identifier at ");
        String::new()
    }
}

fn token_text(k: &TokenKind) -> String {
    match k {
        TokenKind::Ident(s) => s.clone(),
        TokenKind::IntLit(n) => n.to_string(),
        TokenKind::FloatLit(f) => f.to_string(),
        TokenKind::CharLit(c) => format!("'\\x{:x}'", c),
        TokenKind::StrLit(b) => {
            format!("\"{}\"", String::from_utf8_lossy(b))
        }
        other => other.spelling().to_string(),
    }
}

// ============================================================
// Global declaration: variable, function, or modifier-led.
// ============================================================

/// Heuristic: does the cursor look like `IDENT *... IDENT` (a decl
/// using a named class type)? We use this to route bare-ident
/// top-level positions to the decl parser instead of letting them
/// fall through to expression-stmt. The Ident at peek(0) is the
/// proposed class name; we walk through any `*`s and check that
/// peek+N is another non-keyword Ident (the variable / function
/// name being declared).
fn looks_like_named_type_decl(p: &Parser) -> bool {
    // peek(0) is already known to be a non-keyword Ident.
    let mut offset = 1usize;
    while matches!(p.peek_at(offset), TokenKind::Star) {
        offset += 1;
    }
    if let TokenKind::Ident(s) = p.peek_at(offset) {
        return lookup_keyword(s).is_none();
    }
    false
}

fn parse_global_decl(p: &mut Parser) -> Option<TopItem> {
    let start = p.current_pos();
    let modifiers = parse_modifiers(p);
    // After modifiers, either: a class/union, a fwd-class via extern,
    // or a regular type-name.
    if p.at_keyword(Keyword::Class) || p.at_keyword(Keyword::Union) {
        let cls = parse_class_or_union(p, /*is_extern=*/ has_modifier(&modifiers, Modifier::Extern))?;
        return Some(TopItem::Class(cls));
    }
    let ty = match super::type_::parse_type_allow_named(p) {
        Some(t) => t,
        None => {
            // Recover: skip a token to make progress
            if !p.at_eof() { p.bump(); }
            return None;
        }
    };
    // First declarator name.
    let name = match p.peek().clone() {
        TokenKind::Ident(s) if lookup_keyword(&s).is_none() => {
            p.bump();
            s
        }
        TokenKind::Ident(s) => {
            // Ident IS a HolyC keyword — give a specific message
            // instead of the generic "Expecting identifier".
            p.error_at(
                p.current_pos(),
                "keyword-as-name",
                format!(
                    "`{s}` is a HolyC keyword and can't be used as a \
                     variable / function name; rename it"
                ),
            );
            return None;
        }
        _ => {
            p.error_at(p.current_pos(), "expecting-ident", "Expecting identifier at ");
            return None;
        }
    };
    // Function vs variable: peek `(`.
    if matches!(p.peek(), TokenKind::LParen) {
        p.bump();
        let params = super::type_::parse_param_list(p);
        if !p.eat(&TokenKind::RParen) {
            p.error_at(p.current_pos(), "missing-rparen", "Missing ')' at ");
        }
        let body = if matches!(p.peek(), TokenKind::LBrace) {
            Some(parse_fn_body(p))
        } else {
            p.eat(&TokenKind::Semicolon);
            None
        };
        return Some(TopItem::Function(FunctionDef {
            modifiers,
            ret_type: ty,
            name,
            params,
            body,
            span: (start, p.current_pos()),
        }));
    }
    // Variable: parse array dims, init, and possibly more decls.
    let array_dims = parse_array_dims(p);
    let init = parse_initializer_opt(p);
    let mut first = VarDecl {
        modifiers: modifiers.clone(),
        ty: ty.clone(),
        name,
        array_dims,
        init,
        reg_name: None,
        span: (start, p.current_pos()),
    };
    if !matches!(p.peek(), TokenKind::Comma) {
        p.eat(&TokenKind::Semicolon);
        return Some(TopItem::Variable(first));
    }
    // Multi-decl. §5.1/§4.3: at file scope, this is the bug — emit
    // `compiler-multi-decl-global` unless the bug-compat flag allows.
    let multi_pos = p.current_pos();
    if !p.config.allow_multi_decl_globals {
        p.error_at(
            multi_pos,
            "compiler-multi-decl-global",
            "Multi-variable declaration not allowed at file scope ",
        );
    }
    let mut decls = vec![first.clone()];
    while p.eat(&TokenKind::Comma) {
        let dstart = p.current_pos();
        let dname = match p.peek().clone() {
            TokenKind::Ident(s) if lookup_keyword(&s).is_none() => { p.bump(); s }
            _ => {
                p.error_at(p.current_pos(), "expecting-ident", "Expecting identifier at ");
                break;
            }
        };
        let array_dims = parse_array_dims(p);
        let init = parse_initializer_opt(p);
        decls.push(VarDecl {
            modifiers: modifiers.clone(),
            ty: ty.clone(),
            name: dname,
            array_dims,
            init,
            reg_name: None,
            span: (dstart, p.current_pos()),
        });
    }
    p.eat(&TokenKind::Semicolon);
    // Suppress unused-variable warning for `first`; it lives on in `decls`.
    let _ = &mut first;
    Some(TopItem::GlobalDeclList(decls))
}

fn parse_modifiers(p: &mut Parser) -> Vec<Modifier> {
    let mut mods = Vec::new();
    loop {
        if let TokenKind::Ident(s) = p.peek().clone() {
            match lookup_keyword(&s) {
                Some(Keyword::Static) => { p.bump(); mods.push(Modifier::Static); }
                Some(Keyword::Public) => { p.bump(); mods.push(Modifier::Public); }
                Some(Keyword::Interrupt) => { p.bump(); mods.push(Modifier::Interrupt); }
                Some(Keyword::Extern) => {
                    p.bump();
                    mods.push(Modifier::Extern);
                    // Optional `"sym"` after extern (extern "C" foo)
                    if matches!(p.peek(), TokenKind::StrLit(_)) {
                        p.bump();
                    }
                }
                Some(Keyword::ExternUnderscore) => {
                    p.bump();
                    mods.push(Modifier::ExternUnderscore);
                    if let TokenKind::Ident(_) = p.peek() {
                        p.bump();
                    }
                }
                Some(Keyword::Import) => { p.bump(); mods.push(Modifier::Import); }
                Some(Keyword::ImportUnderscore) => {
                    p.bump();
                    mods.push(Modifier::ImportUnderscore);
                    if matches!(p.peek(), TokenKind::StrLit(_)) {
                        p.bump();
                    }
                }
                Some(Keyword::Intern) => {
                    p.bump();
                    mods.push(Modifier::Intern);
                    // _intern N type — N is a const-expr; consume it best-effort.
                    if matches!(p.peek(), TokenKind::IntLit(_)) {
                        p.bump();
                    }
                }
                Some(Keyword::Lock) => { p.bump(); mods.push(Modifier::Lock); }
                Some(Keyword::Lastclass) => { p.bump(); mods.push(Modifier::Lastclass); }
                Some(Keyword::Noreg) => { p.bump(); mods.push(Modifier::Noreg); }
                Some(Keyword::Reg) => {
                    p.bump();
                    mods.push(Modifier::Reg);
                    // optional REG_NAME ident
                    if let TokenKind::Ident(s) = p.peek() {
                        // Don't eat type keywords as reg names.
                        if lookup_keyword(s).is_none() || matches!(lookup_keyword(s), None) {
                            // No way to know without a register table;
                            // accept any ident that's also not a primitive type.
                            let kw = lookup_keyword(s);
                            if matches!(kw, None) {
                                p.bump();
                            }
                        }
                    }
                }
                Some(Keyword::Argpop) => { p.bump(); mods.push(Modifier::Argpop); }
                Some(Keyword::Noargpop) => { p.bump(); mods.push(Modifier::Noargpop); }
                Some(Keyword::Nostkchk) => { p.bump(); mods.push(Modifier::Nostkchk); }
                _ => break,
            }
        } else {
            break;
        }
    }
    mods
}

fn has_modifier(mods: &[Modifier], m: Modifier) -> bool {
    mods.iter().any(|x| *x == m)
}

fn parse_array_dims(p: &mut Parser) -> Vec<Option<Expr>> {
    let mut dims = Vec::new();
    while p.eat(&TokenKind::LBracket) {
        if p.eat(&TokenKind::RBracket) {
            dims.push(None);
        } else {
            let e = crate::parse::expr::parse_expression(p);
            if !p.eat(&TokenKind::RBracket) {
                p.error_at(p.current_pos(), "missing-rbracket", "Missing ']' at ");
            }
            dims.push(e);
        }
    }
    dims
}

fn parse_initializer_opt(p: &mut Parser) -> Option<Initializer> {
    if !p.eat(&TokenKind::Eq) { return None; }
    Some(parse_initializer(p))
}

fn parse_initializer(p: &mut Parser) -> Initializer {
    if p.eat(&TokenKind::LBrace) {
        let mut items = Vec::new();
        if !matches!(p.peek(), TokenKind::RBrace) {
            loop {
                items.push(parse_initializer(p));
                if !p.eat(&TokenKind::Comma) { break; }
                if matches!(p.peek(), TokenKind::RBrace) { break; }
            }
        }
        if !p.eat(&TokenKind::RBrace) {
            p.error_at(p.current_pos(), "missing-rbrace", "Missing '}' at ");
        }
        Initializer::Aggregate(items)
    } else {
        let e = crate::parse::expr::parse_expression(p)
            .unwrap_or(Expr {
                kind: crate::parse::ast::ExprKind::IntLit(0),
                span: (p.current_pos(), p.current_pos()),
            });
        Initializer::Single(e)
    }
}

fn parse_fn_body(p: &mut Parser) -> Vec<Stmt> {
    let mut body = Vec::new();
    // expect `{`
    if !p.eat(&TokenKind::LBrace) {
        return body;
    }
    while !matches!(p.peek(), TokenKind::RBrace | TokenKind::Eof) {
        match crate::parse::stmt::parse_statement(p) {
            Some(s) => body.push(s),
            None => {
                if !p.at_eof() { p.bump(); }
            }
        }
    }
    p.eat(&TokenKind::RBrace);
    body
}

// ============================================================
// Class / Union
// ============================================================

fn parse_class_or_union(p: &mut Parser, is_extern: bool) -> Option<ClassDef> {
    let start = p.current_pos();
    let is_union = p.at_keyword(Keyword::Union);
    p.bump(); // class | union
    let name = match p.peek().clone() {
        TokenKind::Ident(s) if lookup_keyword(&s).is_none() => { p.bump(); s }
        _ => {
            p.error_at(p.current_pos(), "expecting-ident", "Expecting identifier at ");
            return None;
        }
    };
    // Optional `: BaseClass`
    let base = if p.eat(&TokenKind::Colon) {
        match p.peek().clone() {
            TokenKind::Ident(s) => { p.bump(); Some(s) }
            _ => {
                p.error_at(p.current_pos(), "expecting-ident", "Expecting identifier at ");
                None
            }
        }
    } else {
        None
    };
    // Forward decl: `class Foo;` (only with extern)
    if matches!(p.peek(), TokenKind::Semicolon) {
        p.bump();
        if !is_extern {
            // TempleOS rejects a body-less class; record as empty class.
        }
        return Some(ClassDef {
            name,
            base,
            members: Vec::new(),
            span: (start, p.current_pos()),
            is_union,
        });
    }
    if !p.eat(&TokenKind::LBrace) {
        p.error_at(p.current_pos(), "expecting-lbrace", "Expecting '{' at ");
        return None;
    }
    let members = parse_class_body(p);
    if !p.eat(&TokenKind::RBrace) {
        p.error_at(p.current_pos(), "missing-rbrace", "Missing '}' at ");
    }
    p.eat(&TokenKind::Semicolon);
    Some(ClassDef {
        name,
        base,
        members,
        span: (start, p.current_pos()),
        is_union,
    })
}

fn parse_class_body(p: &mut Parser) -> Vec<VarDecl> {
    let mut out = Vec::new();
    while !matches!(p.peek(), TokenKind::RBrace | TokenKind::Eof) {
        let mstart = p.current_pos();
        let modifiers = parse_modifiers(p);
        let (ty, embedded_name) = match super::type_::parse_type_allow_named_with_inline_name(p) {
            Some(p) => p,
            None => {
                // recover to ;
                p.recover_to_semicolon();
                continue;
            }
        };
        // Function-pointer field with embedded name: synthesize a
        // VarDecl directly, eat the trailing `;`, and continue.
        if let Some(name) = embedded_name {
            let array_dims = parse_array_dims(p);
            let init = parse_initializer_opt(p);
            out.push(VarDecl {
                modifiers: modifiers.clone(),
                ty: ty.clone(),
                name,
                array_dims,
                init,
                reg_name: None,
                span: (mstart, p.current_pos()),
            });
            // Multi-decl after fn-pointer fields is not idiomatic;
            // accept a trailing `;` and move on.
            p.eat(&TokenKind::Semicolon);
            continue;
        }
        // Could be a member function: parse declarator(s).
        if let TokenKind::Ident(_n) = p.peek().clone() {
            let n = if let TokenKind::Ident(s) = p.bump().kind { s } else { String::new() };
            // Member function?
            if matches!(p.peek(), TokenKind::LParen) {
                p.bump();
                let _params = super::type_::parse_param_list(p);
                p.eat(&TokenKind::RParen);
                if matches!(p.peek(), TokenKind::LBrace) {
                    // capture body but discard (members are vars in
                    // current AST). Reuse fn body parse to advance.
                    let _ = parse_fn_body(p);
                } else {
                    p.eat(&TokenKind::Semicolon);
                }
                // Not stored in members vector — out of scope of VarDecl.
                continue;
            }
            // Variable member, possibly multi-decl (allowed inside class).
            let array_dims = parse_array_dims(p);
            let init = parse_initializer_opt(p);
            out.push(VarDecl {
                modifiers: modifiers.clone(),
                ty: ty.clone(),
                name: n,
                array_dims,
                init,
                reg_name: None,
                span: (mstart, p.current_pos()),
            });
            while p.eat(&TokenKind::Comma) {
                let dstart = p.current_pos();
                let nm = match p.peek().clone() {
                    TokenKind::Ident(s) => { p.bump(); s }
                    _ => break,
                };
                let array_dims = parse_array_dims(p);
                let init = parse_initializer_opt(p);
                out.push(VarDecl {
                    modifiers: modifiers.clone(),
                    ty: ty.clone(),
                    name: nm,
                    array_dims,
                    init,
                    reg_name: None,
                    span: (dstart, p.current_pos()),
                });
            }
            p.eat(&TokenKind::Semicolon);
        } else {
            p.error_at(p.current_pos(), "expecting-ident", "Expecting identifier at ");
            p.recover_to_semicolon();
        }
    }
    out
}

// ============================================================
// Local declaration (used by stmt::parse_statement for type-keyword path)
// ============================================================

pub fn parse_local_decl(p: &mut Parser) -> Option<Vec<VarDecl>> {
    let start = p.current_pos();
    let modifiers = parse_modifiers(p);
    let ty = match super::type_::parse_type_allow_named(p) {
        Some(t) => t,
        None => return None,
    };
    let mut decls = Vec::new();
    loop {
        let dstart = p.current_pos();
        let name = match p.peek().clone() {
            TokenKind::Ident(s) if lookup_keyword(&s).is_none() => {
                if super::type_::is_reserved_name(&s) {
                    p.error_at(dstart, "reserved-name-collision",
                        super::type_::format_reserved_message(&s));
                }
                p.bump();
                s
            }
            TokenKind::Ident(s) => {
                // Ident IS a HolyC keyword (e.g. `end`, `start`,
                // `case`). Specific message — see parse_global_decl
                // for rationale.
                p.error_at(
                    dstart,
                    "keyword-as-name",
                    format!(
                        "`{s}` is a HolyC keyword and can't be used as a \
                         variable name; rename it"
                    ),
                );
                return None;
            }
            _ => {
                p.error_at(p.current_pos(), "expecting-ident", "Expecting identifier at ");
                return None;
            }
        };
        let array_dims = parse_array_dims(p);
        let init = parse_initializer_opt(p);
        decls.push(VarDecl {
            modifiers: modifiers.clone(),
            ty: ty.clone(),
            name,
            array_dims,
            init,
            reg_name: None,
            span: (dstart, p.current_pos()),
        });
        if !p.eat(&TokenKind::Comma) { break; }
    }
    p.eat(&TokenKind::Semicolon);
    let _ = (start, &start);
    Some(decls)
}

// silence possibly-unused warning across feature combinations
#[allow(dead_code)]
fn _pos_unused(_: Pos) {}

// silence param unused
#[allow(dead_code)]
fn _param_unused(_: Param, _: TypeRef) {}
