//! HolyC scanner. Implementation of the state machine sketched in
//! `docs/lex-spec.md` §2. Bug-compat targets noted with `Q<n>`
//! (matches the spec's quirk numbering).

use crate::diag::{Diag, Severity};
use crate::lex::token::{Pos, Token, TokenKind};

/// TempleOS STR_LEN. Used for ident length cap.
pub const STR_LEN: usize = 144;

pub struct Scanner<'src> {
    src: &'src [u8],
    /// Byte offset.
    i: usize,
    /// Current position (points at byte `i`).
    pos: Pos,
    /// File label for diagnostics.
    file: String,
    /// Diagnostics collected during scanning.
    pub diags: Vec<Diag>,
}

impl<'src> Scanner<'src> {
    pub fn new(file: impl Into<String>, src: &'src str) -> Self {
        Self {
            src: src.as_bytes(),
            i: 0,
            pos: Pos { line: 1, col: 1, byte: 0 },
            file: file.into(),
            diags: Vec::new(),
        }
    }

    pub fn lex_all(mut self) -> (Vec<Token>, Vec<Diag>) {
        let mut out = Vec::new();
        loop {
            let tok = self.next_token();
            let is_eof = matches!(tok.kind, TokenKind::Eof);
            out.push(tok);
            if is_eof {
                break;
            }
        }
        (out, self.diags)
    }

    fn peek(&self) -> Option<u8> {
        self.src.get(self.i).copied()
    }

    fn peek_at(&self, n: usize) -> Option<u8> {
        self.src.get(self.i + n).copied()
    }

    fn bump(&mut self) -> Option<u8> {
        let b = self.peek()?;
        self.i += 1;
        if b == b'\n' {
            self.pos.line += 1;
            self.pos.col = 1;
        } else {
            self.pos.col += 1;
        }
        self.pos.byte = self.i as u32;
        Some(b)
    }

    fn diag(&mut self, severity: Severity, rule: &'static str, message: impl Into<String>, at: Pos) {
        self.diags.push(Diag {
            file: self.file.clone(),
            line: at.line,
            col: at.col,
            severity,
            rule,
            message: message.into(),
        });
    }

    /// Consume whitespace, line comments, and (nested) block comments.
    /// Returns when positioned at the start of a real token, or EOF.
    fn skip_trivia(&mut self) {
        loop {
            match self.peek() {
                None => return,
                Some(b' ') | Some(b'\t') | Some(b'\r') | Some(b'\n') | Some(0x0b) | Some(0x0c) => {
                    self.bump();
                }
                Some(b'/') => match self.peek_at(1) {
                    Some(b'/') => {
                        // line comment
                        self.bump();
                        self.bump();
                        while let Some(b) = self.peek() {
                            if b == b'\n' { break; }
                            self.bump();
                        }
                    }
                    Some(b'*') => {
                        // nested block comment
                        let start = self.pos;
                        self.bump();
                        self.bump();
                        let mut depth: u32 = 1;
                        loop {
                            match self.peek() {
                                None => {
                                    self.diag(
                                        Severity::Error,
                                        "lex-unterminated-comment",
                                        "unterminated /* block comment",
                                        start,
                                    );
                                    return;
                                }
                                Some(b'*') => {
                                    self.bump();
                                    if self.peek() == Some(b'/') {
                                        self.bump();
                                        depth -= 1;
                                        if depth == 0 { break; }
                                    }
                                }
                                Some(b'/') => {
                                    self.bump();
                                    if self.peek() == Some(b'*') {
                                        self.bump();
                                        depth += 1;
                                    }
                                }
                                Some(_) => {
                                    self.bump();
                                }
                            }
                        }
                    }
                    _ => return,
                },
                _ => return,
            }
        }
    }

    /// Top-level scan step: return the next token, whatever it is.
    pub fn next_token(&mut self) -> Token {
        self.skip_trivia();
        let start = self.pos;
        let Some(b) = self.peek() else {
            return Token { kind: TokenKind::Eof, start, end: start };
        };

        let kind = match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'_' | 128..=255 => self.scan_ident(),
            b'0'..=b'9' => self.scan_number(),
            b'"' => self.scan_string(start),
            b'\'' => self.scan_char(start),
            b'.' => self.scan_dot_or_float(start),
            b'#' => { self.bump(); TokenKind::Hash }
            b'@' => { self.bump(); TokenKind::At }
            b'`' => { self.bump(); TokenKind::Backtick }
            // Single-char punct + dual/triple operator dispatch.
            _ => self.scan_punct(start),
        };
        let end = self.pos;
        Token { kind, start, end }
    }

    // ---------- identifiers ----------
    fn scan_ident(&mut self) -> TokenKind {
        let mut buf: Vec<u8> = Vec::with_capacity(16);
        // First char is already at peek() — eat it.
        let first_pos = self.pos;
        let first = self.bump().unwrap();
        buf.push(first);
        while let Some(c) = self.peek() {
            let alnum = matches!(c, b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'_' | 128..=255);
            if !alnum { break; }
            buf.push(c);
            self.bump();
            if buf.len() > STR_LEN {
                self.diag(
                    Severity::Error,
                    "lex-ident-too-long",
                    format!("identifier exceeds STR_LEN ({} bytes)", STR_LEN),
                    first_pos,
                );
                // Keep consuming so we report just one diag.
                while let Some(c) = self.peek() {
                    if !matches!(c, b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'_' | 128..=255) { break; }
                    self.bump();
                }
                break;
            }
        }
        match String::from_utf8(buf) {
            Ok(s) => TokenKind::Ident(s),
            Err(_) => {
                self.diag(
                    Severity::Error,
                    "lex-ident-non-utf8",
                    "identifier contains non-UTF8 bytes (TempleOS allows high-bit bytes; consider whether the source is intentional)",
                    first_pos,
                );
                TokenKind::Ident(String::new())
            }
        }
    }

    // ---------- numbers ----------
    /// Per lex-spec §2:
    ///   - 0x / 0X → hex
    ///   - 0b / 0B → binary
    ///   - leading digit otherwise → decimal, possibly falling through to float
    /// Bug-compat: float = i64 mantissa * Pow10I64(exp). We mirror this
    /// formula. Pow10I64 of a negative is undefined in TempleOS — Q1 in
    /// the spec — so we emit an `exponent-form` warning when a negative
    /// exponent appears, both to flag the bug and to keep tests honest.
    fn scan_number(&mut self) -> TokenKind {
        let start = self.pos;
        let first = self.bump().unwrap();

        // Hex / binary prefix only valid when first digit is '0'.
        if first == b'0' {
            match self.peek().map(|c| c.to_ascii_uppercase()) {
                Some(b'X') => {
                    self.bump();
                    let mut i: i64 = 0;
                    let mut any = false;
                    while let Some(c) = self.peek() {
                        let d = match c {
                            b'0'..=b'9' => (c - b'0') as i64,
                            b'a'..=b'f' => (c - b'a' + 10) as i64,
                            b'A'..=b'F' => (c - b'A' + 10) as i64,
                            _ => break,
                        };
                        i = i.wrapping_mul(16).wrapping_add(d);
                        any = true;
                        self.bump();
                    }
                    if !any {
                        self.diag(
                            Severity::Error,
                            "lex-bad-hex",
                            "0x with no hex digits",
                            start,
                        );
                    }
                    return TokenKind::IntLit(i);
                }
                Some(b'B') => {
                    self.bump();
                    let mut i: i64 = 0;
                    let mut any = false;
                    while let Some(c) = self.peek() {
                        match c {
                            b'0' => { i = i.wrapping_shl(1); any = true; self.bump(); }
                            b'1' => { i = i.wrapping_shl(1).wrapping_add(1); any = true; self.bump(); }
                            _ => break,
                        }
                    }
                    if !any {
                        self.diag(
                            Severity::Error,
                            "lex-bad-binary",
                            "0b with no binary digits",
                            start,
                        );
                    }
                    return TokenKind::IntLit(i);
                }
                _ => {}
            }
        }

        // Decimal int (possibly trailing into float).
        let mut i: i64 = (first - b'0') as i64;
        loop {
            match self.peek() {
                Some(c @ b'0'..=b'9') => {
                    i = i.wrapping_mul(10).wrapping_add((c - b'0') as i64);
                    self.bump();
                }
                _ => break,
            }
        }

        match self.peek() {
            Some(b'.') => {
                // "1.." is int-then-DotDot; check the next char.
                if matches!(self.peek_at(1), Some(b'.')) {
                    return TokenKind::IntLit(i);
                }
                self.bump(); // consume '.'
                self.scan_float_after_dot(start, i)
            }
            Some(b'e') | Some(b'E') => self.scan_float_exponent(start, i, 0),
            _ => TokenKind::IntLit(i),
        }
    }

    /// Continuation after we've consumed the integer part and the dot.
    /// `i` is the running mantissa, `k` will accumulate fractional digit count.
    fn scan_float_after_dot(&mut self, start: Pos, mut i: i64) -> TokenKind {
        let mut k: i32 = 0;
        loop {
            match self.peek() {
                Some(c @ b'0'..=b'9') => {
                    i = i.wrapping_mul(10).wrapping_add((c - b'0') as i64);
                    k += 1;
                    self.bump();
                }
                _ => break,
            }
        }
        match self.peek() {
            Some(b'e') | Some(b'E') => self.scan_float_exponent(start, i, k),
            _ => TokenKind::FloatLit(pow10_apply(i, -k)),
        }
    }

    /// `e`/`E`-prefixed exponent. TempleOS bug Q1: negative exponent
    /// goes through `Pow10I64(neg)` which doesn't exist. We compute
    /// f64::powi normally so callers see a usable value, but flag the
    /// negative-exponent case as a parser-level error since it tripped
    /// the live VM (our actual experience porting mathlib).
    fn scan_float_exponent(&mut self, start: Pos, i: i64, k: i32) -> TokenKind {
        self.bump(); // consume e/E
        let neg = match self.peek() {
            Some(b'-') => { self.bump(); true }
            Some(b'+') => { self.bump(); false }
            _ => false,
        };
        let mut j: i32 = 0;
        let mut any = false;
        loop {
            match self.peek() {
                Some(c @ b'0'..=b'9') => {
                    j = j.saturating_mul(10).saturating_add((c - b'0') as i32);
                    any = true;
                    self.bump();
                }
                _ => break,
            }
        }
        if !any {
            self.diag(
                Severity::Error,
                "lex-bad-exponent",
                "exponent has no digits",
                start,
            );
        }
        let signed_exp = if neg { -j } else { j } - k;
        if signed_exp < 0 {
            self.diag(
                Severity::Error,
                "exponent-float-literal",
                "exponent-form float trips TempleOS Pow10I64(negative); use plain decimal e.g. 0.000000001",
                start,
            );
        }
        TokenKind::FloatLit(pow10_apply(i, signed_exp))
    }

    fn scan_dot_or_float(&mut self, _start: Pos) -> TokenKind {
        let here = self.pos;
        self.bump(); // consume '.'
        match self.peek() {
            Some(b'0'..=b'9') => self.scan_float_after_dot(here, 0),
            Some(b'.') => {
                self.bump();
                if matches!(self.peek(), Some(b'.')) {
                    self.bump();
                    TokenKind::Ellipsis
                } else {
                    TokenKind::DotDot
                }
            }
            _ => TokenKind::Dot,
        }
    }

    // ---------- chars ----------
    /// 1-8 byte char constant packed little-endian. Spec §1.4.3.
    fn scan_char(&mut self, start: Pos) -> TokenKind {
        self.bump(); // consume opening '
        let mut packed: i64 = 0;
        let mut count = 0u32;
        loop {
            let Some(c) = self.peek() else {
                self.diag(
                    Severity::Error,
                    "lex-unterminated-char",
                    "unterminated char constant (EOF)",
                    start,
                );
                return TokenKind::CharLit(packed);
            };
            if c == b'\'' { self.bump(); break; }
            let byte = if c == b'\\' {
                self.bump();
                self.scan_escape()
            } else {
                self.bump();
                c
            };
            if count == 8 {
                self.diag(
                    Severity::Error,
                    "lex-char-too-long",
                    "char constant limited to 8 bytes",
                    start,
                );
                // Drain to closing ' to avoid cascading errors.
                while let Some(c2) = self.peek() {
                    self.bump();
                    if c2 == b'\'' { break; }
                }
                return TokenKind::CharLit(packed);
            }
            packed |= (byte as i64) << (count * 8);
            count += 1;
        }
        TokenKind::CharLit(packed)
    }

    fn scan_escape(&mut self) -> u8 {
        match self.peek() {
            None => 0,
            Some(c) => {
                self.bump();
                match c {
                    b'0' => 0,
                    b'\'' => b'\'',
                    b'"' => b'"',
                    b'`' => b'`',
                    b'\\' => b'\\',
                    b'd' => b'$', // HolyC: \d is the dollar sign (DolDoc escape)
                    b'n' => b'\n',
                    b'r' => b'\r',
                    b't' => b'\t',
                    b'x' | b'X' => {
                        let mut v: u8 = 0;
                        for _ in 0..2 {
                            match self.peek() {
                                Some(d @ (b'0'..=b'9' | b'a'..=b'f' | b'A'..=b'F')) => {
                                    let n = match d {
                                        b'0'..=b'9' => d - b'0',
                                        b'a'..=b'f' => d - b'a' + 10,
                                        _ => d - b'A' + 10,
                                    };
                                    v = (v << 4) | n;
                                    self.bump();
                                }
                                _ => break,
                            }
                        }
                        v
                    }
                    other => {
                        // Unknown escape: emit literal '\' then push back.
                        // Simpler: just emit `other` as-is — TempleOS
                        // emits a literal backslash and pushes back, but
                        // for a parser-only tool the visible byte is
                        // close enough.
                        other
                    }
                }
            }
        }
    }

    // ---------- strings ----------
    fn scan_string(&mut self, start: Pos) -> TokenKind {
        self.bump(); // consume opening "
        let mut out = Vec::new();
        loop {
            let Some(c) = self.peek() else {
                self.diag(
                    Severity::Error,
                    "lex-unterminated-string",
                    "unterminated string literal (EOF)",
                    start,
                );
                return TokenKind::StrLit(out);
            };
            if c == b'"' { self.bump(); break; }
            if c == b'\\' {
                self.bump();
                out.push(self.scan_escape());
            } else {
                out.push(c);
                self.bump();
            }
        }
        TokenKind::StrLit(out)
    }

    // ---------- punctuation + dual/triple-char operators ----------
    fn scan_punct(&mut self, _start: Pos) -> TokenKind {
        let c = self.bump().unwrap();
        match c {
            b';' => TokenKind::Semicolon,
            b',' => TokenKind::Comma,
            b'(' => TokenKind::LParen,
            b')' => TokenKind::RParen,
            b'{' => TokenKind::LBrace,
            b'}' => TokenKind::RBrace,
            b'[' => TokenKind::LBracket,
            b']' => TokenKind::RBracket,
            b'~' => TokenKind::Tilde,
            b'?' => TokenKind::Question,
            // Dual-char families (with an optional triple `<<=`/`>>=`).
            b'!' => self.dual1(b'=', TokenKind::BangEq, TokenKind::Bang),
            b'=' => self.dual1(b'=', TokenKind::EqEq, TokenKind::Eq),
            b':' => self.dual1(b':', TokenKind::ColonColon, TokenKind::Colon),
            b'+' => self.dual2(
                b'+', TokenKind::PlusPlus,
                b'=', TokenKind::PlusEq,
                TokenKind::Plus,
            ),
            b'-' => self.dual3(
                b'>', TokenKind::Arrow,
                b'-', TokenKind::MinusMinus,
                b'=', TokenKind::MinusEq,
                TokenKind::Minus,
            ),
            b'*' => self.dual1(b'=', TokenKind::StarEq, TokenKind::Star),
            b'/' => self.dual1(b'=', TokenKind::SlashEq, TokenKind::Slash),
            b'%' => self.dual1(b'=', TokenKind::PercentEq, TokenKind::Percent),
            b'&' => self.dual2(
                b'&', TokenKind::AmpAmp,
                b'=', TokenKind::AmpEq,
                TokenKind::Amp,
            ),
            b'|' => self.dual2(
                b'|', TokenKind::PipePipe,
                b'=', TokenKind::PipeEq,
                TokenKind::Pipe,
            ),
            b'^' => self.dual2(
                b'=', TokenKind::CaretEq,
                b'^', TokenKind::CaretCaret,
                TokenKind::Caret,
            ),
            b'<' => match self.peek() {
                Some(b'=') => { self.bump(); TokenKind::LtEq }
                Some(b'<') => {
                    self.bump();
                    if self.peek() == Some(b'=') { self.bump(); TokenKind::ShlEq } else { TokenKind::Shl }
                }
                _ => TokenKind::Lt,
            },
            b'>' => match self.peek() {
                Some(b'=') => { self.bump(); TokenKind::GtEq }
                Some(b'>') => {
                    self.bump();
                    if self.peek() == Some(b'=') { self.bump(); TokenKind::ShrEq } else { TokenKind::Shr }
                }
                _ => TokenKind::Gt,
            },
            other => {
                let pos = self.pos;
                self.diag(
                    Severity::Error,
                    "lex-unknown-byte",
                    format!("unexpected byte 0x{:02X}", other),
                    Pos { col: pos.col.saturating_sub(1), ..pos },
                );
                // Try to keep going; return Bang as a placeholder shape.
                TokenKind::Bang
            }
        }
    }

    fn dual1(&mut self, c2: u8, paired: TokenKind, single: TokenKind) -> TokenKind {
        if self.peek() == Some(c2) { self.bump(); paired } else { single }
    }

    fn dual2(
        &mut self,
        a: u8, ta: TokenKind,
        b: u8, tb: TokenKind,
        single: TokenKind,
    ) -> TokenKind {
        match self.peek() {
            Some(c) if c == a => { self.bump(); ta }
            Some(c) if c == b => { self.bump(); tb }
            _ => single,
        }
    }

    fn dual3(
        &mut self,
        a: u8, ta: TokenKind,
        b: u8, tb: TokenKind,
        c: u8, tc: TokenKind,
        single: TokenKind,
    ) -> TokenKind {
        match self.peek() {
            Some(x) if x == a => { self.bump(); ta }
            Some(x) if x == b => { self.bump(); tb }
            Some(x) if x == c => { self.bump(); tc }
            _ => single,
        }
    }
}

/// `i * 10^exp` matching TempleOS's `i * Pow10I64(exp)` semantics for
/// representable cases. For negative `exp` we use f64 powi — this
/// produces a usable host-side value while we separately emit a
/// diagnostic noting that the same input would trip the VM.
fn pow10_apply(i: i64, exp: i32) -> f64 {
    (i as f64) * (10f64).powi(exp)
}
