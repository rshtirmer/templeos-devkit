//! Diagnostics — the host-facing surface. Mirrors TempleOS's compiler
//! error format so diagnostics translate one-for-one against the VM.

use std::fmt;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Severity {
    Error,
    Warning,
}

impl fmt::Display for Severity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Severity::Error => write!(f, "error"),
            Severity::Warning => write!(f, "warning"),
        }
    }
}

#[derive(Clone, Debug)]
pub struct Diag {
    pub file: String,
    pub line: u32,
    pub col: u32,
    pub severity: Severity,
    pub rule: &'static str,
    pub message: String,
}

impl fmt::Display for Diag {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}:{}:{}: {}: {} [{}]",
            self.file, self.line, self.col, self.severity, self.message, self.rule
        )
    }
}

#[derive(Default)]
pub struct DiagBag {
    items: Vec<Diag>,
}

impl DiagBag {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn push(&mut self, d: Diag) {
        self.items.push(d);
    }
    pub fn iter(&self) -> std::slice::Iter<'_, Diag> {
        self.items.iter()
    }
    pub fn errors(&self) -> usize {
        self.items.iter().filter(|d| d.severity == Severity::Error).count()
    }
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
}
