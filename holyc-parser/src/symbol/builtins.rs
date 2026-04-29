//! TempleOS kernel built-ins manifest. The minimum set needed to
//! avoid false-positive "unresolved-identifier" diagnostics on
//! real HolyC code.
//!
//! Sourced empirically from `Doc/Comm.HC.Z`, `Kernel/`, and patterns
//! observed during TempleOS experimentation. Add to this list when a
//! new built-in is referenced — keep it conservative; an over-broad
//! manifest hides real porting bugs.

use std::collections::HashSet;
use std::sync::OnceLock;

/// Returns the set of TempleOS built-in identifiers (functions,
/// constants, globals, intrinsic-style helpers).
pub fn builtin_names() -> &'static HashSet<&'static str> {
    static SET: OnceLock<HashSet<&'static str>> = OnceLock::new();
    SET.get_or_init(|| BUILTINS.iter().copied().collect())
}

pub fn is_builtin(name: &str) -> bool {
    builtin_names().contains(name)
}

/// Curated TempleOS / ZealOS API surface. PascalCase entries are
/// kernel functions; lowercase entries are F64 constants and
/// kernel globals.
const BUILTINS: &[&str] = &[
    // ---- F64 constants ----
    "pi", "eps", "inf", "nan",

    // ---- Kernel globals ----
    "Fs", "adam_task", "sys_task", "comm_ports",
    "ms",        // mouse (TempleOS)
    "tS",        // seconds-since-boot (ZealOS only — harmless to allow)

    // ---- Memory ----
    "MAlloc", "CAlloc", "Free", "MSize",
    "MemSet", "MemCpy", "MemCmp", "MemMove",

    // ---- Strings ----
    "StrLen", "StrCmp", "StrCpy", "StrCat", "StrFind", "StrNew",
    "StrPrint", "MStrCmp",
    "ToUpper", "ToLower",

    // ---- Math ----
    "Sin", "Cos", "Tan", "Sqrt", "Pow",
    "Abs", "AbsI64", "AbsF64",
    "Floor", "Ceil", "Round",
    // TempleOS spells these as `ATan` / `Pow10` / etc. (camelcase
    // with capital second letter on abbreviations). Source of truth:
    // `Kernel/KernelB.HH` in cia-foundation/TempleOS — e.g.
    //   public _intern IC_ATAN F64 ATan(F64 d);
    // There is no `Atan2` in TempleOS; only single-arg `ATan`.
    "ATan", "Exp", "Log", "Log2", "Log10",
    "Pow10I64",
    "Min", "Max",
    "Rand", "RandU16", "RandU32", "RandU64",

    // ---- I/O ----
    "Print", "PrintErr", "PutChars", "PutS",
    "GetChar", "GetS",
    "DocPrint", "DocForm", "DocNew", "DocDel",
    "FileWrite", "FileRead", "FileFind",
    "Cd", "Dir", "ExeFile", "ExePrint", "ExePutS",

    // ---- Comms ----
    "CommInit8n1", "CommPrint", "CommPutChar", "CommPutS",
    "CommGetChar",
    "FifoU8New", "FifoU8Del",
    "FifoU8Insert", "FifoU8Remove",      // ZealOS names
    "FifoU8Ins",    "FifoU8Rem",         // TempleOS names
    "FifoU8Cnt",
    "OutU8", "OutU16", "OutU32",
    "InU8",  "InU16",  "InU32",

    // ---- Tasks / control ----
    "Spawn", "Sys", "Sleep", "Yield",
    "TaskExe", "Once", "OnceExe", "JobsHandler",
    "Kill", "Exit",
    "RFlagsGet",
    "Bt", "Bts", "Btr", "Btc",
    "AHCIPortInit",
    "TCPSocketListen", "TCPSocketAccept",
    "TCPSocketBind",   "TCPSocketClose",

    // ---- Type coercions ----
    "ToI64", "ToF64", "ToBool",

    // ---- Devices / blocks ----
    "BlkDevNextFreeSlot", "BlkDevAdd",

    // ---- Common Boolean literals (TempleOS spells these) ----
    "TRUE", "FALSE", "NULL",

    // ---- DolDoc / framebuffer touches we hit in shuttle code ----
    "DocCenter", "DocClear", "DocBottom", "DocTop",

    // ---- Graphics / DC primitives (Adam/Gr/*) ----
    // The `gr` global is a CGr struct holding the screen DC + state.
    "gr",
    // Color setters
    "GrColor", "GrColor1", "GrColor2",
    // Plot / line / shape primitives
    "GrPlot", "GrLine", "GrLine3", "GrFloodFill",
    "GrRect", "GrRect3", "GrFillRect",
    "GrCircle", "GrCircle3",
    "GrEllipse", "GrEllipse3",
    "GrPrint", "GrPrintCenter",
    "GrBlot",   "GrBitMapPut", "GrBitMapAnd", "GrBitMapOr",
    "GrUpdateScrn", "GrUpdateTaskWin",
    // DC management
    "DCNew", "DCDel", "DCAlias", "DCCopy", "DCFill",
    "DCLoad", "DCSave",
    "DCDepthBufRead", "DCDepthBufNew",
    // Sprite renderer
    "SpriteDraw", "SpriteDrawNoTransform",
    "SpriteEdNew", "SpriteEd",

    // ---- HolyC reserved keywords lex would emit as Ident ----
    // (parsing handles these, but seeding here keeps the resolver
    // honest if the parser hands one through unrecognised)
    "if", "else", "for", "while", "do", "switch", "case", "default",
    "break", "return", "goto", "try", "catch",
    "start", "end", "sizeof", "offset", "defined", "asm",
    "U0", "I0", "U8", "I8", "Bool",
    "U16", "I16", "U32", "I32", "U64", "I64", "F64",
    "extern", "import", "_extern", "_import",
    "public", "static", "interrupt", "lock", "lastclass",
    "noreg", "reg",
    "class", "union", "intern", "argpop", "noargpop", "nostkchk",
];
