//! Recipes — this project's foundation for building cues, the same role
//! grandMA3's Preset system plays: a `Recipe` pairs a *target* (a `Group`
//! or an explicit channel list) with an *apply* (a `Dimmer` level, a
//! `Color`, a `FocusPoint`, or a `Raw` attribute list).
//!
//! A recipe is **stored into** a `Cue` and resolved at output time, not
//! flattened into values when the show loads. That distinction is the
//! whole feature: a stored recipe still knows it targets a *group*, so
//! adding a fixture to that group changes what every cue using it covers
//! with no re-authoring, and the recipe stays something an editor can
//! show and change. `expand_recipe` is the resolver `CuePlayer` calls;
//! see `docs/domain/cue-building-architecture.md` for why resolution
//! lives there rather than at load.
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
use crate::group::Group;
use crate::preset::{ColorPreset, Palettes, Ref};
use crate::selection::{Rig, Selection, resolve, unresolved_names};
use ignition_proto::{Attribute, ColorChannel};
use serde::{Deserialize, Serialize};

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
    pub target: Selection,
    pub apply: RecipeApply,
}

/// Everything expanding a recipe needs to know about the room: who the
/// groups are, what the palettes mean, and where each fixture is hung.
///
/// Bundled into one struct rather than passed as three parallel
/// arguments because every one of them is a property of the *venue*, they
/// are always supplied together, and effects will want the same set.
/// `rig` is flat records rather than a venue-loader callback, so
/// `ignition-core` keeps its no-I/O rule while still being able to answer
/// "which fixtures are tagged `mover`" — a question a
/// `Fn(ChanId) -> Placement` closure cannot be asked, and the reason
/// `Selection::Tag`/`Model` are possible at all.
pub struct Show<'a> {
    pub groups: &'a [Group],
    pub palettes: &'a Palettes,
    pub rig: &'a Rig,
}

impl<'a> Show<'a> {
    /// A show with no palettes — for tests and for a venue that has not
    /// been given a palette file yet.
    pub fn new(groups: &'a [Group], rig: &'a Rig) -> Self {
        Self {
            groups,
            palettes: Palettes::EMPTY,
            rig,
        }
    }
}

/// Expands one `Recipe` into the concrete `CueValue`s it produces for
/// every channel in its resolved selection.
pub fn expand_recipe(recipe: &Recipe, show: &Show<'_>) -> Vec<CueValue> {
    let chans = resolve(&recipe.target, show.groups, show.rig);
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
                if let Some(p) = show.rig.placement(chan) {
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
                if let Some(p) = show.rig.placement(chan) {
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
pub fn unresolved(cues: &[Cue], show: &Show<'_>) -> Vec<String> {
    let mut out = Vec::new();
    for cue in cues {
        for recipe in &cue.recipes {
            for problem in unresolved_names(&recipe.target, show.groups, show.rig) {
                out.push(format!("cue {:?}: {problem}", cue.name));
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
    use crate::selection::{FixtureInfo, Rig};
    use ignition_proto::{Placement, Quat, Vec3};

    fn groups() -> Vec<Group> {
        vec![Group {
            name: "Pars".to_string(),
            chans: vec![1, 2, 3],
        }]
    }

    #[test]
    fn a_group_target_resolves_to_its_real_channels() {
        let recipe = Recipe {
            target: Selection::Group("Pars".to_string()),
            apply: RecipeApply::Dimmer(0.8),
        };
        let values = expand_recipe(&recipe, &Show::new(&groups(), &crate::selection::EMPTY_RIG));
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
            target: Selection::Group("Nonexistent".to_string()),
            apply: RecipeApply::Dimmer(1.0),
        };
        assert!(
            expand_recipe(&recipe, &Show::new(&groups(), &crate::selection::EMPTY_RIG)).is_empty()
        );
    }

    #[test]
    fn a_color_recipe_emits_red_green_blue_per_channel() {
        let recipe = Recipe {
            target: Selection::Chans(vec![5]),
            apply: RecipeApply::Color(Ref::Inline(ColorPreset {
                name: "Amber".to_string(),
                red: 1.0,
                green: 0.5,
                blue: 0.0,
            })),
        };
        let values = expand_recipe(&recipe, &Show::new(&[], &crate::selection::EMPTY_RIG));
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
            target: Selection::Chans(vec![7]),
            apply: RecipeApply::FocusPoint(Ref::Inline(Vec3 {
                x: 0.0,
                y: 0.0,
                z: -5.0,
            })), // straight below
        };
        let rig = Rig::new(vec![FixtureInfo {
            chan: 7,
            placement: Some(placement),
            manufacturer: String::new(),
            model: String::new(),
            tags: Vec::new(),
        }]);
        let values = expand_recipe(&recipe, &Show::new(&[], &rig));
        let pan = values.iter().find(|v| v.attr == Attribute::Pan).unwrap();
        let tilt = values.iter().find(|v| v.attr == Attribute::Tilt).unwrap();
        assert!(pan.value.abs() < 0.5);
        assert!(tilt.value.abs() < 0.5);
    }

    #[test]
    fn a_focus_point_recipe_skips_a_channel_with_no_known_placement() {
        let recipe = Recipe {
            target: Selection::Chans(vec![99]),
            apply: RecipeApply::FocusPoint(Ref::Inline(Vec3 {
                x: 0.0,
                y: 0.0,
                z: 0.0,
            })),
        };
        assert!(expand_recipe(&recipe, &Show::new(&[], &crate::selection::EMPTY_RIG)).is_empty());
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
            target: Selection::Chans(vec![5]),
            apply: RecipeApply::Color(Ref::Named("House Blue".to_string())),
        };
        let pool = palettes();
        let show = Show {
            groups: &[],
            palettes: &pool,
            rig: &crate::selection::EMPTY_RIG,
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
        let cue = Cue {
            name: "Oops".to_string(),
            recipes: vec![Recipe {
                target: Selection::Chans(vec![5]),
                apply: RecipeApply::Color(Ref::Named("Chartreuse".to_string())),
            }],
            ..Default::default()
        };
        let pool = palettes();
        let show = Show {
            groups: &[],
            palettes: &pool,
            rig: &crate::selection::EMPTY_RIG,
        };
        assert!(
            cue.recipes
                .iter()
                .all(|r| expand_recipe(r, &show).is_empty())
        );
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
}
