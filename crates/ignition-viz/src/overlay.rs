//! The operator overlay — what a console's cue-list window shows, in the
//! corner of the 3D view.
//!
//! Windowed mode only. A snapshot has nobody to read it and burning cue
//! names into a render is the wrong thing.
//!
//! The cooked-status marker beside each cue is lifted straight from
//! grandMA3's coloured pots, which the research doc singles out as "a
//! small feature with an outsized effect on operator trust". It answers
//! "is this cue going to do what I think it does" without firing it — a
//! recipe that selects nothing is not an error and the show still runs,
//! so this is the only thing that makes it visible.

use crate::spawn::VenueRes;
use bevy::prelude::*;
use ignition_core::{Attribute, Class, Cook, Show, Status};

use crate::playback::Playback;

/// Marks the overlay's text node.
#[derive(Component)]
pub struct OverlayText;

/// The overlay's font, embedded at build time from whatever
/// `IGNITION_OVERLAY_FONT` pointed at — see `build.rs` and `flake.nix`.
///
/// `None` when built outside the dev shell, in which case the overlay
/// falls back to Bevy's built-in font: text still draws, the
/// cooked-status markers become tofu.
#[cfg(has_overlay_font)]
const FONT: Option<&[u8]> = Some(include_bytes!(concat!(
    env!("OUT_DIR"),
    "/overlay-font.ttf"
)));
#[cfg(not(has_overlay_font))]
const FONT: Option<&[u8]> = None;

/// How many cues either side of the current one to list.
const WINDOW: usize = 4;

pub fn spawn_overlay(mut commands: Commands, mut fonts: ResMut<Assets<Font>>) {
    let font = match FONT {
        Some(bytes) => bevy::text::FontSource::Handle(fonts.add(Font::from_bytes(bytes.to_vec()))),
        None => bevy::text::FontSource::default(),
    };
    commands.spawn((
        Text::new(String::new()),
        TextFont {
            font,
            font_size: bevy::text::FontSize::Px(13.0),
            ..default()
        },
        TextColor(Color::srgba(1.0, 1.0, 1.0, 0.85)),
        Node {
            position_type: PositionType::Absolute,
            top: Val::Px(10.0),
            left: Val::Px(12.0),
            ..default()
        },
        BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.45)),
        OverlayText,
    ));
}

/// Points the overlay at the camera that is actually rendering.
///
/// Bevy UI otherwise picks the primary window's camera, and the
/// headless snapshot path has no primary window — the overlay silently
/// draws nowhere. Done here rather than at spawn because both the camera
/// and the overlay are spawned in `Startup` and neither ordering is
/// worth pinning for this.
pub fn target_overlay_camera(
    mut commands: Commands,
    overlay: Query<
        Entity,
        (
            Or<(With<OverlayText>, With<FpsText>)>,
            Without<UiTargetCamera>,
        ),
    >,
    camera: Query<Entity, With<crate::camera::MainCamera>>,
) {
    let Ok(camera) = camera.single() else {
        return;
    };
    for entity in &overlay {
        commands.entity(entity).insert(UiTargetCamera(camera));
    }
}

/// Marks the standalone frame-rate readout.
#[derive(Component)]
pub struct FpsText;

/// The frame-rate readout on its own, for contexts that want the number
/// without the cue list — the studio being the case, since it draws its
/// own cue list in the Dioxus sidebar.
pub fn spawn_fps(mut commands: Commands, mut fonts: ResMut<Assets<Font>>) {
    let font = match FONT {
        Some(bytes) => bevy::text::FontSource::Handle(fonts.add(Font::from_bytes(bytes.to_vec()))),
        None => bevy::text::FontSource::default(),
    };
    commands.spawn((
        Text::new(String::new()),
        TextFont {
            font,
            font_size: bevy::text::FontSize::Px(13.0),
            ..default()
        },
        TextColor(Color::srgba(1.0, 1.0, 1.0, 0.85)),
        Node {
            position_type: PositionType::Absolute,
            top: Val::Px(10.0),
            // Top-right, where it cannot land on top of the operator
            // overlay if both happen to be on.
            right: Val::Px(12.0),
            ..default()
        },
        BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.45)),
        FpsText,
    ));
}

pub fn update_fps(
    diagnostics: Res<bevy::diagnostic::DiagnosticsStore>,
    mut text: Query<&mut Text, With<FpsText>>,
    mut last_logged: Local<Option<std::time::Instant>>,
) {
    let readout = fps_readout(&diagnostics);
    if let Ok(mut text) = text.single_mut() {
        text.0 = readout.clone();
    }
    // Once a second to the log as well, so the number can be read off
    // stdout from a session nobody is watching — a headless-ish run, or
    // one where the readout is behind another window.
    let now = std::time::Instant::now();
    let due = last_logged.is_none_or(|t| now.duration_since(t).as_secs_f32() >= 1.0);
    if due {
        *last_logged = Some(now);
        tracing::info!(readout = %readout, "viz.fps");
    }
}

/// The frame rate, smoothed, plus the frame time that produced it.
///
/// Both numbers, because they answer different questions. 120 fps is the
/// target; 8.3 ms is the budget, and it is the budget that says how much
/// headroom is left before a heavier cue costs frames.
///
/// Worth knowing what this measures when the visualizer is **embedded**
/// in the studio: Bevy renders when Blitz asks it to, so the rate here is
/// the host's paint cadence, not what Bevy could manage if it drove
/// itself. A standalone `viz` window is the honest measure of the
/// renderer; this one is the honest measure of what the operator sees,
/// which is the number that actually matters on a show.
fn fps_readout(diagnostics: &bevy::diagnostic::DiagnosticsStore) -> String {
    use bevy::diagnostic::FrameTimeDiagnosticsPlugin;
    let fps = diagnostics
        .get(&FrameTimeDiagnosticsPlugin::FPS)
        .and_then(|d| d.smoothed());
    let frame_ms = diagnostics
        .get(&FrameTimeDiagnosticsPlugin::FRAME_TIME)
        .and_then(|d| d.smoothed());
    match (fps, frame_ms) {
        // The history is empty for the first frames after launch, which
        // would otherwise read as a hard 0 fps right when someone is
        // watching to see whether it starts well.
        (Some(fps), Some(ms)) => format!("{fps:>5.1} fps  {ms:>4.1} ms"),
        (Some(fps), None) => format!("{fps:>5.1} fps"),
        _ => "  -- fps".to_string(),
    }
}

/// One line of "who won": every class lowest first, the winner marked.
///
/// The winner is the highest class that asserts the key — the same
/// rule the fold used, restated here so the two cannot disagree without
/// the test below noticing.
fn inspect_line(chan: ignition_core::ChanId, report: &[(Class, Option<f32>)]) -> String {
    let winner = report
        .iter()
        .rev()
        .find(|(_, v)| v.is_some())
        .map(|(class, _)| *class);
    let cells: Vec<String> = report
        .iter()
        .map(|(class, value)| {
            let mark = if Some(*class) == winner { '*' } else { ' ' };
            match value {
                Some(v) => format!("{mark}{class:?} {v:.2}"),
                None => format!("{mark}{class:?} --"),
            }
        })
        .collect();
    format!("ch {chan} dimmer:  {}", cells.join("   "))
}

/// The masters line: GM first, then every playback's own master in
/// stack order, then how much is parked — or nothing about parks when
/// nothing is, so the line stays short on an ordinary night.
fn masters_line(grand: f32, masters: impl Iterator<Item = (Class, f32)>, parked: usize) -> String {
    let mut line = format!("GM {:>3.0}%", grand * 100.0);
    for (class, master) in masters {
        line.push_str(&format!("   {class:?} {:>3.0}%", master * 100.0));
    }
    if parked > 0 {
        line.push_str(&format!("   PARKED {parked}"));
    }
    line
}

pub fn update_overlay(
    venue: Res<VenueRes>,
    mut playback: ResMut<Playback>,
    diagnostics: Res<bevy::diagnostic::DiagnosticsStore>,
    output: Option<Res<crate::output::DmxOutput>>,
    mut text: Query<&mut Text, With<OverlayText>>,
) {
    let Ok(mut text) = text.single_mut() else {
        return;
    };
    let Playback {
        playbacks,
        ringing,
        groups,
        rig,
        speeds,
        profile,
        library,
        bundles,
        focus_overrides,
        programmer,
        ..
    } = &mut *playback;
    let Some(player) = playbacks
        .entries
        .iter()
        .find(|p| p.class == Class::Song)
        .map(|p| &p.player)
    else {
        text.0 = "no show loaded — pass --recipes <path>".into();
        return;
    };
    let venue = &venue.0;
    let show = Show {
        groups,
        palettes: &venue.palettes,
        rig,
        speeds,
        roles: profile,
        library,
        bundles,
        focus_overrides,
        ..Show::new(groups, rig)
    };

    let current = player.current_index();
    let all = player.cues();
    // Only cook the cues actually on screen — the whole list every frame
    // would be resolving hundreds of recipes to draw a dozen lines.
    let first = current.unwrap_or(0).saturating_sub(WINDOW);
    let last = (current.unwrap_or(0) + WINDOW + 1).min(all.len());

    let mut lines = vec![format!(
        "GO space   BACK backspace   RESTART r   TAP t        clock {:>6.1}s   {}",
        player.clock(),
        fps_readout(&diagnostics)
    )];
    if !speeds.is_empty() {
        let mut masters: Vec<String> = speeds
            .iter()
            .map(|(k, v)| format!("{k} {v:.0} BPM"))
            .collect();
        masters.sort();
        lines.push(masters.join("   "));
    }
    // The programmer's state that changes what the picture means: a
    // blind desk is showing a preview, and a program time is why a
    // punch did not snap.
    // The masters above every playback: the grand master, and each
    // list's own. Read here rather than inferred from the picture,
    // because a rig at half is what both a GM at half and a cue at
    // half look like.
    // r[impl playback.grand-master] - shown
    // r[impl playback.playback-master] - shown
    // r[impl playback.park] - the parked count
    lines.push(masters_line(
        programmer.grand,
        playbacks.entries.iter().map(|e| (e.class, e.master)),
        programmer.parked.len(),
    ));
    // Whether the bytes on this picture are leaving the socket. A desk
    // that is silently not sending is worse than one not connected.
    // r[impl dmx.output-toggle] - the transmit state is on the overlay
    if let Some(output) = &output {
        lines.push(output.summary().line());
    }
    lines.push(format!(
        "page {}/{}   prog time {:.2} beats{}{}{}",
        programmer.page + 1,
        programmer.pages.len().max(1),
        programmer.program_time_beats,
        if programmer.blind {
            "   BLIND (preview)"
        } else {
            ""
        },
        if programmer.highlight {
            "   HIGHLIGHT"
        } else {
            ""
        },
        if programmer.lowlight {
            "   LOWLIGHT"
        } else {
            ""
        },
    ));
    // Which playback won the selected fixture's dimmer, and what every
    // player beneath would have produced — the question the priority
    // model exists to make answerable.
    // r[impl playback.inspectable] - shown for the selected fixture
    if let Some(selection) = programmer.selection.as_ref()
        && let Some(chan) = ignition_core::selection::resolve(selection, groups, rig)
            .into_iter()
            .next()
    {
        let key = (chan, Attribute::Dimmer);
        let report = playbacks.inspect(&show, ringing, &key);
        lines.push(inspect_line(chan, &report));
    }
    lines.push(String::new());

    for (i, cue) in all.iter().enumerate().take(last).skip(first) {
        let cook = ignition_core::cook_cue(cue, &show, player.clock());
        let here = if current == Some(i) { '>' } else { ' ' };
        let detail = match cook.status() {
            Status::Failed => {
                let bad = cook.recipes.iter().filter(|c| **c == Cook::Empty).count();
                format!("{bad} recipe(s) select nothing")
            }
            Status::Cooked | Status::Mixed => {
                let fixtures: usize = cook
                    .recipes
                    .iter()
                    .map(|c| match c {
                        Cook::Ok(n) => *n,
                        Cook::Empty | Cook::Disabled => 0,
                    })
                    .sum();
                format!("{} recipes, {fixtures} fixtures", cook.recipes.len())
            }
            Status::Direct => format!("{} direct values", cook.direct),
            Status::Empty => "sets nothing".to_string(),
        };
        lines.push(format!(
            "{here} {} {i:>3}  {:<22} {detail}",
            cook.marker(),
            cue.name
        ));
    }

    text.0 = lines.join("\n");
}

#[cfg(test)]
mod inspect_tests {
    use super::*;

    /// r[verify playback.inspectable]
    #[test]
    fn the_highest_asserting_class_is_marked_as_the_winner() {
        let line = inspect_line(7, &[(Class::Look, Some(0.5)), (Class::Song, Some(0.2))]);
        assert_eq!(line, "ch 7 dimmer:   Look 0.50   *Song 0.20");
    }

    /// r[verify playback.grand-master]
    /// r[verify playback.playback-master]
    /// r[verify playback.park]
    #[test]
    fn the_masters_line_shows_gm_each_playback_and_parks_only_when_any() {
        let line = masters_line(
            0.5,
            [(Class::Look, 1.0), (Class::Song, 0.25)].into_iter(),
            0,
        );
        assert_eq!(line, "GM  50%   Look 100%   Song  25%");
        let line = masters_line(1.0, std::iter::empty(), 3);
        assert_eq!(line, "GM 100%   PARKED 3");
    }

    #[test]
    fn a_class_that_does_not_assert_the_key_cannot_win() {
        let line = inspect_line(1, &[(Class::Look, Some(0.5)), (Class::Song, None)]);
        assert_eq!(line, "ch 1 dimmer:  *Look 0.50    Song --");
    }
}
