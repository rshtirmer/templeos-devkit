//! Sanity that the diag pipeline produces VM-format strings.

use holyc_parser::diag::{Diag, DiagBag, Severity};

#[test]
fn diag_format_matches_holyc_lint_py_output() {
    // holyc-lint.py emits: "path:line:col: [error|warning]: message [rule]".
    // Our Display impl must match so consumers (make lint, editors) can
    // parse both linters' output uniformly.
    let d = Diag {
        file: "src/QuakeMath.ZC".into(),
        line: 42,
        col: 7,
        severity: Severity::Error,
        rule: "comma-decl-list",
        message: "split into separate declarations".into(),
    };
    assert_eq!(
        format!("{d}"),
        "src/QuakeMath.ZC:42:7: error: split into separate declarations [comma-decl-list]"
    );
}

#[test]
fn diag_bag_counts_errors_only() {
    let mut bag = DiagBag::new();
    bag.push(mk("rule-a", Severity::Error));
    bag.push(mk("rule-b", Severity::Warning));
    bag.push(mk("rule-c", Severity::Error));
    assert_eq!(bag.errors(), 2);
    assert_eq!(bag.iter().count(), 3);
}

fn mk(rule: &'static str, severity: Severity) -> Diag {
    Diag {
        file: "x".into(),
        line: 1,
        col: 1,
        severity,
        rule,
        message: "m".into(),
    }
}
