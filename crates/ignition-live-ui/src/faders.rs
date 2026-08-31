//! The busking bank: what the eight faders and the keys play when the
//! app opens.
//!
//! The bank is **data**. Every page — its labels, what each fader plays,
//! its filter, its speed, the parameters it exposes — is declared in the
//! profile (`Profile::pages`, `r[profile.pages]`), and this module only
//! resolves that declaration into the engine's `Fader`s and gives each a
//! colour to draw. The profile the studio resolves through is the
//! shipped one (`ignition_core::macros::shipped_profile`), the same
//! source `bake-profile` writes into `data/profiles/ignition.ig-profile`,
//! so what the app opens on and what the file says cannot drift.
//!
//! The conventions the shipped pages follow, and where they come from:
//!
//! * The first page holds *levels an operator rides*: a key (front)
//!   level, then one fader per family of the rig carrying the effect
//!   that family most often runs (Eos busking templates, the Avolites
//!   and grandMA3 busking pages, Schiller's *Busking*). An effect
//!   fader's level is that effect's intensity — how much of it is in the
//!   room — and the crossfade-weight semantics of
//!   `ignition_core::programmer` give exactly that.
//! * Speed is routed by family (`r[profile.speed-routing]`): the movers
//!   halve the tap, the strobe doubles it, levels and colours follow
//!   the song — so one tapped tempo retimes the whole bank, each family
//!   at the rate it wants.
//! * Flash keys are momentary and *fire* rather than hold: white flash,
//!   a rig stab, a blinder hit. The two exceptions are the keys an
//!   operator reaches for when the *show* is wrong — the rig drop and
//!   the punt look — which are held for as long as the hand is down and
//!   gone the moment it comes up.
//! * Macro keys run a profile macro (`r[profile.macros]`) — the drop,
//!   the build, the breakdown, the end — and look keys latch a profile
//!   look (`r[profile.looks]`) on the held layer until pressed again.

use ignition_core::profile::{Param, Profile};
use ignition_core::selection::EMPTY_RIG;
use ignition_core::{BumpKind, Fader, Recipe, RecipeApply, Selection, Show};

/// The profile the bank is resolved through: the shipped library and
/// busking programming, as `bake-profile` writes them, with the looks
/// authored at the desk laid over (`r[profile.looks.authored]`). See
/// `library::CURRENT` for why it is a leaked `&'static` behind a lock.
static CURRENT: std::sync::RwLock<Option<&'static Profile>> = std::sync::RwLock::new(None);

/// The baked profile plus the authored overlay. An overlay that cannot
/// be read (no disk, in the browser) is an empty one.
// r[impl profile.looks.authored] - merged over the bake
fn baked_with_authored() -> Profile {
    let mut profile = ignition_core::macros::shipped_profile();
    match ignition_core::profile::AuthoredLooks::load(crate::library::looks_path()) {
        Ok(authored) => profile.merge_looks(&authored),
        Err(error) => tracing::warn!(%error, "authored looks not read; the bank shows the bake"),
    }
    profile
}

/// The profile behind the bank — for the widget, which resolves looks
/// and macros through the same one.
pub fn profile() -> &'static Profile {
    if let Some(current) = *CURRENT.read().unwrap_or_else(|e| e.into_inner()) {
        return current;
    }
    let mut slot = CURRENT.write().unwrap_or_else(|e| e.into_inner());
    if let Some(current) = *slot {
        return current;
    }
    let loaded: &'static Profile = Box::leak(Box::new(baked_with_authored()));
    *slot = Some(loaded);
    loaded
}

/// Re-reads the authored looks over the bake. `library::reload_authored_looks`
/// calls this; hosts call that.
pub fn reload_authored_looks() {
    let fresh: &'static Profile = Box::leak(Box::new(baked_with_authored()));
    *CURRENT.write().unwrap_or_else(|e| e.into_inner()) = Some(fresh);
}

/// A fader as the surface presents it: what it does, and what colour to
/// draw it. The colour is presentation and deliberately not part of
/// `ignition_core::Fader`.
pub struct FaderSpec {
    /// The label an operator reads at the desk, from the profile.
    pub name: String,
    pub css: &'static str,
    /// The main recipe, for tests that read what the fader plays.
    #[allow(dead_code)]
    pub recipe: Recipe,
    /// The whole fader as resolved — filter, parameters, further
    /// recipes of a bundle or look, or a role master. What the host
    /// assigns, so all of it reaches the engine.
    pub fader: Fader,
    /// The parameters the page declared — name, range, default — for
    /// the surface to draw a control per parameter.
    // r[impl profile.effect-parameters] - the declaration travels with the fader
    pub params: Vec<Param>,
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
    ///
    /// Boxed: a `Recipe` dwarfs `Flash`'s selection-and-kind pair, and
    /// the key table is an array of these.
    Hold(Box<Recipe>),
}

/// A key that names something in the profile — a macro or a look.
pub struct NamedKey {
    pub label: &'static str,
    /// The `Profile::macros` or `Profile::looks` key.
    pub name: &'static str,
}

fn role(name: &str) -> Selection {
    Selection::Role(name.to_string())
}

/// The whole stage, as the roles that light it — not `Audience`, which
/// a stab or a drop should leave alone, and never the house.
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

/// The colour a fader is drawn in, by what it plays. Families keep
/// their colour across pages so a hand that has learned "amber is the
/// movers" is still right after a page turn.
fn css_for(label: &str) -> &'static str {
    match label {
        "KEY" | "KEY MID" => "#e8d8b8",
        "CHASE" | "WASH MID" | "ZOOM" => "#48c8d8",
        "SPARKLE" | "SPARK HI" => "#8fd0e8",
        "BALLY" | "WINDMILL" | "MOVE MID" => "#d8a848",
        "PAN WAVE" => "#e0b860",
        "BARS" | "STRIPS" | "BARS HI" => "#d84898",
        "BACK" | "BACK LOW" => "#5a48d8",
        "STROBE" | "BEAM LOW" | "WHITE" => "#ffffff",
        "BLIND" | "BLINDERS" | "BLND LOW" => "#fff0c0",
        "2 COLOUR" | "WIPE" | "WARM" => "#d85a98",
        "RAINBOW" | "SAT" | "CLR BUMP" => "#8860d8",
        "IRIS" => "#6ee0d0",
        "FIRE" => "#e07040",
        _ => "#c0b8d0",
    }
}

/// The show the bank resolves through: no rig, no groups — a library
/// effect written against roles needs neither to be *looked up*, only
/// to be played.
fn resolving_show(profile: &Profile) -> Show<'_> {
    Show {
        library: &profile.effects,
        bundles: &profile.bundles,
        looks: &profile.looks,
        ..Show::new(&[], &EMPTY_RIG)
    }
}

/// Every page of the bank, in page order, from the profile's pages.
/// Page one is what the app opens on.
// r[impl profile.pages] - the bank is the profile's pages, resolved
// r[impl playback.pages]
pub fn bank_pages() -> Vec<Vec<FaderSpec>> {
    bank_pages_of(profile())
}

/// The bank a given profile declares.
pub fn bank_pages_of(profile: &Profile) -> Vec<Vec<FaderSpec>> {
    let show = resolving_show(profile);
    profile
        .pages
        .iter()
        .map(|page| {
            page.faders
                .iter()
                .map(|spec| {
                    let fader = profile.resolve_fader(spec, &show);
                    FaderSpec {
                        name: spec.label.clone(),
                        css: css_for(&spec.label),
                        recipe: fader.recipe.clone().unwrap_or_default(),
                        fader,
                        params: spec.params.clone(),
                    }
                })
                .collect()
        })
        .collect()
}

/// The names of the pages, for the surface's page indicator.
pub fn page_names() -> Vec<String> {
    profile().pages.iter().map(|p| p.name.clone()).collect()
}

/// The neutral stage-lit look — faces, wash and back at a working
/// level, warm, nothing moving. One key; held, not fired. The
/// profile's `punt` look.
// r[impl profile.looks] - the punt is a profile look
pub fn punt_look() -> Recipe {
    let profile = profile();
    profile
        .look_recipes("punt", &resolving_show(profile))
        .into_iter()
        .next()
        .expect("the shipped profile has a punt look")
}

/// Everything on stage to zero while the key is down. A momentary
/// blackout the operator plays against a drop, or holds through a
/// mistake. Never the house — see `r[profile.protected-roles]`.
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
            action: KeyAction::Hold(Box::new(rig_drop())),
        },
        KeySpec {
            label: "PUNT",
            action: KeyAction::Hold(Box::new(punt_look())),
        },
    ]
}

/// The macro keys: one press runs the whole move.
// r[impl playback.macro-runner] - the surface's macro keys
pub fn macro_keys() -> Vec<NamedKey> {
    vec![
        NamedKey {
            label: "DROP",
            name: "drop",
        },
        NamedKey {
            label: "BUILD 8",
            name: "build 8",
        },
        NamedKey {
            label: "BREAK",
            name: "breakdown",
        },
        NamedKey {
            label: "END",
            name: "end",
        },
    ]
}

/// The look keys: press to take, press again to let go.
// r[impl playback.look-hold] - the surface's look keys
pub fn look_keys() -> Vec<NamedKey> {
    vec![
        NamedKey {
            label: "VERSE",
            name: "verse bed",
        },
        NamedKey {
            label: "CHORUS",
            name: "chorus full",
        },
        NamedKey {
            label: "PUNT",
            name: "punt",
        },
        NamedKey {
            label: "BLACK",
            name: "blackout",
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
            .any(|a| matches!(a, RecipeApply::Raw(v) if v.iter().any(|(attr, _)| *attr == ignition_core::Attribute::Pan || *attr == ignition_core::Attribute::Tilt)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use ignition_core::profile::FaderSource;
    use ignition_core::recipe::Band;
    use ignition_core::{Attribute, FADERS, Speed};

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

    fn file_profile() -> ignition_core::Profile {
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
        assert_eq!(bank_pages().len(), 4);
        assert_eq!(page_names(), vec!["busking", "movement", "sound", "colour"]);
    }

    /// The library is curated by someone else; this is the contract:
    /// every effect a page names is in it, and resolves to a recipe.
    #[test]
    fn every_effect_the_bank_names_is_in_the_library() {
        let library = ignition_core::effects::library();
        for page in &profile().pages {
            for spec in &page.faders {
                if let FaderSource::Effect(name) = &spec.source {
                    assert!(library.contains_key(name), "library has no {name:?}");
                }
            }
        }
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
            "colour wipe",
            "colour fire",
            "warm chase",
            "saturation breathe",
            "white pop",
            "colour bump",
        ] {
            assert!(library.contains_key(name), "library has no {name:?}");
        }
        for bank in bank_pages() {
            for spec in bank {
                assert!(
                    spec.fader.recipe.is_some() || spec.fader.master.is_some(),
                    "{} resolves to nothing",
                    spec.name
                );
            }
        }
        let _ = flash_keys();
    }

    /// The bank the app opens on is the bank the file declares.
    /// r[verify profile.pages]
    #[test]
    fn the_bank_matches_the_shipped_profile_file() {
        let file = file_profile();
        assert_eq!(file.pages, profile().pages);
        let from_file = bank_pages_of(&file);
        let from_code = bank_pages();
        for (a, b) in from_file.iter().flatten().zip(from_code.iter().flatten()) {
            assert_eq!(a.name, b.name);
            assert_eq!(a.fader, b.fader);
        }
    }

    /// Four pages, and a label an operator can read under a 44px track.
    /// r[verify playback.pages]
    /// r[verify profile.pages.label-fits]
    #[test]
    fn the_bank_has_more_pages_with_short_labels() {
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
        let profile = file_profile();
        let declared: Vec<&str> = profile
            .roles
            .iter()
            .filter(|r| r.kind == ignition_core::RoleKind::Group)
            .map(|r| r.name.as_str())
            .collect();
        let mut used = Vec::new();
        for spec in bank_pages().into_iter().flatten() {
            for recipe in spec.fader.recipes() {
                roles_in(&recipe.target, &mut used);
            }
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

    /// One tapped tempo retimes the whole bank: every effect on it is
    /// slaved to a master — routed by family — rather than to whatever
    /// the library chose.
    /// r[verify profile.speed-routing]
    #[test]
    fn every_effect_fader_follows_a_master() {
        for spec in bank_pages().into_iter().flatten() {
            if spec.recipe.steps.len() > 1 {
                assert!(
                    matches!(
                        spec.recipe.timing.speed,
                        Speed::Master(_) | Speed::Scaled { .. }
                    ),
                    "{} is not on a master: {:?}",
                    spec.name,
                    spec.recipe.timing.speed
                );
            }
        }
        // The movers halve the tap; the strobe doubles it.
        let busking = &bank_pages()[0];
        assert_eq!(
            busking[3].recipe.timing.speed,
            Speed::Scaled {
                master: "Tap".into(),
                scale: 0.5
            },
            "BALLY"
        );
        assert_eq!(
            busking[6].recipe.timing.speed,
            Speed::Scaled {
                master: "Tap".into(),
                scale: 2.0
            },
            "STROBE"
        );
    }

    /// The colours the looks name ship with the profile.
    #[test]
    fn the_looks_use_colours_the_profile_ships() {
        let profile = file_profile();
        let colours: Vec<&str> = profile.colors.iter().map(|c| c.name.as_str()).collect();
        for name in ["Warm White", "Open White", "Hot", "Deep"] {
            assert!(colours.contains(&name), "profile has no colour {name:?}");
        }
    }

    /// Every fader on the sound page reads a band, on a role the
    /// profile declares — the page is the no-chart case and has to
    /// work at a venue that has bound only the required roles.
    /// r[verify playback.sound-as-value]
    #[test]
    fn the_sound_page_reads_a_band_on_every_fader() {
        let profile = file_profile();
        let declared: Vec<&str> = profile.roles.iter().map(|r| r.name.as_str()).collect();
        let page = bank_pages().remove(2);
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

    /// The colour page is filtered to colour where it chases colour,
    /// so a rainbow on the movers cannot move them.
    /// r[verify profile.attribute-filter]
    #[test]
    fn the_colour_page_is_filtered_to_colour() {
        let colour = bank_pages().remove(3);
        for spec in colour.iter().take(6) {
            assert_eq!(
                spec.fader.filter,
                ignition_core::AttrFilter::COLOUR,
                "{}",
                spec.name
            );
        }
    }

    /// Every macro and look key names something the profile ships.
    /// r[verify profile.macros]
    /// r[verify profile.looks]
    #[test]
    fn every_macro_and_look_key_names_something_in_the_profile() {
        let profile = file_profile();
        for key in macro_keys() {
            assert!(
                profile.macros.contains_key(key.name),
                "macro {:?}",
                key.name
            );
            assert!(key.label.len() <= 8);
        }
        for key in look_keys() {
            assert!(profile.looks.contains_key(key.name), "look {:?}", key.name);
            assert!(key.label.len() <= 8);
        }
        // The drop macro is the strobe/blinder/fly-out move that lets go.
        assert!(matches!(
            profile.macros["drop"].steps.last(),
            Some(ignition_core::profile::MacroStep::Release)
        ));
    }
}
