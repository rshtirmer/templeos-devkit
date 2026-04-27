//! Parser cursor + helpers + error recovery surface. Built on the
//! token stream produced by `lex::lex`. Sub-modules (`expr`, `stmt`,
//! `decl`, `type_`) all consume `&mut Parser` and are free to call
//! into each other through the public methods on this struct.

use crate::diag::{Diag, Severity};
use crate::lex::{Pos, Token, TokenKind};

/// Bug-compat configuration. Defaults match what the live VM
/// actually accepts via ExePutS (the JIT path our dev loop uses) —
/// not necessarily what parse-spec literally says. Where the corpus
/// disagrees with the spec, the corpus wins; the spec then has the
/// flag for opt-in strictness.
#[derive(Clone, Copy, Debug)]
pub struct ParseConfig {
    /// Allow `F64 a, b;` at file scope. parse-spec §4.3 says
    /// PrsGlblVarLst (PrsVar.HC:222) rejects, but ExePutS routes
    /// top-level decls through the function-scope path that DOES
    /// handle multi-decl. Corpus snippets 143/166 confirm. Default
    /// true; set false to match the AOT compile path.
    pub allow_multi_decl_globals: bool,
    /// Allow `continue;` keyword. TempleOS has no `continue`.
    pub allow_continue_keyword: bool,
    /// Allow `(TYPE)expr` C-style cast. TempleOS uses postfix.
    pub allow_c_style_cast: bool,
    /// Allow `for (I64 i = 0; ...)` at file scope.
    pub allow_for_decl_top_level: bool,
}

impl Default for ParseConfig {
    fn default() -> Self {
        Self {
            allow_multi_decl_globals: true,
            allow_continue_keyword: false,
            allow_c_style_cast: false,
            allow_for_decl_top_level: false,
        }
    }
}

pub struct Parser {
    file: String,
    tokens: Vec<Token>,
    cursor: usize,
    pub diags: Vec<Diag>,
    pub config: ParseConfig,
}

impl Parser {
    pub fn new(file: impl Into<String>, tokens: Vec<Token>, config: ParseConfig) -> Self {
        Self {
            file: file.into(),
            tokens,
            cursor: 0,
            diags: Vec::new(),
            config,
        }
    }

    pub fn file_name(&self) -> &str {
        &self.file
    }

    // ---- cursor primitives ----
    pub fn peek(&self) -> &TokenKind {
        &self.peek_token().kind
    }

    pub fn peek_token(&self) -> &Token {
        // Tokens always end with Eof; cursor never exceeds last index.
        &self.tokens[self.cursor.min(self.tokens.len() - 1)]
    }

    pub fn peek_at(&self, offset: usize) -> &TokenKind {
        let idx = (self.cursor + offset).min(self.tokens.len() - 1);
        &self.tokens[idx].kind
    }

    pub fn at_eof(&self) -> bool {
        matches!(self.peek(), TokenKind::Eof)
    }

    pub fn current_pos(&self) -> Pos {
        self.peek_token().start
    }

    /// Consume and return the current token.
    pub fn bump(&mut self) -> Token {
        let t = self.tokens[self.cursor].clone();
        if !matches!(t.kind, TokenKind::Eof) {
            self.cursor += 1;
        }
        t
    }

    /// True if the current token equals the expected discriminant.
    pub fn at(&self, kind: &TokenKind) -> bool {
        std::mem::discriminant(self.peek()) == std::mem::discriminant(kind)
    }

    /// If the current token equals `kind`, consume it and return true.
    pub fn eat(&mut self, kind: &TokenKind) -> bool {
        if self.at(kind) {
            self.bump();
            true
        } else {
            false
        }
    }

    /// Expect the current token to be `kind`; if so consume and return
    /// it, otherwise emit a diagnostic and return None.
    pub fn expect(&mut self, kind: &TokenKind, rule: &'static str) -> Option<Token> {
        if self.at(kind) {
            Some(self.bump())
        } else {
            let pos = self.current_pos();
            let actual = self.peek().spelling().to_string();
            self.error_at(pos, rule, format!(
                "expected `{}`, found `{}`",
                kind.spelling(),
                actual,
            ));
            None
        }
    }

    /// Convenience for keyword-resolved idents. Returns true if the
    /// current token is `Ident(s)` with `s` matching the given keyword.
    pub fn at_keyword(&self, kw: crate::lex::Keyword) -> bool {
        if let TokenKind::Ident(s) = self.peek() {
            crate::lex::lookup_keyword(s) == Some(kw)
        } else {
            false
        }
    }

    pub fn eat_keyword(&mut self, kw: crate::lex::Keyword) -> bool {
        if self.at_keyword(kw) {
            self.bump();
            true
        } else {
            false
        }
    }

    // ---- diagnostics ----
    pub fn error_at(&mut self, pos: Pos, rule: &'static str, msg: impl Into<String>) {
        self.diags.push(Diag {
            file: self.file.clone(),
            line: pos.line,
            col: pos.col,
            severity: Severity::Error,
            rule,
            message: msg.into(),
        });
    }

    pub fn warn_at(&mut self, pos: Pos, rule: &'static str, msg: impl Into<String>) {
        self.diags.push(Diag {
            file: self.file.clone(),
            line: pos.line,
            col: pos.col,
            severity: Severity::Warning,
            rule,
            message: msg.into(),
        });
    }

    /// Skip tokens until we hit `;` (consume it) or EOF. Standard
    /// statement-level recovery (parse-spec §6.2).
    pub fn recover_to_semicolon(&mut self) {
        while !self.at_eof() {
            let t = self.bump();
            if matches!(t.kind, TokenKind::Semicolon) { return; }
        }
    }

    /// Skip tokens until matched closing `}` (consume) or EOF. Used
    /// when a block-level construct is unrecoverable.
    pub fn recover_to_rbrace(&mut self) {
        let mut depth = 0;
        while !self.at_eof() {
            let t = self.bump();
            match t.kind {
                TokenKind::LBrace => depth += 1,
                TokenKind::RBrace => {
                    if depth == 0 { return; }
                    depth -= 1;
                }
                _ => {}
            }
        }
    }

    /// Save the cursor position. Used by sub-parsers that need to
    /// backtrack (PrsVarLst's lex-checkpoint pattern, parse-spec §8.3).
    pub fn checkpoint(&self) -> Checkpoint {
        Checkpoint { cursor: self.cursor, diag_count: self.diags.len() }
    }

    pub fn restore(&mut self, c: Checkpoint) {
        self.cursor = c.cursor;
        self.diags.truncate(c.diag_count);
    }
}

/// Opaque token + diag-count snapshot for backtracking.
#[derive(Clone, Copy, Debug)]
pub struct Checkpoint {
    cursor: usize,
    diag_count: usize,
}
