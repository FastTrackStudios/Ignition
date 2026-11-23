//! Tricks, effects made from parameters, and previews.
//!
//! **Tricks** are Ignition's word for what grandMA3 calls MAtricks: how a
//! selection is cut into *units* (blocks, groups, wings, shuffle, shift,
//! reverse, mirror, per axis of the selection grid) and how a value
//! spreads across those units (phase, speed, delay and fade, from–to). A
//! **trick preset** is a named set of those settings in the library
//! (`ignition_show::library::TrickPreset`), usable on any selection.
//!
//! An **effect** is a recipe of two or more steps (grandMA3: a phaser).
//! `IGNITION_EFFECT_MAKE` writes one from what moves, its shape, its
//! rate in beats and its spread; `IGNITION_EFFECT_PREVIEW` runs any
//! effect on any selection of an environment and answers what each
//! fixture does over time, saving nothing.

use std::collections::BTreeMap;

use ignition_show::library::{Library, Pools, TrickPreset};
use ignition_show::tricks::{Grid, GridAxes, apply_all_grid};
use ignition_show::{Attribute, Palettes, Recipe, RecipeApply, Selection, Show, Step, Waveform};
use serde_json::{Value, json};

use super::env::room;
use super::library::{layers, profile_path};
use super::song::{preset_of, roles_of, selection};
use super::{Ctx, Res, arr, count_f, f32_of, flag, list, num, obj, opt_text, text, whole};

/// The units a selection is cut into, where each sits on the grid, and
/// what each gets of a spread. `step` names one unit (wrapping), for
/// stepping through them.
fn units(cx: &Ctx, args: &Value) -> Res {
    let r = room(cx, args)?;
    let l = layers(cx, args)?;
    let chans = r.resolve(args.get("target").ok_or("`target` is required")?)?;
    let preset = match args.get("tricks") {
        Some(t) => preset_of(&l, t)?,
        None => TrickPreset::default(),
    };
    let grid = match &preset.grid {
        Some(rows) => Grid::for_selection(&chans, Some(rows), &r.rig, GridAxes::default()),
        None => Grid::from_rig_in_order(&chans, &r.rig, GridAxes::default()),
    };
    let gu = apply_all_grid(&preset.tricks, &grid);
    let mut probe = Recipe::default();
    preset.apply(&mut probe);
    let n = gu.units.0.len();
    let lerp = |v: Option<ignition_show::library::FromTo>, i: usize| {
        v.map(|v| if n > 1 { (f64::from(v.to) - f64::from(v.from)).mul_add(count_f(i) / count_f(n - 1), f64::from(v.from)) } else { f64::from(v.from) })
    };
    let rows: Vec<Value> = gu
        .units
        .0
        .iter()
        .zip(&gu.pos)
        .enumerate()
        .map(|(i, (unit, pos))| {
            json!({
                "unit": i, "chans": unit, "x": pos.x, "y": pos.y, "z": pos.z,
                "phase_deg": f64::from(probe.timing.spread_fraction_3d(pos)).mul_add(360.0, f64::from(probe.timing.phase_offset_deg)),
                "rate": probe.timing.rate_at(pos.x, pos.count.first().copied().unwrap_or(1)),
                "delay_beats": lerp(preset.delay, i), "fade_beats": lerp(preset.fade, i),
            })
        })
        .collect();
    let stepped = args.get("step").and_then(Value::as_i64).and_then(|s| {
        let len = i64::try_from(n).ok().filter(|l| *l > 0)?;
        rows.get(usize::try_from(s.rem_euclid(len)).ok()?).cloned()
    });
    Ok(json!({
        "fixtures": chans.len(), "grid": { "x": grid.size.first(), "y": grid.size.get(1), "z": grid.size.get(2) },
        "tricks": preset.tricks, "units": rows, "count": n, "step": stepped,
    }))
}

/// Puts a trick preset on parts of a cue: every effect part of `target`
/// (or the one named `effect`) takes its tricks, phase, speed and
/// direction; its delay and fade become the cue's fan.
fn apply(cx: &mut Ctx, args: &Value) -> Res {
    let l = layers(cx, args)?;
    let (path, mut doc) = l.song.clone().ok_or("`song` is required")?;
    let roles = roles_of(cx, &l);
    let preset = preset_of(&l, args.get("tricks").ok_or("`tricks` is required: a preset's name, or its settings")?)?;
    let target = match args.get("target") {
        Some(t) => Some(selection(t, &roles)?),
        None => None,
    };
    let effect = opt_text(args, "effect");
    let cue_name = args.get("cue").ok_or("`cue` is required")?;
    let cues = arr(&mut doc, "cues")?;
    let cue = cues.iter_mut().find(|c| c.get("name") == Some(cue_name)).ok_or_else(|| format!("no cue {cue_name}"))?;
    let mut changed = Vec::new();
    for part in arr(cue, "recipes")? {
        let on_target = target.as_ref().is_none_or(|t| part.get("target") == Some(t));
        let named = effect.is_none_or(|e| opt_text(part, "effect") == Some(e) || opt_text(part, "name") == Some(e));
        if !(on_target && named) || part.get("look").is_some() || part.get("bundle").is_some() {
            continue;
        }
        let label = opt_text(part, "effect").or_else(|| opt_text(part, "name")).unwrap_or("part").to_owned();
        let mut recipe: Recipe = if let Some(name) = opt_text(part, "effect") {
            // A reference becomes the effect written in place: a preset
            // spreads phase and speed, which a reference cannot carry.
            let (_, entry) = l.find("effects", name).ok_or_else(|| format!("no effect {name:?}"))?;
            let mut r: Recipe = serde_json::from_value(entry.get("recipe").cloned().unwrap_or_default()).map_err(|e| e.to_string())?;
            if let Some(t) = part.get("target") {
                r.target = serde_json::from_value(t.clone()).map_err(|e| e.to_string())?;
            }
            if let Some(bars) = num(part, "bars") {
                r.timing.measure = f32_of(bars * 4.0);
            }
            r.name = Some(name.to_owned());
            r
        } else if list(part, "steps").len() > 1 || flag(args, "statics") {
            serde_json::from_value(part.clone()).map_err(|e| e.to_string())?
        } else {
            continue;
        };
        preset.apply(&mut recipe);
        *part = serde_json::to_value(&recipe).map_err(|e| e.to_string())?;
        changed.push(json!({ "part": label, "tricks": recipe.tricks, "phase_spread_deg": recipe.timing.phase_spread_deg, "speed": [recipe.timing.speed_from, recipe.timing.speed_to] }));
    }
    if let Some(fan) = preset.cue_fan() {
        obj(cue)?.insert("fan".into(), serde_json::to_value(fan).map_err(|e| e.to_string())?);
        changed.push(json!({ "cue_fan": true }));
    }
    if changed.is_empty() {
        return Err("the cue has no effect part there to put tricks on".to_owned());
    }
    cx.write_json(&path, &doc)?;
    Ok(json!({ "cue": cue_name, "changed": changed }))
}

fn attribute(what: &str) -> Result<Attribute, String> {
    Ok(match what.to_lowercase().as_str() {
        "dimmer" | "intensity" => Attribute::Dimmer,
        "pan" => Attribute::Pan,
        "tilt" => Attribute::Tilt,
        "zoom" => Attribute::Zoom,
        "focus" => Attribute::Focus,
        "iris" => Attribute::Iris,
        "strobe" => Attribute::Strobe,
        other => return Err(format!("`what` {other:?}: dimmer, pan, tilt, circle, zoom, focus, iris, strobe or color")),
    })
}

fn shape(name: &str) -> Result<Waveform, String> {
    Ok(match name.to_lowercase().as_str() {
        "sine" | "wave" => Waveform::Sine,
        "triangle" => Waveform::Triangle,
        "square" | "chase" | "pulse" => Waveform::Square,
        "ramp" | "ramp_up" | "saw" => Waveform::RampUp,
        "ramp_down" => Waveform::RampDown,
        other => return Err(format!("`shape` {other:?}: sine, triangle, square, ramp or ramp_down")),
    })
}

/// A recipe from parameters. `what` moves (dimmer, pan, tilt, circle,
/// zoom, iris, focus, strobe, color), in a `shape`, once every `beats`,
/// `depth` either side of where it is (`base` makes it absolute), spread
/// by `tricks` (a preset or its settings) or `spread_deg`.
fn build(l: &super::library::Layers, roles: &std::collections::BTreeSet<String>, args: &Value) -> Result<Recipe, String> {
    let target: Selection = serde_json::from_value(selection(args.get("target").ok_or("`target` is required")?, roles)?).map_err(|e| format!("target: {e}"))?;
    let what = opt_text(args, "what").unwrap_or("dimmer");
    let wave = shape(opt_text(args, "shape").unwrap_or("sine"))?;
    let depth = f32_of(num(args, "depth").unwrap_or(0.5));
    let base = num(args, "base").map(f32_of);
    let rel = base.is_none();
    let b = base.unwrap_or(0.0);
    let steps: Vec<Step> = match what.to_lowercase().as_str() {
        "color" | "colour" => {
            let colors = list(args, "colors");
            if colors.len() < 2 {
                return Err("a colour effect needs `colors`: two or more colour or slot names".to_owned());
            }
            let soft = !matches!(wave, Waveform::Square);
            colors
                .iter()
                .map(|c| {
                    let apply: RecipeApply = serde_json::from_value(json!({ "Color": c })).map_err(|e| format!("colors: {e}"))?;
                    let mut s = Step::new(vec![apply]);
                    s.transition = if soft { 1.0 } else { 0.0 };
                    Ok(s)
                })
                .collect::<Result<_, String>>()?
        }
        "circle" => {
            // Pan and tilt a quarter cycle apart is a circle.
            let pan = Waveform::Sine.steps(&Attribute::Pan, b, depth, rel);
            let tilt = ignition_show::step::transform::shift_phase(Waveform::Sine.steps(&Attribute::Tilt, b, depth, rel), 1);
            pan.into_iter()
                .zip(tilt)
                .map(|(mut p, t)| {
                    p.apply.extend(t.apply);
                    p
                })
                .collect()
        }
        other => wave.steps(&attribute(other)?, b, depth, rel),
    };
    let mut recipe = Recipe { target, steps, ..Recipe::default() };
    recipe.timing.speed = serde_json::from_value(args.get("speed").cloned().unwrap_or_else(|| json!({ "Master": "Song" }))).map_err(|e| format!("speed: {e}"))?;
    recipe.timing.measure = f32_of(num(args, "beats").unwrap_or(4.0));
    recipe.timing.phase_spread_deg = f32_of(num(args, "spread_deg").unwrap_or(0.0));
    if let Some(d) = args.get("direction") {
        recipe.timing.direction = serde_json::from_value(d.clone()).map_err(|_| "`direction`: Forward, Reverse, Bounce, Build or Negative".to_owned())?;
    }
    recipe.timing.once = flag(args, "once");
    if let Some(t) = args.get("tricks") {
        preset_of(l, t)?.apply(&mut recipe);
    }
    Ok(recipe)
}

fn family_of(what: &str) -> &'static str {
    match what.to_lowercase().as_str() {
        "pan" | "tilt" | "circle" => "movement",
        "color" | "colour" => "colour",
        "zoom" | "iris" | "focus" | "strobe" => "beam",
        _ => "intensity",
    }
}

fn effect_make(cx: &mut Ctx, args: &Value) -> Res {
    let mut l = layers(cx, args)?;
    let roles = roles_of(cx, &l);
    let name = text(args, "name")?;
    let recipe = build(&l, &roles, args)?;
    let entry = json!({
        "recipe": recipe,
        "family": opt_text(args, "family").unwrap_or_else(|| family_of(opt_text(args, "what").unwrap_or("dimmer"))),
        "about": opt_text(args, "about").unwrap_or(""),
        "tags": args.get("tags").cloned().unwrap_or_else(|| json!([])),
        "folder": opt_text(args, "folder").unwrap_or(""),
    });
    if flag(args, "dry_run") {
        return Ok(json!({ "dry_run": true, "name": name, "entry": entry }));
    }
    let scope = opt_text(args, "scope").unwrap_or("set");
    let lib = match scope {
        "song" => {
            let (_, doc) = l.song.as_mut().ok_or("scope `song` needs `song`")?;
            obj(doc)?.entry("library").or_insert_with(|| json!({}))
        }
        _ => &mut l.set,
    };
    let pool = obj(lib)?.entry("effects").or_insert_with(|| json!({}));
    if obj(pool)?.contains_key(name) && !flag(args, "replace") {
        return Err(format!("the library already has an effect {name:?} (`replace: true` writes over it)"));
    }
    obj(pool)?.insert(name.to_owned(), entry.clone());
    l.save(cx)?;
    Ok(json!({ "name": name, "layer": scope, "entry": entry }))
}

/// Runs an effect (by `effect` name, a `recipe`, or the parameters
/// `IGNITION_EFFECT_MAKE` takes) on a target of an environment for
/// `beats` at `bpm`, and answers what each fixture does at each sample.
/// Nothing is saved.
fn preview(cx: &Ctx, args: &Value) -> Res {
    let r = room(cx, args)?;
    let l = layers(cx, args)?;
    let roles = roles_of(cx, &l);
    let mut recipe: Recipe = if let Some(name) = opt_text(args, "effect") {
        let (_, entry) = l.find("effects", name).ok_or_else(|| format!("no effect {name:?}"))?;
        serde_json::from_value(entry.get("recipe").cloned().unwrap_or_default()).map_err(|e| e.to_string())?
    } else if let Some(given) = args.get("recipe") {
        serde_json::from_value(given.clone()).map_err(|e| format!("recipe: {e}"))?
    } else {
        build(&l, &roles, args)?
    };
    if let Some(t) = args.get("target") {
        let bound: std::collections::BTreeSet<String> = r.binding.groups.keys().cloned().collect();
        recipe.target = serde_json::from_value(selection(t, &bound)?).map_err(|e| format!("target: {e}"))?;
    }
    if opt_text(args, "effect").is_some() || args.get("recipe").is_some() {
        if let Some(t) = args.get("tricks") {
            preset_of(&l, t)?.apply(&mut recipe);
        }
    }
    // What the show would play from: the room's palettes, the profile's
    // colours and effects, the library laid over them.
    let mut palettes: Palettes = cx.json(&r.dir.join("palettes.json")).ok().and_then(|v| serde_json::from_value(v).ok()).unwrap_or_default();
    let profile: ignition_show::Profile = cx.json(&profile_path(cx, &l.show, &l.show_doc)).ok().and_then(|v| serde_json::from_value(v).ok()).unwrap_or_default();
    for c in &profile.colors {
        if palettes.color(&c.name).is_none() {
            palettes.colors.push(c.clone());
        }
    }
    let mut effects = profile.effects.clone();
    let mut looks = profile.looks.clone();
    let mut tricks = profile.tricks.clone();
    let typed = |v: &Value| serde_json::from_value::<Library>(v.clone()).unwrap_or_default();
    let song = l.song.as_ref().and_then(|(_, d)| d.get("library")).map(typed).unwrap_or_default();
    song.over(&typed(&l.set)).lay_over(&mut Pools { palettes: &mut palettes, effects: &mut effects, looks: &mut looks, tricks: &mut tricks });
    let bpm = f32_of(num(args, "bpm").unwrap_or(120.0));
    let speeds: ignition_show::SpeedMasters = [("Song".to_owned(), bpm), ("Tap".to_owned(), bpm)].into_iter().collect();
    let bundles = BTreeMap::new();
    let show = Show {
        palettes: &palettes, speeds: &speeds, roles: &r.binding, library: &effects,
        bundles: &bundles, looks: &looks, named_tricks: &tricks,
        ..Show::new(&r.groups, &r.rig)
    };
    let chans = ignition_show::selection::resolve_with(&recipe.target, &r.groups, &r.rig, &r.binding);
    if chans.is_empty() {
        return Err("the target names no fixtures in this environment".to_owned());
    }
    let beats = num(args, "beats_shown").unwrap_or_else(|| f64::from(recipe.timing.measure));
    let samples = args.get("samples").and_then(whole).unwrap_or(8).clamp(1, 64);
    let mut frames = Vec::new();
    for i in 0..samples {
        let beat = beats * f64::from(i) / f64::from(samples);
        let secs = f32_of(beat * 60.0 / f64::from(bpm));
        let mut per: BTreeMap<u32, BTreeMap<String, f64>> = BTreeMap::new();
        for e in ignition_show::expand_recipe(&recipe, &show, secs) {
            let attr = match serde_json::to_value(&e.value.attr) {
                Ok(Value::String(s)) => s,
                Ok(other) => other.to_string(),
                Err(_) => continue,
            };
            per.entry(e.value.chan).or_default().insert(attr, (f64::from(e.value.value) * 1000.0).round() / 1000.0);
        }
        // One row per attribute, in the selection's order: read across
        // for the picture, down for the motion.
        let mut attrs: BTreeMap<String, Vec<Value>> = BTreeMap::new();
        for chan in &chans {
            for (attr, v) in per.get(chan).into_iter().flatten() {
                attrs.entry(attr.clone()).or_default().push(json!(v));
            }
        }
        frames.push(json!({ "beat": (beat * 100.0).round() / 100.0, "values": attrs }));
    }
    Ok(json!({
        "saved": false, "chans": chans, "bpm": bpm, "beats": beats,
        "relative": recipe.steps.iter().any(|s| s.apply.iter().any(|a| matches!(a, RecipeApply::Delta(_)))),
        "tricks": recipe.tricks, "phase_spread_deg": recipe.timing.phase_spread_deg,
        "frames": frames, "recipe": recipe,
    }))
}

pub(crate) fn run(cx: &mut Ctx, id: &str, args: &Value) -> Option<Res> {
    Some(match id {
        "IGNITION_TRICKS_UNITS" => units(cx, args),
        "IGNITION_TRICKS_APPLY" => apply(cx, args),
        "IGNITION_EFFECT_MAKE" => effect_make(cx, args),
        "IGNITION_EFFECT_PREVIEW" => preview(cx, args),
        _ => return None,
    })
}
