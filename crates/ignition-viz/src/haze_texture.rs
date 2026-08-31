//! Haze that hangs in banks and drifts, rather than filling the room
//! evenly and standing still.
//!
//! A hazed room is not a uniform medium. The output pools near the
//! machine, thins where the air moves, and drifts across the stage all
//! night — which is why a beam crossing a real room brightens and fades
//! along its length instead of reading as a clean solid bar. Every dial
//! in `spawn.rs` sets *one* density for the whole volume, so the
//! visualiser has been showing the clean bar.
//!
//! `FogVolume` already carries the two things needed to fix that: a 3D
//! `density_texture` multiplying the density per point, and a
//! `density_texture_offset` to scroll it by. This module fills in the
//! first with tileable value noise and advances the second, which is
//! what grandMA3 exposes as its haze tab (particle size, layers,
//! animation speed) and what our froxel injection now samples per
//! froxel.
//!
//! The noise is built to average *one*, which is why it is stored as
//! half floats rather than bytes: both renderers multiply the room's
//! density by this texture, so a texture averaging anything else
//! silently rescales the haze and every calibration above it —
//! `FOG_LIGHT_GAIN` especially. A first cut centred on 0.75 in
//! `R8Unorm` dimmed the benchmark cue by eighteen per cent, which is
//! the whole reason this is not a byte texture: a multiplier that
//! averages one has to be able to exceed one.
// r[impl viz.haze-is-volumetric] - the room's haze is uneven and drifts

use bevy::asset::RenderAssetUsages;
use bevy::image::{Image, ImageAddressMode, ImageSampler, ImageSamplerDescriptor};
use bevy::light::FogVolume;
use bevy::pbr::froxel::FroxelVolumetrics;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};

/// Side of the noise cube in voxels. Small on purpose: the shape wanted
/// is banks of haze metres across, not detail, and the sampler
/// interpolates.
const SIZE: u32 = 32;

/// What the texture averages, so multiplying by it leaves the room's
/// density where the dial put it.
const MEAN: f32 = 1.0;

/// How far either side of `MEAN` the noise swings, as a fraction of it.
/// All the way to 1.0 gives holes in the haze a beam disappears into,
/// which reads as a fault rather than as air. Judged against
/// `beam-alias.json`: at 0.55 the beam is still nearly a clean bar, and
/// past 0.85 it starts to look speckled rather than hazy.
const SWING: f32 = 0.75;

/// Every dial for what the air *looks* like, in one place and mutable
/// at runtime.
///
/// Separate from `RenderQuality` on purpose: that ladder is about what
/// a frame costs, and these are about what the room looks like. A dial
/// here is a decision about the picture, not a budget.
///
/// Each field takes its default from the matching `IGNITION_HAZE_*`
/// variable, so a shell can pin one while it is being judged by eye,
/// and anything holding the resource can move it while the show runs.
#[derive(Resource, Debug, Clone, Copy)]
pub struct HazeLook {
    /// Whether the haze is uneven and drifting at all. Off is one flat
    /// density — what every snapshot before this was made with.
    pub uneven: bool,
    /// How far the density swings either side of average, 0..1. Past
    /// about 0.85 it reads as speckle rather than as air.
    pub unevenness: f32,
    /// How many banks of haze fit across the room — the lowest
    /// octave's period. Higher is finer.
    pub banks: i32,
    /// How fast the banks drift, as a multiple of `DRIFT_PER_SEC`.
    pub drift: f32,
    /// The exponent the fog's distance attenuation falls off with, two
    /// being the physical inverse square; lower carries a beam
    /// further. Fog only — surfaces stay physical.
    pub dilution: f32,
}

impl Default for HazeLook {
    fn default() -> Self {
        Self {
            uneven: env_flag("IGNITION_HAZE_ANIMATE").is_none_or(|on| on),
            unevenness: env_num::<f32>("IGNITION_HAZE_SWING")
                .unwrap_or(SWING)
                .clamp(0.0, 1.0),
            banks: env_num::<i32>("IGNITION_HAZE_BANKS")
                .unwrap_or(BASE_PERIOD)
                .clamp(1, 32),
            drift: env_num::<f32>("IGNITION_HAZE_DRIFT")
                .unwrap_or(1.0)
                .max(0.0),
            dilution: env_num::<f32>("IGNITION_HAZE_DILUTION")
                .unwrap_or(2.0)
                .clamp(0.0, 2.0),
        }
    }
}

fn env_num<T: std::str::FromStr>(name: &str) -> Option<T> {
    std::env::var(name).ok()?.trim().parse().ok()
}

fn env_flag(name: &str) -> Option<bool> {
    let raw = std::env::var(name).ok()?;
    Some(!matches!(raw.trim(), "0" | "off" | "false"))
}

/// How many octaves of value noise are summed. The texture spans the
/// whole fog volume once, as Bevy's density textures do, so the lowest
/// octave's period sets the size of a bank of haze: four across a
/// thirty-metre room is banks a few metres wide, which is the scale
/// haze actually hangs at.
const OCTAVES: u32 = 3;

/// The lowest octave's period across the volume — how many banks of
/// haze fit across the room.
const BASE_PERIOD: i32 = 6;

/// How fast the haze drifts, in tiles per second. Slow: haze moves on
/// the room's air currents, and anything quick reads as smoke rather
/// than as the room.
const DRIFT_PER_SEC: Vec3 = Vec3::new(0.006, 0.0015, 0.004);

pub struct HazeTexturePlugin;

impl Plugin for HazeTexturePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<HazeLook>()
            .add_systems(Update, (follow_look, drift).chain());
    }
}

/// `f32` to IEEE half, for the density texture. Only the range this
/// module writes needs to be right — no subnormals, no infinities.
fn f16_bits(value: f32) -> u16 {
    let bits = value.to_bits();
    let sign = ((bits >> 16) & 0x8000) as u16;
    let exponent = ((bits >> 23) & 0xff) as i32 - 127 + 15;
    let mantissa = ((bits >> 13) & 0x3ff) as u16;
    if exponent <= 0 {
        return sign;
    }
    if exponent >= 0x1f {
        return sign | 0x7bff;
    }
    sign | ((exponent as u16) << 10) | mantissa
}

/// Value noise on an integer lattice, hashed rather than stored.
fn hash(x: i32, y: i32, z: i32) -> f32 {
    let n =
        x.wrapping_mul(374_761_393) ^ y.wrapping_mul(668_265_263) ^ z.wrapping_mul(1_274_126_177);
    let n = (n ^ (n >> 13)).wrapping_mul(1_274_126_177);
    ((n ^ (n >> 16)) & 0x00ff_ffff) as f32 / 0x00ff_ffff as f32
}

/// Trilinear value noise, wrapping at `period` so the tile repeats
/// seamlessly — a seam in the haze would sweep across the room as the
/// texture drifts.
fn value_noise(p: Vec3, period: i32) -> f32 {
    let i = p.floor();
    let f = p - i;
    // Smoothstep, so the lattice does not show as a grid of creases.
    let w = f * f * (3.0 - 2.0 * f);
    let wrap = |v: f32| v.rem_euclid(period as f32) as i32;

    let mut result = 0.0;
    for dz in 0..2 {
        for dy in 0..2 {
            for dx in 0..2 {
                let corner = hash(
                    wrap(i.x + dx as f32),
                    wrap(i.y + dy as f32),
                    wrap(i.z + dz as f32),
                );
                let weight = (if dx == 1 { w.x } else { 1.0 - w.x })
                    * (if dy == 1 { w.y } else { 1.0 - w.y })
                    * (if dz == 1 { w.z } else { 1.0 - w.z });
                result += corner * weight;
            }
        }
    }
    result
}

/// Summed octaves, normalised to 0..1.
fn fbm(p: Vec3, banks: i32) -> f32 {
    let (mut sum, mut amplitude, mut total, mut period) = (0.0, 1.0, 0.0, banks);
    for _ in 0..OCTAVES {
        sum += value_noise(p * period as f32, period) * amplitude;
        total += amplitude;
        amplitude *= 0.5;
        period *= 2;
    }
    sum / total
}

/// The density texture: tileable fbm centred on `MEAN`.
pub fn noise_image(look: &HazeLook) -> Image {
    let mut data = Vec::with_capacity((SIZE * SIZE * SIZE) as usize * 2);
    for z in 0..SIZE {
        for y in 0..SIZE {
            for x in 0..SIZE {
                let p = Vec3::new(x as f32, y as f32, z as f32) / SIZE as f32;
                let n = fbm(p, look.banks) * 2.0 - 1.0;
                let density = (MEAN * (1.0 + n * look.unevenness)).max(0.0);
                data.extend_from_slice(&f16_bits(density).to_le_bytes());
            }
        }
    }

    let mut image = Image::new(
        Extent3d {
            width: SIZE,
            height: SIZE,
            depth_or_array_layers: SIZE,
        },
        TextureDimension::D3,
        data,
        TextureFormat::R16Float,
        RenderAssetUsages::RENDER_WORLD,
    );
    // Repeating, because the offset scrolls the texture forever and a
    // clamped edge would smear one voxel across the room.
    image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: ImageAddressMode::Repeat,
        address_mode_v: ImageAddressMode::Repeat,
        address_mode_w: ImageAddressMode::Repeat,
        ..ImageSamplerDescriptor::linear()
    });
    image
}

/// Keeps the room's haze and every froxel camera on the current look.
///
/// Rebuilds the noise only when the shape of it actually changed —
/// turning the unevenness dial is a texture upload, and doing that
/// every frame would be absurd — and pushes the dilution onto the
/// cameras, where the injection reads it.
fn follow_look(
    look: Res<HazeLook>,
    mut images: ResMut<Assets<Image>>,
    mut built: Local<Option<(bool, i32, u32)>>,
    mut fogs: Query<&mut FogVolume>,
    mut froxels: Query<&mut FroxelVolumetrics>,
) {
    let shape = (
        look.uneven,
        look.banks,
        (look.unevenness * 1000.0).round() as u32,
    );
    let new_fogs = fogs.iter().any(|fog| fog.density_texture.is_none());

    if *built != Some(shape) || new_fogs {
        let texture = look.uneven.then(|| images.add(noise_image(&look)));
        for mut fog in &mut fogs {
            fog.density_texture = texture.clone();
        }
        if !fogs.is_empty() {
            *built = Some(shape);
        }
    }

    if look.is_changed() {
        for mut froxel in &mut froxels {
            froxel.dilution = look.dilution;
        }
    }
}

/// Advances the offset so the banks drift across the room.
fn drift(time: Res<Time>, look: Res<HazeLook>, mut fogs: Query<&mut FogVolume>) {
    if !look.uneven || look.drift == 0.0 {
        return;
    }
    let step = DRIFT_PER_SEC * look.drift * time.delta_secs();
    for mut fog in &mut fogs {
        fog.density_texture_offset += step;
    }
}
