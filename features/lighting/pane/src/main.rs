//! Lighting as a process: the same `Lighting` Studio Architect links in,
//! served over vox to the app that started it (`extension.toml`'s
//! `[entry]`) — what a development copy runs.

use architect_extensions::guest::{Guest, GuestError};

fn main() -> Result<(), GuestError> {
    Guest::extension(&ignition_pane::ext::Lighting).serve()
}
