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
use ignition_core::SongMap;
use ignition_core::profile::{AuthoredLooks, Look};
use ignition_core::{Class, RecipeRef, Show};
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
        held.clone_from(name);
    }
}

/// Handle one of the additive commands. Returns `false` for a command
/// this module does not own, so the caller can log it.
///
/// `desk` is the venue's desk show, if it has one
/// (`desk::path_for_venue`); `show_file` is the song show the cue list
/// was loaded from, where `StoreCue` writes; `song` is the song map the
/// running list's positions were resolved against, so the re-read list
/// lands on the same bars.
pub fn apply(
    cmd: &Command,
    playback: &mut Playback,
    desk: Option<&Path>,
    show_file: Option<&Path>,
    song: Option<&SongMap>,
) -> bool {
    note(cmd);
    match cmd {
        // r[impl studio.views.whole-profile] - any effect or bundle, by name, on the macro layer
        Command::Take { name, level } => {
            apply_take(playback, name, *level);
        }
        Command::Untake(name) => apply_untake(playback, name),
        // r[impl studio.live.desk-scenes] - a desk scene is a cue on the Show-class playback
        Command::DeskScene(index) => apply_desk_scene(playback, desk, *index),
        Command::DeskRelease => apply_desk_release(playback),
        // r[impl profile.protected-roles] - toggled from the surface
        Command::Protect { role, on } => apply_protect(playback, role, *on),
        // r[impl studio.program.cue-editing] - store what the hand holds into the file's cue
        Command::StoreCue { index, mode } => {
            apply_store_cue(playback, show_file, song, *index, *mode);
        }
        // r[impl studio.program.cue-editing] - store what the hand holds as a look
        // r[impl profile.looks.authored] - into the overlay, never the baked file
        Command::StoreLook { name, kind } => apply_store_look(playback, name, *kind),
        _ => return false,
    }
    true
}

/// `Command::Take`: any effect or bundle, by name, on the macro layer.
fn apply_take(playback: &mut Playback, name: &str, level: f32) {
    let recipes: Vec<(String, ignition_core::Recipe)> =
        if let Some(bundle) = playback.bundles.get(name) {
            bundle
                .recipes
                .iter()
                .filter_map(|n| playback.library.get(n).map(|r| (n.clone(), r.clone())))
                .collect()
        } else if let Some(recipe) = playback.library.get(name) {
            vec![(name.to_string(), recipe.clone())]
        } else {
            tracing::warn!(name, "studio: no such effect or bundle");
            Vec::new()
        };
    for (n, recipe) in recipes {
        playback.programmer.take_effect(&n, recipe, level);
    }
}

/// `Command::Untake`: releases every recipe a `Take` of this name added.
fn apply_untake(playback: &mut Playback, name: &str) {
    let members: Vec<String> = playback
        .bundles
        .get(name)
        .map_or_else(|| vec![name.to_string()], |b| b.recipes.clone());
    for n in members {
        playback.programmer.release_effect(&n);
    }
}

/// `Command::DeskScene`: a desk scene is a cue on the Show-class
/// playback, built from the venue's desk show on first use.
fn apply_desk_scene(playback: &mut Playback, desk: Option<&Path>, index: usize) {
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
            return;
        };
        match crate::desk::load_list(path) {
            Ok(list) => {
                playbacks.push(Class::Show, ignition_core::CuePlayer::from_list(&list));
            }
            Err(error) => {
                tracing::warn!(%error, "studio: desk show does not load");
                return;
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
        entry.player.jump_to_end_of(index, &show);
    }
}

/// `Command::DeskRelease`: folds the desk playback out of the stack
/// without forgetting it.
fn apply_desk_release(playback: &mut Playback) {
    for entry in playback
        .playbacks
        .entries
        .iter_mut()
        .filter(|e| e.class == Class::Show)
    {
        entry.enabled = false;
    }
}

/// `Command::Protect`: protects, or stops protecting, a role.
fn apply_protect(playback: &mut Playback, role: &str, on: bool) {
    let protected = &mut playback.programmer.protected;
    protected.retain(|p| !p.eq_ignore_ascii_case(role));
    if on {
        protected.push(role.to_string());
    }
}

/// `Command::StoreCue`: writes the programmer's captured values into
/// the show file, then replaces the running player's list with what was
/// just written so the stage shows the stored cue now rather than next
/// launch.
fn apply_store_cue(
    playback: &mut Playback,
    show_file: Option<&Path>,
    song: Option<&SongMap>,
    index: usize,
    mode: ignition_core::cue::StoreMode,
) {
    let Some(path) = show_file else {
        tracing::warn!("studio: no show file to store into");
        return;
    };
    let (name, mut list) = match store_cue(path, index, mode, playback.programmer.captured()) {
        Ok(stored) => stored,
        Err(error) => {
            tracing::warn!(%error, "studio: store failed");
            return;
        }
    };
    tracing::info!(index, name, "studio: stored cue");
    // Positions are resolved against the song the way `Playback::load`
    // resolved them, so nothing moves.
    if let Some(song) = song {
        for problem in list.resolve_positions(song) {
            tracing::warn!("{problem}");
        }
    }
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
        tempo: song.map(|s| &s.tempo),
        ..Show::new(groups, rig)
    };
    if let Some(player) = playbacks.of_class(Class::Song) {
        player.replace_list(&list, &show);
    } else {
        tracing::warn!("studio: no song playback to refresh");
    }
}

/// `Command::StoreLook`: what the hand holds — a latched look plus
/// every apply since CLEAR — as a look of this name and kind, in the
/// profile's authored overlay beside the baked file.
fn apply_store_look(playback: &mut Playback, name: &str, kind: ignition_core::profile::LookKind) {
    let name = name.trim();
    if name.is_empty() {
        tracing::warn!("studio: a look needs a name");
        return;
    }
    let recipes: Vec<RecipeRef> = playback
        .programmer
        .look_recipes()
        .into_iter()
        .map(RecipeRef::Inline)
        .collect();
    if recipes.is_empty() {
        tracing::warn!(name, "studio: nothing in the hand to store as a look");
        return;
    }
    let look = Look {
        kind,
        about: String::new(),
        recipes,
    };
    let path = ignition_live_ui::library::looks_path();
    match AuthoredLooks::store(&path, name, look.clone()) {
        Ok(_) => {
            tracing::info!(name, path = %path.display(), "studio: stored look");
            // The engine's own copy, so a cue may open on it and a key
            // may hold it; the panels re-read the file.
            playback.looks.insert(name.to_string(), look);
            ignition_live_ui::library::reload_authored_looks();
        }
        Err(error) => tracing::warn!(%error, "studio: store look failed"),
    }
}

/// Replace cue `index`'s direct values in the show file with `values`,
/// keeping its name, timing and recipes. Past the end, a new cue is
/// appended. Returns the cue's name and the list as written.
fn store_cue(
    path: &Path,
    index: usize,
    mode: ignition_core::cue::StoreMode,
    values: Vec<ignition_core::CueValue>,
) -> anyhow::Result<(String, ignition_core::CueList)> {
    let raw = std::fs::read_to_string(path)?;
    let mut list: ignition_core::CueList = serde_json::from_str(&raw)?;
    let mut cue = list
        .cues
        .get(index)
        .cloned()
        .unwrap_or_else(|| ignition_core::Cue {
            name: format!("Cue {}", list.cues.len().saturating_add(1)),
            ..Default::default()
        });
    cue.values = values;
    let name = cue.name.clone();
    list.store(index, cue, mode);
    std::fs::write(path, serde_json::to_string_pretty(&list)?)?;
    Ok((name, list))
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
    next.protected.clone_from(&playback.programmer.protected);
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
    #[expect(
        clippy::float_cmp,
        reason = "fade_secs round-trips through JSON unmodified; exact equality is the \
                  property under test, not a coincidence to relax"
    )]
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
        let (name, written) =
            store_cue(&path, 0, ignition_core::cue::StoreMode::Track, values).unwrap();
        assert_eq!(name, "Verse");
        assert_eq!(
            written.cues[0].values.len(),
            1,
            "the list as written comes back"
        );
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

    /// `StoreCue` through `apply`: the file is written *and* the running
    /// Song player shows the stored cue at once, in place, and a
    /// STORE → NEW appends to the running list too.
    /// r[verify studio.program.cue-editing]
    #[test]
    #[expect(
        clippy::float_cmp,
        reason = "dimmer levels here are set, then read back the same tick with no fade in \
                  between; exact equality is the property under test — the store landed on \
                  the stage without a GO — not a coincidence to relax"
    )]
    fn a_stored_cue_is_on_the_stage_without_a_go() {
        use ignition_core::{Attribute, CueList, CuePlayer, RecipeApply, Selection};
        let dir = std::env::temp_dir().join(format!("ig-store-live-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("show.json");
        let list = CueList {
            cues: vec![
                ignition_core::Cue {
                    name: "Verse".into(),
                    values: vec![ignition_core::CueValue {
                        chan: 1,
                        attr: Attribute::Dimmer,
                        value: 0.2,
                    }],
                    ..Default::default()
                },
                ignition_core::Cue {
                    name: "Chorus".into(),
                    ..Default::default()
                },
            ],
            ..Default::default()
        };
        std::fs::write(&path, serde_json::to_string(&list).unwrap()).unwrap();

        let mut playback = Playback::default();
        playback
            .playbacks
            .push(Class::Song, CuePlayer::from_list(&list));
        let bare = Show::new(&[], &ignition_core::selection::EMPTY_RIG);
        let player = playback.playbacks.of_class(Class::Song).unwrap();
        player.go(&bare);
        player.tick(2.0);
        assert_eq!(player.output(&bare)[&(1, Attribute::Dimmer)], 0.2);

        // The hand: channel 1 to 0.9.
        playback.programmer.select(Selection::Chans(vec![1]));
        playback.programmer.apply(RecipeApply::Dimmer(0.9), &bare);
        let handled = apply(
            &Command::StoreCue {
                index: 0,
                mode: ignition_core::cue::StoreMode::Track,
            },
            &mut playback,
            None,
            Some(&path),
            None,
        );
        assert!(handled);
        let player = playback.playbacks.of_class(Class::Song).unwrap();
        assert_eq!(player.current_index(), Some(0), "still on the cue");
        assert!((player.clock() - 2.0).abs() < 1e-6, "the clock ran on");
        assert_eq!(
            player.output(&bare)[&(1, Attribute::Dimmer)],
            0.9,
            "the stage shows the stored value, no GO"
        );
        let back: CueList = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(back.cues[0].values[0].value, 0.9, "and the file has it");

        // STORE → NEW: past the end appends to the file and the list.
        apply(
            &Command::StoreCue {
                index: 2,
                mode: ignition_core::cue::StoreMode::Track,
            },
            &mut playback,
            None,
            Some(&path),
            None,
        );
        let player = playback.playbacks.of_class(Class::Song).unwrap();
        assert_eq!(player.cues().len(), 3);
        assert_eq!(player.current_index(), Some(0));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// `StoreLook` writes the hand into the authored overlay beside the
    /// profile file — never the baked file — and the engine's looks
    /// carry it at once.
    /// r[verify profile.looks.authored]
    #[test]
    fn a_stored_look_lands_in_the_overlay_and_the_engine() {
        use ignition_core::{RecipeApply, Selection};
        let dir = std::env::temp_dir().join(format!("ig-store-look-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let profile_path = dir.join("test.ig-profile");
        // No profile file: the overlay still goes beside where it would be.
        // SAFETY: this test is the only reader of the variable in this
        // binary's tests, and it sets it before any profile is loaded.
        unsafe { std::env::set_var("IGNITION_PROFILE", &profile_path) };

        let mut playback = Playback::default();
        let bare = Show::new(&[], &ignition_core::selection::EMPTY_RIG);
        // Nothing in the hand: nothing stored.
        apply(
            &Command::StoreLook {
                name: "empty".into(),
                kind: ignition_core::profile::LookKind::Bed,
            },
            &mut playback,
            None,
            None,
            None,
        );
        assert!(!AuthoredLooks::path_for(&profile_path).exists());

        playback.programmer.select(Selection::Role("Wash".into()));
        playback.programmer.apply(RecipeApply::Dimmer(0.5), &bare);
        apply(
            &Command::StoreLook {
                name: " verse two ".into(),
                kind: ignition_core::profile::LookKind::Full,
            },
            &mut playback,
            None,
            None,
            None,
        );
        let overlay = AuthoredLooks::load(AuthoredLooks::path_for(&profile_path)).unwrap();
        let look = &overlay.looks["verse two"];
        assert_eq!(look.kind, ignition_core::profile::LookKind::Full);
        assert_eq!(look.recipes.len(), 1);
        assert_eq!(
            look.recipes[0].inline().unwrap().target,
            Selection::Role("Wash".into()),
            "written against the role, not the channels"
        );
        assert_eq!(playback.looks["verse two"], *look, "the engine has it now");
        assert!(
            !profile_path.exists(),
            "the profile file itself is untouched"
        );
        assert!(
            ignition_live_ui::faders::profile()
                .looks
                .contains_key("verse two"),
            "the bank's profile re-read the overlay"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
