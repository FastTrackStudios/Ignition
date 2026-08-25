//! Cue, recipe and effect playback, driven from the Bevy schedule.
//!
//! The players themselves live in `ignition_core` and know nothing about
//! rendering — they produce attribute values, `show.rs` writes those into
//! the DMX universes, and `spawn.rs` reads the universes back out exactly
//! as it would for a real console on the network. That indirection is
//! deliberate: playing a show internally and receiving one over sACN go
//! through the same path, so the visualizer cannot accidentally look
//! right for a local show and wrong for a real one.

use crate::show::{apply_cue_output, tick_and_apply};
use crate::spawn::{DmxRes, VenueRes};
use crate::venue::Venue;
use bevy::prelude::*;
use ignition_core::{CueList, CuePlayer, Group, Rig, Show, SpeedMasters};
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
    /// Named tempo sources every phaser can slave to. Empty until
    /// something drives them — a tap-tempo key, or the session tempo map
    /// from the FastTrackStudio side.
    pub speeds: SpeedMasters,
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
    ) -> anyhow::Result<Self> {
        anyhow::ensure!(
            cuelist.is_none() || recipes.is_none(),
            "pass either --cuelist or --recipes, not both"
        );

        let groups = venue.groups();
        let rig = venue.rig();
        let speeds = SpeedMasters::new();
        let show = Show {
            groups: &groups,
            palettes: &venue.palettes,
            rig: &rig,
            speeds: &speeds,
        };

        let cues: Option<CuePlayer> = match cuelist.or(recipes) {
            Some(path) => {
                let list: CueList = read_json(path, "a cue list")?;
                for problem in ignition_core::unresolved(&list.cues, &show) {
                    eprintln!("warning: {problem}");
                }
                println!(
                    "loaded show {:?}: {} cues against {} groups, {} colour / {} focus palettes",
                    list.name,
                    list.cues.len(),
                    groups.len(),
                    venue.palettes.colors.len(),
                    venue.palettes.focus.len()
                );
                Some(CuePlayer::new(list.cues))
            }
            None => None,
        };

        let mut cues = cues;
        if let Some(player) = cues.as_mut() {
            let index = jump_to_cue.unwrap_or(0);
            player.jump_to_end_of(index, &show);
            println!("cue -> {index} {:?}", player.current_name());
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
            speeds: SpeedMasters::new(),
        })
    }
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
    } = &mut *playback;
    let venue = &venue.0;
    let show = Show {
        groups,
        palettes: &venue.palettes,
        rig,
        speeds,
    };
    if let Some(player) = cues.as_mut() {
        tick_and_apply(&dmx.0, venue, player, dt, &show);
    }
}

/// Space is GO, the way it is on every console an operator has used.
pub fn go_on_space(
    keys: Res<ButtonInput<KeyCode>>,
    venue: Res<VenueRes>,
    dmx: Res<DmxRes>,
    mut playback: ResMut<Playback>,
) {
    if !keys.just_pressed(KeyCode::Space) {
        return;
    }
    let Playback {
        cues,
        groups,
        rig,
        speeds,
    } = &mut *playback;
    if let Some(player) = cues.as_mut() {
        let venue = &venue.0;
        let show = Show {
            groups,
            palettes: &venue.palettes,
            rig,
            speeds,
        };
        player.go(&show);
        info!("cue -> {:?}", player.current_name());
        apply_cue_output(&dmx.0, venue, &player.output(&show));
    }
}
