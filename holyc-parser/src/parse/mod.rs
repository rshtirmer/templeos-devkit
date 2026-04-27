//! Parser — recursive descent over `lex::Token`s, producing an AST
//! described in `ast`. Spec lives in `docs/parse-spec.md`.
//!
//! Public API:
//! - `parse_module(file, src, config) -> (Module, Vec<Diag>)`
//!
//! The parser reuses `lex::lex` internally; callers don't typically
//! invoke the lexer separately unless they want token-level diagnostics.

pub mod ast;
pub mod decl;
pub mod expr;
pub mod parser;
pub mod stmt;
pub mod type_;

pub use ast::{Module, TopItem};
pub use parser::{ParseConfig, Parser};

use crate::diag::Diag;

/// Parse a HolyC source file. Always returns a `Module` (possibly
/// containing `TopItem::Empty` placeholders for items that failed
/// to parse) plus a `Vec<Diag>`.
pub fn parse_module(
    file: impl Into<String>,
    src: &str,
    config: ParseConfig,
) -> (Module, Vec<Diag>) {
    let file: String = file.into();
    let (tokens, mut diags) = crate::lex::lex(&file, src);
    let mut p = Parser::new(file, tokens, config);
    let mut items = Vec::new();
    while !p.at_eof() {
        match decl::parse_top_item(&mut p) {
            Some(item) => items.push(item),
            None => {
                // Avoid infinite loop on stub paths that don't consume.
                if !p.at_eof() {
                    p.bump();
                }
            }
        }
    }
    diags.append(&mut p.diags);
    (Module { items }, diags)
}
