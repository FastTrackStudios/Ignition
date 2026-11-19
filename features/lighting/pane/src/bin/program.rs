//! The programming actions from a shell, with no app running:
//!
//!     ignition-program IGNITION_SONG_LIST '{"show": "jt-recital"}'
//!     ignition-program IGNITION_ACTIONS
//!
//! The same `run` the extension registers, so what works here works from
//! the app's CLI, MCP and Luau (`actions.run`). `IGNITION_ROOT` names the
//! lighting data root (default: the current folder).

use std::process::ExitCode;

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    let Some(id) = args.next() else {
        eprintln!("usage: ignition-program <ACTION> ['<json arguments>']");
        return ExitCode::from(2);
    };
    let raw = args.next().unwrap_or_else(|| "{}".to_owned());
    let parsed = match serde_json::from_str(&raw) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("arguments: {e}");
            return ExitCode::from(2);
        }
    };
    match ignition_pane::program::run(&id, &parsed) {
        Ok(out) => {
            println!("{}", serde_json::to_string_pretty(&out).unwrap_or_default());
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("{id}: {e}");
            ExitCode::FAILURE
        }
    }
}
