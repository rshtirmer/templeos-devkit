//! Type parser (PrsType + PrsArrayDims). Filled by StmtDeclCoder.

use crate::parse::ast::TypeRef;
use crate::parse::parser::Parser;

/// Parse a type specifier at the current cursor. Returns `None` if
/// the cursor isn't at a type token (the caller usually peeks first).
pub fn parse_type(p: &mut Parser) -> Option<TypeRef> {
    // STUB.
    let pos = p.current_pos();
    p.error_at(pos, "todo-type", "type parser not yet implemented");
    None
}
