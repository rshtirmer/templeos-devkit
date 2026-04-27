//! Expression parser. To be filled per parse-spec §2 by the ExprCoder
//! subagent. The function signatures below are the public surface
//! that `stmt.rs` and `decl.rs` rely on — DO NOT change them without
//! also updating callers.

use crate::parse::ast::Expr;
use crate::parse::parser::Parser;

/// Parse a full expression at HolyC's expression-statement level.
/// Returns `None` if the parser couldn't recover an expression at all
/// (in which case a diagnostic was already emitted).
pub fn parse_expression(p: &mut Parser) -> Option<Expr> {
    // STUB — ExprCoder subagent fills this in (parse-spec §2).
    let pos = p.current_pos();
    p.error_at(pos, "todo-expr", "expression parser not yet implemented");
    p.recover_to_semicolon();
    None
}

/// Parse a "trailing expression" (the body of an `if`/`while`/`for`
/// without the trailing `)`). For now identical to `parse_expression`.
pub fn parse_expression_no_terminator(p: &mut Parser) -> Option<Expr> {
    parse_expression(p)
}
