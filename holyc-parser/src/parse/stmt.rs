//! Statement parser. To be filled per parse-spec §3 by the StmtDeclCoder
//! subagent. Calls into `expr::parse_expression`,
//! `decl::parse_local_decl`, and `type_::parse_type`.

use crate::parse::ast::Stmt;
use crate::parse::parser::Parser;

/// Parse a single statement from the current position.
/// Returns `None` if recovery skipped tokens without producing a stmt.
pub fn parse_statement(p: &mut Parser) -> Option<Stmt> {
    // STUB — StmtDeclCoder subagent fills this in.
    let pos = p.current_pos();
    p.error_at(pos, "todo-stmt", "statement parser not yet implemented");
    p.recover_to_semicolon();
    None
}
