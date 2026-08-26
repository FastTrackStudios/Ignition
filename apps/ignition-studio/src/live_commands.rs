//! Engine-side handling for the Live / Program / Library commands.
//!
//! `viz_widget::drain` matches the commands it always has; the additive
//! ones at the end of `Command` — `Take`, `Untake`, `DeskScene`,
//! `DeskRelease`, `Protect`, `StoreCue` — land here so the widget's own
//! file need not grow with every panel. The integrator wires two calls:
//!
//! * in `drain`, for a command the match does not handle, call
//!   [`apply`] with the whole `Playback` (outside the destructuring
//!   block, since `apply` borrows the struct entire);
//! * in `publish`, after the playhead is built from the programmer,
//!   call [`publish`] so the Live surface's badges — held look, effects
//!   playing, desk scene, protected roles, selection — come from the
//!   engine rather than from anything the surface remembers
//!   (`r[studio.one-truth]`).

// Nothing is dead here; it is mounted when `main.rs` hosts `live::Views`
// (and its stylesheet, `live::LIVE_CSS`). Until the integrator wires
// that, the crate root does not reach these items. Remove once mounted.

// r[impl studio.one-truth] - the surface's extras travel back on the playhead

use crate::command::{Command, Playhead};
use ignition_core::{Class, Show};
use ignition_viz::playback::Playback;
use std::path::Path;
use std::sync::Mutex;

/// The look a `Command::Look` last latched. The programmer holds the
/// look's *recipes*, not its name, so the name is kept here at the
/// engine side of the channel — still one truth, because every surface
/// reads it back from the playhead rather than from its own click.
static HELD_LOOK: Mutex<Option<String>> = Mutex::new(None);

/// Call for every command the drain sees, before matching: records the
/// held look's name so `publish` can report it.
pub fn note(cmd: &Command) {
    if let Command::Look(name) = cmd
        && let Ok(mut held) = HELD_LOOK.lock()
    {
        *held = name.clone();
    }
}

/// Handle one of the additive commands. Returns `false` for a command
/// this module does not own, so the caller can log it.
///
/// `desk` is the venue's desk show, if it has one
/// (`desk::path_for_venue`); `show_file` is the song show the cue list
/// was loaded from, where `StoreCue` writes.
pub fn apply(
    cmd: &Command,
    playback: &mut Playback,
    desk: Option<&Path>,
    show_file: Option<&Path>,
) -> bool {
    note(cmd);
    match cmd {
        // r[impl studio.views.whole-profile] - any effect or bundle, by name, on the macro layer
        Command::Take { name, level } => {
            let recipes: Vec<(String, ignition_core::Recipe)> =
                if let Some(bundle) = playback.bundles.get(name) {
                    bundle
                        .recipes
                        .iter()
                        .filter_map(|n| playback.library.get(n).map(|r| (n.clone(), r.clone())))
                        .collect()
                } else if let Some(recipe) = playback.library.get(name) {
                    vec![(name.clone(), recipe.clone())]
                } else {
                    tracing::warn!(name, "studio: no such effect or bundle");
                    Vec::new()
                };
            for (n, recipe) in recipes {
                playback.programmer.take_effect(&n, recipe, *level);
            }
            true
        }
        Command::Untake(name) => {
            let members: Vec<String> = playback
                .bundles
                .get(name)
                .map(|b| b.recipes.clone())
                .unwrap_or_else(|| vec![name.clone()]);
            for n in members {
                playback.programmer.release_effect(&n);
            }
            true
        }
        // r[impl studio.live.desk-scenes] - a desk scene is a cue on the Show-class playback
        Command::DeskScene(index) => {
            let Playback {
                playbacks,
                groups,
                rig,
                palettes,
                speeds,
                profile,
                library,
                bundles,
                looks,
                named_tricks,
                ..
            } = playback;
            if !playbacks.entries.iter().any(|e| e.class == Class::Show) {
                let Some(path) = desk else {
                    tracing::warn!("studio: this venue has no desk show");
                    return true;
                };
                match crate::desk::load_list(path) {
                    Ok(list) => {
                        playbacks.push(Class::Show, ignition_core::CuePlayer::from_list(&list));
                    }
                    Err(error) => {
                        tracing::warn!(%error, "studio: desk show does not load");
                        return true;
                    }
                }
            }
            let show = Show {
                groups,
                palettes,
                rig,
                speeds,
                roles: profile,
                library,
                bundles,
                looks,
                named_tricks,
                ..Show::new(groups, rig)
            };
            if let Some(entry) = playbacks
                .entries
                .iter_mut()
                .find(|e| e.class == Class::Show)
            {
                entry.enabled = true;
                entry.player.jump_to_end_of(*index, &show);
            }
            true
        }
        Command::DeskRelease => {
            for entry in playback
                .playbacks
                .entries
                .iter_mut()
                .filter(|e| e.class == Class::Show)
            {
                entry.enabled = false;
            }
            true
        }
        // r[impl profile.protected-roles] - toggled from the surface
        Command::Protect { role, on } => {
            let protected = &mut playback.programmer.protected;
            protected.retain(|p| !p.eq_ignore_ascii_case(role));
            if *on {
                protected.push(role.clone());
            }
            true
        }
        // r[impl studio.program.cue-editing] - store what the hand holds into the file's cue
        Command::StoreCue { index, mode } => {
            let Some(path) = show_file else {
                tracing::warn!("studio: no show file to store into");
                return true;
            };
            match store_cue(path, *index, *mode, playback.programmer.captured()) {
                Ok(name) => tracing::info!(index, name, "studio: stored cue"),
                Err(error) => tracing::warn!(%error, "studio: store failed"),
            }
            true
        }
        _ => false,
    }
}

/// Replace cue `index`'s direct values in the show file with `values`,
/// keeping its name, timing and recipes. Past the end, a new cue is
/// appended. Returns the cue's name.
fn store_cue(
    path: &Path,
    index: usize,
    mode: ignition_core::cue::StoreMode,
    values: Vec<ignition_core::CueValue>,
) -> anyhow::Result<String> {
    let raw = std::fs::read_to_string(path)?;
    let mut list: ignition_core::CueList = serde_json::from_str(&raw)?;
    let mut cue = list
        .cues
        .get(index)
        .cloned()
        .unwrap_or_else(|| ignition_core::Cue {
            name: format!("Cue {}", list.cues.len() + 1),
            ..Default::default()
        });
    cue.values = values;
    let name = cue.name.clone();
    list.store(index, cue, mode);
    std::fs::write(path, serde_json::to_string_pretty(&list)?)?;
    Ok(name)
}

/// Fill the playhead's Live extras from the engine.
pub fn publish(next: &mut Playhead, playback: &Playback) {
    next.held_look = if playback.programmer.is_holding() {
        HELD_LOOK.lock().ok().and_then(|h| h.clone())
    } else {
        None
    };
    next.effects_playing = playback
        .programmer
        .effects_playing()
        .into_iter()
        .map(str::to_string)
        .collect();
    next.desk_scene = playback
        .playbacks
        .entries
        .iter()
        .find(|e| e.class == Class::Show && e.enabled)
        .and_then(|e| e.player.current_index());
    next.protected = playback.programmer.protected.clone();
    next.selection = playback.programmer.selection.as_ref().map(describe);
    next.captured = playback.programmer.captured().len();
}

/// A selection in words, for the programmer's header.
pub fn describe(selection: &ignition_core::Selection) -> String {
    use ignition_core::Selection as S;
    match selection {
        S::Group(g) => g.clone(),
        S::Role(r) => format!("role {r}"),
        S::Chans(c) => format!("{} fixtures", c.len()),
        S::Tag(t) => format!("tag {t}"),
        S::Model(m) => format!("model {m}"),
        S::Union(items) => items.iter().map(describe).collect::<Vec<_>>().join(" + "),
        S::Intersect(items) => items.iter().map(describe).collect::<Vec<_>>().join(" & "),
        S::Except { of, minus } => format!("{} − {}", describe(of), describe(minus)),
        other => format!("{other:?}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selections_read_as_words() {
        use ignition_core::Selection as S;
        assert_eq!(describe(&S::Group("Washers".into())), "Washers");
        assert_eq!(
            describe(&S::Union(vec![S::Role("Key".into()), S::Chans(vec![1, 2])])),
            "role Key + 2 fixtures"
        );
    }

    /// A store writes the file and keeps everything but the values.
    /// r[verify studio.program.cue-editing]
    #[test]
    fn store_cue_replaces_values_and_keeps_the_rest() {
        let dir = std::env::temp_dir().join(format!("ig-store-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("show.json");
        let list = ignition_core::CueList {
            name: "t".into(),
            cues: vec![ignition_core::Cue {
                name: "Verse".into(),
                fade_secs: 2.0,
                ..Default::default()
            }],
            ..Default::default()
        };
        std::fs::write(&path, serde_json::to_string(&list).unwrap()).unwrap();
        let values = vec![ignition_core::CueValue {
            chan: 3,
            attr: ignition_core::Attribute::Dimmer,
            value: 0.5,
        }];
        let name = store_cue(&path, 0, ignition_core::cue::StoreMode::Track, values).unwrap();
        assert_eq!(name, "Verse");
        let back: ignition_core::CueList =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(back.cues[0].fade_secs, 2.0);
        assert_eq!(back.cues[0].values.len(), 1);
        // Past the end appends.
        store_cue(&path, 5, ignition_core::cue::StoreMode::Track, Vec::new()).unwrap();
        let back: ignition_core::CueList =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(back.cues.len(), 2);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
