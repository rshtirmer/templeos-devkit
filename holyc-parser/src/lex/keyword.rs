//! HolyC keyword resolution. TempleOS resolves keywords via the
//! hash table (CompilerB.HH:KW_*). We keep `Ident` as the lexer's
//! output and provide `lookup(name)` for the parser.
//!
//! Note: HolyC has NO `continue` keyword — bug-compat: a token
//! `continue` resolves as a regular identifier here, and the parser
//! will emit "Undefined identifier" exactly as TempleOS does.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Keyword {
    // Control flow
    If,
    Else,
    For,
    While,
    Do,
    Switch,
    Case,
    Default,
    Break,
    Return,
    Goto,
    Try,
    Catch,
    Sizeof,
    Defined,
    Asm,

    // Types
    U0,
    I0,
    U8,
    I8,
    Bool,
    U16,
    I16,
    U32,
    I32,
    U64,
    I64,
    F64,

    // Storage / linkage modifiers
    Extern,
    Import,
    ExternUnderscore, // _extern
    ImportUnderscore, // _import
    Public,
    Static,
    Interrupt,
    Lock,
    Lastclass,

    // Aggregate
    Class,
    Union,
    Intern,
    Argpop,
    Noargpop,
    Nostkchk,
}

pub fn lookup(name: &str) -> Option<Keyword> {
    use Keyword::*;
    Some(match name {
        "if" => If,
        "else" => Else,
        "for" => For,
        "while" => While,
        "do" => Do,
        "switch" => Switch,
        "case" => Case,
        "default" => Default,
        "break" => Break,
        "return" => Return,
        "goto" => Goto,
        "try" => Try,
        "catch" => Catch,
        // Note: `start` / `end` are CONTEXTUAL keywords — they mark
        // sub-switch ranges per parse-spec §3 only when seen as
        // statements inside a `switch` body. Outside a switch body
        // they must resolve as plain identifiers (kernel code uses
        // `start` as an ordinary local-variable name). The contextual
        // dispatch lives in `parse::stmt::parse_statement_inner`.
        "sizeof" => Sizeof,
        // Note: `offset` is a CONTEXTUAL keyword — it only acts as the
        // offsetof operator when followed by `(`. Otherwise (e.g. as a
        // local variable / parameter name, which kernel code does
        // routinely) it must resolve as a plain identifier. The
        // contextual dispatch lives in `parse::expr::parse_unary_term`.
        "defined" => Defined,
        "asm" => Asm,

        "U0" => U0,
        "I0" => I0,
        "U8" => U8,
        "I8" => I8,
        "Bool" => Bool,
        "U16" => U16,
        "I16" => I16,
        "U32" => U32,
        "I32" => I32,
        "U64" => U64,
        "I64" => I64,
        "F64" => F64,

        "extern" => Extern,
        "import" => Import,
        "_extern" => ExternUnderscore,
        "_import" => ImportUnderscore,
        "public" => Public,
        "static" => Static,
        "interrupt" => Interrupt,
        "lock" => Lock,
        "lastclass" => Lastclass,
        // Note: `reg` / `noreg` are CONTEXTUAL keywords — they only
        // act as register-allocation modifiers at the start of a
        // declaration (i.e. in modifier-prefix position consumed by
        // `parse::decl::parse_modifiers`). Anywhere else (variable
        // names, parameter names, member names) they fall through
        // to plain identifier so kernel asm-adjacent code that uses
        // `reg` as a name parses cleanly.

        "class" => Class,
        "union" => Union,
        "intern" => Intern,
        "argpop" => Argpop,
        "noargpop" => Noargpop,
        "nostkchk" => Nostkchk,

        _ => return None,
    })
}
