//! Reusable, named attribute bundles — grandMA3/Eos "Presets" (Color
//! Preset, Focus/Position Preset, ...). A preset stores *what*, a `Group`
//! (`group.rs`) stores *who*; `recipe.rs::Recipe` is what pairs the two
//! together into concrete cue values. Only the two preset types the
//! operator asked to start with are modelled here — more (Gobo, Beam,
//! Shapers) are additive, not a redesign, when they're wanted.

use crate::color::{Intent, Rgb};
use ignition_proto::{ChanId, Vec3};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// A colour preset — a *rule with scope*, not a value.
///
/// `red`/`green`/`blue` are the linear RGB triple every show file ever
/// written carries, and what `recipe.rs::expand_recipe` emits as
/// `ColorAdd{Red/Green/Blue}`. `intent` is the device-independent form
/// (`color::Intent`: xy, a colour temperature, a gel) that lets a fixture
/// with emitter data — `ignition_viz::show` at encode time — reach the
/// same colour with its white/amber/lime instead of an RGB approximation.
/// When a file gives only the intent, `ColorPreset::from_intent` fills the
/// triple from it, so the two never disagree.
///
/// `scope` answers what happens when the preset meets a fixture it was
/// not written for — see `Scope` and `resolve_for`.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
// r[impl color.model] - a linear RGB triple; HSB via color::Rgb::to_hsb
// r[impl color.intent] - an optional device-independent intent beside the triple
// r[impl color.space-independent] - stored as intent + RGB, never emitter levels
pub struct ColorPreset {
    pub name: String,
    pub red: f32,
    pub green: f32,
    pub blue: f32,
    /// What the colour means beyond RGB; `None` on files that predate it,
    /// in which case the triple *is* the intent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub intent: Option<Intent>,
    /// `0..=1`, MA3's Q fader: 1 favours the fewest, narrowest emitters,
    /// 0 the broadest mix. `None` lets the fixture pick
    /// (`color::DEFAULT_QUALITY`).
    // r[impl color.quality]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quality: Option<f32>,
    // r[impl color.scope] - every preset declares one; absent on disk means universal
    #[serde(default, skip_serializing_if = "Scope::is_universal")]
    pub scope: Scope,
}

/// Which fixtures a colour preset's value is written for — MA3's three
/// preset modes. The universal value is always the preset's own
/// `red`/`green`/`blue`; `Global` and `Selective` add per-type and
/// per-fixture values on top, and `resolve_for` walks them in the
/// spec's fallback order.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
// r[impl color.scope] - universal, global or selective
pub enum Scope {
    /// One value for any fixture whatsoever: the preset's own triple.
    // r[impl color.scope.universal] - no per-type or per-fixture variation stored
    #[default]
    Universal,
    /// One value per fixture *type* (`manufacturer model`, matched
    /// case-insensitively), the preset's own triple serving fixtures of
    /// any other type.
    // r[impl color.scope.global]
    Global(BTreeMap<String, Rgb>),
    /// One value per fixture, by console channel.
    // r[impl color.scope.selective]
    Selective(BTreeMap<ChanId, Rgb>),
}

impl Scope {
    pub fn is_universal(&self) -> bool {
        matches!(self, Scope::Universal)
    }
}

impl ColorPreset {
    /// A universal preset from a triple — what every pre-intent file is.
    pub fn rgb(name: &str, red: f32, green: f32, blue: f32) -> ColorPreset {
        ColorPreset {
            name: name.to_string(),
            red,
            green,
            blue,
            ..Default::default()
        }
    }

    /// A preset from an intent, its triple filled from the intent so a
    /// fixture without emitter data still has something to show. A gel
    /// not in the table yields `None` — the reference cannot be resolved,
    /// which `r[color.unresolved-is-visible]` wants reported, not hidden.
    // r[impl color.intent] - the triple is derived from the intent, never guessed separately
    pub fn from_intent(name: &str, intent: Intent) -> Option<ColorPreset> {
        let rgb = intent.rgb()?;
        Some(ColorPreset {
            name: name.to_string(),
            red: rgb.red,
            green: rgb.green,
            blue: rgb.blue,
            intent: Some(intent),
            ..Default::default()
        })
    }

    /// The universal value, as a triple.
    pub fn universal(&self) -> Rgb {
        Rgb::new(self.red, self.green, self.blue)
    }

    /// The intent to solve against a fixture's emitters: the stored
    /// intent, else the triple.
    pub fn intent(&self) -> Intent {
        self.intent
            .clone()
            .unwrap_or_else(|| Intent::Rgb(self.universal()))
    }

    /// The value this preset gives fixture `chan` of type `model`:
    /// its own selective value, then its type's global value, then the
    /// universal triple, then — for a selective preset whose universal
    /// value was never set — the first selective value. `model` is
    /// matched case-insensitively so `"Uking Par"` and `"uking par"` are
    /// one type, and `None` for a fixture whose type is unknown.
    // r[impl color.scope.fallback-order] - own selective, type global, universal, first selective
    // r[impl color.scope.selective] - a fixture with no entry still gets a value
    pub fn resolve_for(&self, chan: ChanId, model: Option<&str>) -> Rgb {
        match &self.scope {
            Scope::Universal => self.universal(),
            Scope::Global(by_type) => model
                .and_then(|m| {
                    by_type
                        .iter()
                        .find(|(k, _)| k.eq_ignore_ascii_case(m))
                        .map(|(_, v)| *v)
                })
                .unwrap_or_else(|| self.universal()),
            Scope::Selective(by_chan) => {
                if let Some(v) = by_chan.get(&chan) {
                    return *v;
                }
                if self.has_universal() {
                    return self.universal();
                }
                by_chan
                    .values()
                    .next()
                    .copied()
                    .unwrap_or_else(|| self.universal())
            }
        }
    }

    /// Whether the triple was set at all. A selective preset authored
    /// without one serialises as all-zero, and black is not a fallback
    /// anyone meant.
    fn has_universal(&self) -> bool {
        self.intent.is_some() || self.red > 0.0 || self.green > 0.0 || self.blue > 0.0
    }
}

/// A real XYZ location in the room — `recipe.rs::expand_recipe` resolves
/// this into real Pan/Tilt values per fixture via `focus.rs`'s aim-at-point
/// math, using each fixture's own real hung position/orientation. This is
/// the preset type real 3D venue geometry (unique to this project among
/// budget-console-class tools) makes possible.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
// r[impl focus.point] - stored as a room coordinate, resolved per fixture in focus.rs
// r[impl focus.units] - metres
pub struct FocusPointPreset {
    pub name: String,
    pub target: Vec3,
}

/// A reference to a pooled preset, or the value written out in place.
///
/// This is what turns presets from a type into a *pool*: a recipe can say
/// `"Deep Blue"` and mean the palette entry, so re-gelling a show is one
/// edit instead of forty. Writing the value inline stays legal for the
/// one-off that does not deserve a name — and, because the two forms are
/// distinguished structurally (`#[serde(untagged)]`: a JSON string is a
/// name, an object is a value), every show file written before the pool
/// existed still parses unchanged.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
// r[impl color.recall-by-reference] - a cue stores the pool name; the value is looked up at use
pub enum Ref<T> {
    Named(String),
    Inline(T),
}

/// The venue's preset pools — an Eos "palette" or grandMA3 "preset pool".
///
/// Palettes belong to the *venue*, not to a show: "Drums" is a real place
/// in this room and "House Blue" is this rig's blue, so every show in the
/// building should mean the same thing by them. That is also why an
/// unknown name resolves to `None` and the recipe is skipped rather than
/// erroring — the same tolerance `group::find` already has for a group
/// name a venue does not carry.
/// How a list of colours meets a selection.
///
/// Declared on the split or the apply, never inferred from how many
/// colours it holds: three colours might be a three-way block or a
/// two-stop gradient with a middle, and only the author knows which.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
// r[impl color.multi.distribution]
pub enum Distribute {
    /// Repeat the list across the selection in order: A B A B.
    Cycle,
    /// A gradient — the first unit takes the first colour, the last unit
    /// the last, the rest interpolated linearly in RGB between them.
    /// The same from/to rule `tricks::spread` uses for phase.
    Spread,
    /// As many contiguous runs as there are colours: A A B B.
    Block,
}

/// A named multi-colour palette entry — grandMA3's multi-colour preset,
/// shown as a split swatch in the picker and recalled by name in a
/// recipe (`RecipeApply::Split("Fire")`) so a split look is *one*
/// object rather than a list every cue has to restate.
///
/// Members are references, not values: a split names the colours it is
/// made of, so re-gelling `Amber` re-gels every split that uses it. A
/// member name that is not a colour may be another *split*, whose
/// colours are spliced in flat — bounded to `MAX_SPLIT_DEPTH` levels and
/// with a cycle reported rather than followed.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
// r[impl color.multi] - several colours plus a declared distribution, as one named thing
// r[impl color.recall-by-reference] - members are names, resolved at use
// r[impl color.embedding] - a member may name another split
pub struct ColorSplit {
    pub name: String,
    pub colors: Vec<Ref<ColorPreset>>,
    pub distribute: Distribute,
}

/// How deep a split may nest before resolution gives up — MA3's ten
/// levels.
// r[impl color.embedding] - nesting is bounded
pub const MAX_SPLIT_DEPTH: usize = 10;

/// Why a split did not resolve. Each names the split and the reference,
/// which is what the load-time check prints.
// r[impl color.unresolved-is-visible]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SplitProblem {
    /// The recipe named a split the palette does not carry.
    MissingSplit(String),
    /// A member of `split` names neither a colour nor a split.
    MissingColor { split: String, member: String },
    /// Following members led back to `split`.
    Cycle(String),
    /// Nesting passed `MAX_SPLIT_DEPTH` inside `split`.
    TooDeep(String),
}

impl std::fmt::Display for SplitProblem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SplitProblem::MissingSplit(name) => write!(f, "no colour split {name:?}"),
            SplitProblem::MissingColor { split, member } => {
                write!(f, "no colour palette {member:?} (in split {split:?})")
            }
            SplitProblem::Cycle(name) => write!(f, "colour split {name:?} refers to itself"),
            SplitProblem::TooDeep(name) => write!(
                f,
                "colour split {name:?} nests deeper than {MAX_SPLIT_DEPTH} levels"
            ),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
// r[impl files.vocabulary] - the venue's named colours, splits and focus points
pub struct Palettes {
    #[serde(default)]
    pub colors: Vec<ColorPreset>,
    /// Multi-colour entries. Optional on disk so every palette file
    /// written before splits existed still parses.
    // r[impl color.multi]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub splits: Vec<ColorSplit>,
    #[serde(default)]
    pub focus: Vec<FocusPointPreset>,
}

impl Palettes {
    /// A shared empty pool, so `Show::new` can hand out a reference
    /// without every caller having to own one.
    pub const EMPTY: &'static Palettes = &Palettes {
        colors: Vec::new(),
        splits: Vec::new(),
        focus: Vec::new(),
    };

    pub fn color(&self, name: &str) -> Option<&ColorPreset> {
        self.colors.iter().find(|c| c.name == name)
    }

    pub fn focus(&self, name: &str) -> Option<&FocusPointPreset> {
        self.focus.iter().find(|f| f.name == name)
    }

    /// Moves a focus marker — or adds one — so everything aimed at it
    /// moves with it. This is the durable form: the palette *is* where
    /// `Vocal` is. A host moving a marker per frame from a tracker uses
    /// `Show::focus_overrides` instead and leaves the palette alone.
    // r[impl focus.marker-moving] - the palette's marker is updatable
    pub fn set_focus(&mut self, name: &str, target: Vec3) {
        match self.focus.iter_mut().find(|f| f.name == name) {
            Some(f) => f.target = target,
            None => self.focus.push(FocusPointPreset {
                name: name.to_string(),
                target,
            }),
        }
    }

    // r[impl color.recall-by-reference] - a split is looked up by name
    pub fn split(&self, name: &str) -> Option<&ColorSplit> {
        self.splits.iter().find(|s| s.name == name)
    }

    /// Resolves a split reference to its flat colour list and
    /// distribution. All-or-nothing, like `RecipeApply::Colors`: any
    /// problem yields `None`, and `split_problems` says which.
    // r[impl color.multi]
    // r[impl color.recall-by-reference]
    // r[impl color.embedding]
    pub fn resolve_split(&self, r: &Ref<ColorSplit>) -> Option<(Vec<ColorPreset>, Distribute)> {
        let mut problems = Vec::new();
        let colors = self.walk_split(r, &mut Vec::new(), &mut problems);
        (problems.is_empty() && !colors.is_empty()).then_some((colors, self.split_distribute(r)?))
    }

    /// Everything that stops `r` resolving, each naming the split and
    /// the reference. Empty means `resolve_split` succeeds.
    // r[impl color.unresolved-is-visible] - a missing split, a missing member, a cycle, too deep
    pub fn split_problems(&self, r: &Ref<ColorSplit>) -> Vec<SplitProblem> {
        let mut problems = Vec::new();
        self.walk_split(r, &mut Vec::new(), &mut problems);
        problems
    }

    fn split_distribute(&self, r: &Ref<ColorSplit>) -> Option<Distribute> {
        match r {
            Ref::Named(name) => self.split(name).map(|s| s.distribute),
            Ref::Inline(s) => Some(s.distribute),
        }
    }

    /// Flattens a split's members into colours. `stack` is the chain of
    /// named splits being followed — its length is the nesting depth and
    /// its contents are what a cycle is checked against.
    // r[impl color.embedding] - depth bounded by MAX_SPLIT_DEPTH; a cycle is reported, not recursed
    fn walk_split(
        &self,
        r: &Ref<ColorSplit>,
        stack: &mut Vec<String>,
        problems: &mut Vec<SplitProblem>,
    ) -> Vec<ColorPreset> {
        let split: &ColorSplit = match r {
            Ref::Inline(s) => s,
            Ref::Named(name) => match self.split(name) {
                Some(s) => s,
                None => {
                    problems.push(SplitProblem::MissingSplit(name.clone()));
                    return Vec::new();
                }
            },
        };
        let mut out = Vec::new();
        for member in &split.colors {
            match member {
                Ref::Inline(c) => out.push(c.clone()),
                Ref::Named(name) => {
                    if let Some(c) = self.color(name) {
                        out.push(c.clone());
                    } else if self.split(name).is_some() {
                        if stack.iter().any(|s| s == name) || split.name == *name {
                            problems.push(SplitProblem::Cycle(name.clone()));
                        } else if stack.len() + 1 >= MAX_SPLIT_DEPTH {
                            problems.push(SplitProblem::TooDeep(split.name.clone()));
                        } else {
                            stack.push(split.name.clone());
                            out.extend(self.walk_split(&Ref::Named(name.clone()), stack, problems));
                            stack.pop();
                        }
                    } else {
                        problems.push(SplitProblem::MissingColor {
                            split: split.name.clone(),
                            member: name.clone(),
                        });
                    }
                }
            }
        }
        out
    }

    /// Resolves a colour reference against the pool.
    // r[impl color.recall-by-reference]
    pub fn resolve_color(&self, r: &Ref<ColorPreset>) -> Option<ColorPreset> {
        match r {
            Ref::Named(name) => self.color(name).cloned(),
            Ref::Inline(c) => Some(c.clone()),
        }
    }

    /// Resolves a focus reference against the pool. The inline form is a
    /// bare point rather than a named `FocusPointPreset`, because at the
    /// point of use what a recipe wants is the location, not the label.
    pub fn resolve_focus(&self, r: &Ref<Vec3>) -> Option<Vec3> {
        match r {
            Ref::Named(name) => self.focus(name).map(|f| f.target),
            Ref::Inline(v) => Some(*v),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rgb(r: f32, g: f32, b: f32) -> Rgb {
        Rgb::new(r, g, b)
    }

    #[test]
    /// r[verify color.model] - an old three-float file still parses, as universal
    fn a_pre_intent_file_parses_unchanged() {
        let c: ColorPreset =
            serde_json::from_str(r#"{"name":"Open White","red":1.0,"green":1.0,"blue":1.0}"#)
                .unwrap();
        assert_eq!(c.scope, Scope::Universal);
        assert!(c.intent.is_none() && c.quality.is_none());
        let json = serde_json::to_string(&c).unwrap();
        assert!(
            !json.contains("scope") && !json.contains("intent"),
            "{json}"
        );
    }

    #[test]
    /// r[verify color.intent] - a preset given only an intent gets its triple from it
    fn from_intent_fills_the_triple() {
        let c = ColorPreset::from_intent(
            "Warm White",
            Intent::Cct {
                kelvin: 3200.0,
                tint: 0.0,
            },
        )
        .unwrap();
        assert!((c.red - 1.0).abs() < 1e-4);
        assert!(c.blue < c.green && c.green < c.red, "{c:?}");
        let json = serde_json::to_string(&c).unwrap();
        assert!(
            json.contains(r#""intent":{"cct":{"kelvin":3200.0"#),
            "{json}"
        );
        let back: ColorPreset = serde_json::from_str(&json).unwrap();
        assert_eq!(back, c);
    }

    #[test]
    /// r[verify color.gel] - an unknown gel is refused, not guessed
    fn an_unknown_gel_does_not_make_a_preset() {
        assert!(
            ColorPreset::from_intent(
                "x",
                Intent::Gel {
                    manufacturer: "Nobody".into(),
                    number: "1".into()
                }
            )
            .is_none()
        );
    }

    #[test]
    /// r[verify color.scope.universal] - one value for every fixture
    fn universal_gives_every_fixture_the_same_value() {
        let c = ColorPreset::rgb("Open White", 1.0, 1.0, 1.0);
        assert_eq!(c.resolve_for(1, Some("Uking Par")), rgb(1.0, 1.0, 1.0));
        assert_eq!(c.resolve_for(99, None), rgb(1.0, 1.0, 1.0));
    }

    #[test]
    /// r[verify color.scope.global] - a value per type, universal for the rest
    fn global_resolves_by_type_then_universal() {
        let mut c = ColorPreset::rgb("Amber", 1.0, 0.5, 0.1);
        c.scope = Scope::Global(BTreeMap::from([(
            "Uking Par".to_string(),
            rgb(1.0, 0.6, 0.0),
        )]));
        assert_eq!(c.resolve_for(1, Some("uking par")), rgb(1.0, 0.6, 0.0));
        assert_eq!(c.resolve_for(2, Some("Betopper Beam")), rgb(1.0, 0.5, 0.1));
        assert_eq!(c.resolve_for(3, None), rgb(1.0, 0.5, 0.1));
    }

    #[test]
    /// r[verify color.scope.selective] - a fixture with no entry still gets a value
    /// r[verify color.scope.fallback-order]
    fn selective_falls_back_to_universal_then_first_entry() {
        let mut c = ColorPreset::rgb("Rainbow", 0.0, 0.0, 0.0);
        c.scope = Scope::Selective(BTreeMap::from([
            (1, rgb(1.0, 0.0, 0.0)),
            (2, rgb(1.0, 0.5, 0.0)),
        ]));
        assert_eq!(c.resolve_for(2, None), rgb(1.0, 0.5, 0.0));
        assert_eq!(
            c.resolve_for(7, Some("Uking Par")),
            rgb(1.0, 0.0, 0.0),
            "no universal set: first selective"
        );
        c.red = 0.5;
        c.green = 0.5;
        c.blue = 0.5;
        assert_eq!(
            c.resolve_for(7, None),
            rgb(0.5, 0.5, 0.5),
            "universal beats first selective"
        );
        assert_eq!(
            c.resolve_for(1, None),
            rgb(1.0, 0.0, 0.0),
            "own entry beats all"
        );
    }

    #[test]
    /// r[verify color.scope] - the scope round-trips through JSON
    fn scope_serialises_by_mode() {
        let mut c = ColorPreset::rgb("Rainbow", 0.0, 0.0, 0.0);
        c.scope = Scope::Selective(BTreeMap::from([(1, rgb(1.0, 0.0, 0.0))]));
        let json = serde_json::to_string(&c).unwrap();
        assert!(json.contains(r#""scope":{"selective":{"1":"#), "{json}");
        let back: ColorPreset = serde_json::from_str(&json).unwrap();
        assert_eq!(back, c);
    }
}
