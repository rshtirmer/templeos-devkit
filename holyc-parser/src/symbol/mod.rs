//! Symbol table.
//!
//! Tracks declarations (functions, classes, globals) and uses
//! (idents in expression position) across one or more parsed
//! Modules. Reports `unresolved-identifier` for each use that
//! doesn't resolve to a known declaration or kernel built-in.
//!
//! TempleOS-faithful order: ExePutS chunks see symbols in
//! declaration order. We model this by registering decls in the
//! order they appear and validating uses against the registered
//! set at the point of use. Multi-file mode unions all files'
//! decls before validating any uses — this matches how
//! `temple-run.py` pushes `src/*.HC` alphabetically before
//! `tests/T_*.HC`.

pub mod builtins;
pub mod resolver;

pub use builtins::{builtin_names, is_builtin};
pub use resolver::{Resolver, Symbol, SymbolKind};
