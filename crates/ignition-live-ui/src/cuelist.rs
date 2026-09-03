//! The cue list panel.
//!
//! One panel, two column presets. Live shows what running the list
//! needs; Program shows everything a cue carries. Switching between them
//! changes which columns are visible and nothing else — same rows, same
//! order, same selection — so an operator who switches views mid-song is
//! looking at the same list they were.
//!
//! The rows are built once, at load, by [`rows`]: every number is
//! already a string in the units it is shown in, so this module does no
//! domain work and the browser client needs no copy of the engine to
//! draw a sheet.

// r[impl studio.cuelist.one-panel]
// r[impl studio.panels] - the Cue List panel

use crate::command::Command;
use crate::{
    ClassCell, CookState, CookStatus, CueRow, Flags, PartRow, Preset, Row, TrigKind, send,
    use_cue_progress, use_current_cue, use_ringing_hit,
};
use dioxus::prelude::*;
use ignition_core::cue::{AttrClass, Cue, MibMode, Trig};
use ignition_core::recipe::{Cook, RecipeRef, Show, Status, cook_list};
use ignition_core::{Bars, CueList as CoreList, Selection};

// ── Building the rows ────────────────────────────────────────────────

/// The list as rows to draw: every cue, plus the hits the song fires
/// between them, in the order they land.
///
/// `show` is the rig the list was loaded against. Given one, every cue
/// carries its cooked status; without one the status column is blank
/// rather than a guessed green — a cue list that lies about what
/// resolves is worse than one that admits it does not know.
// r[impl cues.cooked-status]
// r[impl studio.cuelist.status]
#[must_use]
pub fn rows(list: &CoreList, show: Option<&Show<'_>>) -> Vec<Row> {
    let cooked = show.map(|s| cook_list(&list.cues, s, 0.0));
    // `(position, is a cue, row)` — sorted by position, cues before the
    // hits at the same one, unpositioned cues keeping their list order
    // at the top.
    let mut rows: Vec<(Option<Bars>, bool, Row)> = list
        .cues
        .iter()
        .enumerate()
        .map(|(index, cue)| {
            let status = cooked.as_ref().and_then(|c| c.get(index)).map(status_of);
            let covers = cooked
                .as_ref()
                .and_then(|c| c.get(index))
                .map(|c| c.recipes.as_slice());
            (
                cue.position(),
                true,
                Row::Cue(Box::new(row_of(list, index, cue, status, covers))),
            )
        })
        .collect();

    // A cutout is two triggers at one position; show it once.
    let mut seen = std::collections::HashSet::new();
    for (index, t) in list.triggers.iter().enumerate() {
        let Some(at) = t.bars() else { continue };
        if t.name.ends_with(" cut") || !seen.insert((at.bar, at.beat.to_bits())) {
            continue;
        }
        rows.push((
            Some(at),
            false,
            Row::Hit {
                index,
                name: t.name.clone(),
                at,
            },
        ));
    }
    rows.sort_by(|a, b| {
        a.0.partial_cmp(&b.0)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(b.1.cmp(&a.1))
    });
    rows.into_iter().map(|(_, _, row)| row).collect()
}

fn row_of(
    list: &CoreList,
    index: usize,
    cue: &Cue,
    status: Option<CookStatus>,
    covers: Option<&[Cook]>,
) -> CueRow {
    CueRow {
        index,
        // r[impl cues.number]
        number: number(list.number_of(index)),
        name: cue.name.clone(),
        // r[impl cues.note]
        note: cue.note.clone(),
        // r[impl cues.appearance]
        appearance: cue.appearance.clone(),
        position: cue.at.as_ref().map(position_of).or_else(|| {
            // An older file carries only the resolved bar.
            cue.resolved.map(|b| bars(&b))
        }),
        at: cue.position(),
        trig: match cue.trig {
            Trig::Go if cue.position().is_some() => TrigKind::At,
            Trig::Go => TrigKind::Go,
            Trig::At(_) => TrigKind::At,
            Trig::Follow { .. } => TrigKind::Follow,
            Trig::Sound { .. } => TrigKind::Sound,
        },
        fade: secs(cue.fade_secs),
        // r[impl studio.cuelist.condensed-timing]
        timing: class_cells(&cue.timing),
        flags: Flags {
            block: cue.block,
            assert: cue.assert,
            cue_only: cue.cue_only,
            breaks: !cue.break_.is_empty(),
            morph: cue.morph,
            fan: cue.fan.is_some(),
            release: !cue.release.is_empty(),
        },
        mib: (cue.mib.mode != MibMode::default()).then(|| match cue.mib.mode {
            MibMode::Early => "early".to_string(),
            MibMode::UponGo => "on go".to_string(),
            MibMode::Late => "late".to_string(),
            MibMode::None => "none".to_string(),
        }),
        commands: cue.commands.clone(),
        status,
        // r[impl studio.cuelist.expand]
        parts: parts_of(cue, covers),
    }
}

/// A cue's parts: its recipes, then the selections it gives their own
/// timing to. Both draw the same shape — a cue has one kind of visible
/// subdivision, not two.
// r[impl cues.parts-are-recipes]
// r[impl studio.cuelist.expand]
fn parts_of(cue: &Cue, covers: Option<&[Cook]>) -> Vec<PartRow> {
    let mut parts: Vec<PartRow> = cue
        .recipes
        .iter()
        .enumerate()
        .map(|(i, r)| {
            let cook = covers.and_then(|c| c.get(i));
            PartRow {
                number: i,
                name: part_name(r),
                note: part_note(r),
                target: part_target(r),
                // r[impl cues.recipe.timing]
                timing: part_timing(r),
                covers: match cook {
                    Some(Cook::Ok(n)) => Some(*n),
                    Some(Cook::Empty) => Some(0),
                    _ => None,
                },
                is_override: false,
                disabled: matches!(cook, Some(Cook::Disabled))
                    || r.inline().is_some_and(|r| !r.enabled),
            }
        })
        .collect();
    // r[impl cues.timing-overrides] - drawn in the same sub-row shape
    for over in &cue.timing_overrides {
        parts.push(PartRow {
            number: parts.len(),
            name: "timing override".to_string(),
            note: String::new(),
            target: target_of(&over.selection),
            timing: class_cells(&over.timing),
            covers: None,
            is_override: true,
            disabled: false,
        });
    }
    parts
}

/// What to call a part. An unnamed inline recipe gets *nothing* rather
/// than "part 3": its number is already in the row and its target is in
/// the next column, so a generated name would be the only thing on the
/// line that says nothing.
fn part_name(r: &RecipeRef) -> String {
    match r {
        RecipeRef::Named { name, effect, .. } => name.clone().unwrap_or_else(|| effect.clone()),
        RecipeRef::Look { look } => look.clone(),
        RecipeRef::Bundle { bundle, .. } => bundle.clone(),
        RecipeRef::Inline(recipe) => recipe.name.clone().unwrap_or_default(),
    }
}

fn part_note(r: &RecipeRef) -> String {
    match r {
        RecipeRef::Named { note, .. } => note.clone().unwrap_or_default(),
        RecipeRef::Inline(recipe) => recipe.note.clone().unwrap_or_default(),
        _ => String::new(),
    }
}

fn part_target(r: &RecipeRef) -> String {
    match r {
        RecipeRef::Named { target, .. } | RecipeRef::Bundle { target, .. } => {
            target.as_ref().map(target_of).unwrap_or_default()
        }
        RecipeRef::Inline(recipe) => target_of(&recipe.target),
        RecipeRef::Look { .. } => String::new(),
    }
}

fn part_timing(r: &RecipeRef) -> Vec<ClassCell> {
    match r {
        RecipeRef::Named { cue_timing, .. } => {
            cue_timing.as_ref().map(class_cells).unwrap_or_default()
        }
        RecipeRef::Inline(recipe) => recipe
            .cue_timing
            .as_ref()
            .map(class_cells)
            .unwrap_or_default(),
        _ => Vec::new(),
    }
}

fn status_of(cook: &ignition_core::recipe::CueCook) -> CookStatus {
    CookStatus {
        state: match cook.status() {
            Status::Cooked => CookState::Cooked,
            Status::Failed => CookState::Failed,
            Status::Mixed => CookState::Mixed,
            Status::Direct => CookState::Direct,
            Status::Empty => CookState::Empty,
        },
        covers: cook
            .recipes
            .iter()
            .map(|c| match c {
                Cook::Ok(n) => *n,
                _ => 0,
            })
            .sum(),
        recipes: cook
            .recipes
            .iter()
            .filter(|c| matches!(c, Cook::Ok(_)))
            .count(),
        direct: cook.direct,
    }
}

/// Per-class timing, condensed: one cell a class, carrying fade and
/// delay together, and only for the classes that say something. Eight
/// columns is how a sheet stops fitting on a screen.
// r[impl studio.cuelist.condensed-timing]
fn class_cells(t: &ignition_core::cue::CueTiming) -> Vec<ClassCell> {
    use ignition_core::step::Ease;
    /// One class's cell, or nothing when the class says nothing.
    fn cell(class: &str, fade: Option<f32>, delay: f32, ease: Ease) -> Option<ClassCell> {
        if fade.is_none() && delay == 0.0 && ease == Ease::Linear {
            return None;
        }
        let value = match (fade, delay) {
            (Some(f), 0.0) => beats(f),
            (Some(f), d) => format!("{}/{}", beats(f), beats(d)),
            (None, d) => format!("–/{}", beats(d)),
        };
        Some(ClassCell {
            class: class.to_string(),
            value,
            ease: (ease != Ease::Linear).then(|| format!("{ease:?}")),
        })
    }

    let mut out = Vec::new();
    // The dimmer is the one class that genuinely needs two numbers: an
    // intensity coming up and one going down are different gestures, and
    // a cue that says so is saying the most useful thing in the row.
    let dim = match (t.dimmer_in, t.dimmer_out) {
        (a, b) if a == b => cell(
            "dim",
            a,
            t.delay.intensity,
            t.ease.get(AttrClass::Intensity),
        ),
        (a, b) => Some(ClassCell {
            class: "dim".to_string(),
            value: format!(
                "{}↑ {}↓",
                a.map_or_else(|| "–".into(), beats),
                b.map_or_else(|| "–".into(), beats)
            ),
            ease: None,
        }),
    };
    out.extend(dim);
    out.extend(cell(
        "col",
        t.color,
        t.delay.colour,
        t.ease.get(AttrClass::Colour),
    ));
    out.extend(cell(
        "pos",
        t.position,
        t.delay.position,
        t.ease.get(AttrClass::Position),
    ));
    out.extend(cell(
        "beam",
        t.beam,
        t.delay.beam,
        t.ease.get(AttrClass::Beam),
    ));
    out
}

fn target_of(s: &Selection) -> String {
    match s {
        Selection::Group(g) => g.clone(),
        Selection::Role(r) => format!("role {r}"),
        Selection::Tag(t) => format!("#{t}"),
        Selection::Model(m) => format!("model {m}"),
        Selection::Chans(c) => match c.as_slice() {
            [only] => format!("chan {only}"),
            _ => format!("{} chans", c.len()),
        },
        Selection::Union(parts) => parts.iter().map(target_of).collect::<Vec<_>>().join(" + "),
        Selection::Intersect(parts) => parts.iter().map(target_of).collect::<Vec<_>>().join(" ∩ "),
        other => {
            // Except / Where / Order — shapes with no short spelling.
            // Name the kind rather than inventing one.
            let kind = format!("{other:?}");
            kind.split(['(', ' ', '{'])
                .next()
                .unwrap_or("selection")
                .to_lowercase()
        }
    }
}

/// The author's own spelling of a position — "CH 1 +4" — because that
/// is what the note in their hand says.
fn position_of(p: &ignition_core::Position) -> String {
    use ignition_core::Position;
    match p {
        Position::Absolute(b) => bars(b),
        Position::LastBar {
            section, ordinal, ..
        } => format!("{} end", section_name(section, *ordinal)),
        Position::Relative {
            section,
            ordinal,
            bars: n,
            beat,
        } => {
            let head = section_name(section, *ordinal);
            match (*n, *beat) {
                (0, b) if (b - 1.0).abs() < 1e-6 => head,
                (n, b) if (b - 1.0).abs() < 1e-6 => format!("{head} +{n}"),
                (n, b) => format!("{head} +{n}.{}", crate::numeric::tenths(b * 10.0)),
            }
        }
    }
}

fn section_name(section: &str, ordinal: usize) -> String {
    if ordinal == 0 {
        section.to_string()
    } else {
        format!("{section} {}", ordinal.saturating_add(1))
    }
}

fn bars(b: &Bars) -> String {
    if (b.beat - 1.0).abs() < 1e-6 {
        format!("{}", b.bar)
    } else {
        format!(
            "{}.{}",
            b.bar,
            crate::numeric::tenths((b.beat - 1.0) * 10.0)
        )
    }
}

/// A number with no more precision than it needs: 5, not 5.0; 5.5 stays
/// 5.5.
fn number(n: f32) -> String {
    if (n - n.round()).abs() < 1e-4 {
        format!("{}", crate::numeric::rounded_i64(n))
    } else {
        format!("{n}")
    }
}

fn secs(s: f32) -> String {
    if s == 0.0 {
        "—".to_string()
    } else {
        format!("{s:.1}s")
    }
}

fn beats(b: f32) -> String {
    if (b - b.round()).abs() < 1e-4 {
        format!("{}", crate::numeric::rounded_i64(b))
    } else {
        format!("{b:.1}")
    }
}

// ── The panel ────────────────────────────────────────────────────────

/// The cue stack. Underneath the busking layer, not beside it: a cue
/// fills in whatever the operator is not currently holding.
///
/// `preset` is where the host starts it; the operator can switch from
/// the header, which is the whole of "the same panel with different
/// chrome".
// r[impl studio.cuelist.one-panel]
// r[impl studio.program.cue-editing] - the same panel Live draws
#[component]
pub fn CueList(cues: Vec<Row>, #[props(default)] preset: Preset) -> Element {
    // What the player is actually standing on, not what was last
    // clicked. A click still fires the cue; it just no longer decides
    // what the list *shows*.
    let current = use_current_cue();
    let ringing = use_ringing_hit();
    // r[impl studio.cuelist.live-state]
    let progress = use_cue_progress();
    let mut view = use_signal(|| preset);
    // Which cues are open into their parts. Per-cue rather than a single
    // "expand everything" switch, because the reason to open one is to
    // read it.
    let mut open = use_signal(std::collections::HashSet::<usize>::new);
    // Columns switched off on top of the preset, so Program can be
    // pared back without leaving it.
    // r[impl studio.cuelist.program-columns]
    let mut hidden = use_signal(std::collections::HashSet::<&'static str>::new);

    let program = view() == Preset::Program;
    let shows = move |col: &'static str| !hidden().contains(col);

    rsx! {
        aside { class: if program { "cues program" } else { "cues" },
            header {
                span { "Cue List" }
                // r[impl studio.cuelist.one-panel] - a view setting, not a second panel
                button {
                    class: if program { "preset on" } else { "preset" },
                    title: "Program shows everything a cue carries; Live shows what running it needs",
                    onclick: move |_| {
                        let next = if view() == Preset::Live { Preset::Program } else { Preset::Live };
                        view.set(next);
                    },
                    if program { "PROGRAM" } else { "LIVE" }
                }
                button { class: "go", onclick: move |_| send(Command::Go), "GO" }
                // GO on the look list — the list beneath the song's that
                // the operator steps by hand.
                button { class: "go look", onclick: move |_| send(Command::LookGo), "LOOK" }
            }
            if program {
                div { class: "colbar",
                    for col in ["note", "timing", "flags", "mib", "cmd", "cover"] {
                        button {
                            key: "{col}",
                            class: if shows(col) { "col on" } else { "col" },
                            onclick: move |_| {
                                let mut h = hidden();
                                if !h.remove(col) { h.insert(col); }
                                hidden.set(h);
                            },
                            "{col}"
                        }
                    }
                }
            }
            ol {
                for (i, row) in cues.iter().enumerate() {
                    match row {
                        Row::Cue(cue) => {
                            let cue = cue.clone();
                            let index = cue.index;
                            let standing = current() == Some(index);
                            let expanded = open().contains(&index);
                            let (fade, next, next_in) = progress();
                            // The fade bar is drawn only while the cue is
                            // still arriving; a landed cue is just lit.
                            let arriving = (standing && fade < 1.0).then_some(fade);
                            let counting = (next == Some(index)).then_some(next_in).flatten();
                            let tint = cue
                                .appearance
                                .as_ref()
                                .map(|a| format!("background: {}", a.color))
                                .unwrap_or_default();
                            rsx! {
                                li {
                                    key: "c{i}",
                                    class: if standing { "cue on" } else { "cue" },
                                    onclick: move |_| send(Command::Cue(index)),
                                    // r[impl studio.cuelist.live-state]
                                    if let Some(f) = arriving {
                                        div { class: "fading",
                                            div { class: "fadefill", style: "width: {f * 100.0}%" }
                                        }
                                    }
                                    div { class: "cue-main",
                                        // r[impl cues.appearance]
                                        span { class: "tint", style: "{tint}",
                                            if let Some(label) = cue.appearance.as_ref().and_then(|a| a.label.clone()) {
                                                "{label}"
                                            }
                                        }
                                        // r[impl cues.number]
                                        span { class: "num", "{cue.number}" }
                                        // r[impl cues.trig]
                                        span { class: "trig", title: "{cue.trig.label()}", "{cue.trig.glyph()}" }
                                        span { class: "name", "{cue.name}" }
                                        if let Some(at) = cue.position.clone() {
                                            span { class: "at", "{at}" }
                                        }
                                        span { class: "fade", "{cue.fade}" }
                                        // A follow takes itself; this is the
                                        // only warning an operator gets.
                                        // r[impl studio.cuelist.live-state]
                                        if let Some(secs) = counting {
                                            span { class: "countdown", title: "takes itself", "{secs:.1}" }
                                        }
                                        // r[impl studio.cuelist.condensed-timing]
                                        if program && shows("timing") {
                                            span { class: "times",
                                                for cell in cue.timing.iter() {
                                                    span { key: "{cell.class}", class: "tcell",
                                                        span { class: "tclass", "{cell.class}" }
                                                        "{cell.value}"
                                                        if let Some(ease) = cell.ease.clone() {
                                                            span { class: "ease", "{ease}" }
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                        if program && shows("flags") {
                                            span { class: "flags",
                                                for (chip, why) in cue.flags.chips() {
                                                    span { key: "{chip}", class: "chip", title: "{why}", "{chip}" }
                                                }
                                            }
                                        }
                                        if program && shows("mib") && let Some(mib) = cue.mib.clone() {
                                            span { class: "mib", title: "pre-positions while dark", "◐{mib}" }
                                        }
                                        if program && shows("cover") && let Some(s) = cue.status {
                                            span { class: "cover",
                                                "{s.covers}"
                                                if s.direct > 0 {
                                                    // r[impl cues.recipes-not-values] - a direct value on a cue is worth seeing
                                                    span { class: "direct", title: "hand-placed values do not follow a rig change", "+{s.direct}" }
                                                }
                                            }
                                        }
                                        // r[impl studio.cuelist.status]
                                        span {
                                            class: cue.status.map_or_else(
                                                || "dot unknown".to_string(),
                                                |s| format!("dot {}", s.state.css()),
                                            ),
                                            title: cue.status.map_or_else(
                                                || "not cooked against a rig".to_string(),
                                                |s| s.state.label().to_string(),
                                            ),
                                        }
                                        if program && !cue.parts.is_empty() {
                                            button {
                                                class: "expand",
                                                onclick: move |e: Event<MouseData>| {
                                                    e.stop_propagation();
                                                    let mut o = open();
                                                    if !o.remove(&index) { o.insert(index); }
                                                    open.set(o);
                                                },
                                                if expanded { "▾" } else { "▸" }
                                            }
                                        }
                                    }
                                    // r[impl cues.note]
                                    if program && shows("note") && !cue.note.is_empty() {
                                        div { class: "cue-note", "{cue.note}" }
                                    }
                                    if program && shows("cmd") && !cue.commands.is_empty() {
                                        div { class: "cue-cmds",
                                            for (n, c) in cue.commands.iter().enumerate() {
                                                span { key: "{n}", class: "cmd", "{c}" }
                                            }
                                        }
                                    }
                                    // r[impl studio.cuelist.expand]
                                    if expanded {
                                        ol { class: "parts",
                                            for part in cue.parts.iter() {
                                                li {
                                                    key: "p{part.number}",
                                                    class: match (part.is_override, part.disabled) {
                                                        (_, true) => "part off",
                                                        (true, _) => "part over",
                                                        _ => "part",
                                                    },
                                                    span { class: "pnum", "{part.number + 1}" }
                                                    span { class: "pname", "{part.name}" }
                                                    if !part.target.is_empty() {
                                                        span { class: "ptarget", "{part.target}" }
                                                    }
                                                    span { class: "times",
                                                        for cell in part.timing.iter() {
                                                            span { key: "{cell.class}", class: "tcell",
                                                                span { class: "tclass", "{cell.class}" }
                                                                "{cell.value}"
                                                            }
                                                        }
                                                    }
                                                    if let Some(n) = part.covers {
                                                        span {
                                                            class: if n == 0 { "pcover none" } else { "pcover" },
                                                            "{n}"
                                                        }
                                                    }
                                                    if !part.note.is_empty() {
                                                        span { class: "pnote", "{part.note}" }
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                        Row::Hit { index, name, at } => {
                            let (index, at) = (*index, *at);
                            rsx! {
                                li {
                                    key: "h{i}",
                                    class: if ringing() == Some(index) { "cue hit on" } else { "cue hit" },
                                    onclick: move |_| send(Command::Locate(at)),
                                    div { class: "cue-main",
                                        span { class: "tint" }
                                        span { class: "num", "♪" }
                                        span { class: "name", "{name}" }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ignition_core::cue::{Appearance, CueTiming};
    use ignition_core::recipe::{Recipe, RecipeApply};

    fn list() -> CoreList {
        let mut part = Recipe::new(
            Selection::Group("Pars".to_string()),
            RecipeApply::Dimmer(1.0),
        );
        part.name = Some("pars up".into());
        part.cue_timing = Some(CueTiming {
            position: Some(4.0),
            ..Default::default()
        });
        CoreList {
            cues: vec![
                Cue {
                    name: "Verse".into(),
                    number: Some(1.0),
                    note: "hold for the count-in".into(),
                    appearance: Some(Appearance {
                        color: "#2d6cdf".into(),
                        label: Some("V".into()),
                    }),
                    fade_secs: 2.0,
                    block: true,
                    recipes: vec![RecipeRef::Inline(part)],
                    ..Default::default()
                },
                Cue {
                    name: "· lift".into(),
                    number: Some(1.5),
                    ..Default::default()
                },
            ],
            ..Default::default()
        }
    }

    /// Everything a Program column needs comes off the row, and nothing
    /// needs the engine to draw it.
    // r[verify studio.cuelist.program-columns]
    // r[verify cues.number]
    // r[verify cues.note]
    #[test]
    fn a_row_carries_what_both_presets_draw() {
        let rows = rows(&list(), None);
        let Row::Cue(first) = &rows[0] else {
            panic!("the first row is not a cue")
        };
        assert_eq!(first.number, "1");
        assert_eq!(first.name, "Verse");
        assert_eq!(first.note, "hold for the count-in");
        assert_eq!(first.fade, "2.0s");
        assert_eq!(first.trig, TrigKind::Go);
        assert!(first.flags.block);
        // Nothing cooked it, so the status is blank rather than green.
        // r[verify studio.cuelist.status]
        assert!(first.status.is_none());
        let Row::Cue(second) = &rows[1] else {
            panic!("the second row is not a cue")
        };
        assert_eq!(second.number, "1.5", "a fractional number stays fractional");
    }

    /// The preset is a view setting, not a second panel: the rows are
    /// built once and are the same either way, so switching mid-song
    /// cannot reorder or drop anything.
    // r[verify studio.cuelist.one-panel]
    // r[verify studio.cuelist.live-columns]
    #[test]
    fn the_preset_changes_columns_and_never_the_rows() {
        assert_eq!(Preset::default(), Preset::Live, "a list opens on Live");
        let built = rows(&list(), None);
        // Nothing about the rows depends on the preset — there is one
        // builder and it takes no preset at all.
        assert_eq!(built, rows(&list(), None));
        let Row::Cue(first) = &built[0] else {
            panic!("the first row is not a cue")
        };
        // Everything the Live preset draws is on the row.
        assert!(first.appearance.is_some());
        assert!(!first.number.is_empty());
        assert!(!first.name.is_empty());
        assert!(!first.fade.is_empty());
        assert_eq!(first.index, 0, "and the index a GO takes");
    }

    /// A cue's recipes are its parts, and the part carries its own name
    /// and its own arrival.
    // r[verify studio.cuelist.expand]
    // r[verify cues.parts-are-recipes]
    #[test]
    fn a_cue_expands_into_its_parts() {
        let rows = rows(&list(), None);
        let Row::Cue(first) = &rows[0] else {
            panic!("the first row is not a cue")
        };
        assert_eq!(first.parts.len(), 1);
        let part = &first.parts[0];
        assert_eq!(part.name, "pars up");
        assert_eq!(part.target, "Pars");
        assert_eq!(part.timing.len(), 1, "one class says something");
        assert_eq!(part.timing[0].class, "pos");
        assert_eq!(part.timing[0].value, "4");
    }

    /// Fade and delay share one cell, split by a slash. Eight timing
    /// columns is how a sheet stops fitting on a screen.
    // r[verify studio.cuelist.condensed-timing]
    #[test]
    fn timing_is_condensed_to_one_cell_a_class() {
        let cells = class_cells(&CueTiming {
            color: Some(2.0),
            position: Some(8.0),
            delay: ignition_core::cue::ClassDelays {
                position: 1.0,
                ..Default::default()
            },
            ..Default::default()
        });
        assert_eq!(cells.len(), 2, "only the classes that say something");
        assert_eq!(cells[0].value, "2");
        assert_eq!(cells[1].value, "8/1", "fade over delay");
    }
}
