//! What the desk asks the visualizer to do, once per frame.
//!
//! One dispatch over [`Command`], drained from the channel the whole UI
//! sends on. It is a single long function on purpose: the `Playback`
//! resource is destructured once and its fields borrowed piecewise for
//! the rest of the body, and every attempt to lift a case out ends in
//! handing that borrow set to a helper as eight arguments. The match
//! arms are the seams a reader wants, and they are already there.

use super::publish::{camera_clock, save_cameras};
use super::sound::SoundFade;
use crate::command::{Command, PageMove, Receiver};
use ignition_core::preset::Ref;
use ignition_core::{HostRequest, MacroRunner};
use ignition_daw::SongTransport;
use ignition_viz::embedded::EmbeddedViz;
use ignition_viz::playback::Playback;

/// Applies everything the UI has said since the last frame.
///
/// Drained rather than blocked on: a dropped frame's worth of messages
/// is better than a stalled frame, and the sender will send again.
pub(super) fn drain(
    commands: &Receiver,
    viz: &mut EmbeddedViz,
    transport: Option<&SongTransport>,
    sound_fade: &mut SoundFade,
    macro_runner: &mut Option<MacroRunner>,
    show_file: Option<&str>,
) {
    use ignition_core::{Class, RecipeApply, Show};

    // A click in the viewport, as the command every other surface
    // sends — so it is logged, applied and published like one.
    // r[impl studio.program.pick-and-gizmos] - a viewport click is a Command::Select
    let picked = viz.take_selection().map(|chans| {
        if chans.is_empty() {
            Command::Deselect
        } else {
            Command::Select(ignition_core::Selection::Chans(chans))
        }
    });
    let world = viz.app_mut().world_mut();
    let Some(mut playback) = world.remove_resource::<Playback>() else {
        return;
    };
    // The Live / Program / Library commands, handled after the block
    // below — `live_commands::apply` wants the whole `Playback`, which
    // the destructuring borrows piecemeal.
    let mut deferred: Vec<Command> = Vec::new();
    {
        let Playback {
            playbacks,
            groups,
            rig,
            palettes,
            speeds,
            programmer,
            profile,
            library,
            bundles,
            ..
        } = &mut playback;
        // The looks and macros the keys name, and the roles a blackout
        // leaves alone, come from the shipped profile — the one the
        // bank is built from.
        let shipped = crate::faders::profile();
        if programmer.protected.is_empty() && !shipped.protected.is_empty() {
            // r[impl profile.protected-roles] - the programmer learns them from the profile
            programmer.protected = shipped.protected.clone();
        }
        let queued = std::iter::from_fn(|| commands.try_recv().ok());
        for command in picked.into_iter().chain(queued) {
            crate::live_commands::note(&command);
            // Rebuilt per command because `Rate` mutates `speeds`, which
            // the `Show` borrows. Cheap — it is four references.
            match command {
                Command::Take { .. }
                | Command::Untake(_)
                | Command::DeskScene(_)
                | Command::DeskRelease
                | Command::Protect { .. }
                | Command::StoreCue { .. }
                | Command::StoreLook { .. } => deferred.push(command),
                Command::Select(selection) => programmer.select(selection),
                Command::Deselect => programmer.deselect(),
                Command::ClearValues => programmer.clear_values(),
                Command::Level(index, level) => programmer.set_level(index, level),
                Command::Fader(index, fader) => programmer.set_fader(index, *fader),
                Command::FaderOnPage { page, index, fader } => {
                    while programmer.pages.len() <= page {
                        programmer.add_page();
                    }
                    if page == programmer.page {
                        programmer.set_fader(index, *fader);
                    } else if let Some(slot) = programmer.pages[page].get_mut(index) {
                        *slot = *fader;
                    }
                }
                Command::Key {
                    index,
                    action,
                    down,
                } => {
                    if down {
                        // A transport key lands on a playback, which the
                        // programmer cannot reach; it hands the request back.
                        // r[impl playback.temp-and-pause] - pause, go back, load
                        if let Some(request) = programmer.key_down(index, action) {
                            let show = Show {
                                groups,
                                palettes,
                                rig,
                                speeds,
                                roles: profile,
                                ..Show::new(groups, rig)
                            };
                            playbacks.transport(request, &show);
                        }
                    } else {
                        programmer.key_up(index);
                    }
                }
                Command::Page(PageMove::Next) => programmer.next_page(),
                Command::Page(PageMove::Prev) => programmer.prev_page(),
                Command::Page(PageMove::Set(page)) => programmer.set_page(page),
                Command::ProgramTime(beats) => programmer.program_time_beats = beats.max(0.0),
                Command::Blind(on) => programmer.blind = on,
                Command::Highlight(on) => programmer.highlight = on,
                Command::Lowlight(on) => programmer.lowlight = on,
                Command::Tap(bpm) => {
                    if bpm.is_finite() && bpm > 0.0 {
                        speeds.insert("Tap".to_string(), bpm);
                    }
                }
                // Raw levels land on the fade, not the engine: what the
                // engine reads is written by `smooth_sound` every frame.
                Command::SoundLevels { low, mid, high } => {
                    sound_fade.raw = [low, mid, high];
                }
                Command::SoundFade(secs) => {
                    sound_fade.secs = secs.clamp(0.0, SoundFade::MAX_SECS);
                }
                // r[impl viz.body-glow] - flips the viz setting in place
                Command::BodyGlow(on) => {
                    if let Some(mut settings) =
                        world.get_resource_mut::<ignition_viz::spawn::VizSettings>()
                    {
                        settings.body_glow = on;
                    }
                    tracing::info!(on, "studio: fixture body glow");
                }
                // r[impl studio.program.pick-and-gizmos] - the overlay keys flip the viz resource
                Command::Overlay { kind, on } => {
                    if let Some(mut overlays) =
                        world.get_resource_mut::<ignition_viz::gizmos::ProgramOverlays>()
                    {
                        use crate::command::OverlayKind;
                        match kind {
                            OverlayKind::Focus => overlays.focus = on,
                            OverlayKind::Beams => overlays.beams = on,
                            OverlayKind::Groups => overlays.groups = on,
                        }
                    }
                }
                Command::Labels(on) => {
                    if let Some(mut overlays) =
                        world.get_resource_mut::<ignition_viz::gizmos::ProgramOverlays>()
                    {
                        overlays.labels = on;
                    }
                }
                Command::ProgramView(on) => {
                    if let Some(mut overlays) =
                        world.get_resource_mut::<ignition_viz::gizmos::ProgramOverlays>()
                        && overlays.program != on
                    {
                        overlays.program = on;
                    }
                }
                // r[impl viz.camera-cuts] - a key or a tile cuts the programme camera
                Command::Camera { target, beats } => {
                    let (now, bpm) = camera_clock(playbacks, speeds);
                    if let Some(mut active) = world.get_resource_mut::<ignition_viz::ActiveCamera>()
                    {
                        let target = match target {
                            ignition_live_ui::command::CameraTarget::Slot(n) => {
                                ignition_viz::CameraTarget::Slot(n)
                            }
                            ignition_live_ui::command::CameraTarget::Preset(name) => {
                                ignition_viz::CameraTarget::Preset(name)
                            }
                        };
                        active.clear_queue();
                        if active.cut_to(&target, beats, now, bpm) {
                            tracing::info!(camera = ?active.preset, "studio: camera");
                        }
                    }
                }
                // r[impl studio.video.cameras-pane] - save the view the viewport is on
                Command::SaveCameraPreset { name } => {
                    let (now, _) = camera_clock(playbacks, speeds);
                    if let Some(mut active) = world.get_resource_mut::<ignition_viz::ActiveCamera>()
                    {
                        let state = active.state_at(now);
                        let preset = ignition_viz::CameraPreset {
                            ortho: state.ortho,
                            focus: state.focus,
                            ..ignition_viz::CameraPreset::new(
                                name.trim(),
                                state.eye.to_array(),
                                state.look.to_array(),
                                state.fov_deg,
                            )
                        };
                        if !preset.name.is_empty() {
                            active.cameras.store(preset);
                            active.preset = Some(name.trim().to_string());
                            save_cameras(&active.cameras);
                        }
                    }
                }
                // r[impl studio.video.cameras-pane] - set as slot N, for the operator and the venue
                Command::SetCameraSlot { slot, name } => {
                    if let Some(mut active) = world.get_resource_mut::<ignition_viz::ActiveCamera>()
                        && active.cameras.set_slot(slot, &name)
                    {
                        save_cameras(&active.cameras);
                        let operator = ignition_live_ui::operators::current_name();
                        if let Err(error) = ignition_live_ui::cameras::save_favourites(
                            &operator,
                            &active.cameras.favourites,
                        ) {
                            tracing::warn!(%error, "studio: camera favourites not saved");
                        }
                    }
                }
                // r[impl studio.video.cameras-pane] - delete
                Command::DeleteCameraPreset { name } => {
                    if let Some(mut active) = world.get_resource_mut::<ignition_viz::ActiveCamera>()
                        && active.cameras.remove(&name)
                    {
                        if active
                            .preset
                            .as_deref()
                            .is_some_and(|p| p.eq_ignore_ascii_case(&name))
                        {
                            active.preset = None;
                        }
                        save_cameras(&active.cameras);
                    }
                }
                // r[impl viz.programme-view] - the wide view's own preset
                Command::Wide { target } => {
                    if let Some(mut active) = world.get_resource_mut::<ignition_viz::ActiveCamera>()
                    {
                        let target = match target {
                            ignition_live_ui::command::CameraTarget::Slot(n) => {
                                ignition_viz::CameraTarget::Slot(n)
                            }
                            ignition_live_ui::command::CameraTarget::Preset(name) => {
                                ignition_viz::CameraTarget::Preset(name)
                            }
                        };
                        if !active.set_wide(&target) {
                            tracing::warn!(?target, "studio: wide names no preset");
                        }
                    }
                }
                // r[impl canvas.camera-source] - TO SCREENS
                Command::CanvasSource { canvas, source } => {
                    let source = if source.trim().is_empty() {
                        None
                    } else {
                        match ignition_viz::camera::CameraSource::parse(&source) {
                            Some(s) => Some(s),
                            None => {
                                tracing::warn!(source, "studio: not a camera source");
                                continue;
                            }
                        }
                    };
                    if let Some(mut switches) =
                        world.get_resource_mut::<ignition_viz::camera::CanvasSwitches>()
                    {
                        switches.set(&canvas, source);
                    }
                }
                Command::HighlightGroup(name) => {
                    if let Some(mut highlight) =
                        world.get_resource_mut::<ignition_viz::gizmos::HighlightGroup>()
                        && highlight.0 != name
                    {
                        highlight.0 = name;
                    }
                }
                // r[impl dmx.output-toggle] - flips the transmitter without touching the engine
                Command::Output(on) => {
                    if let Some(mut output) = world.get_resource_mut::<ignition_viz::DmxOutput>() {
                        output.set_enabled(on);
                    }
                    tracing::info!(on, "studio: dmx output");
                }
                // r[impl playback.grand-master]
                Command::Grand(level) => programmer.set_grand(level),
                // r[impl playback.playback-master]
                Command::PlaybackMaster(class, level) => {
                    for entry in playbacks.entries.iter_mut().filter(|e| e.class == class) {
                        entry.master = level.clamp(0.0, 1.0);
                    }
                }
                // r[impl playback.park] - at the programmer's held value per fixture
                Command::Park { selection, attrs } => {
                    let held: std::collections::HashMap<_, _> = programmer
                        .captured()
                        .into_iter()
                        .map(|v| ((v.chan, v.attr), v.value))
                        .collect();
                    let chans = ignition_core::selection::resolve(&selection, groups, rig);
                    let mut parked = 0usize;
                    for chan in chans {
                        for attr in &attrs {
                            if let Some(value) = held.get(&(chan, attr.clone())) {
                                programmer.park_chan(chan, attr.clone(), *value);
                                parked += 1;
                            }
                        }
                    }
                    tracing::info!(parked, "studio: parked");
                }
                Command::Unpark { selection, attrs } => {
                    for chan in ignition_core::selection::resolve(&selection, groups, rig) {
                        for attr in &attrs {
                            programmer.unpark_chan(chan, attr);
                        }
                    }
                }
                // r[impl playback.speed-keys]
                Command::Speed(key) => {
                    programmer.key_down(0, key.action());
                }
                Command::Rate(bpm) => {
                    speeds.insert("Rate".to_string(), bpm);
                }
                Command::Size(v) => programmer.size = v.clamp(0.0, 1.0),
                Command::EffectRate(v) => programmer.rate = v.max(0.0),
                Command::Master(role, level) => programmer.set_master(&role, level),
                Command::Solo(role) => match role {
                    Some(role) => programmer.solo(&role),
                    None => programmer.clear_solo(),
                },
                Command::Hold(Some(recipe)) => programmer.hold(*recipe),
                Command::Hold(None) => programmer.release_hold(),
                // r[impl playback.macro-runner] - a MACRO key starts one; the tick below runs it
                Command::Macro(name) => match MacroRunner::from_profile(shipped, &name) {
                    Some(runner) => {
                        tracing::info!(name, "studio: macro");
                        *macro_runner = Some(runner);
                    }
                    None => tracing::warn!(name, "studio: no such macro"),
                },
                // r[impl playback.look-hold] - a LOOK key latches the look on the held layer
                Command::Look(Some(name)) => {
                    let show = Show {
                        groups,
                        palettes,
                        rig,
                        speeds,
                        roles: profile,
                        library,
                        bundles,
                        looks: &shipped.looks,
                        ..Show::new(groups, rig)
                    };
                    let recipes = shipped.look_recipes(&name, &show);
                    if recipes.is_empty() {
                        tracing::warn!(name, "studio: no such look");
                    }
                    let safe = shipped
                        .looks
                        .get(&name)
                        .is_some_and(|l| l.kind == ignition_core::profile::LookKind::Safe);
                    programmer.hold_look(recipes, safe);
                }
                Command::Look(None) => programmer.release_hold(),
                // r[impl profile.effect-parameters] - the control reaches the engine's fader
                Command::Param { index, name, value } => programmer.set_param(index, &name, value),
                Command::Flash(target, kind) => {
                    // Fired against the player's clock, which is the song
                    // while a transport is loaded — so a hand-played
                    // flash and a charted one are timed by the same
                    // thing.
                    let now = playbacks
                        .of_class(Class::Song)
                        .map(|c| c.clock())
                        .unwrap_or_default();
                    programmer.flash(target, kind, now);
                }
                Command::Color(name) => {
                    let show = Show {
                        groups,
                        palettes,
                        rig,
                        speeds,
                        roles: profile,
                        ..Show::new(groups, rig)
                    };
                    programmer.apply(RecipeApply::Color(Ref::Named(name)), &show);
                }
                Command::Split(name) => {
                    let show = Show {
                        groups,
                        palettes,
                        rig,
                        speeds,
                        roles: profile,
                        ..Show::new(groups, rig)
                    };
                    programmer.apply(RecipeApply::Split(Ref::Named(name)), &show);
                }
                Command::Focus(name) => {
                    let show = Show {
                        groups,
                        palettes,
                        rig,
                        speeds,
                        roles: profile,
                        ..Show::new(groups, rig)
                    };
                    programmer.apply(RecipeApply::FocusPoint(Ref::Named(name)), &show);
                }
                Command::Dimmer(level) => {
                    let show = Show {
                        groups,
                        palettes,
                        rig,
                        speeds,
                        roles: profile,
                        ..Show::new(groups, rig)
                    };
                    programmer.apply(RecipeApply::Dimmer(level), &show);
                }
                Command::Release => {
                    let show = Show {
                        groups,
                        palettes,
                        rig,
                        speeds,
                        roles: profile,
                        ..Show::new(groups, rig)
                    };
                    programmer.release(&show);
                }
                Command::Go => {
                    if let Some(player) = playbacks.of_class(Class::Song) {
                        let show = Show {
                            groups,
                            palettes,
                            rig,
                            speeds,
                            roles: profile,
                            ..Show::new(groups, rig)
                        };
                        player.go(&show);
                    }
                }
                Command::LookGo => {
                    if let Some(player) = playbacks.of_class(Class::Look) {
                        let show = Show {
                            groups,
                            palettes,
                            rig,
                            speeds,
                            roles: profile,
                            ..Show::new(groups, rig)
                        };
                        player.go(&show);
                    }
                }
                Command::Cue(index) => {
                    if let Some(player) = playbacks.of_class(Class::Song) {
                        let show = Show {
                            groups,
                            palettes,
                            rig,
                            speeds,
                            roles: profile,
                            ..Show::new(groups, rig)
                        };
                        // Take the song to the cue's own position, not
                        // to a section with the cue's name. Every cue is
                        // written at a musical position, so every cue is
                        // seekable — which section names only managed
                        // for the nineteen cues that were sections, and
                        // the accents, being called things like
                        // "· fig 0 · 1/3", simply failed.
                        if let Some(at) = player.cues().get(index).and_then(|c| c.position())
                            && let Some(transport) = transport
                        {
                            transport.locate(at);
                        }
                        player.jump_to_end_of(index, &show);
                    }
                }
                // Transport. Nothing here touches the cue player: the
                // song moves, and `follow_song` notices on the next
                // frame. Keeping the two apart is what lets the same
                // list run with no transport at all.
                Command::Play => {
                    if let Some(transport) = transport {
                        transport.play();
                    }
                }
                Command::Stop => {
                    if let Some(transport) = transport {
                        transport.stop();
                    }
                }
                Command::Section(name) => {
                    if let Some(transport) = transport
                        && !transport.locate_section(&name)
                    {
                        tracing::warn!(name, "studio: no such section");
                    }
                }
                Command::Scrub(fraction) => {
                    if let Some(transport) = transport {
                        transport.scrub(fraction);
                    }
                }
                Command::Locate(position) => {
                    if let Some(transport) = transport {
                        transport.locate(position);
                    }
                }
            }
        }

        // The cues' host commands. `macro <name>` is the show starting
        // a profile macro — the drop on the last chorus's downbeat, the
        // end after the last cue — exactly as a MACRO key would, so a
        // move an operator busks and a move the show fires are one
        // thing. Anything else (`osc …`) is a host line: logged here,
        // and a transmitter's for the taking.
        // r[impl cues.command] - handed out once, when the cue goes live
        // r[impl playback.macro-runner] - a cue can start one
        // `camera …` is the show cutting the programme camera, on the
        // same clock — see `ignition_viz::camera`.
        // r[impl viz.camera-cuts] - a cue's camera command, at the cue change
        let (cam_now, cam_bpm) = camera_clock(playbacks, speeds);
        if let Some(player) = playbacks.of_class(Class::Song) {
            for command in player.drain_commands() {
                if let Some(mut active) = world.get_resource_mut::<ignition_viz::ActiveCamera>()
                    && ignition_viz::camera::apply_command_line(
                        &mut active,
                        &command,
                        cam_now,
                        cam_bpm,
                    )
                {
                    tracing::info!(command, "studio: camera (from cue)");
                    continue;
                }
                match command.strip_prefix("macro ") {
                    Some(name) => match MacroRunner::from_profile(shipped, name.trim()) {
                        Some(runner) => {
                            tracing::info!(name, "studio: macro (from cue)");
                            *macro_runner = Some(runner);
                        }
                        None => tracing::warn!(name, "studio: cue names no such macro"),
                    },
                    None => tracing::info!(command, "studio: cue command"),
                }
            }
        }

        // The running macro, stepped on the song's clock. Steps up to
        // the next wait land this frame; what the programmer cannot do
        // itself — the transmitter switch — comes back as a request.
        // r[impl playback.macro-runner]
        if let Some(runner) = macro_runner {
            let show = Show {
                groups,
                palettes,
                rig,
                speeds,
                roles: profile,
                library,
                bundles,
                looks: &shipped.looks,
                ..Show::new(groups, rig)
            };
            for request in runner.tick(programmer, playbacks, shipped, &show) {
                match request {
                    HostRequest::Output(on) => {
                        if let Some(mut output) =
                            world.get_resource_mut::<ignition_viz::DmxOutput>()
                        {
                            output.set_enabled(on);
                        }
                        tracing::info!(on, "studio: dmx output (macro)");
                    }
                }
            }
            if runner.finished() {
                *macro_runner = None;
            }
        }
    }
    if !deferred.is_empty() {
        let desk = crate::desk::path_for_venue(&crate::venue_dir());
        for command in &deferred {
            crate::live_commands::apply(
                command,
                &mut playback,
                desk.as_deref(),
                show_file.map(std::path::Path::new),
                transport.map(|t| t.song()),
            );
        }
    }
    viz.app_mut().world_mut().insert_resource(playback);
}
