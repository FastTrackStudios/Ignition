//! Recipes — this project's foundation for building cues, the same role
//! grandMA3's Preset system plays: a `Recipe` pairs a *target* (a `Group`
//! or an explicit channel list) with an *apply* (a `Dimmer` level, a
//! `Color`, a `FocusPoint`, or a `Raw` attribute list), and `expand_cue`
//! compiles a list of them down into the flat `Cue`/`CueValue` format
//! `CuePlayer` already plays back — recipes are an authoring-time layer on
//! top of the existing tracking engine, not a change to it. This is
//! deliberately the same split GDTF/OFL import already established
//! elsewhere in this project: a rich, expressive input format compiled
//! down to a small, well-tested runtime representation, rather than
//! teaching the runtime every input format's own concepts.
//!
//! grandMA3 also has "Phaser" recipes — effect generators (waveforms
//! driving an attribute across a group's fixtures with per-fixture phase
//! offset) — which are a genuinely different kind of thing (a continuous
//! function of time, not a fixed target state) and are **not** built here;
//! this module is the static-cue half of the roadmap the operator laid
//! out (`docs/research/lighting-console-landscape.md`'s cue-list Slice),
//! Phasers are the deliberately deferred next slice.

use crate::cue::{Cue, CueValue};
use crate::focus::{pan_tilt_deg_along, pan_tilt_deg_to_point};
use crate::group::{self, Group};
use crate::preset::{ColorPreset, Palettes, Ref};
use ignition_proto::{Attribute, ChanId, ColorChannel, Placement};
use serde::{Deserialize, Serialize};

/// Who a recipe applies to.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum RecipeTarget {
    /// Looked up by name against the venue's real groups
    /// (`ignition_viz::venue::Venue::groups`, sourced from Eos's own
    /// `groups.json`) — unknown name resolves to no fixtures rather than
    /// erroring, matching this project's existing "falls back to nothing
    /// rather than panicking" tolerance for unresolvable references
    /// (`channel_map_for`, `shape_for`, ...).
    Group(String),
    /// An explicit, inline channel list — for a one-off cue that doesn't
    /// warrant naming a reusable group.
    Chans(Vec<ChanId>),
}

/// What to apply to the target.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum RecipeApply {
    Dimmer(f32),
    /// A pooled colour by name (`"House Blue"`), or one written inline.
    /// A name the venue's palette does not carry resolves to nothing and
    /// the recipe is skipped, same tolerance as an unknown group.
    Color(Ref<ColorPreset>),
    /// A real XYZ room location — resolved per-fixture via each target
    /// fixture's actual `Placement` (see `focus.rs`), not a single shared
    /// pan/tilt value. Fixtures with no known `Placement` (an unpatched or
    /// unrecognized channel) are silently skipped, same tolerance as the
    /// rest of this module.
    FocusPoint(Ref<ignition_proto::Vec3>),
    /// A shared world-space *direction* rather than a shared point, so
    /// every fixture in the group ends up beam-parallel with the others
    /// instead of converging. Not expressible as a `FocusPoint` at any
    /// finite distance. Fixtures with no known `Placement` are skipped,
    /// same as `FocusPoint`.
    FocusDirection(ignition_proto::Vec3),
    /// Escape hatch for anything not modelled as its own `RecipeApply`
    /// variant yet — the same role `Attribute::Custom` plays one level
    /// down.
    Raw(Vec<(Attribute, f32)>),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Recipe {
    pub target: RecipeTarget,
    pub apply: RecipeApply,
}

/// A cue authored as a list of recipes rather than raw `CueValue`s —
/// `expand_cue`/`expand_cue_list` compile this into the `Cue` type
/// `CuePlayer` actually plays back.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RecipeCue {
    pub name: String,
    #[serde(default)]
    pub fade_secs: f32,
    pub recipes: Vec<Recipe>,
}

/// A whole show authored as recipe cues — the `RecipeCue` counterpart to
/// `cue::CueList`. `expand_cue_list` compiles `cues` into the flat form
/// `CuePlayer` plays back.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct RecipeCueList {
    #[serde(default)]
    pub name: String,
    pub cues: Vec<RecipeCue>,
}

/// Everything expanding a recipe needs to know about the room: who the
/// groups are, what the palettes mean, and where each fixture is hung.
///
/// Bundled into one struct rather than passed as three parallel
/// arguments because every one of them is a property of the *venue*, they
/// are always supplied together, and effects will want the same set.
/// `placement` stays a function rather than a table so `ignition-core`
/// keeps its no-I/O rule — `ignition_viz` supplies the real one, backed
/// by `Venue::fixtures`.
pub struct Show<'a> {
    pub groups: &'a [Group],
    pub palettes: &'a Palettes,
    pub placement: &'a dyn Fn(ChanId) -> Option<Placement>,
}

impl<'a> Show<'a> {
    /// A show with no palettes — for tests and for a venue that has not
    /// been given a palette file yet.
    pub fn new(groups: &'a [Group], placement: &'a dyn Fn(ChanId) -> Option<Placement>) -> Self {
        Self {
            groups,
            palettes: Palettes::EMPTY,
            placement,
        }
    }
}

/// Resolves a target to its real channel list — `pub(crate)` so
/// `phaser.rs` can share the exact same Group/Chans resolution a static
/// `Recipe` uses, rather than re-implementing it.
pub(crate) fn resolve_target(target: &RecipeTarget, groups: &[Group]) -> Vec<ChanId> {
    match target {
        RecipeTarget::Chans(chans) => chans.clone(),
        RecipeTarget::Group(name) => group::find(groups, name)
            .map(|g| g.chans.clone())
            .unwrap_or_default(),
    }
}

/// Expands one `Recipe` into the concrete `CueValue`s it produces for every
/// channel in its resolved target. `placement`, used only by `FocusPoint`,
/// looks up a channel's real hung position/orientation — kept as a
/// caller-supplied function rather than a field so this stays free of any
/// venue-loading/I-O knowledge (`ignition-core`'s own "no I/O" rule — see
/// `cue.rs`'s module doc); `ignition_viz` supplies the real one, backed by
/// `Venue::fixtures`.
pub fn expand_recipe(recipe: &Recipe, show: &Show<'_>) -> Vec<CueValue> {
    let chans = resolve_target(&recipe.target, show.groups);
    let mut out = Vec::new();
    for chan in chans {
        match &recipe.apply {
            RecipeApply::Dimmer(value) => out.push(CueValue {
                chan,
                attr: Attribute::Dimmer,
                value: *value,
            }),
            RecipeApply::Color(reference) => {
                let Some(c) = show.palettes.resolve_color(reference) else {
                    continue;
                };
                out.push(CueValue {
                    chan,
                    attr: Attribute::ColorAdd {
                        channel: ColorChannel::Red,
                    },
                    value: c.red,
                });
                out.push(CueValue {
                    chan,
                    attr: Attribute::ColorAdd {
                        channel: ColorChannel::Green,
                    },
                    value: c.green,
                });
                out.push(CueValue {
                    chan,
                    attr: Attribute::ColorAdd {
                        channel: ColorChannel::Blue,
                    },
                    value: c.blue,
                });
            }
            RecipeApply::FocusPoint(reference) => {
                let Some(target) = show.palettes.resolve_focus(reference) else {
                    continue;
                };
                if let Some(p) = (show.placement)(chan) {
                    let (pan, tilt) = pan_tilt_deg_to_point(p.position, p.orientation, target);
                    out.push(CueValue {
                        chan,
                        attr: Attribute::Pan,
                        value: pan,
                    });
                    out.push(CueValue {
                        chan,
                        attr: Attribute::Tilt,
                        value: tilt,
                    });
                }
            }
            RecipeApply::FocusDirection(dir) => {
                if let Some(p) = (show.placement)(chan) {
                    let (pan, tilt) = pan_tilt_deg_along(p.orientation, *dir);
                    out.push(CueValue {
                        chan,
                        attr: Attribute::Pan,
                        value: pan,
                    });
                    out.push(CueValue {
                        chan,
                        attr: Attribute::Tilt,
                        value: tilt,
                    });
                }
            }
            RecipeApply::Raw(values) => {
                for (attr, value) in values {
                    out.push(CueValue {
                        chan,
                        attr: attr.clone(),
                        value: *value,
                    });
                }
            }
        }
    }
    out
}

/// Compiles one `RecipeCue` into the flat `Cue` format `CuePlayer` plays
/// back. Recipes are expanded in order, so a later recipe's `CueValue` for
/// the same `(chan, attr)` as an earlier one in the same cue wins — the
/// same last-write convention `CuePlayer::go()` already uses when folding
/// a cue's values into its tracked state.
pub fn expand_cue(raw: &RecipeCue, show: &Show<'_>) -> Cue {
    let mut values = Vec::new();
    for recipe in &raw.recipes {
        values.extend(expand_recipe(recipe, show));
    }
    Cue {
        name: raw.name.clone(),
        fade_secs: raw.fade_secs,
        values,
    }
}

pub fn expand_cue_list(raw: &[RecipeCue], show: &Show<'_>) -> Vec<Cue> {
    raw.iter().map(|c| expand_cue(c, show)).collect()
}

/// Every name in `cues` that this venue cannot resolve, as readable
/// one-liners.
///
/// Expansion deliberately treats an unknown group or palette entry as
/// "no fixtures" rather than an error, which is the right runtime
/// behaviour — a show should not go dark because one cue names a group
/// this room does not have. But it is a miserable *authoring* behaviour:
/// a typo'd group name is a cue that silently does nothing, and the only
/// symptom is lights that never come on. So the tolerance stays and the
/// diagnosis is reported separately, for the loader to print.
pub fn unresolved(cues: &[RecipeCue], show: &Show<'_>) -> Vec<String> {
    let mut out = Vec::new();
    for cue in cues {
        for recipe in &cue.recipes {
            if let RecipeTarget::Group(name) = &recipe.target
                && group::find(show.groups, name).is_none()
            {
                out.push(format!("cue {:?}: no group named {:?}", cue.name, name));
            }
            match &recipe.apply {
                RecipeApply::Color(Ref::Named(name)) if show.palettes.color(name).is_none() => {
                    out.push(format!("cue {:?}: no colour palette {:?}", cue.name, name));
                }
                RecipeApply::FocusPoint(Ref::Named(name))
                    if show.palettes.focus(name).is_none() =>
                {
                    out.push(format!("cue {:?}: no focus palette {:?}", cue.name, name));
                }
                _ => {}
            }
        }
    }
    out.sort();
    out.dedup();
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use ignition_proto::{Quat, Vec3};

    fn groups() -> Vec<Group> {
        vec![Group {
            name: "Pars".to_string(),
            chans: vec![1, 2, 3],
        }]
    }

    #[test]
    fn a_group_target_resolves_to_its_real_channels() {
        let recipe = Recipe {
            target: RecipeTarget::Group("Pars".to_string()),
            apply: RecipeApply::Dimmer(0.8),
        };
        let values = expand_recipe(&recipe, &Show::new(&groups(), &|_| None));
        assert_eq!(values.len(), 3);
        assert!(
            values
                .iter()
                .all(|v| v.attr == Attribute::Dimmer && (v.value - 0.8).abs() < 0.001)
        );
        let mut chans: Vec<_> = values.iter().map(|v| v.chan).collect();
        chans.sort();
        assert_eq!(chans, vec![1, 2, 3]);
    }

    #[test]
    fn an_unknown_group_name_resolves_to_no_fixtures_not_an_error() {
        let recipe = Recipe {
            target: RecipeTarget::Group("Nonexistent".to_string()),
            apply: RecipeApply::Dimmer(1.0),
        };
        assert!(expand_recipe(&recipe, &Show::new(&groups(), &|_| None)).is_empty());
    }

    #[test]
    fn a_color_recipe_emits_red_green_blue_per_channel() {
        let recipe = Recipe {
            target: RecipeTarget::Chans(vec![5]),
            apply: RecipeApply::Color(Ref::Inline(ColorPreset {
                name: "Amber".to_string(),
                red: 1.0,
                green: 0.5,
                blue: 0.0,
            })),
        };
        let values = expand_recipe(&recipe, &Show::new(&[], &|_| None));
        assert_eq!(values.len(), 3);
        assert!(values.iter().any(|v| v.attr
            == Attribute::ColorAdd {
                channel: ColorChannel::Red
            }
            && v.value == 1.0));
        assert!(values.iter().any(|v| v.attr
            == Attribute::ColorAdd {
                channel: ColorChannel::Green
            }
            && v.value == 0.5));
        assert!(values.iter().any(|v| v.attr
            == Attribute::ColorAdd {
                channel: ColorChannel::Blue
            }
            && v.value == 0.0));
    }

    #[test]
    fn a_focus_point_recipe_resolves_real_pan_tilt_from_the_fixtures_placement() {
        let placement = Placement {
            position: Vec3 {
                x: 0.0,
                y: 0.0,
                z: 5.0,
            },
            orientation: Quat {
                w: 1.0,
                x: 0.0,
                y: 0.0,
                z: 0.0,
            },
        };
        let recipe = Recipe {
            target: RecipeTarget::Chans(vec![7]),
            apply: RecipeApply::FocusPoint(Ref::Inline(Vec3 {
                x: 0.0,
                y: 0.0,
                z: -5.0,
            })), // straight below
        };
        let values = expand_recipe(
            &recipe,
            &Show::new(&[], &|chan| {
                if chan == 7 {
                    Some(placement.clone())
                } else {
                    None
                }
            }),
        );
        let pan = values.iter().find(|v| v.attr == Attribute::Pan).unwrap();
        let tilt = values.iter().find(|v| v.attr == Attribute::Tilt).unwrap();
        assert!(pan.value.abs() < 0.5);
        assert!(tilt.value.abs() < 0.5);
    }

    #[test]
    fn a_focus_point_recipe_skips_a_channel_with_no_known_placement() {
        let recipe = Recipe {
            target: RecipeTarget::Chans(vec![99]),
            apply: RecipeApply::FocusPoint(Ref::Inline(Vec3 {
                x: 0.0,
                y: 0.0,
                z: 0.0,
            })),
        };
        assert!(expand_recipe(&recipe, &Show::new(&[], &|_| None)).is_empty());
    }

    fn palettes() -> Palettes {
        Palettes {
            colors: vec![ColorPreset {
                name: "House Blue".to_string(),
                red: 0.1,
                green: 0.2,
                blue: 1.0,
            }],
            focus: vec![crate::preset::FocusPointPreset {
                name: "Drums".to_string(),
                target: Vec3 {
                    x: 1.0,
                    y: 2.0,
                    z: 3.0,
                },
            }],
        }
    }

    #[test]
    fn a_named_colour_resolves_against_the_venues_palette() {
        let recipe = Recipe {
            target: RecipeTarget::Chans(vec![5]),
            apply: RecipeApply::Color(Ref::Named("House Blue".to_string())),
        };
        let pool = palettes();
        let show = Show {
            groups: &[],
            palettes: &pool,
            placement: &|_| None,
        };
        let values = expand_recipe(&recipe, &show);
        assert_eq!(values.len(), 3);
        assert!(values.iter().any(|v| v.attr
            == Attribute::ColorAdd {
                channel: ColorChannel::Blue
            }
            && v.value == 1.0));
    }

    /// The runtime must not go dark over a typo, but the loader has to be
    /// able to say so — the split `unresolved` exists for.
    #[test]
    fn an_unknown_palette_name_is_skipped_but_reported() {
        let cue = RecipeCue {
            name: "Oops".to_string(),
            fade_secs: 0.0,
            recipes: vec![Recipe {
                target: RecipeTarget::Chans(vec![5]),
                apply: RecipeApply::Color(Ref::Named("Chartreuse".to_string())),
            }],
        };
        let pool = palettes();
        let show = Show {
            groups: &[],
            palettes: &pool,
            placement: &|_| None,
        };
        assert!(expand_cue(&cue, &show).values.is_empty());
        let problems = unresolved(std::slice::from_ref(&cue), &show);
        assert_eq!(problems.len(), 1);
        assert!(problems[0].contains("Chartreuse"), "{problems:?}");
    }

    /// A show file written before palettes existed inlines its colours as
    /// objects; `Ref`'s untagged encoding has to keep parsing those.
    #[test]
    fn an_inline_colour_still_parses_from_the_pre_palette_shape() {
        let json = r#"{"name":"Red","red":1.0,"green":0.0,"blue":0.0}"#;
        let parsed: Ref<ColorPreset> = serde_json::from_str(json).unwrap();
        assert!(matches!(parsed, Ref::Inline(c) if c.red == 1.0));
        let named: Ref<ColorPreset> = serde_json::from_str(r#""Red""#).unwrap();
        assert!(matches!(named, Ref::Named(n) if n == "Red"));
    }

    #[test]
    fn expand_cue_compiles_multiple_recipes_into_one_flat_cue() {
        let raw = RecipeCue {
            name: "Wash On".to_string(),
            fade_secs: 2.0,
            recipes: vec![
                Recipe {
                    target: RecipeTarget::Group("Pars".to_string()),
                    apply: RecipeApply::Dimmer(1.0),
                },
                Recipe {
                    target: RecipeTarget::Group("Pars".to_string()),
                    apply: RecipeApply::Color(Ref::Inline(ColorPreset {
                        name: "Red".to_string(),
                        red: 1.0,
                        green: 0.0,
                        blue: 0.0,
                    })),
                },
            ],
        };
        let cue = expand_cue(&raw, &Show::new(&groups(), &|_| None));
        assert_eq!(cue.name, "Wash On");
        assert_eq!(cue.fade_secs, 2.0);
        // 3 fixtures x (1 dimmer + 3 colour) = 12 values.
        assert_eq!(cue.values.len(), 12);
    }
}
