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

use ignition_show::Speed;
use ignition_show::profile::Profile;

/// The profile as shipped, read from `data/profiles/`.
fn default_profile() -> Profile {
    Profile::load(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../../data/profiles/ignition.ig-profile"
    ))
    .expect("the shipped default profile loads")
}

/// The default profile ships the default library, and every entry
/// in it is written against the `Song` master.
///
/// A vocabulary of roles without the effects that use them is half
/// a profile. And the library is what makes "one cycle per bar"
/// one cycle per bar *of this song* — an entry slaved to anything
/// else would keep its own time while the show followed the music.
///
/// r[verify effects.library.profile-ships-it]
/// r[verify effects.masters.song]
#[test]
fn the_default_profile_ships_the_library_written_against_song() {
    let profile = default_profile();
    let library = ignition_effects::effects::library();

    assert!(!library.is_empty(), "the built-in library is empty");
    for name in library.keys() {
        assert!(
            profile.effects.contains_key(name),
            "the shipped profile is missing the library's `{name}`"
        );
    }
    assert!(
        !profile.effect_notes.is_empty(),
        "the notes ship beside the recipes, so a chooser can read them"
    );

    // Every library entry keeps the song's time. `Scaled` is the
    // same master at a multiple — the strobe running double off the
    // same tap — which is still the song.
    for (name, recipe) in &library {
        let master = match &recipe.timing.speed {
            Speed::Master(m) => Some(m.as_str()),
            Speed::Scaled { master, .. } => Some(master.as_str()),
            _ => None,
        };
        if let Some(master) = master {
            assert!(
                master == "Song" || master == "Tap",
                "`{name}` is slaved to `{master}`, which is neither the song nor the tap"
            );
        }
    }
}
