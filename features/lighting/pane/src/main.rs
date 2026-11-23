//! Lighting as a process: the same `Lighting` Studio Architect links in,
//! served over vox to the app that started it (`extension.toml`'s
//! `[entry]`) — what a development copy runs.

#[cfg(not(target_arch = "wasm32"))]
use architect_extensions_dioxus::guest::GuestExt as _;

#[cfg(not(target_arch = "wasm32"))]
fn main() -> Result<(), architect_extensions::guest::GuestError> {
    architect_extensions::guest::Guest::extension(&ignition_pane::ext::Lighting).serve()
}

/// A browser links the extension into its app; there is no process.
#[cfg(target_arch = "wasm32")]
#[allow(
    clippy::missing_const_for_fn,
    reason = "a program's main may not be const"
)]
fn main() {}
