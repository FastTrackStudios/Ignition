//! Cue, recipe and effect playback, driven from the Bevy schedule.
//!
//! The players themselves live in `ignition_core` and know nothing about
//! rendering — they produce attribute values, `show.rs` writes those into
//! the DMX universes, and `spawn.rs` reads the universes back out exactly
//! as it would for a real console on the network. That indirection is
//! deliberate: playing a show internally and receiving one over sACN go
//! through the same path, so the visualizer cannot accidentally look
//! right for a local show and wrong for a real one.

use crate::show::{OutputFrame, apply_output};
use crate::spawn::{DmxRes, VenueRes};
use crate::venue::Venue;
use bevy::prelude::*;
use ignition_core::{
    Attribute, Bars, ChanId, Class, CueList, CuePlayer, Group, Palettes, Playbacks, Programmer,
    Rig, Show, SongMap, SpeedMasters, TriggerBus,
};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::Path;

/// What the audio input last measured: RMS per band, 0..=1.
///
/// Held on the playback rather than the programmer because it is an
/// *input*, like a transport position — a recipe driven by the kick
/// reads it, the operator does not set it.
// r[impl playback.sound-in] - band levels reach the engine
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct SoundLevels {
    pub low: f32,
    pub mid: f32,
    pub high: f32,
}

/// A loaded show, if the CLI was given one.
#[derive(Resource, Default)]
pub struct Playback {
    /// Every cue player, each in its class. The song list — the show the
    /// CLI or the studio loaded — is one entry; a `Look` list the
    /// operator steps through by hand is another, empty until a look is
    /// pushed onto it. `output` folds them per class.
    // r[impl playback.several-players] - the viz loop runs a Playbacks, not one player
    pub playbacks: Playbacks,
    /// The trigger keys ringing on the last fold, kept so the overlay
    /// can re-ask the same question the fold answered — which class
    /// won a key, and what each would have produced.
    pub ringing: HashSet<(ChanId, Attribute)>,
    /// The audio input's band levels, if a sound-in is running.
    pub sound: SoundLevels,
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
    /// The effects library a cue's `RecipeRef::Named` resolves through,
    /// and the bundles beside it — the shipped library the profile
    /// file is baked from, so a show naming `"circle"` finds it.
    // r[impl effects.library.by-name] - the host supplies the library
    // r[impl effects.bundle]
    pub library: BTreeMap<String, ignition_core::Recipe>,
    pub bundles: BTreeMap<String, ignition_core::profile::Bundle>,
    /// The profile's named Tricks, so a recipe may say `"tricks_ref":
    /// "mirror"` and get the shared definition.
    // r[impl tricks.shared-or-inline] - the shared half, from the profile
    pub named_tricks: BTreeMap<String, Vec<ignition_core::Trick>>,
    /// The profile's looks, so a cue may say `{"look": "verse bed"}`
    /// and get the scene the busk keys hold.
    // r[impl profile.looks] - the host passes the profile's looks beside the library
    pub looks: BTreeMap<String, ignition_core::profile::Look>,
    /// Focus markers moved off their palette value — by a tracker, an
    /// operator — consulted before the palette every frame.
    // r[impl focus.marker-moving] - the host's per-frame override map
    pub focus_overrides: HashMap<String, ignition_proto::Vec3>,
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
    /// The rest value of every attribute of every patched fixture —
    /// what a released attribute falls to. Built from the patch at load.
    // r[impl playback.defaults]
    pub defaults: HashMap<(ChanId, Attribute), f32>,
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
    /// The song's hits. Advanced by whoever moves the transport, folded
    /// above the cue player and below the programmer.
    // r[impl triggers.layer]
    pub triggers: TriggerBus,
    /// The cue the player stood on last frame, so a cue being taken can
    /// release whatever the triggers were holding.
    last_cue: Option<usize>,
}

impl Playback {
    /// The song list — the entry `--cue`, `--bar`, GO and the transport
    /// address. `None` when no show was loaded.
    pub fn song(&self) -> Option<&CuePlayer> {
        self.playbacks
            .entries
            .iter()
            .find(|p| p.class == Class::Song)
            .map(|p| &p.player)
    }

    pub fn song_mut(&mut self) -> Option<&mut CuePlayer> {
        self.playbacks.of_class(Class::Song)
    }

    /// The look list — empty until the operator pushes something onto
    /// it, but always present so GO on it has somewhere to land.
    pub fn look_mut(&mut self) -> Option<&mut CuePlayer> {
        self.playbacks.of_class(Class::Look)
    }

    /// A fresh playback with no show: an empty song list would be a
    /// lie, so only the look list exists.
    fn empty() -> Playbacks {
        let mut playbacks = Playbacks::new();
        playbacks.push(Class::Look, CuePlayer::new(Vec::new()));
        playbacks
    }
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
    // r[impl cues.cooked-status] - every cue is cooked on load and the report logged
    // r[impl cues.dead-cue-warns] - cues that select nothing are warned by name; the load continues
    // r[impl cues.seek] - `--bar` seeks the player to a musical position
    // r[impl effects.masters.song] - `--bpm` seeds the Song master when no transport is present
    // r[impl song.relative-position.resolved-on-load] - positions resolve against the song map here
    pub fn load(
        venue: &Venue,
        cuelist: Option<&Path>,
        recipes: Option<&Path>,
        jump_to_cue: Option<usize>,
        effect_time: Option<f32>,
        bar: Option<u32>,
        song_bpm: Option<f32>,
        song: Option<&SongMap>,
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
        // The library is the built-in one the profile file is baked
        // from (`bake-profile`), so no second file has to be found
        // from a venue that only knows its own directory.
        let library = ignition_core::effects::library();
        let bundles = ignition_core::effects::bundles();
        // The named tricks come from the profile file the venue implements.
        // The venue knows the profile's *name*; the file is looked up under
        // `data/profiles/` (or `IGNITION_PROFILE`), and a missing file is
        // simply no shared tricks — a recipe naming one is then reported.
        // The looks are the baked ones with the desk's authored overlay
        // laid over them (`r[profile.looks.authored]`), so a look stored
        // at the desk is one a cue may open on.
        let (named_tricks, looks) = {
            let path = ignition_core::Profile::default_path(&venue.profile.profile);
            match ignition_core::Profile::load_with_authored(&path) {
                Ok(profile) => (profile.tricks, profile.looks),
                Err(error) => {
                    tracing::debug!(%error, path = %path.display(), "no profile file for named tricks");
                    (BTreeMap::new(), BTreeMap::new())
                }
            }
        };
        let show = Show {
            groups: &groups,
            palettes: &venue.palettes,
            rig: &rig,
            speeds: &speeds,
            roles: &venue.profile,
            library: &library,
            bundles: &bundles,
            looks: &looks,
            named_tricks: &named_tricks,
            tempo: song.map(|s| &s.tempo),
            ..Show::new(&groups, &rig)
        };

        let mut triggers = TriggerBus::default();
        let cues: Option<CuePlayer> = match cuelist.or(recipes) {
            Some(path) => {
                let mut list: CueList = read_json(path, "a cue list")?;
                // A cue written "4 bars into the chorus" lands on this
                // arrangement's chorus, not the one it was authored
                // against. Only possible with a song map; without one
                // the stored bar is what plays.
                if let Some(song) = song {
                    for problem in list.resolve_positions(song) {
                        tracing::warn!("{problem}");
                    }
                }
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
                triggers = TriggerBus::new(list.triggers);
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
                    player.seek(Bars::bar(bar), &show);
                    println!("bar {bar} -> {:?}", player.current_name());
                    // A still of a hit: land just before the bar and
                    // step onto it, so a trigger exactly on the bar
                    // fires at clock zero and `--time` names how far
                    // into its envelope the picture is taken.
                    triggers.locate(Bars::new(bar.saturating_sub(1).max(1), 4.99));
                    triggers.advance(Bars::bar(bar), 0.0);
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
        // Song on top, the look list beneath it; the look list is what
        // an operator GOes through by hand under a running chart.
        let mut playbacks = Self::empty();
        if let Some(player) = cues {
            playbacks.push(Class::Song, player);
        }
        Ok(Self {
            playbacks,
            ringing: HashSet::new(),
            sound: SoundLevels::default(),
            groups,
            rig,
            palettes: venue.palettes.clone(),
            profile: venue.profile.clone(),
            library,
            bundles,
            looks,
            named_tricks,
            focus_overrides: HashMap::new(),
            frozen_at: effect_time,
            defaults: venue.patch().defaults(),
            speeds: default_speeds(),
            taps: Vec::new(),
            programmer: Programmer::new(),
            clock: 0.0,
            triggers,
            last_cue: None,
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
// r[impl effects.masters.registry] - the named registry every effect references
// r[impl effects.masters.tap] - `Tap` seeded so a tap-driven show runs at once
// r[impl effects.masters.song] - `Song` seeded until a transport supplies it
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
// r[impl playback.stack] - cue player first, programmer folded on top
// r[impl playback.busking-over-show]
// r[impl cues.fade-is-wall-time] - `tick(dt)` advances fades in real seconds; a frozen still uses `set_clock` so the show clock moves without the fades
// r[impl triggers.retire] - flashes are retired after the frame that read them
// r[impl effects.bump.is-not-held] - a flash is a one-shot that retires itself
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
        playbacks,
        ringing: last_ringing,
        groups,
        rig,
        speeds,
        palettes,
        profile,
        library,
        bundles,
        looks,
        named_tricks,
        focus_overrides,
        programmer,
        clock,
        frozen_at,
        defaults,
        triggers,
        last_cue,
        sound,
        ..
    } = &mut *playback;
    // Held still for a still, so `--time` names a moment rather than an
    // offset the settle frames then walk away from.
    match *frozen_at {
        Some(t) => *clock = t,
        None => *clock += dt,
    }
    let venue = &venue.0;
    // The Tap master is whatever the operator learned (and halved or
    // doubled), on top of the seeded masters; the smoothed band levels
    // ride along so a sound-page recipe hears the room.
    // r[impl playback.speed-keys] - the learned tempo reaches every recipe
    // r[impl playback.sound-as-value] - the host hands smoothed levels in
    let live_speeds = programmer.speeds_for(speeds);
    let show = Show {
        groups,
        palettes,
        rig,
        speeds: &live_speeds,
        roles: profile,
        library,
        bundles,
        looks,
        named_tricks,
        focus_overrides,
        ..Show::new(groups, rig)
    }
    .with_sound(ignition_core::recipe::SoundLevels {
        low: sound.low,
        mid: sound.mid,
        high: sound.high,
    });
    // The cue stack renders first and the programmer folds on top, which
    // is the whole point of the layer order: busking overrides playback,
    // never the other way round.
    // Triggers first, because the cue player needs to know which keys
    // are ringing: a transient outranks a sustained effect across
    // layers, so the chase under a hit steps aside for its duration.
    //
    // On the *player's* clock, not the wall clock: the bus is advanced
    // by whoever drives the transport, stamping each hit with the song
    // time it fired at, and the player's clock is that same song time
    // when a transport exists. Read against the wall clock a hit fired
    // at 61 s looked minus fifty-six seconds old, never retired, and
    // rang for the rest of the session — a white flash every frame with
    // nothing playing.
    // A new cue releases every held hit: the look has moved on.
    let song = playbacks.of_class(Class::Song);
    let now_cue = song.as_ref().and_then(|p| p.current_index());
    if now_cue != *last_cue {
        triggers.release();
        *last_cue = now_cue;
    }
    let trigger_clock = song.map_or(*clock, |p| p.clock());
    let ringing = triggers.output(&show, trigger_clock);
    match *frozen_at {
        // `set_clock` rather than `tick`, because ticking also advances
        // fades — and a frozen still wants the cue fully arrived, not
        // caught mid-crossfade.
        Some(t) => playbacks.set_clock(t),
        None => playbacks.tick(dt),
    }
    // Every player at once, folded by class — the song over the look
    // list — with the dimmer HTP inside a class and everything else
    // LTP. The ringing keys are kept for the overlay's inspector.
    // r[impl triggers.transient-class] - the ringing keys are the transient set every player resolves around
    let keys: HashSet<(ChanId, Attribute)> = ringing.keys().cloned().collect();
    // The operator's SIZE and RATE apply to the show's effects as well
    // as the faders: one control, every effect. Hits stay unscaled — a
    // hit is a moment, not a swing.
    // r[impl effects.size-scales-the-swing]
    // r[impl effects.masters.uniform]
    let scaled = programmer.show_for(&show);
    let mut out = playbacks.output_with_defaults(&scaled, &keys, defaults);
    let intents = output_intents(playbacks, &scaled);
    *last_ringing = keys;
    // Triggers above the show, below the hand. Summed, because two hits
    // landing together are two hits.
    // r[impl playback.stack] - layer 5, the song's transients
    // r[impl playback.triggers-sum]
    for (key, delta) in ringing {
        *out.entry(key).or_insert(0.0) += delta;
    }
    programmer.apply_to(&mut out, &show, *clock);
    // After applying, not before: a flash fired this frame must be seen
    // once before it can be retired, or a bump on a slow frame would be
    // dropped without ever having been drawn.
    programmer.retire_flashes(&show, *clock);
    // r[impl triggers.retire]
    triggers.retire(&show, trigger_clock);
    // Blind: `apply_to` left the programmer out of `out`, which is what
    // the rig would get. The viewport shows the *preview* instead — the
    // look the operator is building on top of the running show — since
    // seeing it is the whole point of going blind with a visualizer.
    // r[impl playback.blind] - the viz draws the preview while blind
    let frame: HashMap<(ChanId, Attribute), f32> = if programmer.blind {
        programmer.preview_output(&out, &show, *clock)
    } else {
        out
    };
    apply_output(
        &dmx.0,
        venue,
        &OutputFrame {
            values: &frame,
            intents: &intents,
            parked_dmx: &programmer.parked_dmx,
        },
    );
}

/// The colour intent each fixture is meant to show, across the stack:
/// every enabled player's intents in class order, so the Song's colour
/// is what a fixture gets unless a later class (a look) names its own.
// r[impl color.intent-to-output] - the intent survives to the output stage
pub fn output_intents(
    playbacks: &Playbacks,
    show: &Show<'_>,
) -> HashMap<ChanId, ignition_core::color::Intent> {
    let mut order: Vec<&ignition_core::Playback> =
        playbacks.entries.iter().filter(|p| p.enabled).collect();
    order.sort_by_key(|p| p.class);
    let mut out = HashMap::new();
    for entry in order {
        out.extend(entry.player.output_intents(show));
    }
    out
}

/// How long a gap before a tap-tempo run is treated as a fresh start
/// rather than a very slow beat.
const TAP_TIMEOUT: f32 = 3.0;

/// The operator keys. Space is GO, the way it is on every console.
// r[impl effects.masters.tap] - four taps retune the `Tap` master
// r[impl cues.seek] - stepping back re-runs the list from the top to the target
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
        playbacks,
        groups,
        rig,
        speeds,
        taps,
        profile,
        library,
        bundles,
        looks,
        named_tricks,
        focus_overrides,
        ..
    } = &mut *playback;
    let Some(player) = playbacks.of_class(Class::Song) else {
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
        library,
        bundles,
        looks,
        named_tricks,
        focus_overrides,
        ..Show::new(groups, rig)
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

    apply_output(
        &dmx.0,
        venue,
        &OutputFrame {
            values: &player.output(&show),
            intents: &player.output_intents(&show),
            ..Default::default()
        },
    );
}

#[cfg(test)]
mod accent_tests {
    use super::*;
    use ignition_core::Attribute;

    /// A figure's first moment must visibly change the fixtures it lands
    /// on, against the real show and the real room.
    ///
    /// This is the "fig 0 did nothing" report, pinned. It was a cue
    /// when first written and is a trigger now; either way, the frame
    /// with the figure ringing has to differ from the frame without.
    // r[verify song.chart.accents-are-additive]
    // r[verify triggers.crossing-fires]
    #[test]
    fn a_figure_changes_the_look_it_lands_on() {
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
            ..Show::new(&groups, &rig)
        };

        let figure = list
            .triggers
            .iter()
            .find(|t| t.name == "fig 0 · 1/3")
            .expect("the show has figure 0");
        let at = figure
            .bars()
            .expect("the shipped show carries resolved bars");

        let mut player = CuePlayer::new(list.cues.clone());
        player.seek(at, &show);
        let before = player.output(&show);

        let mut bus = TriggerBus::new(list.triggers.clone());
        bus.locate(Bars::new(at.bar, at.beat - 0.25));
        bus.advance(at, 0.0);
        let mut after = before.clone();
        for (key, delta) in bus.output(&show, 0.02) {
            *after.entry(key).or_insert(0.0) += delta;
        }

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
