//! The cue-design lint: `docs/domain/cue-design-guide.md`'s checklist,
//! run over a generated [`CueList`] and the [`SongMap`] it was written
//! against.
//!
//! The guide ends in thirty-two machine-checkable rules. The ones here
//! are the ones a *portable* list can answer without a venue — every
//! judgement is made on roles, named colours and library effects, never
//! on a fixture. Rules that need a venue (a camera profile, a wall) or
//! the player (ε-early firing) are left to the tools that have one.
//!
//! Every finding names the rule by the guide's number, so a reader can
//! go from a line of output to the paragraph that explains it.

use std::collections::{BTreeMap, BTreeSet};

use ignition_colour::preset::{ColorSplit, Ref};
use ignition_daw_proto::{Bars, SongMap};
use ignition_proto::Attribute;
use ignition_rig::Selection;
use ignition_show::cue::MibMode;
use ignition_show::{Cue, CueList, Recipe, RecipeApply, RecipeRef, Trigger};

use crate::generate::{Kind, kind_of};
use crate::mib::leaves;

/// One rule the list breaks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding {
    /// The guide's rule number, `1..=32`.
    pub rule: u8,
    /// A short name for the rule.
    pub name: &'static str,
    /// The cue the finding is about, where there is one.
    pub cue: Option<String>,
    pub message: String,
}

impl std::fmt::Display for Finding {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.cue {
            Some(cue) => write!(
                f,
                "rule {:>2} {:<24} {cue:<20} {}",
                self.rule, self.name, self.message
            ),
            None => write!(
                f,
                "rule {:>2} {:<24} {:<20} {}",
                self.rule, self.name, "", self.message
            ),
        }
    }
}

/// The non-key layers a look is judged on, in the profile's order.
const LAYERS: [&str; 8] = [
    "Wash", "Back", "Bars", "Movers", "Beams", "Floor", "Audience", "Drums",
];

/// A hue family. The guide counts hues, not presets; the profile's
/// palette is folded into these so `Lavender` and `Purple` are one
/// idea rather than two.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Family {
    White,
    Warm,
    Violet,
    Cold,
    Green,
    Other,
}

/// The family of a profile colour, by name.
#[must_use]
pub fn family(name: &str) -> Family {
    match name {
        "Open White" | "Warm White" | "Cool White" | "Open" | "Warm" | "Cool" | "Straw" => {
            Family::White
        }
        "Gold" | "Amber" | "Deep Amber" | "Red" | "Deep Red" | "Hot" => Family::Warm,
        "Magenta" | "Pink" | "Lavender" | "Purple" | "Congo" | "Indigo" | "Deep" => Family::Violet,
        "Sky" | "Blue" | "Deep Blue" | "House Blue" | "Cyan" | "Turquoise" => Family::Cold,
        "Green" | "Deep Green" => Family::Green,
        _ => Family::Other,
    }
}

/// What kind of thing a sustained effect does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Class {
    Position,
    Intensity,
    Colour,
    Beam,
    Strobe,
}

/// A sustained (looping) recipe in a cue, reduced to what the rules ask.
#[derive(Debug, Clone)]
struct Effect {
    name: String,
    roles: Vec<String>,
    classes: Vec<Class>,
    /// One loop, in bars — `None` for a one-shot.
    period_bars: Option<f32>,
    once: bool,
    /// The largest swing an intensity step makes; a pulse is texture, a
    /// chase is an effect.
    swing: f32,
    /// A per-unit random generator rather than a step table.
    random: bool,
    stacked: bool,
}

/// One cue, reduced to what the rules ask.
#[derive(Debug, Clone)]
struct Info {
    index: usize,
    name: String,
    block: bool,
    at: Option<Bars>,
    flat: f32,
    fade_beats: f32,
    /// Absolute dimmer per role leaf.
    levels: BTreeMap<String, f32>,
    /// Hue families per role leaf.
    hues: BTreeMap<String, BTreeSet<Family>>,
    /// Role leaves this cue aims.
    aims: BTreeSet<String>,
    effects: Vec<Effect>,
    all_zero: bool,
}

impl Info {
    fn level(&self, role: &str) -> f32 {
        self.levels.get(role).copied().unwrap_or(0.0)
    }
    /// The brightest non-key layer.
    fn level_max(&self) -> f32 {
        LAYERS.iter().map(|r| self.level(r)).fold(0.0, f32::max)
    }
    fn lit_layers(&self) -> BTreeSet<&'static str> {
        LAYERS
            .iter()
            .copied()
            .filter(|r| self.level(r) > 0.0)
            .collect()
    }
    /// Hue families on lit non-key layers, white excluded.
    fn families(&self) -> BTreeSet<Family> {
        let mut out = BTreeSet::new();
        for role in LAYERS {
            if self.level(role) > 0.0
                && let Some(f) = self.hues.get(role)
            {
                out.extend(f.iter().copied().filter(|f| *f != Family::White));
            }
        }
        out
    }
    fn dominant(&self) -> Option<Family> {
        let mut count: BTreeMap<Family, usize> = BTreeMap::new();
        for role in LAYERS {
            if self.level(role) > 0.0
                && let Some(f) = self.hues.get(role)
            {
                for f in f.iter().filter(|f| **f != Family::White) {
                    count
                        .entry(*f)
                        .and_modify(|n| *n = n.saturating_add(1))
                        .or_insert(1);
                }
            }
        }
        count.into_iter().max_by_key(|(_, n)| *n).map(|(f, _)| f)
    }
    /// Effects that carry the guide's sense of "an effect": a position
    /// effect, a chase, a colour cycle. Pulses and breathes (swing ≤ 0.3)
    /// and generators are texture. Stacked recipes are one idea.
    fn counted_effects(&self) -> usize {
        let mut n: usize = 0;
        for e in &self.effects {
            let lit = e.roles.iter().any(|r| self.level(r) > 0.0);
            if e.once || !lit {
                continue;
            }
            let counts = e.classes.contains(&Class::Position)
                || (e.classes.contains(&Class::Intensity) && e.swing > 0.3 && !e.random)
                || (e.classes.contains(&Class::Colour) && !e.random)
                || e.classes.contains(&Class::Strobe);
            if !counts {
                continue;
            }
            // A stacked recipe sums *under* the one it rides on; it is
            // part of that idea, not a second one.
            if !e.stacked {
                n = n.saturating_add(1);
            }
        }
        n
    }
    /// Movers or beams moving.
    fn movement(&self) -> bool {
        self.effects.iter().any(|e| {
            !e.once
                && e.classes.contains(&Class::Position)
                && e.roles.iter().any(|r| self.level(r) > 0.0)
        })
    }
    /// The guide's energy, 0–1: half brightness, a third how much of the
    /// rig is lit, the rest how much of it is moving.
    fn energy(&self) -> f32 {
        let lit_ratio =
            small_count_as_f32(self.lit_layers().len()) / small_count_as_f32(LAYERS.len());
        let effect_ratio = small_count_as_f32(self.counted_effects().min(3)) / 3.0;
        0.2f32.mul_add(
            effect_ratio,
            0.5f32.mul_add(self.level_max(), 0.3 * lit_ratio),
        )
    }
}

/// A small, bounded count — layers, effects — as a float. These never
/// exceed a couple of dozen, far below where an `f32`'s 24-bit mantissa
/// would start dropping precision.
#[expect(
    clippy::as_conversions,
    clippy::cast_precision_loss,
    reason = "counts here are small; see the doc comment"
)]
const fn small_count_as_f32(n: usize) -> f32 {
    n as f32
}

/// A bar number as a float, for the flattened bar-and-beat position the
/// rules compare. A song's bar count is far below where an `f32` stops
/// counting integers exactly.
#[expect(
    clippy::as_conversions,
    clippy::cast_precision_loss,
    reason = "bar numbers in a song are far below f32's precision limit; see the doc comment"
)]
const fn bar_as_f32(bar: u32) -> f32 {
    bar as f32
}

/// The bar number under a flattened bar-and-beat position, floored.
/// Clamped rather than trusting the float: `flat` never produces a
/// negative or NaN value, but the cast is on data, not a proof.
#[expect(
    clippy::as_conversions,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "clamped to a non-negative range before the cast; see the doc comment"
)]
fn bar_of(value: f32) -> u32 {
    if value.is_nan() || value <= 0.0 {
        0
    } else {
        value.min(bar_as_f32(u32::MAX)) as u32
    }
}

fn flat(at: Bars) -> f32 {
    bar_as_f32(at.bar) + (narrow(at.beat) - 1.0) / 4.0
}

/// A small musical quantity — a beat position, a tempo, a section's bar
/// count — as an `f32`. All of them are hand-authored or tempo-derived
/// numbers for one song, far below where an `f32` loses precision that
/// matters here.
#[expect(
    clippy::as_conversions,
    clippy::cast_possible_truncation,
    reason = "musical quantities for one song are far below f32's precision limit; see the doc comment"
)]
const fn narrow(value: f64) -> f32 {
    value as f32
}

const fn is_position(apply: &RecipeApply) -> bool {
    matches!(
        apply,
        RecipeApply::FocusPoint(_)
            | RecipeApply::FocusDirection(_)
            | RecipeApply::FocusFan { .. }
            | RecipeApply::FocusKeyframes(_)
            | RecipeApply::FocusDelta(_)
            | RecipeApply::FocusSplay { .. }
            | RecipeApply::FocusPerFixture { .. }
            | RecipeApply::FocusAxes { .. }
            | RecipeApply::FocusRelative { .. }
    )
}

const fn class_of_attr(attr: &Attribute) -> Class {
    match attr {
        Attribute::Pan | Attribute::Tilt | Attribute::PanFine | Attribute::TiltFine => {
            Class::Position
        }
        Attribute::Dimmer => Class::Intensity,
        Attribute::Strobe => Class::Strobe,
        Attribute::ColorAdd { .. } | Attribute::ColorWheel { .. } => Class::Colour,
        _ => Class::Beam,
    }
}

/// What a recipe's steps accumulate into, across every `apply` in every
/// step — pulled out of [`read_recipe`] so the per-arm logic has
/// somewhere to live besides one long function.
#[derive(Default)]
struct RecipeAccum {
    classes: Vec<Class>,
    swing: f32,
    random: bool,
    period: f32,
    dimmers: Vec<f32>,
}

/// Folds one `apply` into the running summary of a recipe's steps.
fn read_apply(
    info: &mut Info,
    roles: &[String],
    sustained: bool,
    acc: &mut RecipeAccum,
    apply: &RecipeApply,
) {
    match apply {
        RecipeApply::Dimmer(v) => {
            acc.dimmers.push(*v);
            if !sustained {
                for r in roles {
                    info.levels.insert(r.clone(), *v);
                }
            }
        }
        RecipeApply::Color(Ref::Named(c)) => {
            for r in roles {
                info.hues.entry(r.clone()).or_default().insert(family(c));
            }
        }
        RecipeApply::Colors { colors, .. } => {
            for c in colors {
                if let Ref::Named(c) = c {
                    for r in roles {
                        info.hues.entry(r.clone()).or_default().insert(family(c));
                    }
                }
            }
        }
        RecipeApply::Split(Ref::Named(split)) => {
            for r in roles {
                info.hues
                    .entry(r.clone())
                    .or_default()
                    .extend(split_families(split));
            }
        }
        RecipeApply::Delta(pairs) => {
            for (attr, v) in pairs {
                let class = class_of_attr(attr);
                if !acc.classes.contains(&class) {
                    acc.classes.push(class);
                }
                if class == Class::Intensity {
                    acc.swing = acc.swing.max(v.abs());
                }
            }
        }
        RecipeApply::Random(r) => {
            acc.random = true;
            let class = class_of_attr(&r.attr);
            if !acc.classes.contains(&class) {
                acc.classes.push(class);
            }
            acc.swing = acc.swing.max(r.high - r.low);
        }
        RecipeApply::Canvas { recipe, channel } => {
            let class = class_of_attr(&channel.attr);
            if !acc.classes.contains(&class) {
                acc.classes.push(class);
            }
            acc.period = recipe.timing.measure / 4.0;
            if channel.attr == Attribute::Dimmer && !channel.relative {
                for r in roles {
                    info.levels.insert(r.clone(), channel.high);
                }
            }
        }
        a if is_position(a) => {
            if sustained && !acc.classes.contains(&Class::Position) {
                acc.classes.push(Class::Position);
            }
            if !sustained || matches!(a, RecipeApply::FocusDelta(_)) {
                info.aims.extend(roles.iter().cloned());
            }
        }
        _ => {}
    }
}

/// Reads one concrete recipe into the cue's summary.
fn read_recipe(info: &mut Info, name: &str, recipe: &Recipe, stacked: bool) {
    let mut roles = Vec::new();
    leaves(&recipe.target, &mut roles);
    let sustained = recipe.steps.len() > 1
        || recipe.steps.iter().any(|s| {
            s.apply
                .iter()
                .any(|a| matches!(a, RecipeApply::Random(_) | RecipeApply::Canvas { .. }))
        });
    let mut acc = RecipeAccum {
        period: recipe.timing.measure / 4.0,
        ..RecipeAccum::default()
    };
    for step in &recipe.steps {
        for apply in &step.apply {
            read_apply(info, &roles, sustained, &mut acc, apply);
        }
    }
    if sustained && acc.dimmers.len() > 1 {
        let lo = acc.dimmers.iter().copied().fold(f32::MAX, f32::min);
        let hi = acc.dimmers.iter().copied().fold(f32::MIN, f32::max);
        acc.swing = acc.swing.max(hi - lo);
        if !acc.classes.contains(&Class::Intensity) {
            acc.classes.push(Class::Intensity);
        }
    }
    let lower = name.to_ascii_lowercase();
    if (lower.contains("strobe") || lower.contains("blinder"))
        && !acc.classes.contains(&Class::Strobe)
    {
        acc.classes.push(Class::Strobe);
    }
    if sustained {
        info.effects.push(Effect {
            name: name.to_string(),
            roles,
            classes: acc.classes,
            period_bars: (!recipe.timing.once).then_some(acc.period),
            once: recipe.timing.once,
            swing: acc.swing,
            random: acc.random,
            stacked,
        });
    }
}

thread_local! {
    static SPLITS: std::cell::RefCell<Vec<ColorSplit>> = const { std::cell::RefCell::new(Vec::new()) };
}

fn split_families(name: &str) -> BTreeSet<Family> {
    SPLITS.with(|s| {
        s.borrow().iter().find(|s| s.name == name).map_or_else(
            || BTreeSet::from([Family::Other]),
            |s| {
                s.colors
                    .iter()
                    .filter_map(|c| match c {
                        Ref::Named(n) => Some(family(n)),
                        Ref::Inline(_) => None,
                    })
                    .collect()
            },
        )
    })
}

fn read_cue(index: usize, cue: &Cue, song: &SongMap) -> Info {
    let at = cue
        .position()
        .or_else(|| cue.at.as_ref().and_then(|p| p.resolve(song)));
    let bpm = at.map_or(120.0, |a| narrow(song.tempo.at(a).bpm));
    let mut info = Info {
        index,
        name: cue.name.clone(),
        block: cue.block,
        at,
        flat: at.map_or(f32::MAX, flat),
        fade_beats: cue.fade_secs * bpm / 60.0,
        levels: BTreeMap::new(),
        hues: BTreeMap::new(),
        aims: BTreeSet::new(),
        effects: Vec::new(),
        all_zero: false,
    };
    // Resolved the way the player resolves them — through the shipped
    // library, bundles and looks — so the lint judges what will play:
    // a look's recipes count as the cue's, a `bars` param is the loop
    // the rules measure.
    let library = ignition_effects::effects::library();
    let bundles = ignition_effects::effects::bundles();
    let looks = ignition_playback::macros::looks();
    let show = ignition_show::Show {
        library: &library,
        bundles: &bundles,
        looks: &looks,
        ..ignition_show::Show::new(&[], &ignition_rig::selection::EMPTY_RIG)
    };
    for r in &cue.recipes {
        let name = match r {
            RecipeRef::Inline(_) => "inline",
            RecipeRef::Named { effect, .. } => effect.as_str(),
            RecipeRef::Bundle { bundle, .. } => bundle.as_str(),
            RecipeRef::Look { look } => look.as_str(),
        };
        for recipe in r.resolve(&show) {
            let stacked = matches!(r, RecipeRef::Inline(_)) && recipe.stack;
            read_recipe(&mut info, name, &recipe, stacked);
        }
    }
    info.all_zero = !info.levels.is_empty()
        && info.levels.values().all(|v| *v <= 0.0)
        && info.effects.is_empty();
    info
}

/// A song section with the cues that fall inside it.
struct Part {
    name: String,
    kind: Kind,
    start: f32,
    end: f32,
    /// The blocking cue on the section's downbeat.
    cue: Option<usize>,
    /// Every cue inside, section cue first.
    cues: Vec<usize>,
}

/// The energy curve: every section cue's energy (0–1), level and lit
/// layer count, in order — what `--lint` prints so a reader can see
/// the shape the rules judged.
#[must_use]
pub fn energy_curve(
    list: &CueList,
    song: &SongMap,
    splits: &[ColorSplit],
) -> Vec<(String, f32, f32, usize)> {
    SPLITS.with(|s| *s.borrow_mut() = splits.to_vec());
    list.cues
        .iter()
        .enumerate()
        .map(|(i, c)| read_cue(i, c, song))
        .filter(|i| i.block && i.at.is_some())
        .map(|i| {
            (
                i.name.clone(),
                i.energy(),
                i.level_max(),
                i.lit_layers().len(),
            )
        })
        .collect()
}

/// Runs every rule. `splits` is the profile's palette of named splits,
/// so a `{"Split": "Ocean"}` can be read for its hues.
#[must_use]
pub fn lint(list: &CueList, song: &SongMap, splits: &[ColorSplit]) -> Vec<Finding> {
    SPLITS.with(|s| *s.borrow_mut() = splits.to_vec());
    let infos: Vec<Info> = list
        .cues
        .iter()
        .enumerate()
        .map(|(i, c)| read_cue(i, c, song))
        .collect();
    let mut parts: Vec<Part> = song
        .sections
        .iter()
        .map(|s| {
            let start = flat(s.start);
            Part {
                name: s.name.clone(),
                kind: kind_of(&s.name),
                start,
                end: start + narrow(s.bars),
                cue: None,
                cues: Vec::new(),
            }
        })
        .collect();
    for info in &infos {
        if let Some(part) = parts
            .iter_mut()
            .find(|p| info.flat >= p.start && info.flat < p.end)
        {
            if info.block && (info.flat - part.start).abs() < f32::EPSILON && part.cue.is_none() {
                part.cue = Some(info.index);
            }
            part.cues.push(info.index);
        }
    }
    let last_chorus = parts.iter().rposition(|p| p.kind == Kind::Chorus);

    let mut out = Vec::new();
    let mut push = |rule: u8, name: &'static str, cue: Option<&str>, message: String| {
        out.push(Finding {
            rule,
            name,
            cue: cue.map(str::to_string),
            message,
        });
    };

    structure(&infos, &parts, last_chorus, list, &mut push);
    colour(&infos, &parts, last_chorus, &mut push);
    faces(&infos, &parts, list, &mut push);
    movement(&infos, &parts, last_chorus, song, &mut push);
    effects(&infos, &parts, last_chorus, list, &mut push);
    accents(&list.triggers, &parts, song, &mut push);
    portability(list, &mut push);
    out
}

type Push<'a> = dyn FnMut(u8, &'static str, Option<&str>, String) + 'a;

const fn is_vocal(kind: Kind) -> bool {
    matches!(
        kind,
        Kind::Verse | Kind::PreChorus | Kind::Chorus | Kind::Bridge | Kind::Breakdown
    )
}

/// Rule 1: every section has a cue on its downbeat, and few blocking ones.
fn rule_section_has_cue(infos: &[Info], parts: &[Part], push: &mut Push<'_>) {
    for part in parts {
        if part.cue.is_none() {
            push(
                1,
                "section-has-cue",
                None,
                format!("{} has no blocking cue on its downbeat", part.name),
            );
        }
        let blocking = part
            .cues
            .iter()
            .filter(|i| infos.get(**i).is_some_and(|info| info.block))
            .count();
        if blocking > 4 {
            push(
                1,
                "section-has-cue",
                None,
                format!("{} has {blocking} blocking cues", part.name),
            );
        }
    }
}

/// Rule 2: overall cue density.
fn rule_density(infos: &[Info], push: &mut Push<'_>) {
    let non_accent = infos.iter().filter(|i| i.block).count();
    if non_accent > 25 {
        push(2, "density", None, format!("{non_accent} non-accent cues"));
    }
}

/// Rule 4: fades into and out of a chorus.
fn rule_fades(infos: &[Info], parts: &[Part], push: &mut Push<'_>) {
    for (n, part) in parts.iter().enumerate() {
        let Some(c) = part.cue else { continue };
        let Some(info) = infos.get(c) else { continue };
        if part.kind == Kind::Chorus && info.fade_beats > 1.0 {
            push(
                4,
                "fade-into-chorus",
                Some(&info.name),
                format!("fades in over {:.2} beats", info.fade_beats),
            );
        }
        let prev_was_chorus = n
            .checked_sub(1)
            .and_then(|p| parts.get(p))
            .is_some_and(|p| p.kind == Kind::Chorus);
        if prev_was_chorus
            && matches!(part.kind, Kind::Verse | Kind::Bridge)
            && !(4.0..=8.0).contains(&info.fade_beats)
        {
            push(
                4,
                "fade-out-of-chorus",
                Some(&info.name),
                format!("{:.2} beats after a chorus; 4–8 wanted", info.fade_beats),
            );
        }
    }
}

/// Rule 5: adjacent sections of different kind differ in at least two ways.
fn rule_adjacent_differ(infos: &[Info], parts: &[Part], push: &mut Push<'_>) {
    for w in parts.windows(2) {
        let [a, b] = w else { continue };
        if a.kind == b.kind {
            continue;
        }
        let (Some(ia), Some(ib)) = (a.cue, b.cue) else {
            continue;
        };
        let (Some(ia), Some(ib)) = (infos.get(ia), infos.get(ib)) else {
            continue;
        };
        let mut differs: u8 = 0;
        if ia.dominant() != ib.dominant() {
            differs = differs.saturating_add(1);
        }
        if (ia.level_max() - ib.level_max()).abs() >= 0.2 {
            differs = differs.saturating_add(1);
        }
        if ia.lit_layers().len() != ib.lit_layers().len() {
            differs = differs.saturating_add(1);
        }
        if ia.movement() != ib.movement() {
            differs = differs.saturating_add(1);
        }
        if differs < 2 {
            push(
                5,
                "adjacent-differ",
                Some(&ib.name),
                format!(
                    "only {differs} of hue/level/layers/movement change from {}",
                    ia.name
                ),
            );
        }
    }
}

/// Rule 6: each chorus is a superset of the one before it.
fn rule_chorus_superset(infos: &[Info], choruses: &[&Part], push: &mut Push<'_>) {
    for w in choruses.windows(2) {
        let [pa, pb] = w else { continue };
        let (Some(a), Some(b)) = (pa.cue, pb.cue) else {
            continue;
        };
        let (Some(a), Some(b)) = (infos.get(a), infos.get(b)) else {
            continue;
        };
        if a.dominant() != b.dominant() {
            push(
                6,
                "chorus-superset",
                Some(&b.name),
                format!("hue differs from {}", a.name),
            );
        }
        if b.level_max() + 1e-3 < a.level_max() {
            push(
                6,
                "chorus-superset",
                Some(&b.name),
                format!("dimmer than {}", a.name),
            );
        }
        if !a.lit_layers().is_subset(&b.lit_layers()) {
            push(
                6,
                "chorus-superset",
                Some(&b.name),
                format!("lights fewer layers than {}", a.name),
            );
        }
    }
}

/// Rule 7: the energy curve — capped rises, and a peak that is actually
/// the peak.
fn rule_energy_curve(
    infos: &[Info],
    parts: &[Part],
    choruses: &[&Part],
    last_chorus: Option<usize>,
    push: &mut Push<'_>,
) {
    let energies: Vec<(usize, f32)> = infos
        .iter()
        .filter(|i| i.block && i.at.is_some())
        .map(|i| (i.index, i.energy()))
        .collect();
    for (n, c) in choruses.iter().enumerate() {
        let Some(ci) = c.cue else { continue };
        let Some(info_ci) = infos.get(ci) else {
            continue;
        };
        let e = info_ci.energy();
        let this_part = parts.iter().position(|p| std::ptr::eq(p, *c));
        let is_final = this_part.is_some() && this_part == last_chorus;
        let cap = if is_final {
            1.0
        } else if n == 0 {
            0.7
        } else {
            0.85
        };
        if e > cap + 1e-3 {
            push(
                7,
                "energy-curve",
                Some(&info_ci.name),
                format!("energy {e:.2} over the {cap:.2} cap"),
            );
        }
        if is_final {
            for (i, other) in &energies {
                let Some(other_info) = infos.get(*i) else {
                    continue;
                };
                if *i != ci && *other + 0.05 > e {
                    push(
                        7,
                        "energy-curve",
                        Some(&other_info.name),
                        format!("energy {other:.2} within 0.05 of the peak {e:.2}"),
                    );
                }
            }
        }
    }
}

/// Rule 8: the section before the last chorus is the darkest since the
/// intro.
fn rule_dark_before_peak(
    infos: &[Info],
    parts: &[Part],
    last_chorus: Option<usize>,
    push: &mut Push<'_>,
) {
    if let Some(lc) = last_chorus
        && let Some(before) = lc
            .checked_sub(1)
            .and_then(|p| parts.get(p))
            .and_then(|p| p.cue)
        && let Some(before_info) = infos.get(before)
    {
        let min_verse = parts
            .iter()
            .filter(|p| p.kind == Kind::Verse)
            .filter_map(|p| p.cue)
            .filter_map(|c| infos.get(c))
            .map(Info::energy)
            .fold(f32::MAX, f32::min);
        let e = before_info.energy();
        if e > min_verse + 1e-3 {
            push(
                8,
                "dark-before-peak",
                Some(&before_info.name),
                format!("energy {e:.2} above the quietest verse {min_verse:.2}"),
            );
        }
    }
}

/// Rule 9: the vocal range spans enough level to read as dynamics.
fn rule_vocal_range(infos: &[Info], parts: &[Part], push: &mut Push<'_>) {
    let vocal: Vec<f32> = parts
        .iter()
        .filter(|p| is_vocal(p.kind))
        .filter_map(|p| p.cue)
        .filter_map(|c| infos.get(c))
        .map(Info::level_max)
        .collect();
    if let (Some(lo), Some(hi)) = (
        vocal.iter().copied().reduce(f32::min),
        vocal.iter().copied().reduce(f32::max),
    ) && hi - lo < 0.5
    {
        push(
            9,
            "vocal-range",
            None,
            format!("vocal sections span only {lo:.2}–{hi:.2}"),
        );
    }
}

/// Rule 10: `safe` and `reset` cues exist.
fn rule_safe_and_reset(list: &CueList, push: &mut Push<'_>) {
    for name in ["safe", "reset"] {
        if !list.cues.iter().any(|c| c.name == name) {
            push(10, "safe-and-reset", None, format!("no `{name}` cue"));
        }
    }
}

/// Rule 31: an all-zero cue mid-song is tagged as a blackout.
fn rule_blackout(infos: &[Info], push: &mut Push<'_>) {
    for info in infos.iter().filter(|i| i.all_zero && i.at.is_some()) {
        // The end of the song: nothing after it but the reset.
        let followed_by_song = infos
            .get(info.index.saturating_add(1)..)
            .unwrap_or(&[])
            .iter()
            .any(|j| j.at.is_some() && j.name != "reset");
        if !followed_by_song {
            continue;
        }
        if !info.name.to_ascii_lowercase().contains("blackout") {
            push(
                31,
                "blackout",
                Some(&info.name),
                "every layer at zero and not tagged blackout".into(),
            );
        }
    }
}

fn structure(
    infos: &[Info],
    parts: &[Part],
    last_chorus: Option<usize>,
    list: &CueList,
    push: &mut Push<'_>,
) {
    rule_section_has_cue(infos, parts, push);
    rule_density(infos, push);
    rule_fades(infos, parts, push);
    rule_adjacent_differ(infos, parts, push);
    let choruses: Vec<&Part> = parts.iter().filter(|p| p.kind == Kind::Chorus).collect();
    rule_chorus_superset(infos, &choruses, push);
    rule_energy_curve(infos, parts, &choruses, last_chorus, push);
    rule_dark_before_peak(infos, parts, last_chorus, push);
    rule_vocal_range(infos, parts, push);
    rule_safe_and_reset(list, push);
    rule_blackout(infos, push);
}

/// Rule 11: three hues, the third only away from the vocal core.
fn rule_three_hues(infos: &[Info], parts: &[Part], push: &mut Push<'_>) {
    let mut where_used: BTreeMap<Family, Vec<Kind>> = BTreeMap::new();
    for part in parts {
        for c in &part.cues {
            let Some(info) = infos.get(*c) else { continue };
            for f in info.families() {
                let kinds = where_used.entry(f).or_default();
                if !kinds.contains(&part.kind) {
                    kinds.push(part.kind);
                }
            }
        }
    }
    if where_used.len() > 3 {
        push(
            11,
            "three-hues",
            None,
            format!(
                "{} hue families in use: {:?}",
                where_used.len(),
                where_used.keys().collect::<Vec<_>>()
            ),
        );
    }
    if where_used.len() == 3 {
        let core = [Kind::Verse, Kind::PreChorus, Kind::Chorus];
        let in_core: Vec<&Family> = where_used
            .iter()
            .filter(|(_, kinds)| kinds.iter().any(|k| core.contains(k)))
            .map(|(f, _)| f)
            .collect();
        if in_core.len() > 2 {
            push(
                11,
                "three-hues",
                None,
                format!("all three hues reach verse/pre/chorus: {in_core:?}"),
            );
        }
    }
}

/// Rule 12: the chorus owns its hue.
fn rule_chorus_owns_hue(infos: &[Info], parts: &[Part], push: &mut Push<'_>) {
    let chorus_hue = parts
        .iter()
        .find(|p| p.kind == Kind::Chorus)
        .and_then(|p| p.cue)
        .and_then(|c| infos.get(c))
        .and_then(Info::dominant);
    let Some(hue) = chorus_hue else { return };
    for part in parts {
        for c in &part.cues {
            let Some(info) = infos.get(*c) else { continue };
            match part.kind {
                Kind::Verse if info.families().contains(&hue) => {
                    push(
                        12,
                        "chorus-owns-hue",
                        Some(&info.name),
                        format!("uses the chorus hue {hue:?} in a verse"),
                    );
                }
                Kind::PreChorus if info.families().contains(&hue) && info.flat < part.end - 2.0 => {
                    push(
                        12,
                        "chorus-owns-hue",
                        Some(&info.name),
                        format!("uses the chorus hue {hue:?} before the last two bars of the pre"),
                    );
                }
                Kind::Chorus if info.block && info.dominant() != Some(hue) => {
                    push(
                        12,
                        "chorus-owns-hue",
                        Some(&info.name),
                        format!("chorus hue is {:?}, not {hue:?}", info.dominant()),
                    );
                }
                _ => {}
            }
        }
    }
}

/// Rule 13: the key is white, at one temperature.
fn rule_key_is_white(infos: &[Info], push: &mut Push<'_>) {
    let mut key_colours: BTreeSet<Family> = BTreeSet::new();
    for info in infos {
        if let Some(f) = info.hues.get("Key") {
            key_colours.extend(f.iter().copied());
        }
    }
    if key_colours.iter().any(|f| *f != Family::White) {
        push(
            13,
            "key-is-white",
            None,
            format!("key carries {key_colours:?}"),
        );
    }
}

/// Rule 14: two hues per role per cue.
fn rule_two_hues_per_role(infos: &[Info], push: &mut Push<'_>) {
    for info in infos {
        for (role, fams) in &info.hues {
            let n = fams.iter().filter(|f| **f != Family::White).count();
            if n > 2 {
                push(
                    14,
                    "two-hues-per-role",
                    Some(&info.name),
                    format!("{role} carries {n} hue families"),
                );
            }
        }
    }
}

/// Rule 17: the peak has a white layer.
fn rule_peak_has_white(infos: &[Info], parts: &[Part], push: &mut Push<'_>) {
    let Some(info) = parts
        .iter()
        .rev()
        .find(|p| p.kind == Kind::Chorus)
        .and_then(|p| p.cue)
        .and_then(|c| infos.get(c))
    else {
        return;
    };
    let white = LAYERS.iter().any(|r| {
        info.level(r) > 0.0
            && info
                .hues
                .get(*r)
                .is_some_and(|f| f.contains(&Family::White))
    });
    if !white {
        push(
            17,
            "peak-has-white",
            Some(&info.name),
            "no white or open layer at the peak".into(),
        );
    }
}

fn colour(infos: &[Info], parts: &[Part], _last_chorus: Option<usize>, push: &mut Push<'_>) {
    rule_three_hues(infos, parts, push);
    rule_chorus_owns_hue(infos, parts, push);
    rule_key_is_white(infos, push);
    rule_two_hues_per_role(infos, push);
    rule_peak_has_white(infos, parts, push);
}

fn faces(infos: &[Info], parts: &[Part], list: &CueList, push: &mut Push<'_>) {
    // 18. key level in vocal sections
    for part in parts.iter().filter(|p| is_vocal(p.kind)) {
        let Some(c) = part.cue else { continue };
        let Some(info) = infos.get(c) else { continue };
        let key = info.level("Key");
        if !(0.5..=0.8).contains(&key) {
            push(
                18,
                "key-level",
                Some(&info.name),
                format!("key at {key:.2} in a vocal section"),
            );
        }
    }
    // 19. the key is never chased, strobed or accented
    for info in infos {
        for e in &info.effects {
            if e.roles.iter().any(|r| r == "Key")
                && !e.random
                && (e.classes.contains(&Class::Intensity) || e.classes.contains(&Class::Strobe))
            {
                push(
                    19,
                    "key-untouched",
                    Some(&info.name),
                    format!("`{}` runs an intensity effect on the key", e.name),
                );
            }
        }
    }
    for t in &list.triggers {
        let mut roles = Vec::new();
        leaves(&t.recipe.target, &mut roles);
        if roles.iter().any(|r| r == "Key") {
            push(
                19,
                "key-untouched",
                None,
                format!("trigger `{}` accents the key", t.name),
            );
        }
    }
    // 20. backlight whenever the key is up
    for info in infos.iter().filter(|i| i.block) {
        if info.level("Key") > 0.0 && info.level("Back") <= 0.0 {
            push(
                20,
                "back-with-key",
                Some(&info.name),
                "key up with no backlight".into(),
            );
        }
    }
}

fn movement(
    infos: &[Info],
    parts: &[Part],
    last_chorus: Option<usize>,
    song: &SongMap,
    push: &mut Push<'_>,
) {
    // 21. one sustained position effect per role per cue
    for info in infos {
        // A stacked recipe rides under another (`r[effects.relative-stack]`)
        // and is one idea with it, so it is not a second effect.
        let mut per_role: BTreeMap<&str, usize> = BTreeMap::new();
        for e in info
            .effects
            .iter()
            .filter(|e| !e.once && !e.stacked && e.classes.contains(&Class::Position))
        {
            for r in &e.roles {
                per_role
                    .entry(r.as_str())
                    .and_modify(|n| *n = n.saturating_add(1))
                    .or_insert(1);
            }
        }
        for (role, n) in per_role {
            if n > 1 {
                push(
                    21,
                    "one-position-effect",
                    Some(&info.name),
                    format!("{n} position effects on {role}"),
                );
            }
        }
    }
    // 22. position period by section kind
    for (n, part) in parts.iter().enumerate() {
        let floor = match part.kind {
            Kind::Chorus if Some(n) == last_chorus => 0.5,
            Kind::Chorus | Kind::PreChorus | Kind::Break => 1.0,
            _ => 2.0,
        };
        for c in &part.cues {
            let Some(info) = infos.get(*c) else { continue };
            for e in info
                .effects
                .iter()
                .filter(|e| e.classes.contains(&Class::Position))
            {
                if let Some(p) = e.period_bars
                    && p + 1e-3 < floor
                {
                    push(
                        22,
                        "position-period",
                        Some(&info.name),
                        format!("`{}` loops every {p} bars in a {:?}", e.name, part.kind),
                    );
                }
            }
        }
    }
    // 25. the movers move in at most 60 % of the song
    let total: f32 = song.sections.iter().map(|s| narrow(s.bars)).sum();
    let mut moving = 0.0f32;
    let positioned: Vec<&Info> = infos.iter().filter(|i| i.at.is_some()).collect();
    for (k, info) in positioned.iter().enumerate() {
        let runs = info.effects.iter().any(|e| {
            !e.once
                && e.classes.contains(&Class::Position)
                && e.roles.iter().any(|r| r == "Movers" && info.level(r) > 0.0)
        });
        if !runs {
            continue;
        }
        // Until the next cue that blocks or re-aims the movers.
        let end = positioned
            .get(k.saturating_add(1)..)
            .unwrap_or(&[])
            .iter()
            .find(|j| {
                j.block
                    || j.aims.contains("Movers")
                    || j.effects
                        .iter()
                        .any(|e| e.roles.iter().any(|r| r == "Movers"))
            })
            .map_or(total + 1.0, |j| j.flat);
        moving += (end - info.flat).max(0.0);
    }
    if moving > 0.6 * total {
        push(
            25,
            "movers-share",
            None,
            format!("movers run an effect for {moving:.0} of {total:.0} bars"),
        );
    }
}

/// How long an effect on `e.roles` runs from cue `k`: until a cue
/// blocks or restates those roles' class absolutely. Pulled out of
/// [`effects`] so the rules that call it can live in their own
/// functions instead of closing over one giant closure.
fn effect_window(positioned: &[&Info], total: f32, k: usize, e: &Effect) -> f32 {
    let Some(cue) = positioned.get(k) else {
        return 0.0;
    };
    let start = cue.flat;
    let end = positioned
        .get(k.saturating_add(1)..)
        .unwrap_or(&[])
        .iter()
        .find(|j| {
            j.block
                || e.roles.iter().any(|r| {
                    (e.classes.contains(&Class::Intensity) || e.classes.contains(&Class::Strobe))
                        && j.levels.contains_key(r)
                        || e.classes.contains(&Class::Colour) && j.hues.contains_key(r)
                        || e.classes.contains(&Class::Position) && j.aims.contains(r)
                })
        })
        .map_or(total, |j| j.flat);
    (end - start).max(0.0)
}

/// Rule 26: periods are beat subdivisions.
fn rule_effect_period(infos: &[Info], push: &mut Push<'_>) {
    for info in infos {
        for e in &info.effects {
            if let Some(p) = e.period_bars
                && ![4.0, 2.0, 1.0, 0.5].iter().any(|ok| (p - ok).abs() < 1e-3)
            {
                push(
                    26,
                    "effect-period",
                    Some(&info.name),
                    format!("`{}` loops every {p} bars", e.name),
                );
            }
        }
    }
}

/// Rule 16: rainbows never in a verse or bridge, never over four bars.
fn rule_rainbow(positioned: &[&Info], parts: &[Part], total: f32, push: &mut Push<'_>) {
    for (k, info) in positioned.iter().enumerate() {
        for e in &info.effects {
            let lower = e.name.to_ascii_lowercase();
            if !(lower.contains("rainbow") || lower.contains("colour cycle")) {
                continue;
            }
            let kind = parts
                .iter()
                .find(|p| info.flat >= p.start && info.flat < p.end)
                .map(|p| p.kind);
            if matches!(kind, Some(Kind::Verse | Kind::Bridge)) {
                push(
                    16,
                    "rainbow",
                    Some(&info.name),
                    format!("`{}` in a {kind:?}", e.name),
                );
            }
            let w = effect_window(positioned, total, k, e);
            if w > 4.0 + 1e-3 {
                push(
                    16,
                    "rainbow",
                    Some(&info.name),
                    format!("`{}` runs {w:.1} bars", e.name),
                );
            }
        }
    }
}

/// Rule 27: fast intensity effects rest.
fn rule_flicker_fatigue(positioned: &[&Info], total: f32, push: &mut Push<'_>) {
    for (k, info) in positioned.iter().enumerate() {
        for e in &info.effects {
            let fast = e.classes.contains(&Class::Intensity)
                && e.swing > 0.3
                && !e.random
                && e.period_bars.is_some_and(|p| p <= 1.0);
            let w = effect_window(positioned, total, k, e);
            if fast && w > 16.0 + 1e-3 {
                push(
                    27,
                    "flicker-fatigue",
                    Some(&info.name),
                    format!("`{}` runs {w:.0} bars without rest", e.name),
                );
            }
        }
    }
}

/// Rule 28: strobes are last-chorus-only, four bars, two bursts; a
/// riser is a shutter ramp under the bar before a chorus, not a burst.
fn rule_strobe(
    positioned: &[&Info],
    parts: &[Part],
    last_chorus: Option<usize>,
    total: f32,
    list: &CueList,
    push: &mut Push<'_>,
) {
    let mut bursts: u32 = 0;
    for (k, info) in positioned.iter().enumerate() {
        let section = parts
            .iter()
            .position(|p| info.flat >= p.start && info.flat < p.end);
        for e in info
            .effects
            .iter()
            .filter(|e| e.classes.contains(&Class::Strobe))
        {
            let lower = e.name.to_ascii_lowercase();
            if lower.contains("riser") || lower.contains("strobe bed") {
                let next_is_chorus = section
                    .and_then(|s| s.checked_add(1))
                    .and_then(|s| parts.get(s))
                    .is_some_and(|p| p.kind == Kind::Chorus);
                let w = effect_window(positioned, total, k, e);
                if !e.once || !next_is_chorus || w > 1.0 + 1e-3 {
                    push(
                        28,
                        "strobe",
                        Some(&info.name),
                        format!(
                            "`{}` is a riser that does not run into a chorus over its last bar ({w:.1} bars)",
                            e.name
                        ),
                    );
                }
                continue;
            }
            bursts = bursts.saturating_add(1);
            if section != last_chorus {
                push(
                    28,
                    "strobe",
                    Some(&info.name),
                    format!("`{}` outside the last chorus", e.name),
                );
            }
            let w = effect_window(positioned, total, k, e);
            if w > 4.0 + 1e-3 {
                push(
                    28,
                    "strobe",
                    Some(&info.name),
                    format!("`{}` runs {w:.1} bars", e.name),
                );
            }
        }
    }
    if bursts > 2 {
        push(
            28,
            "strobe",
            None,
            format!("{bursts} strobe bursts; two at most"),
        );
    }
    // A trigger may not strobe either.
    for t in &list.triggers {
        if t.recipe.steps.iter().flat_map(|s| s.apply.iter()).any(|a| {
            matches!(a, RecipeApply::Delta(p) if p.iter().any(|(a, _)| *a == Attribute::Strobe))
        }) {
            push(28, "strobe", None, format!("trigger `{}` strobes", t.name));
        }
    }
}

fn effects(
    infos: &[Info],
    parts: &[Part],
    last_chorus: Option<usize>,
    list: &CueList,
    push: &mut Push<'_>,
) {
    let positioned: Vec<&Info> = infos.iter().filter(|i| i.at.is_some()).collect();
    let total: f32 = parts.last().map_or(0.0, |p| p.end);
    rule_effect_period(infos, push);
    rule_rainbow(&positioned, parts, total, push);
    rule_flicker_fatigue(&positioned, total, push);
    rule_strobe(&positioned, parts, last_chorus, total, list, push);
}

fn accents(triggers: &[Trigger], parts: &[Part], song: &SongMap, push: &mut Push<'_>) {
    // 30. density by section kind. The figures in a bar are one accent
    // however many moments they carry: a figure is a person's drawing
    // of the music, and two drawn in one bar are that bar's one idea.
    let mut per_bar: BTreeMap<u32, BTreeSet<String>> = BTreeMap::new();
    for t in triggers {
        let Some(at) = t.bars().or_else(|| t.at.resolve(song)) else {
            continue;
        };
        let id = if t.name.starts_with("fig ") {
            "figure".to_string()
        } else {
            t.name.clone()
        };
        per_bar.entry(at.bar).or_default().insert(id);
    }
    for part in parts {
        let (cap, per) = match part.kind {
            Kind::Chorus
            | Kind::PreChorus
            | Kind::Intro
            | Kind::Bridge
            | Kind::Break
            | Kind::CountIn
            | Kind::Other => (1, 1),
            Kind::Verse | Kind::Outro => (1, 2),
            Kind::Breakdown => (0, 1),
        };
        let first = bar_of(part.start);
        let last = bar_of(part.end).saturating_sub(1);
        let mut bar = first;
        while bar <= last {
            let span_end = bar.saturating_add(per).saturating_sub(1).min(last);
            let n: usize = (bar..=span_end)
                .map(|b| per_bar.get(&b).map_or(0, BTreeSet::len))
                .sum();
            // A breakdown may hit its final bar: the drop is coming.
            let allowed = if part.kind == Kind::Breakdown && span_end == last {
                1
            } else {
                cap
            };
            if n > allowed {
                push(
                    30,
                    "accent-density",
                    None,
                    format!(
                        "{n} accents in bars {bar}–{span_end} of {} ({:?}); {allowed} per {per} bar(s) allowed",
                        part.name, part.kind
                    ),
                );
            }
            bar = bar.saturating_add(per);
        }
    }
    // Accents ride relative, decay within two beats.
    for t in triggers {
        let absolute = t
            .recipe
            .steps
            .iter()
            .flat_map(|s| s.apply.iter())
            .any(|a| matches!(a, RecipeApply::Dimmer(_) | RecipeApply::Color(_)));
        if absolute {
            push(
                30,
                "accent-density",
                None,
                format!("trigger `{}` sets an absolute level or colour", t.name),
            );
        }
    }
}

fn portability(list: &CueList, push: &mut Push<'_>) {
    fn check(sel: &Selection, out: &mut Vec<String>) {
        match sel {
            Selection::Chans(c) if !c.is_empty() => out.push(format!("{} channels", c.len())),
            Selection::Group(g) => out.push(format!("group {g:?}")),
            Selection::Union(v) | Selection::Intersect(v) => v.iter().for_each(|s| check(s, out)),
            Selection::Where { of, .. } | Selection::Order { of, .. } => check(of, out),
            _ => {}
        }
    }
    for cue in &list.cues {
        // sections block; accents do not
        let accent = cue.name.starts_with('·');
        if cue.block == accent && cue.at.is_some() {
            push(
                1,
                "sections-block",
                Some(&cue.name),
                if accent {
                    "an accent that blocks"
                } else {
                    "a section cue that does not block"
                }
                .into(),
            );
        }
        if !cue.values.is_empty() {
            push(
                1,
                "recipes-not-values",
                Some(&cue.name),
                format!("{} direct values", cue.values.len()),
            );
        }
        let mut found = Vec::new();
        for r in &cue.recipes {
            match r {
                RecipeRef::Inline(recipe) => check(&recipe.target, &mut found),
                RecipeRef::Named {
                    target: Some(t), ..
                }
                | RecipeRef::Bundle {
                    target: Some(t), ..
                } => check(t, &mut found),
                _ => {}
            }
        }
        for f in found {
            push(1, "roles-only", Some(&cue.name), format!("names {f}"));
        }
    }
    for t in &list.triggers {
        let mut found = Vec::new();
        check(&t.recipe.target, &mut found);
        for f in found {
            push(
                1,
                "roles-only",
                None,
                format!("trigger `{}` names {f}", t.name),
            );
        }
    }
    // 23. move in black on every re-aim
    for (i, cue) in list.cues.iter().enumerate() {
        if crate::mib::reaims(list, i) && cue.mib.mode == MibMode::None {
            push(
                23,
                "mib-on-reaim",
                Some(&cue.name),
                "re-aims a mover with pre-positioning off".into(),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ignition_daw_proto::{Section, TempoMap};

    fn song() -> SongMap {
        let mut sections = Vec::new();
        let mut bar = 1;
        for (name, bars) in [("VS 1", 8.0), ("CH 1", 8.0), ("VS 2", 8.0), ("CH 2", 8.0)] {
            sections.push(Section {
                name: name.into(),
                start: Bars::bar(bar),
                bars,
            });
            bar = bar.saturating_add(bar_of(narrow(bars)));
        }
        SongMap {
            name: "t".into(),
            tempo: TempoMap::constant(120.0, ignition_daw_proto::TimeSignature::default()),
            sections,
        }
    }

    fn look(role: &str, level: f32, colour: &str) -> RecipeRef {
        let mut r = Recipe::new(Selection::Role(role.into()), RecipeApply::Dimmer(level));
        r.steps[0]
            .apply
            .push(RecipeApply::Color(Ref::Named(colour.into())));
        r.into()
    }

    fn cue(name: &str, section: &str, recipes: Vec<RecipeRef>) -> Cue {
        Cue {
            name: name.into(),
            recipes,
            block: !name.starts_with('·'),
            at: Some(ignition_daw_proto::Position::at(section, 0)),
            ..Default::default()
        }
    }

    #[test]
    fn a_verse_wearing_the_chorus_hue_and_a_chased_key_are_caught() {
        let song = song();
        let mut list = CueList {
            name: "t".into(),
            cues: vec![
                cue(
                    "VS 1",
                    "VS 1",
                    vec![
                        look("Key", 0.6, "Warm White"),
                        look("Wash", 0.4, "Gold"),
                        look("Back", 0.3, "Gold"),
                    ],
                ),
                cue(
                    "CH 1",
                    "CH 1",
                    vec![
                        look("Key", 0.7, "Warm White"),
                        look("Wash", 0.7, "Gold"),
                        look("Back", 0.7, "Amber"),
                        RecipeRef::Named {
                            effect: "chase".into(),
                            name: None,
                            note: None,
                            cue_timing: None,
                            target: Some(Selection::Role("Key".into())),
                            bars: None,
                            tricks: None,
                            params: BTreeMap::new(),
                            filter: None,
                            speed: None,
                        },
                    ],
                ),
                cue(
                    "VS 2",
                    "VS 2",
                    vec![
                        look("Key", 0.6, "Warm White"),
                        look("Wash", 0.4, "Purple"),
                        look("Back", 0.3, "Purple"),
                    ],
                ),
                cue(
                    "CH 2",
                    "CH 2",
                    vec![
                        look("Key", 0.7, "Warm White"),
                        look("Wash", 0.9, "Gold"),
                        look("Back", 0.9, "Amber"),
                    ],
                ),
            ],
            triggers: Vec::new(),
            ..Default::default()
        };
        list.resolve_positions(&song);
        let findings = lint(&list, &song, &[]);
        let rules: BTreeSet<u8> = findings.iter().map(|f| f.rule).collect();
        assert!(rules.contains(&12), "{findings:#?}");
        assert!(rules.contains(&19), "{findings:#?}");
        assert!(rules.contains(&10), "{findings:#?}");
        assert!(!rules.contains(&14), "{findings:#?}");
    }

    #[test]
    fn families_fold_the_palette_into_three_hues_and_white() {
        assert_eq!(family("Lavender"), family("Magenta"));
        assert_eq!(family("Deep Blue"), family("Cyan"));
        assert_eq!(family("Warm White"), Family::White);
        assert_ne!(family("Gold"), family("Purple"));
    }
}
