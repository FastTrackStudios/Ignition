//! Cue, recipe and effect playback, driven from the Bevy schedule.
//!
//! The players themselves live in `ignition_core` and know nothing about
//! rendering — they produce attribute values, `show.rs` writes those into
//! the DMX universes, and `spawn.rs` reads the universes back out exactly
//! as it would for a real console on the network. That indirection is
//! deliberate: playing a show internally and receiving one over sACN go
//! through the same path, so the visualizer cannot accidentally look
//! right for a local show and wrong for a real one.

use crate::show::apply_cue_output;
use crate::spawn::{DmxRes, VenueRes};
use crate::venue::Venue;
use bevy::prelude::*;
use ignition_core::{CueList, CuePlayer, Group, Palettes, Programmer, Rig, Show, SpeedMasters};
use std::path::Path;

/// A loaded show, if the CLI was given one.
#[derive(Resource, Default)]
pub struct Playback {
    /// Cues stepped by GO — space, same convention as a real console.
    pub cues: Option<CuePlayer>,
    /// The venue's groups, resolved once. Recipes are resolved every
    /// frame now (see `docs/domain/cue-building-architecture.md`), and
    /// re-parsing 127 channel-range strings per frame to do it would be
    /// a silly way to spend the budget that decision bought.
    pub groups: Vec<Group>,
    /// The patched rig, likewise resolved once — `Selection::Tag` and the
    /// spatial filters resolve against this every frame.
    pub rig: Rig,
    /// The venue's palettes, cached here for the same reason `groups`
    /// and `rig` are: everything that resolves a recipe needs them, and
    /// a host driving this from outside the ECS should not have to go
    /// find the venue to build a `Show`.
    pub palettes: Palettes,
    /// How this venue fills the profile's roles.
    ///
    /// Cached alongside the rest for the same reason: every `Show` built
    /// anywhere needs it, and a recipe targeting `Role("Wash")` resolves
    /// to nothing without it — silently, since an unbound role is
    /// legitimately empty. Carrying it here is what stops that being a
    /// thing each construction site has to remember.
    pub profile: ignition_core::profile::VenueProfile,
    /// A show clock held still, for a still.
    ///
    /// `--time T` means "render the show as it is at T", and it only
    /// means that if the clock stops there. It used to *add* T once at
    /// load and then let the clock run, which made a snapshot of any
    /// transient impossible to take: the twenty settle frames Bevy needs
    /// to warm its pipelines are longer than a bump, so every still of a
    /// one-shot showed the moment *after* it. Fewer frames only trades
    /// that for a picture of a scene that has not finished loading.
    ///
    /// Diagnosing a figure that "does nothing" is exactly the case, and
    /// the tool could not answer it.
    pub frozen_at: Option<f32>,
    /// Named tempo sources every phaser can slave to. Empty until
    /// something drives them — a tap-tempo key, or the session tempo map
    /// from the FastTrackStudio side.
    pub speeds: SpeedMasters,
    /// Recent tap-tempo taps, in show-clock seconds. Drives the `Tap`
    /// speed master — the cheapest possible demonstration that a phaser
    /// really is slaved to something outside itself, and the same seam
    /// the session tempo map will arrive through.
    taps: Vec<f32>,
    /// The live layer. Busking is the primary way this desk is played;
    /// the cue player underneath fills in whatever the operator is not
    /// currently holding.
    pub programmer: Programmer,
    /// Show clock for the programmer's own faders, which run whether or
    /// not a cue stack is loaded.
    clock: f32,
}

impl Playback {
    /// Loads whichever show files the CLI was given.
    ///
    /// `cuelist` and `recipes` are the same format now and either flag
    /// accepts either file: a `Cue` carries direct values *and* recipes
    /// as the two layers of one cascade, so the two show shapes that
    /// used to need separate types are one type with different fields
    /// filled in. Both spellings are kept because both appear in scripts
    /// and in this repo's own data.
    ///
    /// `jump_to_cue` puts the player at the *end* of that cue's fade
    /// rather than starting it — how you capture a programmed look
    /// headlessly without a keyboard to press GO with. Likewise
    /// `effect_time` freezes a running effect at a chosen moment.
    pub fn load(
        venue: &Venue,
        cuelist: Option<&Path>,
        recipes: Option<&Path>,
        jump_to_cue: Option<usize>,
        effect_time: Option<f32>,
        bar: Option<u32>,
        song_bpm: Option<f32>,
    ) -> anyhow::Result<Self> {
        anyhow::ensure!(
            cuelist.is_none() || recipes.is_none(),
            "pass either --cuelist or --recipes, not both"
        );

        let groups = venue.groups();
        let rig = venue.rig();
        let mut speeds = default_speeds();
        // `--bpm` replaces the seeded placeholder; the transport does the
        // same at run time.
        if let Some(bpm) = song_bpm {
            speeds.insert("Song".to_string(), bpm);
        }
        let show = Show {
            groups: &groups,
            palettes: &venue.palettes,
            rig: &rig,
            speeds: &speeds,
            roles: &venue.profile,
        };

        let cues: Option<CuePlayer> = match cuelist.or(recipes) {
            Some(path) => {
                let list: CueList = read_json(path, "a cue list")?;
                for problem in ignition_core::unresolved(&list.cues, &show) {
                    tracing::warn!("{problem}");
                }

                // The cook report — what every cue resolves to before
                // one has fired. A recipe that selects nothing is not an
                // error and the show still runs, so this is the only
                // thing that makes it visible.
                //
                // Only the cues that resolve to *nothing* are printed.
                // The full list was a hundred and five lines at every
                // launch, which buried the one line that mattered — a
                // report nobody reads is not a report, and the whole
                // point of a cooked-status marker is that a problem
                // stands out. `RUST_LOG=ignition_viz=debug` still shows
                // every cue.
                let cooked = ignition_core::cook_list(&list.cues, &show, 0.0);
                let dead: Vec<&ignition_core::CueCook> = cooked
                    .iter()
                    .filter(|c| {
                        c.recipes
                            .iter()
                            .any(|r| matches!(r, ignition_core::Cook::Empty))
                    })
                    .collect();
                for (i, cook) in cooked.iter().enumerate() {
                    tracing::debug!(
                        cue = i,
                        name = %cook.name,
                        recipes = cook.recipes.len(),
                        direct = cook.direct,
                        "cooked"
                    );
                }
                if dead.is_empty() {
                    tracing::info!(
                        show = %list.name,
                        cues = list.cues.len(),
                        groups = groups.len(),
                        colors = venue.palettes.colors.len(),
                        focus = venue.palettes.focus.len(),
                        "loaded, every cue resolves"
                    );
                } else {
                    tracing::warn!(
                        show = %list.name,
                        cues = list.cues.len(),
                        empty = dead.len(),
                        "loaded, but some cues resolve to nothing"
                    );
                    for cook in dead {
                        tracing::warn!(name = %cook.name, "selects nothing");
                    }
                }
                Some(CuePlayer::new(list.cues))
            }
            None => None,
        };

        let mut cues = cues;
        if let Some(player) = cues.as_mut() {
            // `--bar` addresses the show the way it is written; `--cue`
            // addresses the list. Both end in the same place, which is
            // the point of positioned cues.
            match bar {
                Some(bar) => {
                    player.seek(ignition_core::Bars::bar(bar), &show);
                    println!("bar {bar} -> {:?}", player.current_name());
                }
                None => {
                    let index = jump_to_cue.unwrap_or(0);
                    player.jump_to_end_of(index, &show);
                    println!("cue -> {index} {:?}", player.current_name());
                }
            }
        }
        // `show` borrows `groups`, which the struct below takes
        // ownership of; nothing reads it past here.
        // Advancing the clock without advancing the fade is how a
        // running phaser gets snapshotted at a chosen moment.
        if let (Some(player), Some(t)) = (cues.as_mut(), effect_time) {
            player.advance_clock(t);
        }
        Ok(Self {
            cues,
            groups,
            rig,
            palettes: venue.palettes.clone(),
            profile: venue.profile.clone(),
            frozen_at: effect_time,
            speeds: default_speeds(),
            taps: Vec::new(),
            programmer: Programmer::new(),
            clock: 0.0,
        })
    }
}

/// The speed masters a show can assume exist.
///
/// `Tap` is seeded at a plausible tempo rather than left empty so a
/// tap-driven show runs the moment it loads — an operator should not
/// have to tap four times to find out whether their chase works. The
/// `T` key retunes it; `unresolved()` still reports any *other* master
/// a show names, which is the case that really is a wiring mistake.
fn default_speeds() -> SpeedMasters {
    SpeedMasters::from([
        ("Tap".to_string(), 120.0),
        // `Song` is seeded for the same reason `Tap` is, and the cost of
        // not doing it was louder. Every accent in a generated show runs
        // off the song's tempo, so with no entry here the load-time
        // check reported "no speed master" once per cue — eighty lines
        // of warning about a master that the transport supplies a moment
        // later, drowning the report that a *real* wiring mistake would
        // appear in. A show opened without a transport now also runs its
        // song-slaved effects at a plausible tempo instead of freezing
        // them.
        ("Song".to_string(), 120.0),
    ])
}

fn read_json<T: serde::de::DeserializeOwned>(path: &Path, what: &str) -> anyhow::Result<T> {
    let raw = std::fs::read_to_string(path)
        .map_err(|e| anyhow::anyhow!("reading {}: {e}", path.display()))?;
    serde_json::from_str(&raw)
        .map_err(|e| anyhow::anyhow!("parsing {} as {what}: {e}", path.display()))
}

/// Advances whatever is loaded and writes the result into the universes.
pub fn tick_playback(
    time: Res<Time>,
    venue: Res<VenueRes>,
    dmx: Res<DmxRes>,
    mut playback: ResMut<Playback>,
) {
    let dt = time.delta_secs();
    // Split the borrow: resolving needs the groups while the player is
    // held mutably.
    let Playback {
        cues,
        groups,
        rig,
        speeds,
        palettes,
        profile,
        programmer,
        clock,
        frozen_at,
        ..
    } = &mut *playback;
    // Held still for a still, so `--time` names a moment rather than an
    // offset the settle frames then walk away from.
    match *frozen_at {
        Some(t) => *clock = t,
        None => *clock += dt,
    }
    let venue = &venue.0;
    let show = Show {
        groups,
        palettes,
        rig,
        speeds,
            roles: profile,
    };
    // The cue stack renders first and the programmer folds on top, which
    // is the whole point of the layer order: busking overrides playback,
    // never the other way round.
    let mut out = match cues.as_mut() {
        Some(player) => {
            match *frozen_at {
                // `set_clock` rather than `tick`, because ticking also
                // advances fades — and a frozen still wants the cue
                // fully arrived, not caught mid-crossfade.
                Some(t) => player.set_clock(t),
                None => player.tick(dt),
            }
            player.output(&show)
        }
        None => Default::default(),
    };
    programmer.apply_to(&mut out, &show, *clock);
    // After applying, not before: a flash fired this frame must be seen
    // once before it can be retired, or a bump on a slow frame would be
    // dropped without ever having been drawn.
    programmer.retire_flashes(&show, *clock);
    apply_cue_output(&dmx.0, venue, &out);
}

/// How long a gap before a tap-tempo run is treated as a fresh start
/// rather than a very slow beat.
const TAP_TIMEOUT: f32 = 3.0;

/// The operator keys. Space is GO, the way it is on every console.
pub fn operator_keys(
    keys: Res<ButtonInput<KeyCode>>,
    venue: Res<VenueRes>,
    dmx: Res<DmxRes>,
    mut playback: ResMut<Playback>,
) {
    let go = keys.just_pressed(KeyCode::Space);
    let back = keys.just_pressed(KeyCode::Backspace);
    let restart = keys.just_pressed(KeyCode::KeyR);
    let tap = keys.just_pressed(KeyCode::KeyT);
    if !(go || back || restart || tap) {
        return;
    }

    let Playback {
        cues,
        groups,
        rig,
        speeds,
        taps,
        profile,
        ..
    } = &mut *playback;
    let Some(player) = cues.as_mut() else {
        return;
    };

    if tap {
        let now = player.clock();
        if taps.last().is_some_and(|t| now - t > TAP_TIMEOUT) {
            taps.clear();
        }
        taps.push(now);
        // Four taps is one bar of four, which is how people tap.
        if taps.len() > 4 {
            taps.remove(0);
        }
        if let (Some(first), Some(last)) = (taps.first(), taps.last())
            && taps.len() > 1
        {
            let interval = (last - first) / (taps.len() - 1) as f32;
            if interval > 0.05 {
                let bpm = 60.0 / interval;
                info!("tap tempo -> {bpm:.1} BPM");
                speeds.insert("Tap".to_string(), bpm);
            }
        }
    }

    let venue = &venue.0;
    let show = Show {
        groups,
        palettes: &venue.palettes,
        rig,
        speeds,
            roles: profile,
    };

    // Stepping backwards re-runs the show from the top to the target,
    // because tracking means a cue's state is the sum of everything
    // before it — there is no "undo one cue" that is correct.
    if back || restart {
        let target = if restart {
            0
        } else {
            player.current_index().unwrap_or(0).saturating_sub(1)
        };
        let cues_owned = player.cues().to_vec();
        let mut fresh = ignition_core::CuePlayer::new(cues_owned);
        fresh.advance_clock(player.clock());
        fresh.jump_to_end_of(target, &show);
        *player = fresh;
        info!("cue -> {} {:?}", target, player.current_name());
    } else if go {
        player.go(&show);
        info!("cue -> {:?}", player.current_name());
    }

    apply_cue_output(&dmx.0, venue, &player.output(&show));
}

#[cfg(test)]
mod accent_tests {
    use super::*;
    use ignition_core::Attribute;

    /// A figure's bump must visibly lift the fixtures it lands on.
    ///
    /// This is the "fig 0 did nothing" report, pinned. Those cues were
    /// present, resolved to the right fixtures and carried the right
    /// level — and produced no change on stage, because the accent's
    /// delta replaced the section's running chase in a single modulator
    /// slot instead of adding to it.
    #[test]
    fn a_figure_lifts_the_look_it_lands_on() {
        let venue = match Venue::load("../../data/venues/norco") {
            Ok(v) => v,
            // The venue is repo data; if a test runner has no working
            // directory pointing at it, skip rather than fail.
            Err(_) => return,
        };
        let list: CueList = match std::fs::read_to_string("../../data/songs/bye-bye-bye.json")
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
        {
            Some(l) => l,
            None => return,
        };

        let groups = venue.groups();
        let rig = venue.rig();
        let speeds = default_speeds();
        let show = Show {
            groups: &groups,
            palettes: &venue.palettes,
            rig: &rig,
            speeds: &speeds,
            roles: &venue.profile,
        };

        let figure = list
            .cues
            .iter()
            .position(|c| c.name.starts_with("· fig 0 · 1/"))
            .expect("the show has figure 0");

        // The look immediately before it, and then the accent on top.
        let mut player = CuePlayer::new(list.cues.clone());
        player.jump_to_end_of(figure - 1, &show);
        let before = player.output(&show);

        let mut player = CuePlayer::new(list.cues.clone());
        player.jump_to_end_of(figure, &show);
        let after = player.output(&show);

        // *Changed*, not *lifted*. A figure was a bump when this was
        // written and is now a cutout, which mostly makes things darker
        // — asserting "something got brighter" tests the old design and
        // fails on a working new one.
        let changed = after
            .iter()
            .filter(|((chan, attr), value)| {
                *attr == Attribute::Dimmer
                    && (**value - before.get(&(*chan, attr.clone())).copied().unwrap_or(0.0)).abs()
                        > 0.05
            })
            .count();
        assert!(
            changed > 0,
            "figure 0 changed nothing: {} channels before, {} after",
            before.len(),
            after.len()
        );
    }
}

