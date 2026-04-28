//! Statement parser. Implements parse-spec §3 (PrsStmt) for HolyC.
//!
//! Calls `expr::parse_expression`, `decl::parse_local_decl`, and
//! `type_::at_type`. Tolerates `expr::parse_expression` returning
//! `None` (ExprCoder may still be a stub).

use crate::lex::{Keyword, TokenKind, lookup_keyword};
use crate::parse::ast::{CaseValue, Expr, Stmt, StmtKind};
use crate::parse::parser::Parser;

/// Function-scope parse: dispatches to all the keyword + composite
/// statement forms. Returns `None` when the dispatcher couldn't
/// recover anything.
pub fn parse_statement(p: &mut Parser) -> Option<Stmt> {
    parse_statement_inner(p, /*at_file_scope=*/ false)
}

/// File-scope variant — applies §5.1 rejections (return/label).
pub fn parse_statement_top(p: &mut Parser) -> Option<Stmt> {
    parse_statement_inner(p, /*at_file_scope=*/ true)
}

fn parse_statement_inner(p: &mut Parser, at_file_scope: bool) -> Option<Stmt> {
    let start = p.current_pos();

    match p.peek().clone() {
        TokenKind::Eof => None,
        TokenKind::Semicolon => {
            p.bump();
            Some(Stmt { kind: StmtKind::Empty, span: (start, p.current_pos()) })
        }
        TokenKind::LBrace => Some(parse_block(p, start)),
        TokenKind::Ident(name) => {
            // Contextual keywords: `start` / `end` are sub-switch
            // markers (parse-spec §3) only inside a switch body AND
            // only when followed by `:` (label-like syntax). Outside
            // a switch — or in any other position — they fall through
            // to the regular identifier path so kernel code like
            // `I64 start = 0;` and `start = 5;` parses cleanly.
            if (name == "start" || name == "end")
                && p.switch_depth > 0
                && matches!(p.peek_at(1), TokenKind::Colon)
            {
                return parse_sub_switch_marker(p, &name, start);
            }
            // Contextual keywords: `reg` / `noreg` are register-
            // allocation modifiers only when leading a declaration.
            // Otherwise they're plain identifiers (kernel asm-adjacent
            // code uses `reg` as a variable name).
            if (name == "reg" || name == "noreg")
                && crate::parse::decl::looks_like_decl_after_reg_modifier(p)
            {
                return parse_local_decl_stmt(p, at_file_scope, start);
            }
            match lookup_keyword(&name) {
                Some(kw) => parse_keyword_stmt(p, kw, at_file_scope, start),
                None => {
                    // Could be: label `IDENT :`, local decl with named-
                    // class type (e.g. `cvar_t *var;`), or expression-stmt.
                    if matches!(p.peek_at(1), TokenKind::Colon) {
                        parse_label_stmt(p, name, at_file_scope, start)
                    } else if looks_like_named_type_local_decl(p) {
                        parse_local_decl_stmt(p, at_file_scope, start)
                    } else {
                        parse_expr_stmt(p, start)
                    }
                }
            }
        },
        TokenKind::StrLit(_) | TokenKind::CharLit(_) => {
            // Implicit Print/PutChars at any scope (parse-spec §5.13).
            // After the leading literal, accept any comma-separated
            // expression list before the `;`. We model this as
            // `ExprKind::Comma([head, args...])`; downstream typeck
            // can lower to a Print/PutChars call.
            parse_implicit_print_stmt(p, start)
        }
        _ => parse_expr_stmt(p, start),
    }
}

fn parse_block(p: &mut Parser, start: crate::lex::Pos) -> Stmt {
    p.bump(); // {
    let mut body = Vec::new();
    while !matches!(p.peek(), TokenKind::RBrace | TokenKind::Eof) {
        match parse_statement(p) {
            Some(s) => body.push(s),
            None => {
                // recovery: bump one token to make progress
                if !p.at_eof() { p.bump(); }
            }
        }
    }
    if !p.eat(&TokenKind::RBrace) {
        p.error_at(p.current_pos(), "expecting-rbrace", "Missing '}' at ");
    }
    Stmt { kind: StmtKind::Block(body), span: (start, p.current_pos()) }
}

fn parse_expr_stmt(p: &mut Parser, start: crate::lex::Pos) -> Option<Stmt> {
    let e = crate::parse::expr::parse_expression(p);
    // `parse_expression` may have already eaten the `;`. Try to eat
    // it; if not present, it's not fatal — diag was emitted upstream.
    p.eat(&TokenKind::Semicolon);
    let expr = e?;
    Some(Stmt { kind: StmtKind::Expr(expr), span: (start, p.current_pos()) })
}

/// `"%d\n", 5;` — implicit Print/PutChars (parse-spec §5.13). The
/// generic expression parser would mis-handle the trailing comma;
/// we consume the head literal directly and walk the comma list.
fn parse_implicit_print_stmt(p: &mut Parser, start: crate::lex::Pos) -> Option<Stmt> {
    use crate::parse::ast::{Expr, ExprKind};
    let head_tok = p.bump();
    let head_kind = match head_tok.kind {
        TokenKind::StrLit(bytes) => ExprKind::StrLit(bytes),
        TokenKind::CharLit(v) => ExprKind::CharLit(v),
        _ => unreachable!("dispatcher guaranteed StrLit or CharLit"),
    };
    let head = Expr { kind: head_kind, span: (head_tok.start, head_tok.end) };

    let mut args: Vec<Expr> = vec![head];
    while p.eat(&TokenKind::Comma) {
        match crate::parse::expr::parse_expression(p) {
            Some(e) => args.push(e),
            None => break,
        }
    }
    p.eat(&TokenKind::Semicolon);
    let expr = if args.len() == 1 {
        args.pop().unwrap()
    } else {
        Expr { kind: ExprKind::Comma(args), span: (start, p.current_pos()) }
    };
    Some(Stmt { kind: StmtKind::Expr(expr), span: (start, p.current_pos()) })
}

/// Parse a `start :` / `end :` sub-switch marker. The dispatcher
/// only routes here when we're inside a switch body (parse-spec §3).
fn parse_sub_switch_marker(
    p: &mut Parser,
    name: &str,
    start: crate::lex::Pos,
) -> Option<Stmt> {
    p.bump(); // ident
    if matches!(p.peek(), TokenKind::Colon) {
        p.bump();
    }
    let kind = if name == "start" {
        StmtKind::SubSwitchStart
    } else {
        StmtKind::SubSwitchEnd
    };
    Some(Stmt { kind, span: (start, p.current_pos()) })
}

/// `IDENT *... IDENT` at function-scope start of statement looks
/// like a local decl whose type is a user-defined class. Mirrors
/// `decl::looks_like_named_type_decl`.
fn looks_like_named_type_local_decl(p: &Parser) -> bool {
    let mut offset = 1usize;
    while matches!(p.peek_at(offset), TokenKind::Star) {
        offset += 1;
    }
    if let TokenKind::Ident(s) = p.peek_at(offset) {
        return lookup_keyword(s).is_none();
    }
    false
}

fn parse_label_stmt(
    p: &mut Parser,
    name: String,
    at_file_scope: bool,
    start: crate::lex::Pos,
) -> Option<Stmt> {
    let pos = p.current_pos();
    p.bump(); // ident
    p.bump(); // :
    if at_file_scope {
        p.error_at(pos, "no-global-labels", "No global labels at ");
        return None;
    }
    Some(Stmt { kind: StmtKind::Label(name), span: (start, p.current_pos()) })
}

fn parse_keyword_stmt(
    p: &mut Parser,
    kw: Keyword,
    at_file_scope: bool,
    start: crate::lex::Pos,
) -> Option<Stmt> {
    use Keyword as K;
    // Type-keyword → local decl.
    if super::type_::at_type(p) {
        return parse_local_decl_stmt(p, at_file_scope, start);
    }
    match kw {
        // Storage modifiers introducing a local decl. Note: `reg` /
        // `noreg` are no longer keywords (they're contextual idents);
        // their decl-prefix routing happens in `parse_statement_inner`.
        K::Static => parse_local_decl_stmt(p, at_file_scope, start),

        K::If => parse_if(p, start),
        K::While => parse_while(p, start),
        K::Do => parse_do_while(p, start),
        K::For => parse_for(p, at_file_scope, start),
        K::Switch => parse_switch(p, start),
        K::Break => {
            p.bump();
            p.eat(&TokenKind::Semicolon);
            Some(Stmt { kind: StmtKind::Break, span: (start, p.current_pos()) })
        }
        K::Return => parse_return(p, at_file_scope, start),
        K::Goto => parse_goto(p, start),
        K::Try => parse_try(p, start),
        K::Lock => {
            p.bump();
            let inner = parse_statement(p)?;
            Some(Stmt { kind: StmtKind::Lock(Box::new(inner)), span: (start, p.current_pos()) })
        }
        K::Asm => parse_asm(p, start),
        K::Case => parse_case(p, start),
        K::Default => {
            p.bump();
            if !p.eat(&TokenKind::Colon) {
                p.error_at(p.current_pos(), "expecting-colon", "Expecting ':' at ");
            }
            Some(Stmt { kind: StmtKind::Default, span: (start, p.current_pos()) })
        }
        // `start` / `end` (sub-switch markers) are no longer keywords —
        // they're contextual idents handled in `parse_statement_inner`
        // when seen inside a switch body.
        // `no_warn` is not actually a TempleOS keyword; skipped here.
        _ => {
            // Fallback: treat as expression-stmt (lets random idents
            // become Expr or recovery happen).
            let _ = at_file_scope;
            parse_expr_stmt(p, start)
        }
    }
}

fn parse_local_decl_stmt(
    p: &mut Parser,
    at_file_scope: bool,
    start: crate::lex::Pos,
) -> Option<Stmt> {
    if at_file_scope {
        // At file scope, decl path runs through decl::parse_top_item;
        // this branch shouldn't usually be hit. Defer to local_decl
        // and surface multi-decl errors as appropriate.
    }
    let decls = crate::parse::decl::parse_local_decl(p)?;
    Some(Stmt { kind: StmtKind::LocalDecl(decls), span: (start, p.current_pos()) })
}

fn parse_if(p: &mut Parser, start: crate::lex::Pos) -> Option<Stmt> {
    p.bump(); // if
    if !p.eat(&TokenKind::LParen) {
        p.error_at(p.current_pos(), "expecting-lparen", "Expecting '(' at ");
    }
    let cond = parse_paren_cond(p);
    let then_branch = Box::new(parse_statement(p)?);
    let else_branch = if p.at_keyword(Keyword::Else) {
        p.bump();
        Some(Box::new(parse_statement(p)?))
    } else {
        None
    };
    Some(Stmt {
        kind: StmtKind::If { cond: cond?, then_branch, else_branch },
        span: (start, p.current_pos()),
    })
}

fn parse_while(p: &mut Parser, start: crate::lex::Pos) -> Option<Stmt> {
    p.bump(); // while
    if !p.eat(&TokenKind::LParen) {
        p.error_at(p.current_pos(), "expecting-lparen", "Expecting '(' at ");
    }
    let cond = parse_paren_cond(p);
    let body = Box::new(parse_statement(p)?);
    Some(Stmt {
        kind: StmtKind::While { cond: cond?, body },
        span: (start, p.current_pos()),
    })
}

fn parse_do_while(p: &mut Parser, start: crate::lex::Pos) -> Option<Stmt> {
    p.bump(); // do
    let body = Box::new(parse_statement(p)?);
    if !p.eat_keyword(Keyword::While) {
        p.error_at(p.current_pos(), "missing-while", "Missing 'while' at");
    }
    if !p.eat(&TokenKind::LParen) {
        p.error_at(p.current_pos(), "expecting-lparen", "Expecting '(' at ");
    }
    let cond = parse_paren_cond(p);
    // §5.5: PrsDoWhile consumes its own trailing semicolon.
    if !p.eat(&TokenKind::Semicolon) {
        p.error_at(p.current_pos(), "missing-semi", "Missing ';' at");
    }
    Some(Stmt {
        kind: StmtKind::DoWhile { body, cond: cond? },
        span: (start, p.current_pos()),
    })
}

fn parse_for(
    p: &mut Parser,
    at_file_scope: bool,
    start: crate::lex::Pos,
) -> Option<Stmt> {
    p.bump(); // for
    if !p.eat(&TokenKind::LParen) {
        p.error_at(p.current_pos(), "expecting-lparen", "Expecting '(' at ");
    }
    // init slot. Either empty (`;`), a type-decl statement, or a
    // comma-separated expression list terminated by `;`. HolyC accepts
    // `for (i = 0, j = 10; ...; ...)` — both init and update clauses
    // can be comma-operator expression lists.
    let init = if matches!(p.peek(), TokenKind::Semicolon) {
        p.bump();
        None
    } else if super::type_::at_type(p) {
        // §5.4: at file scope, a type-decl in init is buggy in
        // TempleOS. We emit an error unless the bug-compat flag
        // `allow_for_decl_top_level` is set.
        if at_file_scope && !p.config.allow_for_decl_top_level {
            let pos = p.current_pos();
            p.error_at(
                pos,
                "for-decl-top-level",
                "Expecting type at ",
            );
        }
        let s = parse_statement(p);
        s.map(Box::new)
    } else {
        // Expression-statement form. Allow comma-operator list.
        let init_start = p.current_pos();
        let e = parse_for_clause_expr_list(p, init_start);
        p.eat(&TokenKind::Semicolon);
        e.map(|expr| Box::new(Stmt {
            kind: StmtKind::Expr(expr),
            span: (init_start, p.current_pos()),
        }))
    };
    // cond slot.
    let cond = if matches!(p.peek(), TokenKind::Semicolon) {
        p.bump();
        None
    } else {
        let e = crate::parse::expr::parse_expression(p);
        p.eat(&TokenKind::Semicolon);
        e
    };
    // update slot — terminated by `)`. Allow comma-operator list.
    let update = if matches!(p.peek(), TokenKind::RParen) {
        None
    } else {
        let upd_start = p.current_pos();
        parse_for_clause_expr_list(p, upd_start)
    };
    if !p.eat(&TokenKind::RParen) {
        p.error_at(p.current_pos(), "missing-rparen", "Missing ')' at ");
    }
    let body = Box::new(parse_statement(p)?);
    Some(Stmt {
        kind: StmtKind::For { init, cond, update, body },
        span: (start, p.current_pos()),
    })
}

/// Parse a comma-separated expression list for a `for(...)` init or
/// update slot. Returns a single bare expression for length 1, or an
/// `ExprKind::Comma` for 2+ entries. Returns `None` only if the very
/// first expression failed to parse.
fn parse_for_clause_expr_list(
    p: &mut Parser,
    start: crate::lex::Pos,
) -> Option<Expr> {
    use crate::parse::ast::ExprKind;
    let first = crate::parse::expr::parse_expression(p)?;
    if !matches!(p.peek(), TokenKind::Comma) {
        return Some(first);
    }
    let mut items = vec![first];
    while p.eat(&TokenKind::Comma) {
        match crate::parse::expr::parse_expression(p) {
            Some(e) => items.push(e),
            None => break,
        }
    }
    Some(Expr {
        kind: ExprKind::Comma(items),
        span: (start, p.current_pos()),
    })
}

fn parse_switch(p: &mut Parser, start: crate::lex::Pos) -> Option<Stmt> {
    p.bump(); // switch
    let square = if p.eat(&TokenKind::LBracket) {
        true
    } else if p.eat(&TokenKind::LParen) {
        false
    } else {
        p.error_at(p.current_pos(), "expecting-lparen", "Expecting '(' at ");
        false
    };
    let scrut = crate::parse::expr::parse_expression(p);
    let close_ok = if square {
        p.eat(&TokenKind::RBracket)
    } else {
        p.eat(&TokenKind::RParen)
    };
    if !close_ok {
        p.error_at(p.current_pos(), "missing-rparen", "Missing ')' at ");
    }
    if !p.eat(&TokenKind::LBrace) {
        p.error_at(p.current_pos(), "expecting-lbrace", "Expecting '{' at ");
    }
    // Track switch nesting so `start` / `end` (contextual keywords)
    // become sub-switch markers only inside a switch body.
    p.switch_depth += 1;
    let mut body = Vec::new();
    while !matches!(p.peek(), TokenKind::RBrace | TokenKind::Eof) {
        match parse_statement(p) {
            Some(s) => body.push(s),
            None => {
                if !p.at_eof() { p.bump(); }
            }
        }
    }
    p.switch_depth -= 1;
    if !p.eat(&TokenKind::RBrace) {
        p.error_at(p.current_pos(), "expecting-rbrace", "Missing '}' at ");
    }
    let scrutinee = scrut.unwrap_or(Expr {
        kind: crate::parse::ast::ExprKind::IntLit(0),
        span: (start, p.current_pos()),
    });
    Some(Stmt {
        kind: StmtKind::Switch { scrutinee, body, square_brackets: square },
        span: (start, p.current_pos()),
    })
}

fn parse_case(p: &mut Parser, start: crate::lex::Pos) -> Option<Stmt> {
    p.bump(); // case
    let mut values = Vec::new();
    // `case :` (auto-increment).
    if matches!(p.peek(), TokenKind::Colon) {
        p.bump();
        values.push(CaseValue::AutoIncrement);
        return Some(Stmt {
            kind: StmtKind::Case(values),
            span: (start, p.current_pos()),
        });
    }
    // Loop accepts a single value or range, possibly comma-separated
    // (TempleOS doesn't actually support comma-cases but we keep the
    // structural list anyway; one entry is the common case).
    loop {
        let lo = match crate::parse::expr::parse_expression(p) {
            Some(e) => e,
            None => break,
        };
        if p.eat(&TokenKind::Ellipsis) || p.eat(&TokenKind::DotDot) {
            let hi = match crate::parse::expr::parse_expression(p) {
                Some(e) => e,
                None => {
                    values.push(CaseValue::Single(lo));
                    break;
                }
            };
            values.push(CaseValue::Range(lo, hi));
        } else {
            values.push(CaseValue::Single(lo));
        }
        if !p.eat(&TokenKind::Comma) { break; }
    }
    if !p.eat(&TokenKind::Colon) {
        p.error_at(p.current_pos(), "expecting-colon", "Expecting ':' at ");
    }
    Some(Stmt {
        kind: StmtKind::Case(values),
        span: (start, p.current_pos()),
    })
}

fn parse_return(
    p: &mut Parser,
    at_file_scope: bool,
    start: crate::lex::Pos,
) -> Option<Stmt> {
    let pos = p.current_pos();
    p.bump(); // return
    if at_file_scope {
        p.error_at(pos, "return-at-file-scope", "Not in fun.  Can't return a val ");
        // skip until ';' for recovery
        p.recover_to_semicolon();
        return None;
    }
    let val = if matches!(p.peek(), TokenKind::Semicolon) {
        None
    } else {
        crate::parse::expr::parse_expression(p)
    };
    p.eat(&TokenKind::Semicolon);
    Some(Stmt { kind: StmtKind::Return(val), span: (start, p.current_pos()) })
}

fn parse_goto(p: &mut Parser, start: crate::lex::Pos) -> Option<Stmt> {
    p.bump(); // goto
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
    p.eat(&TokenKind::Semicolon);
    Some(Stmt { kind: StmtKind::Goto(name), span: (start, p.current_pos()) })
}

fn parse_try(p: &mut Parser, start: crate::lex::Pos) -> Option<Stmt> {
    p.bump(); // try
    if !p.eat(&TokenKind::LBrace) {
        p.error_at(p.current_pos(), "expecting-lbrace", "Expecting '{' at ");
    }
    let mut body = Vec::new();
    while !matches!(p.peek(), TokenKind::RBrace | TokenKind::Eof) {
        if let Some(s) = parse_statement(p) {
            body.push(s);
        } else if !p.at_eof() {
            p.bump();
        }
    }
    p.eat(&TokenKind::RBrace);
    if !p.eat_keyword(Keyword::Catch) {
        p.error_at(p.current_pos(), "missing-catch", "Missing 'catch' at");
    }
    if !p.eat(&TokenKind::LBrace) {
        p.error_at(p.current_pos(), "expecting-lbrace", "Expecting '{' at ");
    }
    let mut catch_body = Vec::new();
    while !matches!(p.peek(), TokenKind::RBrace | TokenKind::Eof) {
        if let Some(s) = parse_statement(p) {
            catch_body.push(s);
        } else if !p.at_eof() {
            p.bump();
        }
    }
    p.eat(&TokenKind::RBrace);
    Some(Stmt {
        kind: StmtKind::Try { body, catch_body },
        span: (start, p.current_pos()),
    })
}

fn parse_asm(p: &mut Parser, start: crate::lex::Pos) -> Option<Stmt> {
    p.bump(); // asm
    let mut text = String::new();
    if p.eat(&TokenKind::LBrace) {
        // Capture tokens between balanced braces as opaque text. We
        // don't have access to the source bytes directly, so we use
        // each token's spelling.
        let mut depth: i32 = 1;
        while !p.at_eof() {
            let t = p.bump();
            match t.kind {
                TokenKind::LBrace => { depth += 1; text.push('{'); }
                TokenKind::RBrace => {
                    depth -= 1;
                    if depth == 0 { break; }
                    text.push('}');
                }
                _ => {
                    text.push_str(t.kind.spelling());
                    text.push(' ');
                }
            }
        }
    } else {
        // single-instruction form: capture until `;`.
        while !p.at_eof() && !matches!(p.peek(), TokenKind::Semicolon) {
            let t = p.bump();
            text.push_str(t.kind.spelling());
            text.push(' ');
        }
        p.eat(&TokenKind::Semicolon);
    }
    Some(Stmt { kind: StmtKind::Asm(text), span: (start, p.current_pos()) })
}

fn parse_paren_cond(p: &mut Parser) -> Option<Expr> {
    let e = crate::parse::expr::parse_expression(p);
    p.eat(&TokenKind::RParen);
    e
}
