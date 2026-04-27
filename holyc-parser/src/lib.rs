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
pub mod preproc;
pub mod parse;
pub mod symbol;
pub mod types;
