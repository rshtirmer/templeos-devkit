//! Parser — recursive descent. Spec lives in `docs/parse-spec.md`.
//!
//! Grammar is derived from TempleOS's PrsExp.HC, PrsStmt.HC, PrsVar.HC,
//! and PrsLib.HC. We mirror the structure so error sites and recovery
//! points line up with the VM's parser.
