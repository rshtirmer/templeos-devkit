//! Lexer — tokenizer for HolyC. Backed by `scanner::Scanner`.
//!
//! Public API:
//! - `lex(file, src) -> (Vec<Token>, Vec<Diag>)` — one-shot tokenize.
//! - `Token`, `TokenKind`, `Pos` — token data types.
//! - `keyword::Keyword`, `keyword::lookup` — keyword resolution
//!   the parser uses on `TokenKind::Ident`.
//!
//! See `docs/lex-spec.md` for the underlying design and the 23
//! TempleOS-faithful quirks we reproduce.

pub mod keyword;
pub mod scanner;
pub mod token;

pub use keyword::{Keyword, lookup as lookup_keyword};
pub use token::{Pos, Token, TokenKind};

use crate::diag::Diag;

/// Tokenize `src` (file label `file` for diagnostics). Always
/// terminates with `TokenKind::Eof`.
pub fn lex(file: impl Into<String>, src: &str) -> (Vec<Token>, Vec<Diag>) {
    scanner::Scanner::new(file, src).lex_all()
}
