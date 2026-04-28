//! holyc-parser — host-side HolyC front-end.
//!
//! Translates lexer + parser + type-checker from TempleOS's `Compiler/`
//! HolyC sources to Rust. Bug-compatibility principle: we REPRODUCE
//! TempleOS's parser bugs (one-line `if (cond) continue;` failure,
//! parametrized `#define` flakiness, `1e-9` exponent rejection, etc.)
//! rather than fixing them. The simulator's value is "if this passes,
//! the VM accepts it."

pub mod diag;
pub mod lex;
pub mod lint;
pub mod parse;
pub mod symbol;

// NOTE: there is no dedicated `preproc` or `types` module. Preprocessor
// directives are recognized by the parser and surface as
// `parse::ast::PpDirective` AST nodes (we deliberately do not expand
// macros — TempleOS's parametrized `#define` is unreliable across
// ExePutS chunks, and the lint warns rather than expanding). Type
// information is tracked just enough to drive type-aware lint rules
// in `lint::TypeContext` (function signatures + global var types +
// integer-typed `#define` bodies). A full type checker would
// re-introduce a `types` module; today neither one is needed.
