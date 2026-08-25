//! Embeds the operator overlay's font.
//!
//! `flake.nix` puts a DejaVu path in `IGNITION_OVERLAY_FONT`; this
//! copies it into `OUT_DIR` so `overlay.rs` can `include_bytes!` it. The
//! font therefore ends up *inside* the binary rather than referenced by
//! a nix store path, which would be machine-specific — a visualizer that
//! only drew its own UI inside the dev shell would be a poor trade for
//! one font.
//!
//! Building without the variable set is not an error: the overlay falls
//! back to Bevy's built-in font, which draws text fine and only loses
//! the cooked-status markers.

use std::path::PathBuf;

fn main() {
    println!("cargo::rerun-if-env-changed=IGNITION_OVERLAY_FONT");
    println!("cargo::rustc-check-cfg=cfg(has_overlay_font)");

    let out = PathBuf::from(std::env::var_os("OUT_DIR").expect("OUT_DIR"));
    let dest = out.join("overlay-font.ttf");

    let Some(src) = std::env::var_os("IGNITION_OVERLAY_FONT") else {
        // Leave any previously-copied font in place rather than half a
        // build: `include_bytes!` needs the file to exist or not, and
        // the cfg below is what decides which.
        let _ = std::fs::remove_file(&dest);
        println!(
            "cargo::warning=IGNITION_OVERLAY_FONT unset; the overlay will use Bevy's \
             built-in font and its status markers will not render. Build inside \
             `nix develop`."
        );
        return;
    };

    let src = PathBuf::from(src);
    println!("cargo::rerun-if-changed={}", src.display());
    match std::fs::copy(&src, &dest) {
        Ok(_) => println!("cargo::rustc-cfg=has_overlay_font"),
        Err(e) => println!(
            "cargo::warning=could not read IGNITION_OVERLAY_FONT at {}: {e}",
            src.display()
        ),
    }
}
