//! holycc — CLI driver. `holycc lint <paths...>` runs the lexer +
//! parser over each `.HC` / `.ZC` / `.HH` file and prints
//! diagnostics in the same `path:line:col: severity: msg [rule]`
//! shape as `holyc-lint.py`, so `make lint` consumers see one
//! uniform format.
//!
//! Exit code: 1 if any *errors* were emitted, 0 otherwise. Warnings
//! don't fail.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use holyc_parser::diag::Severity;
use holyc_parser::parse::{parse_module, ParseConfig};

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() {
        eprintln!("usage: holycc lint <file-or-dir>...");
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

fn cmd_lint(paths: &[String]) -> ExitCode {
    if paths.is_empty() {
        eprintln!("usage: holycc lint <file-or-dir>...");
        return ExitCode::from(2);
    }
    let mut files = Vec::new();
    for arg in paths {
        collect(Path::new(arg), &mut files);
    }
    if files.is_empty() {
        eprintln!("holycc lint: no .HC/.ZC/.HH files found in {paths:?}");
        return ExitCode::from(2);
    }

    let cfg = ParseConfig::default();
    let mut total_errors = 0usize;
    let mut total_warnings = 0usize;

    for file in &files {
        let src = match std::fs::read_to_string(file) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("{}: read error: {e}", file.display());
                total_errors += 1;
                continue;
            }
        };
        let (_module, diags) = parse_module(file.display().to_string(), &src, cfg);
        for d in &diags {
            println!("{d}");
            match d.severity {
                Severity::Error => total_errors += 1,
                Severity::Warning => total_warnings += 1,
            }
        }
    }
    eprintln!(
        "{} error(s), {} warning(s) in {} file(s)",
        total_errors, total_warnings, files.len()
    );
    if total_errors > 0 {
        ExitCode::from(1)
    } else {
        ExitCode::from(0)
    }
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
