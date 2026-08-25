//! Cue, recipe and effect playback, driven from the Bevy schedule.
//!
//! The players themselves live in `ignition_core` and know nothing about
//! rendering — they produce attribute values, `show.rs` writes those into
//! the DMX universes, and `spawn.rs` reads the universes back out exactly
//! as it would for a real console on the network. That indirection is
//! deliberate: playing a show internally and receiving one over sACN go
//! through the same path, so the visualizer cannot accidentally look
//! right for a local show and wrong for a real one.

use crate::show::{apply_cue_output, tick_and_apply, tick_and_apply_effects};
use crate::spawn::{DmxRes, VenueRes};
use crate::venue::Venue;
use bevy::prelude::*;
use ignition_core::{CueList, CuePlayer, EffectList, EffectPlayer, RecipeCueList};
use std::path::Path;

/// A loaded show, if the CLI was given one.
#[derive(Resource, Default)]
pub struct Playback {
    /// Cues stepped by GO — space, same convention as a real console.
    pub cues: Option<CuePlayer>,
    /// Effects run continuously from the moment they load; an
    /// `EffectRecipe` is a function of time, not a stepped state.
    pub effects: Option<EffectPlayer>,
}

impl Playback {
    /// Loads whichever show files the CLI was given.
    ///
    /// `cuelist` is the flat, already-compiled form; `recipes` is the
    /// authoring form (a cue is a list of group + dimmer/colour/focus
    /// recipes) compiled here against the venue's own real groups and
    /// fixture placements. They are alternatives, not layers.
    ///
    /// `jump_to_cue` puts the player at the *end* of that cue's fade
    /// rather than starting it — how you capture a programmed look
    /// headlessly without a keyboard to press GO with. Likewise
    /// `effect_time` freezes a running effect at a chosen moment.
    pub fn load(
        venue: &Venue,
        cuelist: Option<&Path>,
        recipes: Option<&Path>,
        effects: Option<&Path>,
        jump_to_cue: Option<usize>,
        effect_time: Option<f32>,
    ) -> anyhow::Result<Self> {
        anyhow::ensure!(
            cuelist.is_none() || recipes.is_none(),
            "pass either --cuelist or --recipes, not both"
        );

        let cues = if let Some(path) = cuelist {
            let list: CueList = read_json(path, "a cue list")?;
            println!("loaded cue list {:?}: {} cues", list.name, list.cues.len());
            Some(CuePlayer::new(list.cues))
        } else if let Some(path) = recipes {
            let list: RecipeCueList = read_json(path, "a recipe cue list")?;
            let groups = venue.groups();
            let cues = ignition_core::expand_cue_list(&list.cues, &groups, &|chan| {
                venue.placement_of(chan)
            });
            println!(
                "loaded recipe cue list {:?}: {} cues, compiled against {} real venue groups",
                list.name,
                cues.len(),
                groups.len()
            );
            Some(CuePlayer::new(cues))
        } else {
            None
        };

        let effects = match effects {
            Some(path) => {
                let list: EffectList = read_json(path, "an effect list")?;
                println!(
                    "loaded effect list {:?}: {} effects",
                    list.name,
                    list.effects.len()
                );
                Some(EffectPlayer::new(list.effects))
            }
            None => None,
        };

        let mut playback = Self { cues, effects };
        if let Some(player) = playback.cues.as_mut() {
            let index = jump_to_cue.unwrap_or(0);
            player.jump_to_end_of(index);
            println!("cue -> {} {:?}", index + 1, player.current_name());
        }
        if let (Some(player), Some(t)) = (playback.effects.as_mut(), effect_time) {
            player.tick(t);
        }
        Ok(playback)
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
    if let Some(player) = playback.cues.as_mut() {
        tick_and_apply(&dmx.0, &venue.0, player, dt);
    }
    if let Some(player) = playback.effects.as_mut() {
        // Layered after cues, so a running effect modifies whatever the
        // current cue set rather than being overwritten by it.
        tick_and_apply_effects(&dmx.0, &venue.0, player, dt);
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
    if let Some(player) = playback.cues.as_mut() {
        player.go();
        info!("cue -> {:?}", player.current_name());
        apply_cue_output(&dmx.0, &venue.0, &player.output());
    }
}
