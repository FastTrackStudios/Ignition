//! The default busking bank: what the eight faders and the flash keys
//! play when the app opens.
//!
//! Laid out the way a busking page is laid out on a real desk, not the
//! way the engine is laid out. The conventions this follows, and where
//! they come from:
//!
//! * The first bank of faders holds *levels an operator rides*: a key
//!   (front) level, then one fader per family of the rig carrying the
//!   effect that family most often runs (Eos busking templates, the
//!   Avolites and grandMA3 busking pages, Schiller's *Busking*). An
//!   effect fader's level is that effect's intensity — how much of it is
//!   in the room — and the crossfade-weight semantics of
//!   `ignition_core::programmer` give exactly that.
//! * Effect rate and size are *global* masters, not per fader; the
//!   RATE / SIZE / SPEED faders on the right already are, so every
//!   library effect here is re-slaved to the `Rate` master on the way
//!   in, and one slider retimes the whole bank.
//! * Flash keys are momentary and *fire* rather than hold: white flash,
//!   a rig stab, a blinder hit. The two exceptions are the keys an
//!   operator reaches for when the *show* is wrong — the rig drop and
//!   the punt look — which are held for as long as the hand is down and
//!   gone the moment it comes up.
//! * The punt look is one key. Faces lit, warm, nothing moving — the
//!   state a stage can always be dropped into.
//!
//! Every effect is referenced from the library **by name**. The library
//! is curated elsewhere; the test at the bottom is what keeps this bank
//! honest against it.

use ignition_core::preset::Ref;
use ignition_core::recipe::{Band, Random};
use ignition_core::{Attribute, BumpKind, Recipe, RecipeApply, Selection, Speed};

/// A fader as the surface presents it: what it does, and what colour to
/// draw it. The colour is presentation and deliberately not part of
/// `ignition_core::Fader`.
pub struct FaderSpec {
    /// The label an operator reads at the desk. Short enough to sit
    /// under a 44px track without wrapping — the family is the label
    /// where the effect is the one that family runs, and the effect is
    /// the label where it is not (CHASE and SPARKLE both ride the wash).
    pub name: &'static str,
    pub css: &'static str,
    pub recipe: Recipe,
}

/// A momentary key.
pub struct KeySpec {
    pub label: &'static str,
    pub action: KeyAction,
}

/// What a key does while it is down.
pub enum KeyAction {
    /// Fires a bump — an envelope that retires itself.
    Flash(Selection, BumpKind),
    /// Holds a look at full until the key comes up.
    Hold(Recipe),
}

fn role(name: &str) -> Selection {
    Selection::Role(name.to_string())
}

/// The whole stage, as the roles that light it — not `Audience`, which
/// a stab or a drop should leave alone.
fn stage() -> Selection {
    Selection::Union(vec![
        role("Key"),
        role("Wash"),
        role("Back"),
        role("Movers"),
        role("Bars"),
        role("Beams"),
    ])
}

/// A library effect by name, re-slaved to the surface's `Rate` master
/// so one slider retimes the whole bank.
///
/// Panics on a name the library does not have. That is deliberate: a
/// bank that silently opened with an empty slot would be found out on a
/// stage, and the test below finds it out at build time instead.
fn effect(name: &str) -> Recipe {
    let mut recipe = ignition_core::effects::library()
        .remove(name)
        .unwrap_or_else(|| panic!("no effect named {name:?} in the library"));
    recipe.timing.speed = Speed::Master("Rate".into());
    recipe
}

/// A static look: a level and a colour on a role.
fn look(target: Selection, colour: &str, level: f32) -> Recipe {
    let mut recipe = Recipe::new(target, RecipeApply::Dimmer(level));
    recipe.steps[0]
        .apply
        .push(RecipeApply::Color(Ref::Named(colour.to_string())));
    recipe
}

/// A look on several roles at once — one recipe per role, folded into
/// one target so it sits under a single key.
fn stage_look(level: f32, colour: &str) -> Recipe {
    look(
        Selection::Union(vec![role("Key"), role("Wash"), role("Back")]),
        colour,
        level,
    )
}

/// The eight faders, left to right.
///
/// The order is the order an operator's hand learns: the level they
/// ride most on the left, the ones they reach for on a chorus in the
/// middle, the ones that are a decision on the right.
pub fn default_bank() -> Vec<FaderSpec> {
    vec![
        // 1. Front light. On every busking page this is the first fader
        //    — faces are what the audience came for, and the one level
        //    that is never an effect. Absolute, so at full it *is* the
        //    key light and at half it is halfway there from whatever
        //    the cue stack has.
        FaderSpec {
            name: "KEY",
            css: "#e8d8b8",
            recipe: look(role("Key"), "Warm White", 1.0),
        },
        // 2–3. The wash: the travelling chase and the twinkle, the two
        //    textures a wash runs most nights. Relative, so they ride on
        //    whatever colour the cue has the wash in.
        FaderSpec {
            name: "CHASE",
            css: "#48c8d8",
            recipe: effect("chase"),
        },
        FaderSpec {
            name: "SPARKLE",
            css: "#8fd0e8",
            recipe: effect("sparkle"),
        },
        // 4. Movers. Ballyhoo is what movers do when nobody has decided
        //    what they should do — which is most of a busked set.
        FaderSpec {
            name: "BALLY",
            css: "#d8a848",
            recipe: effect("ballyhoo"),
        },
        // 5. Bars (pixel strips). A chase along them is the one strip
        //    effect that reads from the back of the room.
        FaderSpec {
            name: "BARS",
            css: "#d84898",
            recipe: effect("strip chase"),
        },
        // 6. Back wall, breathing. Slow, so it sits under a verse
        //    without becoming the verse.
        FaderSpec {
            name: "BACK",
            css: "#5a48d8",
            recipe: effect("back breathe"),
        },
        // 7. Strobe. Far right of the effects, and the library's plain
        //    strobe — random pops are a different, rarer thing. Pushed
        //    up rather than fired, because on a fader the operator
        //    chooses how much strobe, and a strobe at 30% on top of a
        //    look is usable where a full strobe is a moment.
        FaderSpec {
            name: "STROBE",
            css: "#ffffff",
            recipe: effect("strobe"),
        },
        // 8. The audience. Blinders are the one fader an operator rides
        //    *up* for a whole chorus and not an effect at all: a level,
        //    open white. Last, at the edge, where a hand finds it in
        //    the dark.
        FaderSpec {
            name: "BLIND",
            css: "#fff0c0",
            recipe: look(role("Audience"), "Open White", 1.0),
        },
    ]
}

/// The second page: movement and colour.
///
/// Page one is levels and textures an operator rides all night; this
/// page is the things reached for on a chorus — the movers doing
/// something, the wash changing colour, the beams and strips running.
/// Same shape as page one so a hand that has learned "fader four is the
/// movers" is still right after a page turn: the *family* stays in the
/// slot, only what it plays changes.
// r[impl playback.pages] - a second page of eight assignments
pub fn movement_bank() -> Vec<FaderSpec> {
    vec![
        // 1. Movers, windmilling — the big shape.
        FaderSpec {
            name: "WINDMILL",
            css: "#d8a848",
            recipe: effect("windmill"),
        },
        // 2. Movers, a pan wave — the small shape, for a verse.
        FaderSpec {
            name: "PAN WAVE",
            css: "#e0b860",
            recipe: effect("pan wave"),
        },
        // 3–4. The wash in colour: a two-colour chase, then the
        //    rainbow that reads from the back of the room.
        FaderSpec {
            name: "2 COLOUR",
            css: "#d85a98",
            recipe: effect("two colour chase"),
        },
        FaderSpec {
            name: "RAINBOW",
            css: "#8860d8",
            recipe: effect("rainbow"),
        },
        // 5–6. Beams: zoom breathing and an iris chase, the two beam
        //    textures that read through haze.
        FaderSpec {
            name: "ZOOM",
            css: "#48c8d8",
            recipe: effect("zoom pulse"),
        },
        FaderSpec {
            name: "IRIS",
            css: "#6ee0d0",
            recipe: effect("iris chase"),
        },
        // 7. Bars, chasing — the same slot family as page one.
        FaderSpec {
            name: "STRIPS",
            css: "#d84898",
            recipe: effect("strip chase"),
        },
        // 8. Blinders chasing rather than held: still the audience, at
        //    the edge where a hand finds it.
        FaderSpec {
            name: "BLINDERS",
            css: "#fff0c0",
            recipe: effect("blinder chase"),
        },
    ]
}

/// A role's attribute driven by a sound band: `low` in silence, `high`
/// at full — the bass lifting the blinders. Relative where the band
/// should ride *on top of* whatever the cue set rather than replace it.
// r[impl playback.sound-as-value] - a band level as a fader's recipe
fn sound(
    target: Selection,
    band: Band,
    attr: Attribute,
    low: f32,
    high: f32,
    relative: bool,
) -> Recipe {
    Recipe::new(
        target,
        RecipeApply::Sound {
            band,
            attr,
            low,
            high,
            relative,
        },
    )
}

/// The third page: the no-chart busking case, where the room's own
/// sound drives the rig.
///
/// Every fader here reads a band of the audio input as a *value* — a
/// level, or a generator's range — through the sound fade the operator
/// sets in the transport bar. Nothing on this page needs a song map or
/// a chart; a support act with no show gets blinders on the kick and
/// sparkle on the hats from the first bar. Same slot families as the
/// other pages so a hand that has learned the shape is still right.
// r[impl playback.sound-as-value] - a page of sound-driven faders
pub fn sound_bank() -> Vec<FaderSpec> {
    vec![
        // 1. Faces breathe with the mids — the vocal range — but never
        //    drop below a working level: a lift on the key, not a gate.
        FaderSpec {
            name: "KEY MID",
            css: "#e8d8b8",
            recipe: sound(role("Key"), Band::Mid, Attribute::Dimmer, 0.0, 0.3, true),
        },
        // 2. The wash lifts on the mids over whatever colour and level
        //    the cue has it at — relative, so the look stays the look.
        FaderSpec {
            name: "WASH MID",
            css: "#48c8d8",
            recipe: sound(role("Wash"), Band::Mid, Attribute::Dimmer, 0.0, 0.5, true),
        },
        // 3. Sparkle whose density follows the highs: the hats bring
        //    the twinkle up, and it settles to nothing in a breakdown.
        FaderSpec {
            name: "SPARK HI",
            css: "#8fd0e8",
            recipe: sparkle_on(Band::High),
        },
        // 4. Movers open on the mids.
        FaderSpec {
            name: "MOVE MID",
            css: "#d8a848",
            recipe: sound(
                role("Movers"),
                Band::Mid,
                Attribute::Dimmer,
                0.0,
                1.0,
                false,
            ),
        },
        // 5. Bars flash on the highs.
        FaderSpec {
            name: "BARS HI",
            css: "#d84898",
            recipe: sound(role("Bars"), Band::High, Attribute::Dimmer, 0.0, 1.0, false),
        },
        // 6. The back wall thumps with the lows.
        FaderSpec {
            name: "BACK LOW",
            css: "#5a48d8",
            recipe: sound(role("Back"), Band::Low, Attribute::Dimmer, 0.1, 1.0, false),
        },
        // 7. Beams punch on the lows.
        FaderSpec {
            name: "BEAM LOW",
            css: "#ffffff",
            recipe: sound(role("Beams"), Band::Low, Attribute::Dimmer, 0.0, 1.0, false),
        },
        // 8. Blinders on the kick. The whole reason the page exists, at
        //    the edge where a hand finds it.
        FaderSpec {
            name: "BLND LOW",
            css: "#fff0c0",
            recipe: sound(
                role("Audience"),
                Band::Low,
                Attribute::Dimmer,
                0.0,
                1.0,
                false,
            ),
        },
    ]
}

/// A random twinkle on the wash whose *range* is a band's level: the
/// generator rolls between zero and `low + level × (high − low)`, so at
/// full the wash twinkles to full and in silence it lies still.
// r[impl playback.sound-as-value] - a generator's range
fn sparkle_on(band: Band) -> Recipe {
    let mut recipe = Recipe::new(
        role("Wash"),
        RecipeApply::Random(Random {
            attr: Attribute::Dimmer,
            low: 0.0,
            high: 1.0,
            high_from_band: Some(band),
            ..Random::default()
        }),
    );
    recipe.timing.speed = Speed::Master("Rate".into());
    recipe
}

/// Every page of the bank, in page order. Page one is what the app
/// opens on.
pub fn bank_pages() -> Vec<Vec<FaderSpec>> {
    vec![default_bank(), movement_bank(), sound_bank()]
}

/// The neutral stage-lit look — faces, wash and back at a working
/// level, warm, nothing moving. One key; held, not fired.
pub fn punt_look() -> Recipe {
    stage_look(0.6, "Warm White")
}

/// Everything on stage to zero while the key is down. A momentary
/// blackout the operator plays against a drop, or holds through a
/// mistake.
pub fn rig_drop() -> Recipe {
    Recipe::new(stage(), RecipeApply::Dimmer(0.0))
}

/// The flash keys, top to bottom.
pub fn flash_keys() -> Vec<KeySpec> {
    vec![
        // The one accent that reads through any colour.
        KeySpec {
            label: "WHITE",
            action: KeyAction::Flash(role("Wash"), BumpKind::White),
        },
        // A hit on the whole stage: what a snare wants.
        KeySpec {
            label: "STAB",
            action: KeyAction::Flash(stage(), BumpKind::Level),
        },
        // Blinders into the crowd, and gone.
        KeySpec {
            label: "BLIND",
            action: KeyAction::Flash(role("Audience"), BumpKind::White),
        },
        // The strobe as a moment rather than a level — a burst on the
        // wash. Where a strobe fader is a decision, this is a reflex.
        KeySpec {
            label: "BURST",
            action: KeyAction::Flash(role("Wash"), BumpKind::Burst),
        },
        // Held keys. Down is on; up is off; nothing lingers.
        KeySpec {
            label: "DROP",
            action: KeyAction::Hold(rig_drop()),
        },
        KeySpec {
            label: "PUNT",
            action: KeyAction::Hold(punt_look()),
        },
    ]
}

/// Whether the stage look touches only the dimmer and colour — a punt
/// that moved the movers would be a surprise, not a safe place.
#[allow(dead_code)]
fn is_static(recipe: &Recipe) -> bool {
    recipe.steps.len() == 1
        && recipe.steps[0].apply.iter().all(|a| {
            matches!(
                a,
                RecipeApply::Dimmer(_) | RecipeApply::Color(_) | RecipeApply::Raw(_)
            )
        })
        && !recipe.steps[0]
            .apply
            .iter()
            .any(|a| matches!(a, RecipeApply::Raw(v) if v.iter().any(|(attr, _)| *attr == Attribute::Pan || *attr == Attribute::Tilt)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use ignition_core::FADERS;

    /// Every role name the bank refers to, walked out of its selections.
    fn roles_in(selection: &Selection, out: &mut Vec<String>) {
        match selection {
            Selection::Role(name) => out.push(name.clone()),
            Selection::Union(items) | Selection::Intersect(items) => {
                items.iter().for_each(|s| roles_in(s, out))
            }
            Selection::Except { of, minus } => {
                roles_in(of, out);
                roles_in(minus, out);
            }
            Selection::Where { of, .. } | Selection::Order { of, .. } => roles_in(of, out),
            _ => {}
        }
    }

    fn profile() -> ignition_core::Profile {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../data/profiles/ignition.ig-profile"
        );
        serde_json::from_str(&std::fs::read_to_string(path).expect("profile file"))
            .expect("profile parses")
    }

    #[test]
    fn the_bank_fills_every_fader_and_no_more() {
        for (page, bank) in bank_pages().iter().enumerate() {
            assert_eq!(bank.len(), FADERS, "page {page}");
        }
    }

    /// The library is curated by someone else; this is the contract.
    #[test]
    fn every_effect_the_bank_names_is_in_the_library() {
        let library = ignition_core::effects::library();
        for name in [
            "chase",
            "sparkle",
            "ballyhoo",
            "strip chase",
            "back breathe",
            "strobe",
            "windmill",
            "pan wave",
            "two colour chase",
            "rainbow",
            "zoom pulse",
            "iris chase",
            "blinder chase",
        ] {
            assert!(library.contains_key(name), "library has no {name:?}");
        }
        // And building the bank does not panic.
        let _ = bank_pages();
        let _ = flash_keys();
    }

    /// Two pages, and a label an operator can read under a 44px track.
    /// r[verify playback.pages]
    #[test]
    fn the_bank_has_a_second_page_with_short_labels() {
        let pages = bank_pages();
        assert!(pages.len() >= 2);
        for bank in &pages {
            for spec in bank {
                assert!(spec.name.len() <= 8, "{:?} will wrap", spec.name);
            }
        }
    }

    #[test]
    fn every_role_the_bank_uses_is_declared_by_the_profile() {
        let profile = profile();
        let declared: Vec<&str> = profile
            .roles
            .iter()
            .filter(|r| r.kind == ignition_core::RoleKind::Group)
            .map(|r| r.name.as_str())
            .collect();
        let mut used = Vec::new();
        for spec in bank_pages().into_iter().flatten() {
            roles_in(&spec.recipe.target, &mut used);
        }
        for key in flash_keys() {
            match key.action {
                KeyAction::Flash(sel, _) => roles_in(&sel, &mut used),
                KeyAction::Hold(recipe) => roles_in(&recipe.target, &mut used),
            }
        }
        // `Key`, `Wash` and `Back` are required by the profile; the rest
        // are optional, and a fader on a role the venue has not bound
        // resolves to nothing and simply does nothing — see
        // `r[profile.required-and-optional]`.
        for name in ["Key", "Wash", "Back", "Movers", "Bars", "Beams", "Audience"] {
            used.push(name.to_string());
        }
        for role in used {
            assert!(
                declared.contains(&role.as_str()),
                "role {role:?} is not in the default profile"
            );
        }
    }

    /// One slider retimes the whole bank: every effect on it is slaved
    /// to the `Rate` master rather than to whatever the library chose.
    #[test]
    fn every_effect_fader_follows_the_rate_master() {
        for spec in bank_pages().into_iter().flatten() {
            if spec.recipe.steps.len() > 1 {
                assert_eq!(
                    spec.recipe.timing.speed,
                    Speed::Master("Rate".into()),
                    "{} is not on the Rate master",
                    spec.name
                );
            }
        }
    }

    /// The colours the looks name ship with the profile.
    #[test]
    fn the_looks_use_colours_the_profile_ships() {
        let profile = profile();
        let colours: Vec<&str> = profile.colors.iter().map(|c| c.name.as_str()).collect();
        for name in ["Warm White", "Open White"] {
            assert!(colours.contains(&name), "profile has no colour {name:?}");
        }
    }

    /// Every fader on the sound page reads a band, on a role the
    /// profile declares — the page is the no-chart case and has to
    /// work at a venue that has bound only the required roles.
    /// r[verify playback.sound-as-value]
    #[test]
    fn the_sound_page_reads_a_band_on_every_fader() {
        let profile = profile();
        let declared: Vec<&str> = profile.roles.iter().map(|r| r.name.as_str()).collect();
        let page = sound_bank();
        assert_eq!(page.len(), FADERS);
        let mut bands = std::collections::BTreeSet::new();
        for spec in &page {
            assert!(spec.name.len() <= 8, "{:?} will wrap", spec.name);
            let heard = spec
                .recipe
                .steps
                .iter()
                .flat_map(|s| s.apply.iter())
                .any(|a| match a {
                    RecipeApply::Sound { band, .. } => {
                        bands.insert(format!("{band:?}"));
                        true
                    }
                    RecipeApply::Random(r) => {
                        if let Some(band) = r.high_from_band {
                            bands.insert(format!("{band:?}"));
                        }
                        r.high_from_band.is_some()
                    }
                    _ => false,
                });
            assert!(heard, "{} reads no band", spec.name);
            let mut roles = Vec::new();
            roles_in(&spec.recipe.target, &mut roles);
            assert!(!roles.is_empty(), "{} targets no role", spec.name);
            for role in roles {
                assert!(
                    declared.contains(&role.as_str()),
                    "{role:?} is not a profile role"
                );
            }
        }
        // The kick, the vocal and the hats are all used.
        assert_eq!(bands.len(), 3, "{bands:?}");
        // Blinders on the low band, the case the spec names.
        let blind = page
            .iter()
            .find(|s| s.name.starts_with("BLND"))
            .expect("blinders");
        assert!(matches!(
            blind.recipe.steps[0].apply[0],
            RecipeApply::Sound {
                band: Band::Low,
                attr: Attribute::Dimmer,
                ..
            }
        ));
        assert_eq!(blind.recipe.target, role("Audience"));
    }

    #[test]
    fn the_punt_look_is_static_and_lit() {
        let punt = punt_look();
        assert!(is_static(&punt));
        assert!(
            punt.steps[0]
                .apply
                .iter()
                .any(|a| matches!(a, RecipeApply::Dimmer(l) if (*l - 0.6).abs() < 1e-6))
        );
        assert!(is_static(&rig_drop()));
    }
}
