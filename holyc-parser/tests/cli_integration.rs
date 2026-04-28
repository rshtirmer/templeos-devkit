//! End-to-end CLI tests for the `holycc` binary.
//!
//! Spawns the bin via `cargo run` against multi-file fixtures laid
//! down in a temp dir per test. Validates exit codes, output format,
//! and cross-file resolution. Each fixture is a self-contained set
//! of `.HC` files written into the test's tempdir at runtime.

use std::io::Write;
use std::path::PathBuf;
use std::process::Command;

/// One fixture file: relative path within the tempdir + its contents.
struct File<'a> {
    name: &'a str,
    body: &'a str,
}

/// Write `files` into a fresh tempdir, run `holycc lint <dir>`, and
/// return (exit_code, stdout, stderr). Bin path is resolved via
/// CARGO_BIN_EXE_holycc — Cargo builds the binary before running
/// integration tests when this env var is referenced.
fn run_lint(files: &[File]) -> (i32, String, String) {
    let dir = tempdir();
    for f in files {
        let path = dir.join(f.name);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        let mut fh = std::fs::File::create(&path).unwrap();
        fh.write_all(f.body.as_bytes()).unwrap();
    }
    let bin = env!("CARGO_BIN_EXE_holycc");
    let output = Command::new(bin)
        .arg("lint")
        .arg(&dir)
        .output()
        .expect("failed to spawn holycc");
    let code = output.status.code().unwrap_or(-1);
    (
        code,
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    )
}

/// Cargo gives us a clean per-test target dir via `CARGO_TARGET_TMPDIR`,
/// but it's a single shared dir across all integration tests. Make a
/// per-test subdir keyed off the test's name (whatever the caller set).
fn tempdir() -> PathBuf {
    static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let base = std::env::temp_dir().join(format!(
        "holycc-cli-{}-{}",
        std::process::id(),
        n
    ));
    std::fs::create_dir_all(&base).unwrap();
    base
}

// ---------- clean tree ----------

#[test]
fn clean_tree_exits_zero() {
    let (code, _out, err) = run_lint(&[
        File {
            name: "A.HC",
            body: "U0 Hello() { Print(\"hi\"); }\n",
        },
        File {
            name: "B.HC",
            body: "U0 World() { Hello(); }\n",
        },
    ]);
    assert_eq!(code, 0, "expected exit 0; stderr={err}");
    assert!(err.contains("0 error(s)"), "stderr={err}");
    assert!(err.contains("2 file(s)"), "stderr={err}");
}

// ---------- diagnostic format ----------

#[test]
fn syntax_error_exits_one_with_path_line_col() {
    // Garbage at top-level produces a parser error. The output line
    // must use `path:line:col: severity: msg [rule]` shape so
    // `make lint` consumers can grep / jump in their editor.
    let (code, out, _err) = run_lint(&[File {
        name: "Bad.HC",
        body: "U0 F() { @@@@ }\n",
    }]);
    assert_eq!(code, 1, "expected exit 1 on syntax error; got {code}");
    let has_diagnostic = out.lines().any(|l| {
        // path:line:col: <error|warning>: ...
        let parts: Vec<&str> = l.splitn(4, ':').collect();
        parts.len() >= 4
            && parts[0].ends_with("Bad.HC")
            && parts[1].parse::<u32>().is_ok()
            && parts[2].parse::<u32>().is_ok()
    });
    assert!(has_diagnostic, "no path:line:col diag in stdout: {out}");
}

// ---------- cross-file resolution ----------

#[test]
fn cross_file_callee_resolves_when_pushed_first() {
    // A.HC defines Bootstrap helpers, B.HC consumes them. Files are
    // pushed alphabetically so A is registered first — the use in B
    // must resolve cleanly.
    let (code, _out, err) = run_lint(&[
        File {
            name: "A.HC",
            body: "U0 Helper() {}\n",
        },
        File {
            name: "B.HC",
            body: "U0 Caller() { Helper(); }\n",
        },
    ]);
    assert_eq!(code, 0, "expected exit 0; stderr={err}");
}

#[test]
fn cross_file_use_before_decl_in_push_order_errors() {
    // A.HC sorts first and uses a symbol defined in B.HC. The
    // resolver mirrors `temple-run.py`'s push semantics: a use in
    // file N can only see decls in files 1..=N. So this must fail.
    let (code, out, _err) = run_lint(&[
        File {
            name: "A.HC",
            body: "U0 Caller() { Helper(); }\n",
        },
        File {
            name: "B.HC",
            body: "U0 Helper() {}\n",
        },
    ]);
    assert_eq!(code, 1, "expected exit 1 on push-order violation");
    assert!(
        out.contains("unresolved-identifier"),
        "expected unresolved-identifier diagnostic in stdout: {out}"
    );
}

// ---------- empty input ----------

#[test]
fn no_files_in_dir_exits_nonzero() {
    // Empty dir → bin should exit with error code (no files found
    // is a usage error, not "everything is fine").
    let (code, _out, _err) = run_lint(&[]);
    assert_ne!(code, 0, "empty dir should not exit 0");
}
