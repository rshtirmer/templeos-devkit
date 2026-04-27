//! Corpus harness — walks `tests/corpus/{passing,failing}` and runs
//! `holyc_parser::parse::parse_module` on each `.hc`. The corpus is
//! VM-validated ground truth (see `tests/corpus/README.md`); when our
//! parser disagrees we report the disagreement category.
//!
//! This test is intentionally **non-gating** for now — the Rust port
//! is bug-compatible work-in-progress, so we expect false positives
//! (parser too strict) and false negatives (parser too lenient) on
//! the corpus. The test prints a summary the developer can inspect.
//! Once the agreement rate is high enough, we'll convert specific
//! counts into hard assertions.

use std::fs;
use std::path::{Path, PathBuf};

use holyc_parser::diag::Severity;
use holyc_parser::parse::{parse_module, ParseConfig};

#[derive(Default, Debug)]
struct Tally {
    /// Snippet was expected to pass; we agreed (no errors emitted).
    pass_match: u32,
    /// Expected pass; we emitted error(s) — false positive (too strict).
    pass_fp: u32,
    /// Snippet was expected to fail; we agreed (≥1 error emitted).
    fail_match: u32,
    /// Expected fail; we emitted no errors — false negative (too lenient).
    fail_fn: u32,
}

impl Tally {
    fn total(&self) -> u32 {
        self.pass_match + self.pass_fp + self.fail_match + self.fail_fn
    }
    fn agreement(&self) -> f64 {
        let n = self.total();
        if n == 0 { return 1.0; }
        (self.pass_match + self.fail_match) as f64 / n as f64
    }
}

fn corpus_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/corpus")
}

fn list_hc(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let Ok(entries) = fs::read_dir(dir) else { return out; };
    for entry in entries.flatten() {
        let p = entry.path();
        if p.extension().and_then(|s| s.to_str()) == Some("hc") {
            out.push(p);
        }
    }
    out.sort();
    out
}

fn run_one(path: &Path) -> Vec<String> {
    let src = fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("read {path:?}: {e}"));
    let (_module, diags) = parse_module(
        path.file_name().and_then(|s| s.to_str()).unwrap_or("?"),
        &src,
        ParseConfig::default(),
    );
    diags
        .into_iter()
        .filter(|d| d.severity == Severity::Error)
        .map(|d| format!("{}:{}: {}", d.line, d.col, d.message))
        .collect()
}

#[test]
fn corpus_agreement_summary() {
    let root = corpus_root();
    let passing = root.join("passing");
    let failing = root.join("failing");

    if !passing.exists() || !failing.exists() {
        eprintln!("corpus directories not found at {root:?}; skipping");
        return;
    }

    let mut t = Tally::default();
    let mut sample_fp: Vec<(PathBuf, Vec<String>)> = Vec::new();
    let mut sample_fn: Vec<PathBuf> = Vec::new();

    for path in list_hc(&passing) {
        let errs = run_one(&path);
        if errs.is_empty() {
            t.pass_match += 1;
        } else {
            t.pass_fp += 1;
            if sample_fp.len() < 5 { sample_fp.push((path, errs)); }
        }
    }
    for path in list_hc(&failing) {
        let errs = run_one(&path);
        if errs.is_empty() {
            t.fail_fn += 1;
            if sample_fn.len() < 5 { sample_fn.push(path); }
        } else {
            t.fail_match += 1;
        }
    }

    let pct = (t.agreement() * 100.0).round();
    println!("\n==== corpus_agreement_summary ====");
    println!("  total:       {}", t.total());
    println!("  pass_match:  {} (we agreed: snippet passes, no errors)", t.pass_match);
    println!("  pass_fp:     {} (false positive: we erred on passing snippet)", t.pass_fp);
    println!("  fail_match:  {} (we agreed: snippet fails, errors emitted)", t.fail_match);
    println!("  fail_fn:     {} (false negative: failing snippet, we accepted)", t.fail_fn);
    println!("  agreement:   {pct}%");

    if !sample_fp.is_empty() {
        println!("\n  sample false positives (first 5):");
        for (p, errs) in &sample_fp {
            println!("    {}", p.file_name().unwrap().to_string_lossy());
            for e in errs.iter().take(2) {
                println!("      → {e}");
            }
        }
    }
    if !sample_fn.is_empty() {
        println!("\n  sample false negatives (first 5):");
        for p in &sample_fn {
            println!("    {}", p.file_name().unwrap().to_string_lossy());
        }
    }
    println!();

    // Non-gating for now. Once agreement is consistently >90% we'll
    // convert this to: `assert!(t.agreement() > 0.90)`.
    assert!(t.total() > 0, "corpus appears empty — check fixture dirs");
}
