//! Tests that span two layers of the domain, and so live in the upper one.
//!
//! Each of these was a unit test inside `ignition-show` (or
//! `ignition-rig`) that reached *up* the layering for something it
//! asserted about — the shipped effect library, the desk macros, the
//! recipe type. Once those layers became separate crates the reach up
//! stopped being possible: a `#[cfg(test)]` module compiles its own copy
//! of its crate, so the types it saw through a dev-dependency were a
//! second, incompatible `ignition_show`.
//!
//! An integration test links the real library instead, which is why
//! these belong here. The layering did not change; where the test is
//! allowed to sit did.

// Integration test: `clippy.toml`'s test allowances only reach
// `#[cfg(test)]` modules, so the panic set is lifted here instead.
// See docs/ops/clippy.md.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::arithmetic_side_effects,
    clippy::as_conversions,
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "integration test — see docs/ops/clippy.md"
)]

use ignition_show::Show;
use ignition_show::profile::{FaderSource, Profile};

/// The profile as shipped, read from `data/profiles/`.
fn default_profile() -> Profile {
    Profile::load(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../../data/profiles/ignition.ig-profile"
    ))
    .expect("the shipped default profile loads")
}

/// The busking programming ships in the file, baked from
/// `macros::shipped`, and the file agrees with the code.
/// r[verify profile.looks]
/// r[verify profile.macros]
/// r[verify profile.pages]
/// r[verify profile.speed-routing]
#[test]
fn the_default_profile_ships_the_busking_programming() {
    let p = default_profile();
    let shipped = ignition_playback::macros::shipped();
    assert_eq!(p.looks, shipped.looks);
    assert_eq!(p.macros, shipped.macros);
    assert_eq!(p.pages, shipped.pages);
    assert_eq!(p.protected, shipped.protected);
    assert_eq!(p.speed_routing, shipped.speed_routing);
    for name in ["verse bed", "chorus full", "punt", "blackout"] {
        assert!(p.looks.contains_key(name), "look {name}");
    }
    for name in ["drop", "build 8", "breakdown", "end"] {
        assert!(p.macros.contains_key(name), "macro {name}");
    }
    assert_eq!(p.pages.len(), 4);
    // Every effect a page or a look or a macro names is in the file's library.
    let show = Show {
        library: &p.effects,
        bundles: &p.bundles,
        ..Show::new(&[], &ignition_rig::selection::EMPTY_RIG)
    };
    for page in &p.pages {
        for spec in &page.faders {
            if let FaderSource::Effect(name) = &spec.source {
                assert!(p.effects.contains_key(name), "{}: {name}", spec.label);
            }
        }
    }
    for (name, look) in &p.looks {
        for r in &look.recipes {
            assert!(
                r.missing(&show).is_empty(),
                "look {name}: {:?}",
                r.missing(&show)
            );
        }
    }
}
