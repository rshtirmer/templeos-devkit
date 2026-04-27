//! Declaration parser (PrsVar + class/union/extern dispatch).
//! Filled by StmtDeclCoder per parse-spec §4.

use crate::parse::ast::{TopItem, VarDecl};
use crate::parse::parser::Parser;

/// Parse one top-level item (declaration, statement, preprocessor
/// directive, …). Returns `None` at EOF or unrecoverable error.
pub fn parse_top_item(p: &mut Parser) -> Option<TopItem> {
    // STUB.
    if p.at_eof() { return None; }
    let pos = p.current_pos();
    p.error_at(pos, "todo-decl", "top-item parser not yet implemented");
    p.bump();
    None
}

/// Parse a local var decl statement (`F64 x = 1.0;`). Used by
/// `stmt::parse_statement` when the cursor is at a type keyword.
pub fn parse_local_decl(p: &mut Parser) -> Option<Vec<VarDecl>> {
    // STUB.
    let pos = p.current_pos();
    p.error_at(pos, "todo-decl-local", "local-decl parser not yet implemented");
    p.recover_to_semicolon();
    None
}
