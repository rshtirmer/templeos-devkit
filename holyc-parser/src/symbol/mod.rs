//! Symbol table — mirrors TempleOS's `hash_table->next` chain semantics.
//!
//! Order matters: ExePutS chunks see symbols in declaration order, and
//! Spawn'd tasks don't reliably resolve adam's symbols via the next-chain.
//! We model this so static analysis matches what the VM JIT actually sees.
