//! Procedural canvases on the GPU.
//!
//! A `proc:` canvas used to be rastered on the CPU every frame — 320
//! pixels wide, on a worker, copied into a texture — and then sampled
//! by the screen quads like a clip. That was three milliseconds a frame
//! for three canvases at Norco, and a 320-wide picture stretched across
//! a wall of TVs. Now the recipe is a [`Material`]: `canvas.wgsl`
//! evaluates `ignition_core::canvas::CanvasRecipe::sample` per fragment
//! from a uniform block, at whatever resolution the screen has, and the
//! only thing the CPU does per frame is write one float — the effect
//! clock — into that block.
//!
//! The CPU path (`CanvasRecipe::render`, `canvas::ProceduralSource`)
//! stays as the reference: the cooker's bitmap channels sample it, and
//! [`tests::the_gpu_paints_what_the_cpu_paints`] renders one frame of
//! each kind headlessly and compares.
//!
//! Slices and cover-fit are the same arithmetic as the texture path
//! (`canvas::Slice::cover_at`) — worked out once at spawn and carried in
//! the uniform as a rectangle, so a panel's quad keeps plain 0..1 UVs.

use crate::canvas::Slice;
use crate::spawn::{CanvasClock, ScreenSurface};
use bevy::asset::embedded_asset;
use bevy::pbr::MaterialPlugin;
use bevy::prelude::*;
use bevy::render::render_resource::{AsBindGroup, ShaderType};
use bevy::shader::ShaderRef;
use ignition_core::canvas::{CanvasRecipe, Procedural, Travel};
use ignition_core::step::SpeedMasters;

const SHADER: &str = "embedded://ignition_viz/canvas.wgsl";

/// The most colours a ramp carries to the shader. `named("rainbow")`
/// has six; the uniform has room for eight.
pub const MAX_COLORS: usize = 8;

/// The uniform block `canvas.wgsl` reads. Every field is a `vec4` so
/// the layout is the same on both sides without padding rules.
#[derive(ShaderType, Clone, Copy, Debug, Default, PartialEq)]
pub struct CanvasParams {
    /// The slice this panel shows, cover-fitted: `u0, v0, u1, v1`.
    pub rect: Vec4,
    /// `kind, seed, count, direction` (0 horizontal, 1 vertical).
    pub ints: UVec4,
    /// `cycles, glow, angle (radians), width`.
    pub scalars: Vec4,
    /// `noise scale, sparkle density, colour count, spare`.
    pub extra: Vec4,
    /// The ramp (gradient, noise) or the one colour (the rest) in `[0]`.
    pub colors: [Vec4; MAX_COLORS],
}

const KIND_SOLID: u32 = 0;
const KIND_GRADIENT: u32 = 1;
const KIND_WIPE: u32 = 2;
const KIND_NOISE: u32 = 3;
const KIND_BAND: u32 = 4;
const KIND_SPARKLE: u32 = 5;

impl CanvasParams {
    /// The block for `recipe`, showing `rect` of the canvas at `glow`
    /// (the emissive strength the texture path gives a screen) and with
    /// the clock at zero.
    // r[impl canvas.procedural] - the recipe becomes uniforms; the GPU does the picture
    pub fn new(recipe: &CanvasRecipe, rect: Slice, glow: f32) -> Self {
        let mut p = CanvasParams {
            rect: Vec4::new(rect.u0, rect.v0, rect.u1, rect.v1),
            scalars: Vec4::new(0.0, glow, 0.0, 0.0),
            ..default()
        };
        let mut set_colors = |colors: &[[f32; 3]]| {
            for (slot, c) in p.colors.iter_mut().zip(colors.iter().take(MAX_COLORS)) {
                *slot = Vec4::new(c[0], c[1], c[2], 1.0);
            }
            p.extra.z = colors.len().min(MAX_COLORS) as f32;
        };
        let travel = |d: Travel| match d {
            Travel::Horizontal => 0,
            Travel::Vertical => 1,
        };
        match &recipe.source {
            Procedural::Solid(c) => {
                set_colors(std::slice::from_ref(c));
                p.ints.x = KIND_SOLID;
            }
            Procedural::Gradient { colors, angle_deg } => {
                set_colors(colors);
                p.ints.x = KIND_GRADIENT;
                p.scalars.z = angle_deg.to_radians();
            }
            Procedural::Wipe {
                color,
                width,
                direction,
            } => {
                set_colors(std::slice::from_ref(color));
                p.ints = UVec4::new(KIND_WIPE, 0, 0, travel(*direction));
                p.scalars.w = *width;
            }
            Procedural::Noise {
                scale,
                seed,
                colors,
            } => {
                set_colors(colors);
                p.ints = UVec4::new(KIND_NOISE, *seed, 0, 0);
                p.extra.x = *scale;
            }
            Procedural::Band {
                color,
                width,
                count,
                direction,
            } => {
                set_colors(std::slice::from_ref(color));
                p.ints = UVec4::new(KIND_BAND, 0, *count, travel(*direction));
                p.scalars.w = *width;
            }
            Procedural::Sparkle {
                density,
                seed,
                color,
            } => {
                set_colors(std::slice::from_ref(color));
                p.ints = UVec4::new(KIND_SPARKLE, *seed, 0, 0);
                p.extra.y = *density;
            }
        }
        p
    }
}

/// One panel's procedural picture.
#[derive(Asset, TypePath, AsBindGroup, Clone, Debug)]
pub struct CanvasMaterial {
    #[uniform(0)]
    pub params: CanvasParams,
}

impl Material for CanvasMaterial {
    fn fragment_shader() -> ShaderRef {
        SHADER.into()
    }

    /// A screen is its own light; the rig never shadows it.
    fn enable_shadows() -> bool {
        false
    }
}

/// The recipe behind a panel's material, and the clock it runs on.
#[derive(Component, Clone)]
pub struct ProceduralCanvas {
    pub recipe: CanvasRecipe,
    masters: SpeedMasters,
    last_cycles: Option<f32>,
}

impl ProceduralCanvas {
    pub fn new(recipe: CanvasRecipe) -> Self {
        Self {
            recipe,
            masters: SpeedMasters::new(),
            last_cycles: None,
        }
    }

    /// The effect clock at `secs`, or `None` when it has not moved
    /// since the last call — a stopped transport rewrites nothing.
    pub fn advance(&mut self, secs: f64) -> Option<f32> {
        let cycles = self.recipe.cycles_at(secs as f32, &self.masters);
        if self.last_cycles == Some(cycles) {
            return None;
        }
        self.last_cycles = Some(cycles);
        Some(cycles)
    }
}

/// Spawns a panel's display quad showing `recipe`: a unit rectangle
/// scaled to the panel, just proud of its bezel, under `body`. `rect`
/// is the panel's slice of the canvas, already cover-fitted.
#[allow(clippy::too_many_arguments)]
pub fn spawn_panel(
    commands: &mut Commands,
    body: Entity,
    recipe: &CanvasRecipe,
    rect: Slice,
    panel_size: Vec3,
    depth: f32,
    glow: f32,
    materials: &mut Assets<CanvasMaterial>,
    meshes: &mut Assets<Mesh>,
) -> Entity {
    commands
        .spawn((
            ScreenSurface,
            ProceduralCanvas::new(recipe.clone()),
            Mesh3d(meshes.add(Rectangle::new(1.0, 1.0))),
            MeshMaterial3d(materials.add(CanvasMaterial {
                params: CanvasParams::new(recipe, rect, glow),
            })),
            Transform {
                translation: Vec3::Z * (depth * 0.5 + 0.005),
                scale: Vec3::new(panel_size.x * 0.94, panel_size.y * 0.94, 1.0),
                ..default()
            },
            ChildOf(body),
        ))
        .id()
}

/// Writes the effect clock into every procedural panel whose picture
/// has moved. The speed masters come from the playback when there is
/// one — a recipe timed `Master("Song")` follows the song's tempo — and
/// from the defaults otherwise.
// r[impl canvas.clip-is-a-source] - the same clock a clip is presented at
pub fn drive_canvases(
    clock: Res<CanvasClock>,
    playback: Option<Res<crate::playback::Playback>>,
    mut panels: Query<(&mut ProceduralCanvas, &MeshMaterial3d<CanvasMaterial>)>,
    mut materials: ResMut<Assets<CanvasMaterial>>,
) {
    let seconds = clock.seconds;
    for (mut panel, material) in &mut panels {
        if let Some(playback) = playback.as_deref()
            && panel.masters != playback.speeds
        {
            panel.masters = playback.speeds.clone();
            panel.last_cycles = None;
        }
        let Some(cycles) = panel.advance(seconds) else {
            continue;
        };
        if let Some(mut material) = materials.get_mut(&material.0) {
            material.params.scalars.x = cycles;
        }
    }
}

pub struct CanvasMaterialPlugin;

impl Plugin for CanvasMaterialPlugin {
    fn build(&self, app: &mut App) {
        embedded_asset!(app, "canvas.wgsl");
        app.add_plugins(MaterialPlugin::<CanvasMaterial>::default())
            .init_resource::<CanvasClock>()
            .add_systems(
                Update,
                drive_canvases.after(crate::spawn::update_canvas_videos),
            );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ignition_core::step::{Speed, Timing};

    fn recipe(source: Procedural) -> CanvasRecipe {
        CanvasRecipe {
            source,
            timing: Timing {
                speed: Speed::Hz(1.0),
                ..Timing::default()
            },
        }
    }

    /// r[verify canvas.procedural]
    #[test]
    fn a_recipe_packs_into_the_uniform_block() {
        let rainbow = ignition_core::canvas::named("rainbow").unwrap();
        let p = CanvasParams::new(&rainbow, Slice::FULL, 2.0);
        assert_eq!(p.ints.x, KIND_GRADIENT);
        assert_eq!(p.extra.z, 6.0, "six colours in the rainbow");
        assert_eq!(p.colors[0], Vec4::new(1.0, 0.0, 0.0, 1.0));
        assert_eq!(p.scalars.y, 2.0, "glow");
        assert_eq!(p.rect, Vec4::new(0.0, 0.0, 1.0, 1.0));

        let band = recipe(Procedural::Band {
            color: [1.0, 0.5, 0.0],
            width: 0.5,
            count: 4,
            direction: Travel::Vertical,
        });
        let p = CanvasParams::new(&band, Slice::FULL, 1.0);
        assert_eq!(p.ints, UVec4::new(KIND_BAND, 0, 4, 1));
        assert_eq!(p.scalars.w, 0.5);
        assert_eq!(p.extra.z, 1.0);
    }

    /// The clock only reaches the material when the picture moved.
    #[test]
    fn the_clock_is_written_only_when_it_moves() {
        let mut panel = ProceduralCanvas::new(recipe(Procedural::Solid([1.0; 3])));
        assert_eq!(panel.advance(0.0), Some(0.0));
        assert_eq!(panel.advance(0.0), None);
        assert_eq!(panel.advance(0.25), Some(0.25));
    }

    /// One frame of each kind, rendered headlessly through the material
    /// and read back, against the CPU reference at the same clock —
    /// every pixel, to a tolerance that covers the sRGB round trip and
    /// float order, with a few edge pixels allowed to land on the other
    /// side of a hard boundary.
    // r[verify canvas.procedural] - the GPU paints what the CPU reference paints
    #[test]
    fn the_gpu_paints_what_the_cpu_paints() {
        use bevy::asset::RenderAssetUsages;
        use bevy::camera::{RenderTarget, ScalingMode};
        use bevy::render::RenderPlugin;
        use bevy::render::gpu_readback::{Readback, ReadbackComplete};
        use bevy::render::render_resource::{
            Extent3d, TextureDimension, TextureFormat, TextureUsages,
        };
        use bevy::window::ExitCondition;
        use bevy::winit::WinitPlugin;
        use std::sync::{Arc, Mutex};

        if !gpu_available() {
            eprintln!("skipping: no GPU adapter");
            return;
        }

        const W: u32 = 256;
        const H: u32 = 144;
        let cycles = 0.37_f32;
        let recipes = [
            ("rainbow", ignition_core::canvas::named("rainbow").unwrap()),
            ("noise", ignition_core::canvas::named("noise").unwrap()),
            ("bands", ignition_core::canvas::named("bands").unwrap()),
            ("sparkle", ignition_core::canvas::named("sparkle").unwrap()),
            (
                "wipe-vertical",
                recipe(Procedural::Wipe {
                    color: [0.2, 0.9, 0.4],
                    width: 0.3,
                    direction: Travel::Vertical,
                }),
            ),
        ];

        for (name, recipe) in recipes {
            let mut app = App::new();
            app.add_plugins(
                DefaultPlugins
                    .set(WindowPlugin {
                        primary_window: None,
                        exit_condition: ExitCondition::DontExit,
                        ..default()
                    })
                    .set(RenderPlugin {
                        synchronous_pipeline_compilation: true,
                        ..default()
                    })
                    .disable::<WinitPlugin>(),
            )
            .add_plugins(CanvasMaterialPlugin)
            .insert_resource(ClearColor(Color::BLACK));

            let mut target = Image::new_uninit(
                Extent3d {
                    width: W,
                    height: H,
                    depth_or_array_layers: 1,
                },
                TextureDimension::D2,
                TextureFormat::Rgba8UnormSrgb,
                RenderAssetUsages::RENDER_WORLD,
            );
            target.texture_descriptor.usage |=
                TextureUsages::RENDER_ATTACHMENT | TextureUsages::COPY_SRC;
            let target = app.world_mut().resource_mut::<Assets<Image>>().add(target);

            // The readback node can run before the camera has drawn
            // its first frame, and the mesh and material take a frame
            // to reach the render world, so the first few captures are
            // the clear colour. The frame compared is a later one.
            let got: Arc<Mutex<(usize, Option<Vec<u8>>)>> = Arc::new(Mutex::new((0, None)));
            let sink = got.clone();
            let mut params = CanvasParams::new(&recipe, Slice::FULL, 1.0);
            params.scalars.x = cycles;
            let world = app.world_mut();
            let material = world
                .resource_mut::<Assets<CanvasMaterial>>()
                .add(CanvasMaterial { params });
            let quad = world
                .resource_mut::<Assets<Mesh>>()
                .add(Rectangle::new(1.0, 1.0));
            world.spawn((Mesh3d(quad), MeshMaterial3d(material)));
            world.spawn((
                Camera3d::default(),
                RenderTarget::Image(target.clone().into()),
                bevy::core_pipeline::tonemapping::Tonemapping::None,
                Msaa::Off,
                Projection::Orthographic(OrthographicProjection {
                    scaling_mode: ScalingMode::Fixed {
                        width: 1.0,
                        height: 1.0,
                    },
                    ..OrthographicProjection::default_3d()
                }),
                Transform::from_xyz(0.0, 0.0, 1.0).looking_at(Vec3::ZERO, Vec3::Y),
            ));
            world
                .spawn(Readback::texture(target))
                .observe(move |event: On<ReadbackComplete>| {
                    let mut got = sink.lock().unwrap();
                    got.0 += 1;
                    if got.0 >= 6 {
                        got.1 = Some(event.data.clone());
                    }
                });
            app.finish();
            app.cleanup();

            let mut frame = None;
            for _ in 0..120 {
                app.update();
                if let Some(data) = got.lock().unwrap().1.take() {
                    frame = Some(data);
                    break;
                }
            }
            let frame = frame.unwrap_or_else(|| panic!("{name}: no frame came back"));
            assert_eq!(frame.len(), (W * H * 4) as usize, "{name}: frame size");

            let reference = recipe.render(W, H, cycles);
            let (mut off, mut total) = (0usize, 0u64);
            for (g, r) in frame.chunks(4).zip(reference.chunks(4)) {
                let d = (0..3)
                    .map(|i| (g[i] as i32 - r[i] as i32).unsigned_abs())
                    .max()
                    .unwrap();
                total += d as u64;
                if d > 3 {
                    off += 1;
                }
            }
            let pixels = (W * H) as usize;
            let mean = total as f64 / pixels as f64;
            assert!(
                off * 200 < pixels && mean < 1.0,
                "{name}: {off} of {pixels} pixels differ by more than 3, mean {mean:.2}; \
                 first pixel gpu {:?} cpu {:?}, centre gpu {:?} cpu {:?}",
                &frame[..4],
                &reference[..4],
                &frame[((H / 2 * W + W / 2) * 4) as usize..][..4],
                &reference[((H / 2 * W + W / 2) * 4) as usize..][..4],
            );
        }
    }

    fn gpu_available() -> bool {
        let instance = wgpu::Instance::default();
        bevy::tasks::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default()))
            .is_ok()
    }
}
