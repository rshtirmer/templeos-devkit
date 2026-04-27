//! holycc — CLI driver. `holycc lint <paths...>` runs the lexer +
//! parser over each `.HC` / `.ZC` / `.HH` file and prints
//! diagnostics in the same `path:line:col: severity: msg [rule]`
//! shape as `holyc-lint.py`, so `make lint` consumers see one
//! uniform format.
//!
//! Multi-file mode (the default when more than one file is given):
//! all files are parsed, their declarations unioned into a global
//! symbol table, then every file is re-walked for unresolved
//! identifier uses. This catches cross-file dependency bugs (a
//! symbol used in one file but defined in another that doesn't
//! sort first in `temple-run.py`'s push order).
//!
//! Exit code: 1 if any *errors* were emitted, 0 otherwise.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use holyc_parser::diag::{Diag, Severity};
use holyc_parser::parse::{parse_module, ParseConfig};
use holyc_parser::parse::ast::Module;
use holyc_parser::symbol::Resolver;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() {
        eprintln!("usage: holycc lint [--no-resolve] <file-or-dir>...");
        return ExitCode::from(2);
    }
    match args[0].as_str() {
        "lint" => cmd_lint(&args[1..]),
        cmd => {
            eprintln!("holycc: unknown command '{cmd}'");
            ExitCode::from(2)
        }
    }
}

fn cmd_lint(args: &[String]) -> ExitCode {
    let mut paths: Vec<&str> = Vec::new();
    let mut do_resolve = true;
    for a in args {
        match a.as_str() {
            "--no-resolve" => do_resolve = false,
            other => paths.push(other),
        }
    }
    if paths.is_empty() {
        eprintln!("usage: holycc lint [--no-resolve] <file-or-dir>...");
        return ExitCode::from(2);
    }
    let mut files = Vec::new();
    for arg in &paths {
        collect(Path::new(arg), &mut files);
    }
    if files.is_empty() {
        eprintln!("holycc lint: no .HC/.ZC/.HH files found in {paths:?}");
        return ExitCode::from(2);
    }

    let cfg = ParseConfig::default();
    let mut all_diags: Vec<Diag> = Vec::new();
    let mut modules: Vec<(String, Module)> = Vec::new();

    // Phase 1: parse each file, collect parser diags.
    for file in &files {
        let src = match std::fs::read_to_string(file) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("{}: read error: {e}", file.display());
                return ExitCode::from(2);
            }
        };
        let label = file.display().to_string();
        let (m, diags) = parse_module(label.clone(), &src, cfg);
        all_diags.extend(diags);
        modules.push((label, m));
    }

    // Phase 2 (optional): symbol resolution. Register-then-check per
    // file in input order. This matches `temple-run.py`'s push semantics:
    // each `*.HC` file is JIT-compiled in alphabetical order, so a use
    // in file N can only see decls from files 1..=N (plus locals + the
    // builtin manifest). Catches cross-file ordering bugs that the
    // earlier "register-all-then-check-all" pass missed.
    if do_resolve {
        let mut resolver = Resolver::new();
        for (label, m) in &modules {
            resolver.register_module(label, m);
            all_diags.extend(resolver.check_module(label, m));
        }
    }

    let mut total_errors = 0usize;
    let mut total_warnings = 0usize;
    for d in &all_diags {
        println!("{d}");
        match d.severity {
            Severity::Error => total_errors += 1,
            Severity::Warning => total_warnings += 1,
        }
    }
    eprintln!(
        "{} error(s), {} warning(s) in {} file(s){}",
        total_errors,
        total_warnings,
        files.len(),
        if do_resolve { " (cross-file resolution on)" } else { "" }
    );
    if total_errors > 0 { ExitCode::from(1) } else { ExitCode::from(0) }
}

/// Recursive walk: take any `.HC`/`.ZC`/`.HH` files at or under `p`.
fn collect(p: &Path, out: &mut Vec<PathBuf>) {
    if p.is_file() {
        if is_holyc(p) {
            out.push(p.to_path_buf());
        }
        return;
    }
    if !p.is_dir() {
        return;
    }
    let Ok(entries) = std::fs::read_dir(p) else { return; };
    let mut sorted: Vec<_> = entries.flatten().collect();
    sorted.sort_by_key(|e| e.path());
    for entry in sorted {
        let path = entry.path();
        if path.is_dir() {
            collect(&path, out);
        } else if is_holyc(&path) {
            out.push(path);
        }
    }
}

fn is_holyc(p: &Path) -> bool {
    matches!(
        p.extension().and_then(|s| s.to_str()),
        Some("HC") | Some("ZC") | Some("HH") | Some("hc") | Some("zc") | Some("hh")
    )
}
