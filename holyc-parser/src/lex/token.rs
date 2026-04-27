//! Token enum + position info. See `docs/lex-spec.md` §1 for the
//! TempleOS-side mapping. Keywords are NOT separated from idents at
//! the lexer layer — TempleOS resolves keywords through the hash
//! table at the parser layer. We do the same: `Ident` is emitted for
//! every word, and `is_keyword(name)` provides the resolution.

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Pos {
    /// 1-based line.
    pub line: u32,
    /// 1-based column (in bytes — TempleOS source is plain ASCII).
    pub col: u32,
    /// 0-based byte offset into the source.
    pub byte: u32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Token {
    pub kind: TokenKind,
    pub start: Pos,
    pub end: Pos,
}

#[derive(Clone, Debug, PartialEq)]
pub enum TokenKind {
    // -------- pseudo-tokens --------
    Eof,

    // -------- literals --------
    /// Identifier. Keywords are detected via `keyword::lookup` at
    /// parse time (TempleOS resolves them through the hash table —
    /// CompilerB.HH:KW_*).
    Ident(String),
    /// Decimal/hex/binary integer literal.
    IntLit(i64),
    /// Float literal. Computed via i64-mantissa * Pow10I64(exp) per
    /// TempleOS semantics; we reproduce overflow + Pow10I64(neg)
    /// bugs (see lex-spec §3 quirks Q1, Q3).
    FloatLit(f64),
    /// 1-8 byte char constant packed little-endian into i64.
    CharLit(i64),
    /// String literal, NUL-terminated to mirror cur_str_len semantics.
    /// Bytes do NOT include the trailing NUL — consumers can append
    /// when interop matters.
    StrLit(Vec<u8>),

    // -------- single-char punctuation (returned as ASCII byte values
    // by TempleOS; we name them) --------
    Semicolon,    // ;
    Comma,        // ,
    Colon,        // :
    Question,     // ?
    LParen,       // (
    RParen,       // )
    LBrace,       // {
    RBrace,       // }
    LBracket,     // [
    RBracket,     // ]
    Tilde,        // ~
    Bang,         // !
    Dot,          // .
    Plus,         // +
    Minus,        // -
    Star,         // *
    Slash,        // /
    Percent,      // %
    Amp,          // &
    Pipe,         // |
    Caret,        // ^
    Eq,           // =
    Lt,           // <
    Gt,           // >
    /// Power operator `a `b == a^b. PREC_EXP, right-assoc per parse-spec.
    Backtick,
    /// `@` when `CCF_KEEP_AT_SIGN` is set. By default `@` is part of
    /// idents; we always treat `@` inside idents and emit `At` only if
    /// it appears bare (rare; document the flag-driven behavior in §1.5).
    At,
    /// `#` only when `CCF_KEEP_SIGN_NUM` is set. By default `#`
    /// triggers preprocessor handling — we leave preprocessor for
    /// the preproc/ module and emit `Hash` here so a host caller can
    /// see directives.
    Hash,

    // -------- dual / triple-char operators --------
    BangEq,       // !=
    EqEq,         // ==
    LtEq,         // <=
    GtEq,         // >=
    AmpAmp,       // &&
    PipePipe,     // ||
    CaretCaret,   // ^^   (HolyC short-circuit XOR)
    PlusPlus,     // ++
    MinusMinus,   // --
    Shl,          // <<
    Shr,          // >>
    ShlEq,        // <<=
    ShrEq,        // >>=
    StarEq,       // *=
    SlashEq,      // /=
    PercentEq,    // %=
    PlusEq,       // +=
    MinusEq,      // -=
    AmpEq,        // &=
    PipeEq,       // |=
    CaretEq,      // ^=
    Arrow,        // ->
    ColonColon,   // ::
    DotDot,       // ..
    Ellipsis,     // ...
}

impl TokenKind {
    /// One-line spelling for diagnostics. Idents/literals fall back
    /// to a canonical placeholder.
    pub fn spelling(&self) -> &'static str {
        use TokenKind::*;
        match self {
            Eof => "<eof>",
            Ident(_) => "<ident>",
            IntLit(_) => "<int>",
            FloatLit(_) => "<float>",
            CharLit(_) => "<char>",
            StrLit(_) => "<string>",
            Semicolon => ";",
            Comma => ",",
            Colon => ":",
            Question => "?",
            LParen => "(",
            RParen => ")",
            LBrace => "{",
            RBrace => "}",
            LBracket => "[",
            RBracket => "]",
            Tilde => "~",
            Bang => "!",
            Dot => ".",
            Plus => "+",
            Minus => "-",
            Star => "*",
            Slash => "/",
            Percent => "%",
            Amp => "&",
            Pipe => "|",
            Caret => "^",
            Eq => "=",
            Lt => "<",
            Gt => ">",
            Backtick => "`",
            At => "@",
            Hash => "#",
            BangEq => "!=",
            EqEq => "==",
            LtEq => "<=",
            GtEq => ">=",
            AmpAmp => "&&",
            PipePipe => "||",
            CaretCaret => "^^",
            PlusPlus => "++",
            MinusMinus => "--",
            Shl => "<<",
            Shr => ">>",
            ShlEq => "<<=",
            ShrEq => ">>=",
            StarEq => "*=",
            SlashEq => "/=",
            PercentEq => "%=",
            PlusEq => "+=",
            MinusEq => "-=",
            AmpEq => "&=",
            PipeEq => "|=",
            CaretEq => "^=",
            Arrow => "->",
            ColonColon => "::",
            DotDot => "..",
            Ellipsis => "...",
        }
    }
}
