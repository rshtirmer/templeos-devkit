//! Preprocessor — `#include`, `#define`, conditional compilation.
//!
//! Parametrized macros are flagged as warnings rather than expanded —
//! TempleOS's parametrized #define is unreliable across ExePutS chunks,
//! so production code shouldn't rely on it.
