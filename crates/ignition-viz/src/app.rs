//! The one place that owns a `bevy::App`.
//!
//! Two entry points over one `VizPlugin`: `run_windowed` opens a window
//! and presents frames until the operator closes it, and `run_snapshot`
//! renders a fixed number of frames into an offscreen image, writes a PNG
//! and returns. The scene, the systems and the resources are identical —
//! only the render target and who drives the loop differ. That is why
//! there is a single `viz` binary now where there used to be `shot` and
//! `live`: those were two binaries because they were two separately
//! hand-written wgpu pipelines, not two different scenes.
//!
//! The snapshot path follows Bevy's own `externally_driven_headless_
//! renderer` example: no window, no winit, the update loop pumped by
//! hand, and the device polled after each frame so the capture has
//! actually happened before the next one is asked for.

use crate::gdtf_geometry::GdtfLibrary;
use crate::output::DmxOutput;
use crate::playback::{Playback, operator_keys, tick_playback};
use crate::spawn::{
    CanvasClock, DmxRes, GdtfLibraryRes, LiveDmx, VenueRes, VizSettings, apply_ambient,
    resolve_live_dmx, spawn_venue, update_beams, update_canvas_videos, update_fixture_bodies,
    update_live_fixtures,
};
use crate::video::export::{self, ExportRequest, FrameSchedule};
use crate::view::ViewPreset;
use crate::{DmxUniverses, Venue, dmx};
use bevy::app::SubApps;
use bevy::asset::{AssetPlugin, RenderAssetUsages};
use bevy::camera::{Hdr, RenderTarget};
use bevy::core_pipeline::tonemapping::Tonemapping;
use bevy::image::Image;
use bevy::post_process::auto_exposure::{AutoExposure, AutoExposureCompensationCurve};
use bevy::post_process::bloom::Bloom;
use bevy::prelude::*;
use bevy::render::RenderPlugin;
use bevy::render::render_resource::{
    Extent3d, PollType, TextureDimension, TextureFormat, TextureUsages,
};
use bevy::render::renderer::RenderDevice;
use bevy::render::view::screenshot::{Screenshot, ScreenshotCaptured, save_to_disk};
use bevy::time::TimeUpdateStrategy;
use bevy::window::ExitCondition;
use bevy::winit::WinitPlugin;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

/// Everything the CLI can set. A plain struct, so `bin/viz.rs` stays
/// argument parsing and nothing else.
#[derive(Clone)]
pub struct VizConfig {
    pub venue: Venue,
    pub view: ViewPreset,
    pub width: u32,
    pub height: u32,
    pub haze: f32,
    pub ambient: f32,
    /// Whether to draw the props layer (people, drum kit, speakers,
    /// mics). On by default now that the people and the kit are real
    /// models rather than the placeholder boxes that used to clutter
    /// every shot without adding information worth the noise.
    pub show_props: bool,
    /// An explicit eye/target pair, overriding `view`'s framing. What
    /// `--eye`/`--look` set, for inspecting one corner of the room.
    pub camera: Option<(Vec3, Vec3)>,
    /// The venue's camera presets, slots and setups — see `camera.rs`.
    /// Loaded from `cameras.json` by whoever loads the venue; the three
    /// auto-framed views when there is no file.
    pub cameras: crate::camera::Cameras,
    /// Start on this preset, by name — `--camera <preset>`. Overrides
    /// `view` and `camera` when it names one.
    pub camera_preset: Option<String>,
    /// Draw the operator overlay — the cue list with cooked status.
    /// Always on in a window; `--overlay` turns it on for a snapshot
    /// too, which is how a still can carry the cue context that makes it
    /// mean something.
    pub overlay: bool,
    /// Draw the frame-rate readout in the corner.
    ///
    /// Separate from `overlay` because the studio wants this without the
    /// cue list — it already draws the cue list in its own sidebar, and
    /// two of them on one screen is worse than none.
    pub fps: bool,
    /// Room objects to leave out — see `VizSettings::exclude`.
    pub exclude: Vec<String>,
    /// Exposure offset in stops on top of `STAGE_EV100` — see
    /// `stage_exposure`. `0.0` is the calibrated stage camera; `+1`
    /// doubles the picture, `-1` halves it.
    pub exposure: f32,
    /// Whether the camera's exposure follows the frame like an eye —
    /// see `auto_exposure`. On by default; `--auto-exposure off`.
    pub auto_exposure: bool,
    /// The look applied after tonemapping — see `Grade`.
    pub grade: Grade,
    /// What the venue's screens display — see
    /// `VizSettings::screen_content`.
    pub screen_content: Option<String>,
    /// Per-canvas sources — `--canvas main=clips/city.png`. A canvas
    /// with no entry falls back to `screen_content`.
    pub canvas_content: std::collections::HashMap<String, String>,
    /// Where each canvas's crop is centred in its source, 0..=1; see
    /// `VizSettings::canvas_focus`.
    pub canvas_focus: std::collections::HashMap<String, f32>,
    /// Root directory the asset server loads from.
    pub assets_dir: String,
    /// Highest sACN/Art-Net universe to listen on.
    pub max_universe: u16,
    /// Render one frame to this path and exit, instead of opening a
    /// window.
    pub snapshot: Option<PathBuf>,
    /// How many frames to run before capturing. Bevy's first frames
    /// upload assets and warm pipelines, and a live rig also needs its
    /// first DMX packets to arrive; grabbing frame 0 catches a half-built
    /// scene and a dark rig.
    pub settle_frames: u32,
    /// How much GPU to spend per pixel — see `RenderQuality`.
    pub quality: RenderQuality,
    /// Whether DMX leaves the socket. The transmitter is bound either
    /// way, from the venue's config, so the switch is instant; this is
    /// its starting position. Off by default: a visualizer on a laptop
    /// should not take over a rig until asked.
    // r[impl dmx.output-toggle] - the starting position of the switch
    pub output: bool,
    /// Attach the loopback sink — see `output::LoopbackSink` for why
    /// this is a verification path and not the normal one.
    pub loopback: bool,
    /// Whether a lit fixture's housing glows its own colour — see
    /// `VizSettings::body_glow`. Off by default: the real fixtures are
    /// black.
    pub body_glow: bool,
    /// Start with the DMX address drawn over every fixture — see
    /// `gizmos::ProgramOverlays`. `--labels`.
    pub labels: bool,
}

impl VizConfig {
    /// A config for a headless host — the bench and the tests — with the
    /// CLI's defaults and the live quality, so a test drives the same
    /// path the studio does.
    pub fn headless(venue: Venue, width: u32, height: u32) -> Self {
        Self {
            quality: RenderQuality::live(),
            venue,
            view: ViewPreset::House,
            width,
            height,
            haze: 1.6,
            ambient: 0.15,
            max_universe: 4,
            snapshot: None,
            settle_frames: 20,
            show_props: true,
            camera: None,
            cameras: Default::default(),
            camera_preset: None,
            overlay: false,
            fps: false,
            exclude: Vec::new(),
            exposure: 0.0,
            auto_exposure: true,
            grade: Grade::Neutral,
            screen_content: None,
            canvas_content: Default::default(),
            canvas_focus: Default::default(),
            assets_dir: concat!(env!("CARGO_MANIFEST_DIR"), "/assets").to_string(),
            output: false,
            loopback: false,
            body_glow: false,
            labels: false,
        }
    }
}

/// The per-pixel cost dials: what a still can afford and a live view
/// cannot.
///
/// The volumetric pass raymarches every pixel `fog_steps` times, and
/// MSAA multiplies every fragment by its sample count. Together they
/// were most of the frame in the studio, where the picture has to
/// arrive every 8 ms; a snapshot has all day and keeps the old numbers
/// so stills do not change.
/// The quality ladder, coarsest first.
///
/// One name for a whole set of dials, because the dials are not
/// independent and an operator should not have to know that. The two
/// that matter are the raymarch's step count and how many pixels the
/// haze camera may have, and they trade against exactly one thing: a
/// mover's shaft staying a solid cone rather than breaking into a
/// string of dots. Everything else — the reflections, the occlusion,
/// the anti-aliasing — is worth a couple of frames a second between
/// them and is switched with the tier rather than argued about.
///
/// Measured on the benchmark cue at 5120x1440, rig lit, effects
/// running, screens playing — `just profile-bench`. The numbers are in
/// `docs/ops/profiling.md`, and they are a reference GPU's, not a
/// promise.
// r[impl viz.quality-presets]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Preset {
    /// Whatever it takes. The shafts *will* break into dots and the
    /// room will be flat — this is for a laptop on a bus, or for
    /// finding out whether a stutter is the GPU at all.
    Potato,
    /// A thin picture that still reads as lighting: solid-ish shafts,
    /// no reflections, no occlusion.
    Low,
    /// The default, and what the studio has always rendered: the fewest
    /// steps that keep every shaft solid, a haze camera bounded by a
    /// pixel budget, and the whole post chain.
    #[default]
    Medium,
    /// A finer march and twice the haze pixels, for a machine with room
    /// to spare.
    High,
    /// As close to a still as a live view gets — the haze marched on
    /// the camera itself, at full size.
    Ultra,
}

impl Preset {
    /// `IGNITION_QUALITY`, or [`Preset::Medium`].
    ///
    /// An unknown name is a warning and the default rather than an
    /// error: a typo in an environment variable should not stop a show
    /// from opening.
    pub fn from_env() -> Self {
        let Ok(name) = std::env::var("IGNITION_QUALITY") else {
            return Preset::default();
        };
        match Self::parse(&name) {
            Some(preset) => preset,
            None => {
                tracing::warn!(
                    name,
                    "viz: no such quality preset; using medium (potato, low, medium, high, ultra)"
                );
                Preset::default()
            }
        }
    }

    pub fn parse(name: &str) -> Option<Self> {
        match name.trim().to_ascii_lowercase().as_str() {
            "potato" => Some(Preset::Potato),
            "low" => Some(Preset::Low),
            "medium" | "med" => Some(Preset::Medium),
            "high" => Some(Preset::High),
            "ultra" => Some(Preset::Ultra),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Preset::Potato => "potato",
            Preset::Low => "low",
            Preset::Medium => "medium",
            Preset::High => "high",
            Preset::Ultra => "ultra",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RenderQuality {
    /// `VolumetricFog::step_count`.
    pub fog_steps: u32,
    /// How many pixels the haze camera may have when `fog_scale` is 0.
    ///
    /// Unused while every tier marches on the camera itself — see
    /// `fog_scale` — and kept because that is the dial the reduced path
    /// is bounded by, and it is coming back.
    ///
    /// A budget rather than a divisor, because a divisor means
    /// something different on every viewport: the fog's cost is this
    /// times the step count, so this is the number that fixes the
    /// haze's share of the frame whatever the window is doing. See
    /// `haze::haze_scale`.
    pub haze_pixels: u32,
    /// The froxel grid's shape, when the froxel path is running.
    ///
    /// Width is what a beam's *sharpness* costs: the grid is sampled
    /// trilinearly, so a tile covering sixteen screen pixels gives a
    /// beam a sixteen-pixel edge. Depth is what its *continuity*
    /// costs, and `froxel_samples` buys that far more cheaply — which
    /// is why the ladder below spends its budget sideways.
    ///
    /// The grid is what resolves a beam, so this is that path's
    /// equivalent of `fog_steps` and `haze_pixels` together — and it
    /// is far cheaper than either. At 5120x1440 on the benchmark cue
    /// the whole pass costs 0.22 ms at 160x90x64 and 1.5 ms at
    /// 320x180x192, against 18 ms a frame for the march it replaces.
    pub froxel_grid: (u32, u32, u32),
    /// Points lit per froxel. One leans on the temporal blend alone,
    /// which needs frames a still does not have; more resolves a thin
    /// beam within the frame itself.
    pub froxel_samples: u32,
    /// How much of last frame's froxel grid survives into this one, as
    /// a percentage — a whole number so the ladder stays comparable.
    ///
    /// The other half of resolving a beam thinner than a froxel, and
    /// the half that costs nothing. Too much smears behind a moving
    /// mover, so a tier that already pays for samples can afford less
    /// of it.
    pub froxel_history_pct: u32,
    /// Whether the camera multisamples. Bloom and fog blur beam edges
    /// enough that turning it off is hard to see and easy to measure.
    pub msaa: bool,
    /// The haze is marched at `1/fog_scale` of the picture's size — see
    /// `haze.rs`. `1` marches it on the camera itself, at full size, as
    /// Bevy ships volumetric fog; `0` picks the scale from the
    /// picture's size so a separate haze camera stays within
    /// `haze::HAZE_PIXEL_BUDGET` pixels whatever the viewport.
    ///
    /// **Every tier is `1`.** The reduced haze camera is roughly four
    /// times cheaper and is the reason the ladder has a pixel budget at
    /// all, but it is wrong in two ways that a single viewport cannot
    /// show. Its composite quad sits on the default render layer, so
    /// with two panes up each camera draws the *other* camera's haze
    /// over its own picture — a person occluding a shaft in the
    /// Visualizer punches a hole in the Programme pane's fog, from the
    /// wrong viewpoint. And the occluders it marches against are twin
    /// meshes on their own layer, which is a second copy of every
    /// moving thing in the room to keep in step.
    ///
    /// `IGNITION_FOG_SCALE=0` restores it, per viewport, for measuring.
    /// It should not go back on the ladder until the composite is
    /// confined to the camera that owns it.
    pub fog_scale: u32,
    /// Temporal anti-aliasing on the camera. Needs `msaa` off. Beyond
    /// the edges, it is what averages the haze's per-frame jitter into
    /// a smooth cone.
    pub taa: bool,
    /// Screen-space reflections: the wet-look deck mirrors the rig
    /// standing on it. Marches against the deferred prepasses, which
    /// `deferred` is what actually asks for.
    pub ssr: bool,
    /// Whether the camera runs the depth and deferred prepasses.
    ///
    /// Separate from `ssr` because the venue's deck is a *deferred*
    /// material: without these passes it does not render, reflections
    /// or no reflections. A camera that wants a cheap picture of the
    /// room still needs the room.
    pub deferred: bool,
    /// Screen-space ambient occlusion: contact shadow where the risers
    /// meet the deck and the people stand on it.
    pub ssao: bool,
    /// Depth of field, focused on the stage centre. Stills and exports
    /// only: a blurred house is a photograph, not an operator's view.
    pub dof: bool,
    /// Whether the auto exposure lands on its level in one frame. A
    /// still has no half-second to adapt in; a live view adapts at an
    /// eye's pace.
    pub instant_adaptation: bool,
    /// Whether this tier pays [`NARROW_BEAM_FOG_STEPS`] when the rig in
    /// front of it cuts a narrow shaft — see [`RenderQuality::for_rig`].
    ///
    /// A still can afford 256 steps and a live viewport cannot, and
    /// that is the whole of it. This used to be inferred from
    /// `fog_scale == 1`, which meant the same thing only for as long as
    /// a still was the only thing marching at full size: the moment
    /// every tier did, the whole ladder quietly inherited a still's
    /// step count and the studio ran at nine frames a second.
    pub fine_for_narrow_shafts: bool,
}

impl RenderQuality {
    /// What a headless still renders at: the quality every existing
    /// snapshot was made with.
    // r[impl viz.post-processing] - a still pays for everything
    pub const STILL: Self = Self {
        fog_steps: 192,
        // A still has all the time in the world for a narrow shaft.
        fine_for_narrow_shafts: true,
        froxel_samples: 4,
        froxel_history_pct: 85,
        froxel_grid: (640, 360, 128),
        deferred: true,
        // Unused at `fog_scale: 1`, which marches on the camera itself.
        haze_pixels: crate::haze::HAZE_PIXEL_BUDGET,
        // Off since the deferred deck and SSAO do not multisample; TAA
        // over the settle frames takes the edges instead.
        msaa: false,
        fog_scale: 1,
        taa: true,
        ssr: true,
        ssao: true,
        dof: true,
        instant_adaptation: true,
    };

    /// What a *second* view of the same room costs.
    ///
    /// The Programme pane and a canvas source are previews: a small
    /// picture of the scene the operator is already looking at. They
    /// rendered with the viewport's own dials, so each paid for
    /// screen-space reflections and ambient occlusion over a picture
    /// nobody inspects — and both are full-screen passes over their own
    /// target, so a small pane still pays a real share.
    ///
    /// What stays is as deliberate as what goes. `deferred` stays,
    /// because the deck is a deferred material and a preview without
    /// the room in it is not a preview — dropping it with `ssr` is
    /// exactly the mistake this exists to make impossible. Temporal
    /// anti-aliasing stays, because it is what averages the haze's
    /// per-frame jitter into a solid shaft, and beads on the Programme
    /// pane would be worse than reflections missing from it. The march
    /// stays too: the haze budget is already shared across cameras, so
    /// a preview's share shrinks without its quality being singled out.
    // r[impl viz.performance-budget] - a preview is not the viewport
    pub const fn preview(self) -> Self {
        Self {
            ssr: false,
            ssao: false,
            dof: false,
            deferred: true,
            ..self
        }
    }

    /// The dials for one rung of the ladder.
    ///
    /// What carries it is `fog_steps`: the raymarch costs that times
    /// the picture's pixels times the number of lights in the fog.
    /// `haze_pixels` used to carry the other half, by marching the fog
    /// on a smaller camera of its own, and every tier now marches on
    /// the camera itself instead — see `fog_scale` for what that path
    /// got wrong with more than one pane up. The tiers below are
    /// therefore steps-only for the moment, and each one costs more
    /// than it did.
    // r[impl viz.quality-presets]
    pub const fn preset(preset: Preset) -> Self {
        // Every tier below shares these: MSAA off (the deferred deck and
        // SSAO do not multisample), no depth of field (a blurred house
        // is a photograph, not an operator's view), and an eye's pace
        // for the exposure rather than a still's instant.
        let base = Self {
            msaa: false,
            dof: false,
            instant_adaptation: false,
            // No live tier takes the narrow-shaft raise. It is 256
            // steps at the picture's own size — twelve frames a second
            // at a studio pane, measured — and a rig with one beam
            // fixture in it is an ordinary rig, so this would be the
            // ordinary case. A dotted beam beats a slideshow; a still
            // still marches finely, where the time is free.
            fine_for_narrow_shafts: false,
            ..Self::STILL
        };
        match preset {
            // Enough steps to say there is a beam. It will dot; that
            // is the deal — and it dots harder now that the march is
            // at the picture's own size with no small buffer to hide
            // the spacing.
            Preset::Potato => Self {
                froxel_history_pct: 94,
                froxel_samples: 1,
                froxel_grid: (200, 112, 64),
                fog_steps: 12,
                haze_pixels: 160 * 240,
                fog_scale: 1,
                taa: false,
                ssr: false,
                ssao: false,
                ..base
            },
            Preset::Low => Self {
                froxel_history_pct: 92,
                froxel_samples: 2,
                froxel_grid: (280, 158, 80),
                fog_steps: 20,
                haze_pixels: 320 * 480,
                fog_scale: 1,
                taa: true,
                ssr: false,
                ssao: false,
                ..base
            },
            // 128, and it went to 96 for a while on the strength of a
            // comparison that did not cover the worst case.
            //
            // The rule is unchanged: the fewest steps that keep every
            // shaft a solid shaft rather than a string of beads. What
            // moved was the evidence. Sizing the dither properly
            // (`haze::fog_jitter_for`) made 96 look identical to 128 on
            // the beams that were compared — and those were all
            // diagonal.
            //
            // A beam pointing *straight up*, seen from the house, is
            // the case that decides this, and it is the one that was
            // not looked at. The camera ray crosses such a beam's
            // width rather than running along its length: over a
            // thirty-metre room a 96-step march is a step every 0.31 m,
            // so a beam a couple of decimetres across collects one
            // sample or none, and comes out as beads. At 128 the step
            // is 0.23 m and it holds.
            //
            // So the number goes back, and the eight frames a second it
            // was worth go with it. A quality dial justified against
            // the wrong picture is not justified.
            Preset::Medium => Self {
                froxel_history_pct: 90,
                froxel_samples: 2,
                froxel_grid: (360, 203, 96),
                fog_steps: 32,
                // The full budget — dormant while the march is on the
                // camera itself, and recorded because the reasoning
                // still holds for the day it comes back.
                //
                // Halving it is worth about ten per cent — 110 fps to
                // 124 on the benchmark cue — and it is available as
                // `IGNITION_HAZE_PIXELS=307200` for anyone who wants
                // it. It is not the default because of an artifact
                // that predates all of this: the fade at a wide beam's
                // far end carries a faint diamond lattice, which is the
                // raymarch's own noise pattern surviving the stretch,
                // and a coarser haze camera magnifies it. Halving the
                // budget does not create the lattice, it enlarges it,
                // and enlarging a defect is not a default.
                //
                // The lattice is the next real piece of work here — see
                // docs/ops/profiling.md. Fix it and this halves.
                haze_pixels: crate::haze::HAZE_PIXEL_BUDGET,
                fog_scale: 1,
                taa: true,
                ssr: true,
                ssao: true,
                ..base
            },
            Preset::High => Self {
                froxel_history_pct: 88,
                froxel_samples: 2,
                froxel_grid: (440, 248, 96),
                fog_steps: 48,
                haze_pixels: crate::haze::HAZE_PIXEL_BUDGET * 2,
                fog_scale: 1,
                taa: true,
                ssr: true,
                ssao: true,
                ..base
            },
            // The tier the others have now joined: marched on the
            // camera itself, at the picture's own size, with `for_rig`
            // free to raise the count for a narrow shaft.
            Preset::Ultra => Self {
                froxel_history_pct: 85,
                froxel_samples: 3,
                froxel_grid: (560, 315, 96),
                // The tier that spends everything a *live* view can
                // spend, which is not the same as everything. At 128
                // this was eighteen frames a second at a studio pane's
                // size, and at 128 with the narrow-shaft raise on top
                // it was eleven — a tier an operator can select and
                // find the studio unusable is not a quality setting.
                fog_steps: 64,
                haze_pixels: crate::haze::HAZE_PIXEL_BUDGET * 2,
                fog_scale: 1,
                taa: true,
                ssr: true,
                ssao: true,
                ..base
            },
        }
    }

    /// What a window or the studio renders at: a preset from
    /// `IGNITION_QUALITY`, with each dial still overridable on its own.
    ///
    /// The per-dial variables outrank the preset deliberately. A tier
    /// is for choosing a picture; `IGNITION_FOG_STEPS=96` is for
    /// finding out what a step costs on *this* GPU, and that is a
    /// different job that should not need a new tier invented for it.
    pub fn live() -> Self {
        let preset = Preset::from_env();
        let quality = Self::preset(preset);
        let number = |name: &str, fallback: u32| {
            std::env::var(name)
                .ok()
                .and_then(|v| v.trim().parse::<u32>().ok())
                .filter(|n| *n > 0)
                .unwrap_or(fallback)
        };
        // `IGNITION_TAA`, `IGNITION_SSR`, `IGNITION_SSAO` = 0 switch one
        // off, for measuring what each costs on a given GPU.
        let switch = |name: &str, fallback: bool| {
            std::env::var(name)
                .ok()
                .map(|v| v.trim() != "0")
                .unwrap_or(fallback)
        };
        // r[impl viz.post-processing] - the live picture keeps what fits the budget
        let live = Self {
            fog_steps: number("IGNITION_FOG_STEPS", quality.fog_steps),
            haze_pixels: number("IGNITION_HAZE_PIXELS", quality.haze_pixels),
            // `IGNITION_FOG_SCALE` forces a divisor; 0 is the budget.
            fog_scale: std::env::var("IGNITION_FOG_SCALE")
                .ok()
                .and_then(|v| v.trim().parse::<u32>().ok())
                .unwrap_or(quality.fog_scale),
            taa: switch("IGNITION_TAA", quality.taa),
            ssr: switch("IGNITION_SSR", quality.ssr),
            ssao: switch("IGNITION_SSAO", quality.ssao),
            ..quality
        };
        // Which tier, and what it actually resolved to after any
        // per-dial override — because "I set IGNITION_QUALITY and
        // nothing changed" is otherwise a guess, and a stale
        // `IGNITION_FOG_STEPS` in a shell is exactly how that happens.
        // r[impl viz.quality-presets]
        tracing::info!(
            preset = preset.name(),
            fog_steps = live.fog_steps,
            haze_pixels = live.haze_pixels,
            fog_scale = live.fog_scale,
            taa = live.taa,
            ssr = live.ssr,
            ssao = live.ssao,
            "viz: render quality"
        );
        live
    }

    /// The same dials, with the raymarch made fine enough for the rig
    /// in front of it. A narrow shaft — a mini gobo mover's 11 degrees,
    /// a beam fixture's 2 — is a few decimetres across for most of its
    /// length, and at a live step count the fog samples cross it only
    /// once or twice per pixel: the shaft comes out as a string of dots
    /// and rings instead of a solid cone. Only a rig that actually cuts
    /// such a shaft pays for the finer march; a room of pars keeps the
    /// live count.
    pub fn for_rig(self, venue: &Venue, gdtf: Option<&GdtfLibrary>) -> Self {
        // An explicit `IGNITION_FOG_STEPS` is the operator comparing
        // counts on their own GPU; the rig does not get to overrule it.
        let forced = std::env::var("IGNITION_FOG_STEPS")
            .ok()
            .and_then(|v| v.trim().parse::<u32>().ok())
            .filter(|n| *n > 0);
        // Only a tier that can afford it — see `fine_for_narrow_shafts`.
        let steps = match (forced, self.fine_for_narrow_shafts) {
            (Some(forced), _) => forced,
            (None, true) => {
                fog_steps_for(self.fog_steps, narrowest_shaft_full_angle_deg(venue, gdtf))
            }
            (None, false) => self.fog_steps,
        };
        Self {
            fog_steps: steps,
            ..self
        }
    }
}

/// The stage camera's EV100 with `VizConfig::exposure` at zero.
///
/// Lights now carry their real photometry (`spawn::spot_lumens` hands a
/// `SpotLight` the lumens an omnidirectional source of the fixture's
/// peak candela would have, nothing else), so the picture's level is
/// the camera's business, as it is in a venue: this is the exposure
/// that renders a 36 W par's 180 lux at 3 m as a soft wash on a black
/// deck and leaves a 150 W beam fixture saturating through its core.
/// Between Bevy's `EV100_INDOOR` (7) and a dim room; the old
/// `--exposure 12` multiplier on Bevy's Blender default (9.7) is
/// `9.7 - log2(12)`, which is where this number comes from.
// r[impl viz.exposure] - one EV100 for the stage
pub const STAGE_EV100: f32 = 6.1;

/// The camera's exposure: `STAGE_EV100` less `ev_offset` stops, so a
/// positive offset opens the camera up.
// r[impl viz.exposure] - --exposure is stops on the stage EV
pub fn stage_exposure(ev_offset: f32) -> bevy::camera::Exposure {
    bevy::camera::Exposure {
        ev100: STAGE_EV100 - ev_offset,
    }
}

/// How the exposure follows the frame: an eye, not a light meter.
///
/// Bevy's auto exposure aims the frame's average luminance at middle
/// grey, which for a black room with a band lit in the middle of it
/// means pinning the exposure at whatever its range allows: a dark
/// venue is *meant* to be dark. So the compensation curve does the
/// adapting instead: the exposure moves by `AUTO_EXPOSURE_GAIN` stops
/// per stop the frame's average sits from `CHORUS_LOG_LUMINANCE` — a
/// full chorus is the level the stage camera was set for and gets no
/// correction; a dark verse opens up part of the way toward it and a
/// blackout opens up `AUTO_EXPOSURE_RANGE_STOPS` and no further, so a
/// blackout stays black and a verse stays a verse.
///
/// The speeds are an eye's: a blackout into a full chorus is met in
/// about half a second; the chorus dropping to one par takes a couple
/// of seconds to open up to. A still has no time to adapt in, so
/// `instant` lands it on its level in one frame. The overlay is UI,
/// drawn after tonemapping, and is never metered.
// r[impl viz.post-processing] - the exposure adapts like an eye
pub fn auto_exposure(
    curves: &mut Assets<AutoExposureCompensationCurve>,
    instant: bool,
) -> AutoExposure {
    let range = AUTO_EXPOSURE_RANGE_STOPS;
    // Bevy steps the exposure by `speed * dt * delta / transition`
    // toward the target (`auto_exposure_step`), so `speed * dt` has to
    // stay below the transition distance or the step overshoots and
    // the exposure rings. Instant is a thousand stops a second against
    // a twenty-stop transition: half the gap per 10 ms frame, settled
    // well within a still's settle frames, and only stable up to a
    // 20 ms frame — a still's real frame time, never a video's, which
    // is why `run_export` adapts at the live speeds instead.
    let (speed_brighten, speed_darken, transition) = if instant {
        (
            AUTO_EXPOSURE_INSTANT_SPEED,
            AUTO_EXPOSURE_INSTANT_SPEED,
            AUTO_EXPOSURE_INSTANT_TRANSITION,
        )
    } else {
        (range / 2.0, range / 0.5, AUTO_EXPOSURE_LIVE_TRANSITION)
    };
    AutoExposure {
        // The histogram's span, in log2 luminance: everything a stage
        // can show, so no bin is ever dropped from the average.
        range: -10.0..=10.0,
        // Meter the middle of the histogram: the beam cores and the
        // black house are what an eye ignores.
        filter: 0.15..=0.85,
        speed_brighten,
        speed_darken,
        exponential_transition_distance: transition,
        compensation_curve: curves.add(adaptation_curve()),
        ..default()
    }
}

/// The compensation curve behind `auto_exposure`: Bevy's target
/// exposure is `compensation(average) - average`, so a line of slope
/// one through the chorus level is "no change", and this bends it by
/// `AUTO_EXPOSURE_GAIN` around that point, flat again once the
/// correction reaches `AUTO_EXPOSURE_RANGE_STOPS` either way.
pub fn adaptation_curve() -> AutoExposureCompensationCurve {
    let reach = AUTO_EXPOSURE_RANGE_STOPS / AUTO_EXPOSURE_GAIN;
    let points: Vec<Vec2> = [
        -10.0,
        CHORUS_LOG_LUMINANCE - reach,
        CHORUS_LOG_LUMINANCE,
        CHORUS_LOG_LUMINANCE + reach,
        10.0,
    ]
    .into_iter()
    .map(|x| Vec2::new(x, x + adaptation_stops(x)))
    .collect();
    AutoExposureCompensationCurve::from_curve(bevy::math::cubic_splines::LinearSpline::new(points))
        .expect("a monotonic line is a valid compensation curve")
}

/// How many stops the eye opens (positive) or closes for a frame whose
/// metered average is `log_luminance`, in log2 scene luminance at the
/// stage exposure.
// r[impl viz.post-processing] - the adaptation, as a number
pub fn adaptation_stops(log_luminance: f32) -> f32 {
    (-AUTO_EXPOSURE_GAIN * (log_luminance - CHORUS_LOG_LUMINANCE))
        .clamp(-AUTO_EXPOSURE_RANGE_STOPS, AUTO_EXPOSURE_RANGE_STOPS)
}

/// How far, in stops, the auto exposure may pull the picture from
/// `stage_exposure` either way.
pub const AUTO_EXPOSURE_RANGE_STOPS: f32 = 1.5;

/// The instant adaptation's speed in stops per second and the distance
/// over which it goes exponential: a still's settle frames land it.
const AUTO_EXPOSURE_INSTANT_SPEED: f32 = 1.0e3;
const AUTO_EXPOSURE_INSTANT_TRANSITION: f32 = 20.0;

/// How far from its target, in stops, a live adaptation eases in
/// exponentially rather than moving at full speed.
const AUTO_EXPOSURE_LIVE_TRANSITION: f32 = 0.5;

/// How long an export runs the clock before its first frame so the
/// exposure has adapted: the full range at the slower (brightening)
/// speed is a second and a third to the transition distance, and the
/// exponential tail inside it decays at `speed / transition` = 1.5/s,
/// another 2.6 seconds to a hundredth of a stop; four seconds covers
/// the worst case at any frame rate (the test walks them).
pub const AUTO_EXPOSURE_PREROLL_SECS: f32 = 4.0;

/// One frame of Bevy's adaptation (`auto_exposure.wgsl`): the exposure
/// moves toward `target` by `delta * speed * dt / transition`, capped at
/// `speed * dt` — the same arithmetic, so the speeds can be checked for
/// ringing without a GPU.
pub fn auto_exposure_step(
    exposure: f32,
    target: f32,
    speed_brighten: f32,
    speed_darken: f32,
    transition: f32,
    dt: f32,
) -> f32 {
    let delta = target - exposure;
    if target > exposure {
        let speed = speed_darken * dt;
        exposure + (delta * speed / transition).min(speed)
    } else {
        let speed = speed_brighten * dt;
        exposure + (delta * speed / transition).max(-speed)
    }
}

/// Stops of correction per stop the frame sits from the chorus level.
/// One would hold every look at the same brightness — a light meter,
/// not an eye; this leaves most of the show's dynamics in the picture.
pub const AUTO_EXPOSURE_GAIN: f32 = 0.5;

/// The metered log2 luminance of a full chorus at the stage exposure:
/// the level that gets no correction. Measured on *Bye Bye Bye*'s
/// chorus at Norco from the house.
pub const CHORUS_LOG_LUMINANCE: f32 = -3.4;

/// A look applied after tonemapping. `Neutral` is the tonemapper alone.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Grade {
    #[default]
    Neutral,
    /// Tungsten warmth, for a room lit by pars.
    Warm,
    /// A cool cast, for a rig that lives in blues.
    Cool,
    /// Saturation up: how a phone camera would show it.
    Punchy,
}

impl Grade {
    pub fn parse(name: &str) -> Option<Self> {
        Some(match name {
            "neutral" => Self::Neutral,
            "warm" => Self::Warm,
            "cool" => Self::Cool,
            "punchy" => Self::Punchy,
            _ => return None,
        })
    }

    pub fn color_grading(self) -> bevy::render::view::ColorGrading {
        use bevy::render::view::ColorGrading;
        let mut grading = ColorGrading::default();
        match self {
            Self::Neutral => {}
            Self::Warm => grading.global.temperature = 0.15,
            Self::Cool => grading.global.temperature = -0.15,
            Self::Punchy => {
                grading.global.post_saturation = 1.25;
                grading.midtones.contrast = 1.1;
            }
        }
        grading
    }
}

/// The main camera's spec, kept so a programme or source camera can be
/// spawned later with the same post-processing.
#[derive(Resource, Clone, Copy)]
pub struct MainCameraSpec(pub(crate) CameraSpec);

/// Everything a main camera is spawned from.
#[derive(Clone, Copy)]
pub(crate) struct CameraSpec {
    pub view: ViewPreset,
    pub free: Option<(Vec3, Vec3)>,
    pub min: Vec3,
    pub max: Vec3,
    pub quality: RenderQuality,
    pub ev_offset: f32,
    pub auto_exposure: bool,
    pub grade: Grade,
}

impl CameraSpec {
    pub fn new(config: &VizConfig, quality: RenderQuality) -> Self {
        let (min, max) = config.venue.bounds();
        Self {
            view: config.view,
            free: config.camera,
            min,
            max,
            quality,
            ev_offset: config.exposure,
            auto_exposure: config.auto_exposure,
            grade: config.grade,
        }
    }

    /// Where the camera looks: the stage centre for a preset, the
    /// operator's own target for a free camera. What depth of field
    /// focuses on.
    pub fn focus_distance(&self) -> f32 {
        let (eye, target) = match self.free {
            Some(pair) => pair,
            None => self.view.eye_target(self.min, self.max),
        };
        eye.distance(target).max(0.5)
    }
}

/// Full beam angles below this need the finer march — see
/// `RenderQuality::for_rig`.
pub const NARROW_BEAM_FULL_ANGLE_DEG: f32 = 15.0;

/// The step count a narrow shaft needs to read as a solid cone in the
/// studio viewport: twice the live count, which is where the dots go.
pub const NARROW_BEAM_FOG_STEPS: u32 = 256;

/// `base`, raised to `NARROW_BEAM_FOG_STEPS` when the rig's narrowest
/// shaft-cutting beam is under `NARROW_BEAM_FULL_ANGLE_DEG`. Never
/// lowered: a still's count is already above it.
pub fn fog_steps_for(base: u32, narrowest_shaft_full_angle_deg: Option<f32>) -> u32 {
    match narrowest_shaft_full_angle_deg {
        Some(angle) if angle < NARROW_BEAM_FULL_ANGLE_DEG => base.max(NARROW_BEAM_FOG_STEPS),
        _ => base,
    }
}

/// The full beam angle of the narrowest patched fixture bright enough
/// per-direction to cut a shaft (see `SHAFT_CANDELA_THRESHOLD`), if any.
///
/// The angle is the profile's where one resolves, the patch's only
/// otherwise — the same rule the emitters follow.
pub fn narrowest_shaft_full_angle_deg(venue: &Venue, gdtf: Option<&GdtfLibrary>) -> Option<f32> {
    use crate::fixture_profile::{
        LUMENS_PER_WATT, SHAFT_CANDELA_THRESHOLD, peak_candela, power_watts,
    };
    use crate::spawn::fixture_optics;
    venue
        .fixtures
        .iter()
        .filter(|f| f.patched)
        .filter_map(|f| {
            let manufacturer = f.manufacturer.as_deref().unwrap_or("");
            let model = f.model.as_deref().unwrap_or("");
            let profile = gdtf.and_then(|lib| lib.find(manufacturer, model));
            let half = fixture_optics(f, profile).beam_half_deg;
            let lumens = power_watts(manufacturer, model) * LUMENS_PER_WATT;
            (peak_candela(lumens, half) >= SHAFT_CANDELA_THRESHOLD).then_some(half * 2.0)
        })
        .min_by(|a, b| a.total_cmp(b))
}

/// The visualizer itself: resources, the venue spawn, and the per-frame
/// live update. Deliberately free of any window or camera setup, so the
/// windowed and headless paths can each supply their own render target
/// and otherwise share everything.
pub struct VizPlugin {
    pub config: VizConfig,
    pub dmx: DmxUniverses,
    /// Taken on `build`, since `Plugin::build` only gets `&self` and a
    /// parsed GDTF library is neither cloneable nor cheap. A `Mutex`
    /// rather than a `Cell` because a `Plugin` must be `Sync`.
    pub gdtf: Mutex<Option<GdtfLibrary>>,
    /// The transmitter, taken on `build` for the same reason — a
    /// `Sender` owns threads and sockets and is not cloneable. `None`
    /// means a viz that sends nothing (a snapshot, an export).
    pub output: Mutex<Option<DmxOutput>>,
}

/// Binds the venue's output config, enabled per `config.output`, with
/// the loopback sink when asked for. Errors land on the resource, to be
/// shown; nothing here can fail the launch.
// r[impl dmx.venue-config] - bound from the venue, at load
pub fn bind_output(config: &VizConfig, dmx: &DmxUniverses) -> DmxOutput {
    let source = "Ignition".to_string();
    let output = DmxOutput::bind(&config.venue.output_config(), &source, config.output);
    if crate::output::loopback_requested(config.loopback) {
        tracing::info!("dmx output: loopback sink attached");
        output.with_sink(Box::new(crate::output::LoopbackSink(dmx.clone())))
    } else {
        output
    }
}

impl Plugin for VizPlugin {
    fn build(&self, app: &mut App) {
        // The spot shadow map's size, and it is a *beam* setting as much
        // as a shadow one.
        //
        // The volumetric fog samples each shadowed light's map at every
        // raymarch step, so whatever that map cannot resolve becomes
        // structure in the shaft — and structure the march's own step
        // count and dither cannot touch, because it is not the march's.
        // Unreal documents the same coupling: "the resolution of the
        // volumetric light shaft is directly related to the resolution
        // of the shadow map of that light".
        //
        // `IGNITION_SHADOW_MAP` sizes it; Bevy's default is 1024.
        // r[impl viz.performance-budget] - the shaft is only as smooth as the map it samples
        if let Some(size) = std::env::var("IGNITION_SHADOW_MAP")
            .ok()
            .and_then(|v| v.trim().parse::<usize>().ok())
            .filter(|n| *n >= 256)
        {
            app.insert_resource(bevy::light::PointLightShadowMap { size });
        }
        // The main camera's spec, so a programme or source camera can be
        // spawned later with the same post-processing. Computed in its
        // own statement: the builder chain below takes the GDTF library
        // out of the same mutex, and a guard created inside a chain
        // argument lives to the end of the whole chain — a second lock
        // in the chain deadlocked the studio at startup.
        let spec = {
            let gdtf = self.gdtf.lock().expect("gdtf lock");
            CameraSpec::new(
                &self.config,
                self.config
                    .quality
                    .for_rig(&self.config.venue, gdtf.as_ref()),
            )
        };
        app.insert_resource(ClearColor(Color::BLACK))
            .insert_resource(VenueRes(self.config.venue.clone()))
            .insert_resource(DmxRes(self.dmx.clone()))
            .insert_resource(VizSettings {
                haze: self.config.haze,
                ambient: self.config.ambient,
                show_props: self.config.show_props,
                overlay: self.config.overlay,
                fps: self.config.fps,
                exclude: self.config.exclude.clone(),
                body_glow: self.config.body_glow,
                reflective_deck: self.config.quality.ssr,
                screen_content: self.config.screen_content.clone(),
                canvas_content: self.config.canvas_content.clone(),
                canvas_focus: self.config.canvas_focus.clone(),
                assets_dir: self.config.assets_dir.clone(),
            })
            // Free-running by default, which is what the standalone
            // binary wants; a host driving the visualizer from a
            // transport overwrites it every frame. See `CanvasClock`.
            .init_resource::<CanvasClock>()
            .insert_resource(GdtfLibraryRes(
                self.gdtf.lock().expect("gdtf library lock").take(),
            ))
            // r[impl studio.program.pick-and-gizmos] - picking and the programmer's overlays
            // Off unless asked: a still is a picture of the rig, and the
            // studio switches the overlays on when its Program view shows.
            .insert_resource(crate::gizmos::ProgramOverlays {
                labels: self.config.labels,
                program: self.config.labels,
                ..Default::default()
            })
            .add_plugins((
                crate::picking::FixturePickingPlugin,
                crate::gizmos::ProgramGizmosPlugin,
            ))
            .add_plugins(crate::haze::HazePlugin)
            // GLB fixture parts: the placeholder-to-scene swap.
            .add_plugins(crate::gdtf_assets::GdtfAssetsPlugin)
            // `proc:` canvases, evaluated on the GPU.
            .add_plugins(crate::canvas_material::CanvasMaterialPlugin)
            .add_plugins(crate::gobo::GoboPlugin)
            .add_plugins(crate::haze_texture::HazeTexturePlugin)
            .add_plugins(SolariIfEnabled)
            // Not in `DefaultPlugins`: the eye that follows the frame.
            .add_plugins(bevy::post_process::auto_exposure::AutoExposurePlugin)
            // Room for the whole rig's lights before the GPU clustering
            // buffers have to grow. Bevy resizes them when they overflow
            // and says so — "the scene lighting may have been corrupted
            // for a few frames" — which on a venue with seventy-odd
            // fixtures happens on the first busy cue. Sized up front, so
            // the corruption never happens rather than happening once.
            .init_resource::<LiveDmx>()
            .insert_resource(
                self.output
                    .lock()
                    .expect("dmx output lock")
                    .take()
                    .unwrap_or_default(),
            )
            .add_systems(Startup, widen_light_clusters)
            // The programme camera: presets, slots and cue cuts; the
            // programme view and cameras as canvas sources.
            .insert_resource(crate::camera::active_from_config(&self.config))
            .insert_resource(MainCameraSpec(spec))
            .insert_resource(crate::camera::ProgrammeView {
                target: Handle::default(),
                size: crate::camera::SOURCE_SIZE,
                host_wants: false,
                canvas_wants: false,
                camera: None,
            })
            .init_resource::<crate::camera::CameraSources>()
            .init_resource::<crate::camera::CanvasSwitches>()
            .add_systems(Startup, allocate_programme_target.before(spawn_venue))
            .add_systems(
                Update,
                (
                    crate::camera::cue_camera_commands,
                    crate::camera::camera_keys,
                    crate::camera::apply_canvas_switches,
                    crate::camera::manage_camera_views,
                    crate::camera::drive_camera,
                )
                    .chain()
                    .after(tick_playback),
            )
            // Frame timing for the overlay's FPS readout. Bevy's own
            // plugin rather than a hand-rolled delta average, because it
            // keeps a smoothed history — a per-frame reciprocal flickers
            // too much to read, and the question is whether the rig
            // *holds* a rate, not what one frame did.
            .add_plugins(bevy::diagnostic::FrameTimeDiagnosticsPlugin::default())
            .add_systems(Startup, spawn_venue)
            // Playback runs before the fixture update so a beam reflects
            // this frame's cue state, not last frame's.
            .add_systems(
                Update,
                (
                    apply_ambient,
                    operator_keys,
                    tick_playback,
                    // The frame goes out from the same bytes the next
                    // system decodes — after the encoder, before the
                    // picture.
                    crate::output::send_output,
                    resolve_live_dmx,
                    update_live_fixtures,
                    update_fixture_bodies,
                )
                    .chain(),
            )
            .add_systems(Update, crate::props::pose_new_characters)
            // Not in the chain above: a canvas frame does not depend on
            // anything a fixture did this frame, and holding up the rig
            // for a texture upload is the one thing the whole video path
            // is built to avoid.
            .add_systems(Update, update_canvas_videos)
            // Camera targeting covers both readouts, so it is gated on
            // either being on — not on `overlay`, or an fps-only studio
            // would spawn the text and then draw it nowhere.
            .add_systems(
                Update,
                crate::overlay::target_overlay_camera
                    .run_if(|s: Res<VizSettings>| s.overlay || s.fps),
            )
            .add_systems(
                Update,
                crate::overlay::update_overlay
                    .after(crate::overlay::target_overlay_camera)
                    .run_if(|s: Res<VizSettings>| s.overlay),
            )
            .add_systems(
                Startup,
                crate::overlay::spawn_overlay.run_if(|s: Res<VizSettings>| s.overlay),
            )
            .add_systems(
                Update,
                crate::overlay::update_fps.run_if(|s: Res<VizSettings>| s.fps),
            )
            .add_systems(
                Startup,
                crate::overlay::spawn_fps.run_if(|s: Res<VizSettings>| s.fps),
            )
            // After transform propagation, because a beam's world pose is
            // whatever the joints `update_live_fixtures` just moved ended
            // up producing — see `update_beams`.
            .add_systems(
                PostUpdate,
                update_beams.after(bevy::transform::TransformSystems::Propagate),
            );
    }
}

/// `playback` is inserted as a resource rather than carried on the plugin
/// because `Plugin::build` only gets `&self`, and the players are not
/// cloneable — they hold live fade state.
pub fn run(config: VizConfig, playback: Playback, gdtf: Option<GdtfLibrary>) {
    let dmx = DmxUniverses::new();
    dmx::spawn_sacn_listener(dmx.clone(), config.max_universe);
    dmx::spawn_artnet_listener(dmx.clone());
    let output = bind_output(&config, &dmx);

    match config.snapshot.clone() {
        Some(path) => run_snapshot(config, dmx, playback, gdtf, &path),
        None => run_windowed(config, dmx, playback, gdtf, output),
    }
}

fn run_windowed(
    config: VizConfig,
    dmx: DmxUniverses,
    playback: Playback,
    gdtf: Option<GdtfLibrary>,
    output: DmxOutput,
) {
    let quality = config.quality.for_rig(&config.venue, gdtf.as_ref());
    let spec = CameraSpec::new(&config, quality);
    let (width, height) = (config.width, config.height);
    let assets_dir = config.assets_dir.clone();

    App::new()
        // Before `DefaultPlugins`: an asset source must exist before the
        // asset server is built. See `gdtf_assets`.
        .add_plugins(crate::gdtf_assets::GdtfSourcePlugin)
        .add_plugins(
            DefaultPlugins
                .set(AssetPlugin {
                    file_path: assets_dir.clone(),
                    ..default()
                })
                .set(WindowPlugin {
                    primary_window: Some(Window {
                        title: "Ignition — visualizer".into(),
                        resolution: (width, height).into(),
                        ..default()
                    }),
                    ..default()
                }),
        )
        .add_plugins(VizPlugin {
            config,
            dmx,
            gdtf: Mutex::new(gdtf),
            output: Mutex::new(Some(output)),
        })
        .insert_resource(playback)
        .add_systems(
            Startup,
            move |mut commands: Commands,
                  mut curves: ResMut<Assets<AutoExposureCompensationCurve>>| {
                spawn_camera(&mut commands, &mut curves, spec);
            },
        )
        .run();
}

/// A headless app rendering into an offscreen image target, finished
/// and cleaned up, its sub-apps handed back for a loop to pump by hand.
/// Shared by the snapshot and the export.
pub(crate) struct Headless {
    pub(crate) subapps: SubApps,
    target: RenderTarget,
}

impl Headless {
    pub(crate) fn build(
        config: VizConfig,
        dmx: DmxUniverses,
        playback: Playback,
        gdtf: Option<GdtfLibrary>,
    ) -> Self {
        let quality = config.quality.for_rig(&config.venue, gdtf.as_ref());
        let spec = CameraSpec::new(&config, quality);
        let (width, height) = (config.width, config.height);

        let assets_dir = config.assets_dir.clone();
        let mut app = App::new();
        app.add_plugins(crate::gdtf_assets::GdtfSourcePlugin);
        app.add_plugins(
            DefaultPlugins
                .set(AssetPlugin {
                    file_path: assets_dir,
                    ..default()
                })
                .set(WindowPlugin {
                    // A lot of Bevy still expects the plugin to be present,
                    // just with nothing to show.
                    primary_window: None,
                    exit_condition: ExitCondition::DontExit,
                    ..default()
                })
                .set(RenderPlugin {
                    // Otherwise the first frames render with pipelines that
                    // have not finished compiling, and a snapshot can catch
                    // a scene mid-warm-up.
                    synchronous_pipeline_compilation: true,
                    ..default()
                })
                .disable::<WinitPlugin>(),
        )
        .add_plugins(VizPlugin {
            config,
            dmx,
            gdtf: Mutex::new(gdtf),
            // A still or an export sends nothing: there is no rig to
            // drive from a frame rendered offline.
            output: Mutex::new(None),
        })
        .insert_resource(playback);

        let mut target = Image::new_uninit(
            Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            TextureDimension::D2,
            TextureFormat::Rgba8UnormSrgb,
            RenderAssetUsages::RENDER_WORLD,
        );
        target.texture_descriptor.usage |= TextureUsages::RENDER_ATTACHMENT;
        let target: RenderTarget = app
            .world_mut()
            .resource_mut::<Assets<Image>>()
            .add(target)
            .into();

        let camera_target = target.clone();
        app.add_systems(
            Startup,
            move |mut commands: Commands,
                  mut curves: ResMut<Assets<AutoExposureCompensationCurve>>| {
                // `RenderTarget` is its own component — adding it to the
                // camera is what points it at the offscreen image instead
                // of a window.
                let camera = spawn_camera(&mut commands, &mut curves, spec);
                commands.entity(camera).insert(camera_target.clone());
            },
        );

        // No `run()`: the schedule runner is replaced by the caller's
        // loop, so the app has to be finished and cleaned up by hand.
        app.finish();
        app.cleanup();
        let subapps: SubApps = std::mem::take(app.sub_apps_mut());
        Self { subapps, target }
    }

    /// One frame, waited for on the GPU so a capture requested this
    /// frame is complete before the next one starts.
    pub(crate) fn step(&mut self) {
        self.subapps.update();
        self.subapps
            .main
            .world()
            .resource::<RenderDevice>()
            .wgpu_device()
            .poll(PollType::Wait {
                submission_index: None,
                timeout: None,
            })
            .expect("polling the render device");
    }

    pub(crate) fn target_image(&self) -> Handle<Image> {
        self.target
            .as_image()
            .expect("offscreen target is an image")
            .clone()
    }
}

/// No window, no winit — an offscreen image target, the loop pumped by
/// hand, and the GPU polled between frames.
fn run_snapshot(
    config: VizConfig,
    dmx: DmxUniverses,
    playback: Playback,
    gdtf: Option<GdtfLibrary>,
    path: &Path,
) {
    let settle_frames = config.settle_frames.max(1);
    let mut headless = Headless::build(config, dmx, playback, gdtf);
    for _ in 0..settle_frames {
        headless.step();
    }
    let out = path.to_path_buf();
    let image = headless.target_image();
    headless
        .subapps
        .main
        .world_mut()
        .spawn(Screenshot::image(image))
        .observe(save_to_disk(out));
    // The capture is queued on the render world and the PNG encode runs
    // on a task pool thread, so the app has to keep stepping for a few
    // frames afterwards, and then outlive the encode itself — dropping
    // `subapps` takes the task pools down with it.
    for _ in 0..8 {
        headless.step();
    }
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while !path.exists() && std::time::Instant::now() < deadline {
        headless.step();
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    if !path.exists() {
        eprintln!("viz: snapshot never reached {}", path.display());
    }
}

/// Renders the show to a video file, frame by frame against the song's
/// clock: Bevy's time is stepped by exactly one frame per update
/// (`TimeUpdateStrategy::ManualDuration`), so the playback advances the
/// same amount every frame however long the GPU took, and each frame is
/// captured and handed to the sink before the next is started. The
/// playback should already be located at the export's first bar
/// (`Playback::load` with `bar`) and carry the song's BPM.
///
/// Like `run`, this owns the thread until the export is finished.
// r[impl viz.export] - offline, frame by frame, at a chosen size
pub fn run_export(
    config: VizConfig,
    playback: Playback,
    gdtf: Option<GdtfLibrary>,
    request: &ExportRequest,
) -> anyhow::Result<()> {
    let bpm = playback.speeds.get("Song").copied().unwrap_or(120.0) as f64;
    let schedule = FrameSchedule::new(request, bpm);
    let (width, height) = (config.width, config.height);
    let settle_frames = config.settle_frames.max(1);
    let mut sink = export::open_sink(request, width, height)?;
    let frame_count = schedule.frame_count();
    println!(
        "exporting bars {}..{} at {} fps: {} frames of {}x{}",
        schedule.from_bar, schedule.to_bar, schedule.fps, frame_count, width, height
    );

    let dmx = DmxUniverses::new();
    // A video adapts like an eye, not a still: the instant speeds are
    // set for a still's real 10 ms frames, and at a video's 33 ms
    // frame they overshoot the level and ring, so each frame comes out
    // alternately blown out and black. The live speeds converge at any
    // frame rate a video is exported at (`auto_exposure_step` tests).
    // r[impl viz.post-processing] - an export adapts at an eye's pace
    let mut config = config;
    config.quality.instant_adaptation = false;
    let mut headless = Headless::build(config, dmx, playback, gdtf);
    let dt = Duration::from_secs_f64(schedule.dt());
    // Pre-roll: the song held at frame 0 while the clock runs at the
    // export's frame time, so the exposure has adapted to the first
    // frame's look — Bevy's auto exposure steps by `speed * dt`, and
    // with the clock stopped it would never move at all — and the
    // temporal passes have settled, before frame 0 is written.
    let first = schedule.frames().next();
    let preroll =
        settle_frames.max((AUTO_EXPOSURE_PREROLL_SECS * schedule.fps as f32).ceil() as u32);
    for _ in 0..preroll {
        let world = headless.subapps.main.world_mut();
        if let Some(frame) = &first {
            follow_position(world, frame.position, song_secs(&schedule, frame.position));
        }
        world.insert_resource(TimeUpdateStrategy::ManualDuration(dt));
        headless.step();
    }

    let (tx, rx) = std::sync::mpsc::channel::<Image>();
    for frame in schedule.frames() {
        // Frame 0 renders the located moment; every later frame first
        // advances the clock by one dt.
        let advance = if frame.index == 0 { Duration::ZERO } else { dt };
        let image = headless.target_image();
        let world = headless.subapps.main.world_mut();
        // The song drives the show: this frame's musical position lands
        // the clocked cues (and their `camera …` commands) and the hits,
        // exactly as the studio's transport does frame by frame.
        // r[impl viz.export] - the cues follow the song, so the cut does too
        // r[impl viz.camera-cuts] - an export renders the cut programme
        follow_position(world, frame.position, song_secs(&schedule, frame.position));
        world.insert_resource(TimeUpdateStrategy::ManualDuration(advance));
        let tx = tx.clone();
        world
            .spawn(Screenshot::image(image))
            .observe(move |captured: On<ScreenshotCaptured>| {
                let _ = tx.send(captured.image.clone());
            });
        // The capture completes on the render world a frame or two
        // later; keep stepping (with the clock held) until it lands so
        // frames reach the sink in order.
        let mut captured = None;
        for _ in 0..32 {
            headless.step();
            headless
                .subapps
                .main
                .world_mut()
                .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::ZERO));
            if let Ok(image) = rx.try_recv() {
                captured = Some(image);
                break;
            }
        }
        let Some(image) = captured else {
            anyhow::bail!("frame {} was never captured", frame.index);
        };
        let rgba = image
            .try_into_dynamic()
            .map_err(|e| anyhow::anyhow!("frame {}: {e:?}", frame.index))?
            .to_rgba8();
        sink.push(&frame, rgba.as_raw(), rgba.width(), rgba.height())?;
        if frame.index % schedule.fps == 0 {
            println!(
                "  frame {}/{} — bar {} beat {:.2}",
                frame.index, frame_count, frame.position.bar, frame.position.beat
            );
        }
    }
    sink.finish()?;
    println!("export finished: {}", request.path.display());
    Ok(())
}

/// Seconds into the song at a musical position, on the export's tempo.
fn song_secs(schedule: &FrameSchedule, position: ignition_core::Bars) -> f32 {
    ((position.bar.max(1) - 1) as f64 * schedule.secs_per_bar()
        + (position.beat - 1.0).max(0.0) * 60.0 / schedule.bpm) as f32
}

/// Points the song list and the hits at a musical position — what the
/// studio's `follow_song` does every frame from its transport, done
/// here from the export's own schedule.
fn follow_position(world: &mut World, position: ignition_core::Bars, secs: f32) {
    use ignition_core::{Class, Show};
    let Some(mut playback) = world.get_resource_mut::<Playback>() else {
        return;
    };
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
        triggers,
        ..
    } = &mut *playback;
    triggers.advance(position, secs);
    if let Some(player) = playbacks.of_class(Class::Song) {
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
        player.set_clock(secs);
        player.seek(position, &show);
    }
}

/// The camera every mode uses: HDR so bloom has something above white to
/// work with, a tonemapper that desaturates as it clips, and the preset's
/// own framing.
/// The programme camera's texture, allocated before the venue spawns so
/// a `camera:programme` canvas can bind it at spawn.
fn allocate_programme_target(
    mut images: ResMut<Assets<Image>>,
    mut programme: ResMut<crate::camera::ProgrammeView>,
) {
    programme.target = images.add(crate::camera::camera_target(programme.size));
}

/// Grows the clustering buffers past what this rig will ever need.
///
/// A Startup system rather than an inserted resource because the plugin
/// builds `GlobalClusterSettings` from the device's own capabilities —
/// whether storage buffers and GPU clustering are supported at all — and
/// replacing it wholesale would mean deciding those here, wrongly, on
/// hardware this code cannot see.
fn widen_light_clusters(mut settings: ResMut<bevy::light::cluster::GlobalClusterSettings>) {
    if let Some(gpu) = settings.gpu_clustering.as_mut() {
        gpu.initial_z_slice_list_capacity = gpu.initial_z_slice_list_capacity.max(4096);
        gpu.initial_index_list_capacity = gpu.initial_index_list_capacity.max(16384);
    }
}

/// Spawns a main camera from `spec`, with every post-process the
/// quality asks for. The optional extras go on afterwards, so the
/// bundle itself stays one static tuple.
// r[impl viz.post-processing] - each feature is a RenderQuality switch
/// Solari raytraced lighting, built only with `--features solari` —
/// see `solari.rs`. In the default build this plugin adds nothing.
struct SolariIfEnabled;

impl Plugin for SolariIfEnabled {
    #[cfg(feature = "solari")]
    fn build(&self, app: &mut App) {
        app.add_plugins(crate::solari::SolariPlugin);
    }
    #[cfg(not(feature = "solari"))]
    fn build(&self, _app: &mut App) {}
}

/// `IGNITION_FROXEL_GRID=WxHxD`, for finding the right grid by eye.
pub fn froxel_grid_override() -> Option<(u32, u32, u32)> {
    let raw = std::env::var("IGNITION_FROXEL_GRID").ok()?;
    let mut parts = raw.split(['x', 'X']).map(|n| n.trim().parse::<u32>());
    let (x, y, z) = (
        parts.next()?.ok()?,
        parts.next()?.ok()?,
        parts.next()?.ok()?,
    );
    Some((x.max(1), y.max(1), z.max(1)))
}

/// Whether the froxel volumetric path replaces the screen-space march.
///
/// **Off.** The march is the picture that ships. The grid is roughly
/// three times cheaper and does not bead a beam, and it carries gobos
/// and shadows in the air, but as it stands it renders a room with no
/// beams in it at all — `beam-tests.json` shows that plainly against
/// the march, and the cause is not yet found. A path that is cheaper
/// and wrong is not a default.
///
/// `IGNITION_FROXEL=1` selects it, which is how the regression is
/// being chased. See docs/domain/froxel-volumetrics.md.
// r[impl viz.haze-is-volumetric] - the march by default, IGNITION_FROXEL=1 for froxels
pub fn froxels_wanted() -> bool {
    std::env::var("IGNITION_FROXEL").is_ok_and(|v| v.trim() == "1")
}

pub(crate) fn spawn_camera(
    commands: &mut Commands,
    curves: &mut Assets<AutoExposureCompensationCurve>,
    spec: CameraSpec,
) -> Entity {
    use bevy::anti_alias::taa::TemporalAntiAliasing;
    use bevy::core_pipeline::prepass::{DeferredPrepass, DepthPrepass, NormalPrepass};
    use bevy::pbr::{ScreenSpaceAmbientOcclusion, ScreenSpaceReflections};
    use bevy::post_process::dof::{DepthOfField, DepthOfFieldMode};

    let CameraSpec {
        view,
        free,
        min,
        max,
        quality,
        ev_offset,
        auto_exposure: adapt,
        grade,
    } = spec;
    let mut camera = commands.spawn((
        Camera3d::default(),
        crate::camera::MainCamera,
        if quality.msaa {
            Msaa::default()
        } else {
            Msaa::Off
        },
        // Where shadow level-of-detail is measured from. Bevy warns when
        // point or spot lights exist and nothing declares one — which is
        // every frame here, since a lighting rig is nothing but point and
        // spot lights. Putting it on the camera is the intended answer:
        // detail should fall off with distance from the viewer.
        bevy::camera::ShadowLodOrigin,
        // Without HDR the additive beam material clips at 1.0 and there
        // is nothing left above white for bloom to find — which is most
        // of what makes a beam read as light rather than as a shape.
        Hdr,
        // The lights are real photometry; this is the camera set for
        // the stage, and the operator's `--exposure` is stops on it.
        stage_exposure(ev_offset),
        Projection::Perspective(PerspectiveProjection {
            fov: view.fov_y_deg().to_radians(),
            near: 0.05,
            far: view.far(min, max),
            ..default()
        }),
        // Desaturates toward white as it clips, which is what stops a
        // saturated red wash from flattening into one flat red — the
        // exact failure the hand-written ACES pass produced.
        Tonemapping::TonyMcMapface,
        grade.color_grading(),
        Bloom::NATURAL,
        match free {
            Some((eye, target)) => ViewPreset::free_transform(eye, target),
            None => view.transform(min, max),
        },
        // The haze: marched on this camera at full size for a still, or
        // on a camera of its own at a fraction of the size for a live
        // view — `haze.rs` reads this and sets up whichever. The step
        // count is per pixel; a still goes well above the default 64
        // because a beam crossing a 20 m room at the default spacing
        // comes out in stair-steps, and a live view pays for fewer
        // pixels instead.
        crate::haze::HazeView {
            fog_steps: quality.fog_steps,
            scale: quality.fog_scale,
            pixels: quality.haze_pixels,
        },
    ));
    // The froxel path replaces the screen-space march rather than
    // joining it: with both running the beams are drawn twice and
    // neither can be judged. `HazeView` is what sets the march up, so
    // taking it off is how the old path is turned off.
    // r[impl viz.haze-is-volumetric] - froxels, behind IGNITION_FROXEL
    if froxels_wanted() {
        // The preset sizes the grid; `IGNITION_FROXEL_GRID` still
        // outranks it, the way every other dial here works.
        let (x, y, z) = froxel_grid_override().unwrap_or(quality.froxel_grid);
        camera.remove::<crate::haze::HazeView>();
        camera.insert(bevy::pbr::froxel::FroxelVolumetrics {
            dimensions: bevy::math::UVec3::new(x, y, z),
            samples: std::env::var("IGNITION_FROXEL_SAMPLES")
                .ok()
                .and_then(|v| v.trim().parse().ok())
                .unwrap_or(quality.froxel_samples),
            history_weight: std::env::var("IGNITION_FROXEL_HISTORY")
                .ok()
                .and_then(|v| v.trim().parse::<f32>().ok())
                .unwrap_or(quality.froxel_history_pct as f32 / 100.0),
            dilution: crate::haze_texture::HazeLook::default().dilution,
            ..bevy::pbr::froxel::FroxelVolumetrics::default()
        });
    }
    if adapt {
        camera.insert(auto_exposure(curves, quality.instant_adaptation));
    }
    if quality.taa && !quality.msaa {
        camera.insert(TemporalAntiAliasing::default());
    }
    // The deck is the one deferred material (`spawn::deck`), so the
    // prepasses are not optional wherever it has to appear — the
    // reflections that march against them are.
    //
    // These used to be one switch, and that is a bug waiting to be
    // found: turning `ssr` off to make a preview camera cheaper took
    // `DeferredPrepass` with it, and the pane rendered nothing at all.
    // A camera can want the deferred path and not want reflections;
    // saying so takes two flags.
    // r[impl viz.post-processing] - the deferred path and the reflections are separate costs
    if quality.ssr || quality.deferred {
        camera.insert((DepthPrepass, DeferredPrepass));
    }
    if quality.ssr {
        camera.insert(ScreenSpaceReflections {
            // A wet deck, not a mirror: the reflection fades out
            // toward the deck's own roughness.
            max_perceptual_roughness: 0.3..0.45,
            ..default()
        });
    }
    if quality.ssao {
        camera.insert((
            DepthPrepass,
            NormalPrepass,
            ScreenSpaceAmbientOcclusion {
                quality_level: bevy::pbr::ScreenSpaceAmbientOcclusionQualityLevel::Medium,
                ..default()
            },
        ));
    }
    if quality.dof {
        camera.insert(DepthOfField {
            mode: DepthOfFieldMode::Bokeh,
            focal_distance: spec.focus_distance(),
            // Open enough to soften the far wall behind the band, not
            // so open the drummer goes with it.
            aperture_f_stops: 2.0,
            ..default()
        });
    }
    camera.id()
}

#[cfg(test)]
mod quality_tests {
    use super::*;

    /// The stage EV100 reproduces the old picture: the `--exposure 12`
    /// multiplier on Bevy's Blender default, now on the camera.
    // r[verify viz.exposure] - one EV100 for the stage, stops on it
    #[test]
    fn the_stage_exposure_is_the_old_multiplier_on_the_camera() {
        let stage = stage_exposure(0.0).exposure();
        let old = bevy::camera::Exposure::BLENDER.exposure() * 12.0;
        assert!((stage / old - 1.0).abs() < 0.1, "{stage} vs {old}");
        // A stop is a doubling, either way.
        assert!((stage_exposure(1.0).exposure() / stage - 2.0).abs() < 1e-3);
        assert!((stage_exposure(-1.0).exposure() / stage - 0.5).abs() < 1e-3);
    }

    /// A chorus gets no correction, a darker frame opens part of the
    /// way, and a blackout no further than the range.
    // r[verify viz.post-processing] - the eye, as a number
    #[test]
    fn the_adaptation_opens_up_to_a_dark_frame_and_no_further() {
        assert_eq!(adaptation_stops(CHORUS_LOG_LUMINANCE), 0.0);
        let verse = adaptation_stops(CHORUS_LOG_LUMINANCE - 1.0);
        assert!((verse - AUTO_EXPOSURE_GAIN).abs() < 1e-6, "{verse}");
        assert_eq!(
            adaptation_stops(CHORUS_LOG_LUMINANCE - 20.0),
            AUTO_EXPOSURE_RANGE_STOPS
        );
        assert_eq!(
            adaptation_stops(CHORUS_LOG_LUMINANCE + 20.0),
            -AUTO_EXPOSURE_RANGE_STOPS
        );
        // And the curve behind it builds.
        let _ = adaptation_curve();
    }

    /// Live and still say, for each feature, whether it is on.
    /// Steps `auto_exposure_step` from `start` toward `target` at `dt`,
    /// failing on the first step that crosses the target (ringing), and
    /// returns the frames it took to get within a hundredth of a stop.
    fn adapt_without_ringing(start: f32, target: f32, speeds: (f32, f32, f32), dt: f32) -> u32 {
        let (brighten, darken, transition) = speeds;
        let mut exposure = start;
        for frame in 0..10_000 {
            if (target - exposure).abs() < 0.01 {
                return frame;
            }
            let next = auto_exposure_step(exposure, target, brighten, darken, transition, dt);
            assert!(
                (target - next) * (target - start) >= 0.0,
                "rang past {target} at frame {frame}: {exposure} -> {next} (dt {dt})"
            );
            exposure = next;
        }
        panic!("never adapted at dt {dt}");
    }

    // r[verify viz.post-processing] - an export adapts at an eye's pace, without ringing
    #[test]
    fn the_export_adaptation_converges_at_every_video_frame_rate() {
        let range = AUTO_EXPOSURE_RANGE_STOPS;
        let live = (range / 2.0, range / 0.5, AUTO_EXPOSURE_LIVE_TRANSITION);
        for fps in [24u32, 25, 30, 48, 50, 60, 120] {
            let dt = 1.0 / fps as f32;
            for (start, target) in [(0.0, range), (0.0, -range), (range, 0.0), (-0.3, 0.2)] {
                let frames = adapt_without_ringing(start, target, live, dt);
                assert!(
                    frames as f32 * dt <= AUTO_EXPOSURE_PREROLL_SECS,
                    "{start} -> {target} took {frames} frames at {fps} fps, past the pre-roll"
                );
            }
        }
    }

    #[test]
    fn the_instant_adaptation_rings_at_a_video_frame_but_not_at_a_stills() {
        let instant = (
            AUTO_EXPOSURE_INSTANT_SPEED,
            AUTO_EXPOSURE_INSTANT_SPEED,
            AUTO_EXPOSURE_INSTANT_TRANSITION,
        );
        // A still's 10 ms frame: half the gap a frame, settled in 20.
        let frames = adapt_without_ringing(0.0, 1.5, instant, 0.010);
        assert!(frames <= 20, "{frames}");
        // A 30 fps video frame: the first step overshoots (the bug).
        let next = auto_exposure_step(0.0, 1.5, instant.0, instant.1, instant.2, 1.0 / 30.0);
        assert!(next > 1.5, "{next}");
    }

    // r[verify viz.post-processing] - each feature is a RenderQuality switch
    #[test]
    fn a_still_pays_for_everything_and_a_live_view_for_what_fits() {
        let still = RenderQuality::STILL;
        assert!(still.taa && still.ssr && still.ssao && still.dof && still.instant_adaptation);
        assert!(!still.msaa, "TAA, deferred and SSAO need MSAA off");
        let live = RenderQuality::live();
        assert!(live.taa && !live.msaa);
        assert!(!live.dof, "an operator's view is not a photograph");
        assert!(
            !live.instant_adaptation,
            "a live view adapts at an eye's pace"
        );
    }

    /// The dots inside a narrow shaft are the raymarch crossing it a
    /// sample or two per pixel; the finer march is paid only by a rig
    /// that has such a shaft.
    #[test]
    fn narrow_shafts_raise_the_live_fog_steps_and_nothing_else_does() {
        let live = RenderQuality::live().fog_steps;
        assert_eq!(
            fog_steps_for(live, None),
            live,
            "a rig with no shaft keeps the live count"
        );
        assert_eq!(
            fog_steps_for(live, Some(25.0)),
            live,
            "a par rig keeps the live count"
        );
        assert_eq!(
            fog_steps_for(live, Some(11.0)),
            NARROW_BEAM_FOG_STEPS,
            "an 11-degree mover doubles it"
        );
        assert_eq!(
            fog_steps_for(RenderQuality::STILL.fog_steps, Some(1.72)),
            NARROW_BEAM_FOG_STEPS.max(RenderQuality::STILL.fog_steps),
            "a still is never made coarser"
        );
    }
}

#[cfg(test)]
mod quality_preset_tests {
    use super::*;

    /// Medium is the studio's own picture: the whole post chain, and
    /// the fog marched on the camera itself.
    ///
    /// The step count is a fraction of what it was, and that is not a
    /// downgrade of the picture so much as the price of where the march
    /// happens. It used to run on a haze camera at a fixed pixel
    /// budget, so 128 steps cost 128 times six hundred thousand
    /// samples whatever the window was; on the camera itself the same
    /// count at 5120x1440 is twelve times that. The reduced camera is
    /// coming back — see `RenderQuality::fog_scale` for what has to be
    /// fixed first — and the count comes back with it.
    /// r[verify viz.quality-presets]
    #[test]
    fn medium_is_the_studio_s_own_picture() {
        let medium = RenderQuality::preset(Preset::Medium);
        assert_eq!(medium.fog_steps, 32);
        assert_eq!(medium.fog_scale, 1, "medium marches on the camera itself");
        assert!(medium.taa && medium.ssr && medium.ssao);
        assert!(!medium.msaa && !medium.dof && !medium.instant_adaptation);
    }

    /// A live tier does not pay a still's step count for a narrow
    /// shaft.
    ///
    /// This is a regression, pinned. The raise to
    /// `NARROW_BEAM_FOG_STEPS` used to be gated on `fog_scale == 1`,
    /// which meant "a still" only for as long as a still was the only
    /// thing marching at full size. The moment every tier did, a rig
    /// with one beam fixture in it put the whole ladder on 256 steps at
    /// the picture's own size, and the studio ran at nine frames a
    /// second — for a picture nobody had asked to be finer.
    /// r[verify viz.quality-presets]
    #[test]
    fn only_a_still_pays_a_stills_step_count_for_a_narrow_shaft() {
        // `STILL` is a constant, so "a still has all the time in the
        // world" is checked at compile time rather than at test time —
        // what the loop below owns is that no *live* tier claims it.
        const _: () = assert!(RenderQuality::STILL.fine_for_narrow_shafts);
        for preset in [
            Preset::Potato,
            Preset::Low,
            Preset::Medium,
            Preset::High,
            Preset::Ultra,
        ] {
            assert!(
                !RenderQuality::preset(preset).fine_for_narrow_shafts,
                "{} would double the frame's largest cost for one beam fixture",
                preset.name()
            );
        }
        assert!(
            !RenderQuality::preset(Preset::Ultra).fine_for_narrow_shafts,
            "not even ultra: it is a tier an operator can select, and 256 steps at the \
             picture's own size is eleven frames a second"
        );
    }

    /// The ladder has to be a ladder: every rung at least as fine as
    /// the one below it, on both dials that carry the cost. A tier that
    /// is prettier in one respect and coarser in another is not a tier,
    /// it is a preference.
    /// r[verify viz.quality-presets]
    #[test]
    fn the_ladder_only_climbs() {
        let rungs = [
            Preset::Potato,
            Preset::Low,
            Preset::Medium,
            Preset::High,
            Preset::Ultra,
        ];
        for pair in rungs.windows(2) {
            let (lower, upper) = (
                RenderQuality::preset(pair[0]),
                RenderQuality::preset(pair[1]),
            );
            assert!(
                upper.fog_steps >= lower.fog_steps,
                "{} marches coarser than {}",
                pair[1].name(),
                pair[0].name()
            );
            // A tier marching on the camera itself has no pixel budget
            // to compare — which is every tier at present, so this
            // holds vacuously and stays for the day the reduced haze
            // camera is fit to come back.
            if upper.fog_scale == 0 {
                assert!(
                    upper.haze_pixels >= lower.haze_pixels,
                    "{} hazes coarser than {}",
                    pair[1].name(),
                    pair[0].name()
                );
            }
            for (name, lo, up) in [
                ("taa", lower.taa, upper.taa),
                ("ssr", lower.ssr, upper.ssr),
                ("ssao", lower.ssao, upper.ssao),
            ] {
                assert!(
                    up || !lo,
                    "{} drops {name} that {} has",
                    pair[1].name(),
                    pair[0].name()
                );
            }
        }
    }

    /// A typo in an environment variable must not stop a show opening.
    /// r[verify viz.quality-presets]
    #[test]
    fn a_name_round_trips_and_nonsense_is_not_fatal() {
        for preset in [
            Preset::Potato,
            Preset::Low,
            Preset::Medium,
            Preset::High,
            Preset::Ultra,
        ] {
            assert_eq!(Preset::parse(preset.name()), Some(preset));
        }
        assert_eq!(Preset::parse("  ULTRA "), Some(Preset::Ultra));
        assert_eq!(Preset::parse("med"), Some(Preset::Medium));
        assert_eq!(Preset::parse("cinematic"), None);
    }

    /// A coarser tier really does cost less at a real viewport size,
    /// not merely claim to.
    ///
    /// The bill is pixels times steps, and while every tier marches on
    /// the camera the pixels are the same for all of them — so this is
    /// the step count doing all the work. It is written as the bill
    /// anyway, because that is the quantity that shows up in the frame
    /// and the one that has to keep climbing when the haze camera
    /// returns and the pixels differ per tier again.
    /// r[verify viz.quality-presets]
    #[test]
    fn a_coarser_tier_costs_less_at_5120x1440() {
        let view = bevy::math::UVec2::new(4142, 577);
        let bill = |preset: Preset| {
            let q = RenderQuality::preset(preset);
            let size = crate::haze::haze_size(view, q.fog_scale, q.haze_pixels);
            u64::from(size.x * size.y) * u64::from(q.fog_steps)
        };
        assert!(bill(Preset::Potato) < bill(Preset::Low));
        assert!(bill(Preset::Low) < bill(Preset::Medium));
        assert!(bill(Preset::Medium) < bill(Preset::High));
        assert!(bill(Preset::High) < bill(Preset::Ultra));

        // And the coarsest tier is actually cheap: a live viewport
        // marching the picture's own pixels is the whole reason these
        // counts came down, so potato has to be a fraction of what the
        // studio's own tier costs, not a rung below it.
        assert!(
            bill(Preset::Potato) * 2 < bill(Preset::Medium),
            "potato is not a rescue from a slideshow"
        );
    }
}
