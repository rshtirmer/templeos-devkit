//! holycc — CLI driver. `holycc lint <paths...>` runs the front-end
//! over each file and prints diagnostics. Exit code = error count
//! (clamped to 1 for shell-friendly use).

use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() {
        eprintln!("usage: holycc lint <file-or-dir>...");
        return ExitCode::from(2);
    }
    match args[0].as_str() {
        "lint" => {
            // TODO: walk paths, lex/parse each .HC/.ZC, print diagnostics.
            eprintln!("holycc lint: not yet implemented (scaffold only)");
            ExitCode::from(0)
        }
        cmd => {
            eprintln!("holycc: unknown command '{cmd}'");
            ExitCode::from(2)
        }
    }
}
