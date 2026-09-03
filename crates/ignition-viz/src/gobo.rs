//! Gobos and prism, drawn with Bevy's clustered decals.
//!
//! A gobo is a stencil at the fixture's gate: the beam gets through where
//! the stencil is open and nowhere else, so what lands on the wall is the
//! stencil's pattern. Here that stencil is a `ClusteredDecal` hanging off
//! the emitter, child of the `<Beam>` node in the fixture's own transform
//! tree (`r[viz.one-emitter-tree]`): pan and tilt reach it through
//! propagation and nothing recomputes where it points. The decal paints
//! black onto whatever surface it reaches, with alpha wherever the gobo
//! is *closed*, so the spill light — which still lights the whole cone —
//! is masked to the pattern. Clustered decals are clustered like lights
//! and cost a texture fetch per fragment they cover, not a render pass.
//!
//! A clustered decal is an orthographic unit cube projecting along its
//! local -Z, not a frustum, so it is sized per frame to the beam at the
//! surface it lands on: `reach` (the room's face the beam hits, from
//! `BeamThrow`) times the tangent of the field half-angle. A surface
//! nearer than that — a person in the beam — gets the wall's pattern
//! size, a little too large; nothing else about it is wrong.
//!
//! The screen-space march does not read decals (`volumetric_fog.wgsl`
//! knows lights and their shadow maps, nothing about decals), so under
//! it the shaft in the haze stays unbroken and only what lands on a
//! surface carries the pattern. The froxel path reads them: its
//! injection walks the same decal cluster list, matches each stencil to
//! the lamp at whose gate it hangs, and rescales the decal's parallel
//! projection by the beam's radius at that depth — so the pattern is in
//! the air too, converging on the gate. See
//! `docs/domain/froxel-volumetrics.md`.
//!
//! The wheel comes from the fixture's GDTF: the `Gobo1` channel's
//! `ChannelSet`s say which byte ranges land on which `WheelSlotIndex`,
//! and each slot's `MediaFileName` is a PNG inside the archive (three of
//! the workspace's profiles ship real art). A profile that names a gobo
//! wheel but ships no images gets a built-in set of eight classic
//! patterns generated at startup; a fixture with a `GoboWheel` channel
//! and no profile at all gets the same set at uniform eight-byte steps.
//!
//! A prism (`Prism1`) is a three-way split: three decals, each tilted a
//! few degrees off the beam axis in directions 120 degrees apart, the
//! triplet turning when the prism-rotation range is engaged. The spill
//! light itself is not split — a real prism triples the beam, this
//! triples the pattern.
// r[impl viz.gobo-raster] - gobos and prism are clustered decals hung off the emitter

use crate::fixture_profile::BeamThrow;
use crate::num::{
    byte_of_f32, byte_of_f64, f32_of_u32, f32_of_usize, f64_of_u64, u8_of_usize, usize_of_i32,
    usize_of_u32,
};
use crate::spawn::{BeamEmitter, EmitterState, GdtfLibraryRes, LiveDmx, VenueRes};
use bevy::asset::RenderAssetUsages;
use bevy::image::Image;
use bevy::light::ClusteredDecal;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use gdtf::dmx_mode::LogicalChannel;
use gdtf::fixture_type::FixtureType;
use gdtf::{GdtfFile, ResourceMap};
use ignition_proto::Attribute;
use std::collections::HashMap;
use std::io::Read as _;
use std::path::{Path, PathBuf};

/// Side of a generated gobo mask in pixels.
pub const GOBO_SIZE: u32 = 256;

/// Facets the prism splits the beam into.
pub const PRISM_FACETS: usize = 3;

/// How far off the beam axis each prism facet is thrown, in degrees.
pub const PRISM_SPREAD_DEG: f32 = 4.0;

/// The decal's depth as a multiple of the beam's reach, so it passes
/// through the surface the beam lands on rather than stopping at it.
pub const DECAL_DEPTH_FACTOR: f32 = 1.5;

/// How fast an engaged wheel-spin or prism-rotation range turns at its
/// fastest, in radians per second.
const MAX_SPIN_RAD_PER_SEC: f32 = 2.0;

pub struct GoboPlugin;

impl Plugin for GoboPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<GoboLibrary>()
            .add_systems(Startup, load_gobo_library)
            .add_systems(
                Update,
                (attach_gobo_projectors, update_gobo_projectors)
                    .chain()
                    .after(crate::spawn::update_live_fixtures),
            );
    }
}

// ── masks ────────────────────────────────────────────────────────────────

/// A gobo as the decal draws it: an RGBA image, black everywhere, with
/// alpha 1 where the gobo blocks light and 0 where it is open. Square.
#[derive(Clone, Debug)]
pub struct GoboMask {
    pub size: u32,
    pub rgba: Vec<u8>,
}

impl GoboMask {
    /// Decodes a GDTF wheel image. Manufacturers draw these two ways —
    /// white pattern on an opaque black disc (Lixada), or an opaque
    /// black disc with white cut-outs on a transparent ground (U'King)
    /// — and both read the same under one rule: light gets through in
    /// proportion to luminance times alpha, so opaque white is open and
    /// both black and transparent are blocked.
    ///
    /// # Errors
    ///
    /// If `bytes` isn't a valid PNG.
    pub fn from_png(bytes: &[u8]) -> anyhow::Result<Self> {
        let decoded = image::load_from_memory_with_format(bytes, image::ImageFormat::Png)?;
        let rgba = decoded.to_rgba8();
        let (w, h) = rgba.dimensions();
        let size = w.min(h);
        let mut out = Vec::with_capacity(usize_of_u32(size.saturating_mul(size).saturating_mul(4)));
        for y in 0..size {
            for x in 0..size {
                let p = rgba.get_pixel(x, y).0;
                let lum = 0.0722f32.mul_add(
                    f32::from(p[2]),
                    0.2126f32.mul_add(f32::from(p[0]), 0.7152 * f32::from(p[1])),
                ) / 255.0;
                let open = lum * (f32::from(p[3]) / 255.0);
                out.extend_from_slice(&[0, 0, 0, blocked_byte(open)]);
            }
        }
        Ok(Self { size, rgba: out })
    }

    /// Builds a mask from an openness function over unit coordinates
    /// centred on the gate, `(-1, -1)` to `(1, 1)`; everything outside
    /// the unit disc — the gate itself — is blocked.
    pub fn procedural(size: u32, open_at: impl Fn(f32, f32) -> f32) -> Self {
        let mut rgba =
            Vec::with_capacity(usize_of_u32(size.saturating_mul(size).saturating_mul(4)));
        let edge = 1.5 / f32_of_u32(size);
        for y in 0..size {
            for x in 0..size {
                let u = ((f32_of_u32(x) + 0.5) / f32_of_u32(size)).mul_add(2.0, -1.0);
                let v = ((f32_of_u32(y) + 0.5) / f32_of_u32(size)).mul_add(-2.0, 1.0);
                let r = u.hypot(v);
                // The gate: a soft one-pixel rim so it does not alias.
                let gate = ((1.0 - r) / edge).clamp(0.0, 1.0);
                let open = open_at(u, v).clamp(0.0, 1.0) * gate;
                rgba.extend_from_slice(&[0, 0, 0, blocked_byte(open)]);
            }
        }
        Self { size, rgba }
    }

    /// How open the mask is at a pixel, 0 (blocked) to 1.
    #[must_use]
    pub fn openness(&self, x: u32, y: u32) -> f32 {
        // Row-major, 4 bytes a pixel, alpha last. A byte the caller's
        // `x`/`y` puts outside the mask reads as fully blocked rather
        // than panicking or reading someone else's pixel.
        let index = usize_of_u32(
            self.size
                .saturating_mul(y)
                .saturating_add(x)
                .saturating_mul(4)
                .saturating_add(3),
        );
        let alpha = self.rgba.get(index).copied().unwrap_or(255);
        1.0 - f32::from(alpha) / 255.0
    }

    /// The fraction of the whole mask that lets light through.
    #[must_use]
    pub fn open_fraction(&self) -> f32 {
        let n = f32_of_u32(self.size.saturating_mul(self.size));
        self.rgba
            .chunks_exact(4)
            // `chunks_exact(4)` guarantees each `p` is 4 bytes long, so
            // this `get` never falls back — it exists to keep the index
            // audited rather than because index 3 can be out of range.
            .map(|p| 1.0 - f32::from(p.get(3).copied().unwrap_or(255)) / 255.0)
            .sum::<f32>()
            / n
    }

    /// The mask as a Bevy image for the decal to sample.
    #[must_use]
    pub fn to_image(&self) -> Image {
        Image::new(
            Extent3d {
                width: self.size,
                height: self.size,
                depth_or_array_layers: 1,
            },
            TextureDimension::D2,
            self.rgba.clone(),
            TextureFormat::Rgba8Unorm,
            RenderAssetUsages::RENDER_WORLD,
        )
    }
}

fn blocked_byte(open: f32) -> u8 {
    byte_of_f32((1.0 - open.clamp(0.0, 1.0)) * 255.0)
}

/// The built-in gobo set, for wheels whose profile ships no art.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Builtin {
    Dots,
    Bars,
    Breakup,
    Star,
    Spiral,
    Radial,
    Tri,
    Open,
}

impl Builtin {
    pub const ALL: [Self; 8] = [
        Self::Dots,
        Self::Bars,
        Self::Breakup,
        Self::Star,
        Self::Spiral,
        Self::Radial,
        Self::Tri,
        Self::Open,
    ];

    /// The patterned ones, in wheel order — `Open` is the wheel's own
    /// open slot, not a pattern.
    pub const PATTERNS: [Self; 7] = [
        Self::Dots,
        Self::Bars,
        Self::Breakup,
        Self::Star,
        Self::Spiral,
        Self::Radial,
        Self::Tri,
    ];

    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Dots => "dots",
            Self::Bars => "bars",
            Self::Breakup => "breakup",
            Self::Star => "star",
            Self::Spiral => "spiral",
            Self::Radial => "radial",
            Self::Tri => "tri",
            Self::Open => "open",
        }
    }

    /// The pattern as a mask of `size` pixels a side.
    #[must_use]
    pub fn mask(self, size: u32) -> GoboMask {
        use core::f32::consts::{PI, TAU};
        // A crisp edge a couple of pixels wide, in unit coordinates.
        let soft = 3.0 / f32_of_u32(size);
        let step = move |x: f32| (x / soft).clamp(0.0, 1.0);
        match self {
            Self::Open => GoboMask::procedural(size, |_, _| 1.0),
            // A hex-ish grid of round holes.
            Self::Dots => GoboMask::procedural(size, move |u, v| {
                let pitch = 0.36;
                let row = (v / (pitch * 0.866)).round();
                // `row` is already integer-valued; `rem_euclid` reads its
                // parity the same way `row as i32 % 2` did, without a
                // cast, and agrees with it on negative rows too.
                let shift = if row.rem_euclid(2.0) < 0.5 {
                    0.0
                } else {
                    pitch * 0.5
                };
                let cx = ((u - shift) / pitch).round().mul_add(pitch, shift);
                let cy = row * pitch * 0.866;
                let d = (u - cx).hypot(v - cy);
                step(0.11 - d)
            }),
            // Parallel slits.
            Self::Bars => GoboMask::procedural(size, move |u, _| {
                let phase = (u * 3.0).rem_euclid(1.0);
                step(phase - 0.3) * step(0.7 - phase)
            }),
            // Leafy breakup: a hash of overlapping blobs.
            Self::Breakup => GoboMask::procedural(size, move |u, v| {
                let mut best: f32 = 0.0;
                for i in 0..28 {
                    let a = f32_of_usize(i) * 2.399_963; // golden angle
                    let r = 0.75f32.mul_add((f32_of_usize(i) * 0.618_034).fract(), 0.15);
                    let (cx, cy) = (r * a.cos(), r * a.sin());
                    let rad = 0.09f32.mul_add((f32_of_usize(i) * 0.381_966).fract(), 0.07);
                    let d = (u - cx).hypot(v - cy);
                    best = best.max(step(rad - d));
                }
                best
            }),
            // Five-pointed star.
            Self::Star => GoboMask::procedural(size, move |u, v| {
                let r = u.hypot(v);
                let ang = v.atan2(u) + PI / 2.0;
                let t = (ang / (TAU / 5.0)).rem_euclid(1.0);
                let t = (t - 0.5).abs() * 2.0; // 0 at a point, 1 between points
                let edge = 0.5f32.mul_add(-t, 0.85);
                step(edge - r)
            }),
            // Three-armed spiral.
            Self::Spiral => GoboMask::procedural(size, move |u, v| {
                let r = u.hypot(v);
                let ang = v.atan2(u);
                let phase = (r.mul_add(6.0, ang) * 3.0 / TAU).rem_euclid(1.0);
                step(phase - 0.25) * step(0.75 - phase) * step(r - 0.08)
            }),
            // Radial spokes.
            Self::Radial => GoboMask::procedural(size, move |u, v| {
                let ang = v.atan2(u);
                let phase = (ang * 8.0 / TAU).rem_euclid(1.0);
                step(phase - 0.3) * step(0.7 - phase)
            }),
            // Triangles tiled across the gate.
            Self::Tri => GoboMask::procedural(size, move |u, v| {
                let pitch = 0.5;
                let uu = u / pitch;
                let vv = v / (pitch * 0.866);
                let row = vv.floor();
                let fx = 0.5f32.mul_add(-row, uu).rem_euclid(1.0);
                let fy = vv - row;
                let inside = if fx + fy < 1.0 {
                    1.0 - fx - fy
                } else {
                    fx + fy - 1.0
                };
                step((inside.min(fx).min(fy).min(1.0 - fx).min(1.0 - fy)) - 0.08)
            }),
        }
    }
}

/// Every built-in gobo, name and mask, at `GOBO_SIZE`.
#[must_use]
pub fn builtin_masks() -> Vec<(&'static str, GoboMask)> {
    Builtin::ALL
        .iter()
        .map(|b| (b.name(), b.mask(GOBO_SIZE)))
        .collect()
}

// ── wheel ────────────────────────────────────────────────────────────────

/// A gobo wheel as decoded from a profile: what each slot shows and
/// which bytes select, spin or split it. Pure data, so it is testable
/// without a renderer.
#[derive(Clone, Debug, Default)]
pub struct WheelSpec {
    /// Slot masks, wheel order (index 0 is `WheelSlotIndex` 1). `None`
    /// is an open slot.
    pub slots: Vec<Option<GoboMask>>,
    /// `(first byte, slot index)` pairs, ascending by byte; a byte lands
    /// on the last pair at or below it.
    pub select: Vec<(u8, usize)>,
    /// Byte ranges (`from..=to`) where the wheel spins continuously,
    /// with the direction: `+1` for the range's named direction.
    pub spin: Vec<(u8, u8, f32)>,
    /// The byte from which the prism is in, when the mode has one.
    pub prism_from: Option<u8>,
    /// The byte from which the prism rotates, when the mode has one.
    pub prism_spin_from: Option<u8>,
    /// Whether any slot carries the manufacturer's own art.
    pub has_art: bool,
}

impl WheelSpec {
    /// The slot a wheel byte selects.
    #[must_use]
    pub fn slot_for_byte(&self, byte: u8) -> Option<usize> {
        self.select
            .iter()
            .take_while(|(from, _)| *from <= byte)
            .last()
            .map(|(_, slot)| *slot)
            .filter(|slot| *slot < self.slots.len())
    }

    /// The spin the byte asks for: `None` outside every spin range,
    /// otherwise a signed speed, radians per second, fastest at the
    /// range's far end.
    #[must_use]
    pub fn spin_for_byte(&self, byte: u8) -> Option<f32> {
        self.spin.iter().find_map(|(from, to, sign)| {
            (byte >= *from && byte <= *to).then(|| {
                let span = (f32::from(*to) - f32::from(*from)).max(1.0);
                let t = (f32::from(byte) - f32::from(*from)) / span;
                sign * MAX_SPIN_RAD_PER_SEC * 0.85f32.mul_add(t, 0.15)
            })
        })
    }

    #[must_use]
    pub fn prism_in(&self, byte: u8) -> bool {
        self.prism_from.is_some_and(|from| byte >= from)
    }

    /// The prism's rotation speed, radians per second, when the byte is
    /// in the rotation range.
    #[must_use]
    pub fn prism_spin_for_byte(&self, byte: u8) -> Option<f32> {
        let from = self.prism_spin_from?;
        (byte >= from).then(|| {
            let t = (f32::from(byte) - f32::from(from)) / (255.0 - f32::from(from)).max(1.0);
            MAX_SPIN_RAD_PER_SEC * 0.85f32.mul_add(t, 0.15)
        })
    }

    /// The wheel of a fixture that has a `GoboWheel` channel but no
    /// profile to say what is on it: open, then the seven built-in
    /// patterns, at eight-byte steps.
    #[must_use]
    pub fn builtin_default() -> Self {
        let mut spec = Self {
            slots: vec![None],
            select: vec![(0, 0)],
            ..Default::default()
        };
        for (i, pattern) in Builtin::PATTERNS.iter().enumerate() {
            spec.slots.push(Some(pattern.mask(GOBO_SIZE)));
            let slot_index = i.saturating_add(1);
            spec.select.push((
                u8_of_usize(i).saturating_add(1).saturating_mul(8),
                slot_index,
            ));
        }
        spec
    }

    /// Fills a wheel that ships no art with the built-in patterns —
    /// the first slot stays open, the rest cycle through the set.
    pub fn fill_with_builtins(&mut self) {
        if self.has_art {
            return;
        }
        for (i, slot) in self.slots.iter_mut().enumerate().skip(1) {
            // `skip(1)` guarantees `i >= 1`, so the subtraction never
            // underflows; `PATTERNS` is a fixed 7-element array so the
            // `checked_rem` never actually misses, and the `get` below
            // never falls back, since the modulo always lands inside it.
            let offset = i
                .saturating_sub(1)
                .checked_rem(Builtin::PATTERNS.len())
                .unwrap_or(0);
            let Some(pattern) = Builtin::PATTERNS.get(offset).copied() else {
                continue;
            };
            *slot = Some(pattern.mask(GOBO_SIZE));
        }
    }
}

/// Reads the gobo wheel (and prism) of a `.gdtf` file's first fixture
/// type: `mode_name`'s DMX mode, or the first when `None`. `Ok(None)`
/// when the mode has no `Gobo1` channel.
///
/// # Errors
///
/// If the file can't be opened, isn't a valid GDTF zip, or names a mode
/// that doesn't exist in it.
pub fn wheel_from_gdtf(path: &Path, mode_name: Option<&str>) -> anyhow::Result<Option<WheelSpec>> {
    let file = std::fs::File::open(path)?;
    let mut gdtf =
        GdtfFile::new(file).map_err(|e| anyhow::anyhow!("parsing {}: {e}", path.display()))?;
    let GdtfFile {
        description,
        resources,
    } = &mut gdtf;
    let fixture_type = description
        .fixture_types
        .first()
        .ok_or_else(|| anyhow::anyhow!("{}: no fixture types", path.display()))?;
    let mode = mode_name.map_or_else(
        || fixture_type.dmx_modes.first(),
        |name| {
            fixture_type.dmx_modes.iter().find(|m| {
                m.name
                    .as_deref()
                    .map(std::string::ToString::to_string)
                    .as_deref()
                    == Some(name)
            })
        },
    );
    let Some(mode) = mode else { return Ok(None) };

    let mut spec = WheelSpec::default();
    let mut found = false;
    for channel in &mode.dmx_channels {
        for logical in &channel.logical_channels {
            let attr = logical.attribute.to_string();
            if attr == "Gobo1" {
                found = true;
                read_gobo_channel(logical, fixture_type, resources, &mut spec)?;
            } else if attr == "Prism1" {
                read_prism_channel(logical, &mut spec);
            }
        }
    }
    if !found {
        return Ok(None);
    }
    spec.select.sort_by_key(|(from, _)| *from);
    spec.select.dedup_by_key(|(from, _)| *from);
    // A wheel the select table reaches past what the slots say: pad with
    // open, so a byte never lands nowhere.
    let highest = spec
        .select
        .iter()
        .map(|(_, s)| s.saturating_add(1))
        .max()
        .unwrap_or(0);
    while spec.slots.len() < highest {
        spec.slots.push(None);
    }
    if spec.select.is_empty() && !spec.slots.is_empty() {
        // Slots but no channel sets: uniform steps. The `checked_div` can
        // never miss — `.max(1)` rules out a zero divisor — and when
        // there is exactly one slot the truncating `256 -> u8` step below
        // is inert either way, since a lone entry at byte 0 already
        // covers every byte.
        let divisor = spec.slots.len().max(1);
        let step = u8_of_usize(256usize.checked_div(divisor).unwrap_or(256));
        spec.select = (0..spec.slots.len())
            .map(|i| (u8_of_usize(i).saturating_mul(step), i))
            .collect();
    }
    spec.fill_with_builtins();
    Ok(Some(spec))
}

/// Reads one `Gobo1` logical channel: each function's select bytes or
/// spin range, then — once, from the first function that names a wheel —
/// the wheel's own slot art out of the archive.
///
/// # Errors
///
/// If a slot's media file can't be read out of the archive.
fn read_gobo_channel(
    logical: &LogicalChannel,
    fixture_type: &FixtureType,
    resources: &mut ResourceMap,
    spec: &mut WheelSpec,
) -> anyhow::Result<()> {
    let mut wheel_name = None;
    let functions = &logical.channel_functions;
    for (i, f) in functions.iter().enumerate() {
        let fattr = f.attribute.to_string();
        let from = dmx_byte(f.dmx_from);
        let to = functions
            .get(i.saturating_add(1))
            .map_or(255, |n| dmx_byte(n.dmx_from).saturating_sub(1));
        if wheel_name.is_none() {
            wheel_name = f.wheel.as_ref().map(std::string::ToString::to_string);
        }
        if fattr.contains("Spin") {
            let name = f.name.as_deref().map(str::to_lowercase).unwrap_or_default();
            let sign = if name.contains("counter") || name.contains("ccw") {
                -1.0
            } else {
                1.0
            };
            spec.spin.push((from, to, sign));
        } else if fattr == "Gobo1" || fattr.contains("Shake") {
            for set in &f.channel_sets {
                // The crate hands `WheelSlotIndex` back
                // zero-based, `None` for a set with none.
                if let Some(index) = set.wheel_slot_index.filter(|i| *i >= 0) {
                    spec.select
                        .push((dmx_byte(set.dmx_from), usize_of_i32(index)));
                }
            }
        }
    }
    let wheel = wheel_name.as_deref().and_then(|n| fixture_type.wheel(n));
    if let Some(wheel) = wheel {
        for slot in &wheel.slots {
            let mask = match &slot.media_name {
                Some(media) => {
                    let mut bytes = Vec::new();
                    match resources.read_wheel_media(media) {
                        Ok(mut r) => {
                            r.read_to_end(&mut bytes)?;
                            GoboMask::from_png(&bytes).ok()
                        }
                        Err(_) => None,
                    }
                }
                None => None,
            };
            spec.has_art |= mask.is_some();
            spec.slots.push(mask);
        }
    }
    Ok(())
}

/// Reads one `Prism1` logical channel: the byte the prism engages at,
/// and separately the byte its rotation range starts at when the mode
/// has one.
fn read_prism_channel(logical: &LogicalChannel, spec: &mut WheelSpec) {
    let functions = &logical.channel_functions;
    for f in functions {
        let fattr = f.attribute.to_string();
        let name = f.name.as_deref().map(str::to_lowercase).unwrap_or_default();
        let from = dmx_byte(f.dmx_from);
        let closed = name.contains("closed")
            || name.contains("off")
            || name.contains("no prism")
            || (f.physical_from == 0.0 && f.physical_to == 0.0);
        if fattr.contains("Spin") || name.contains("rotat") {
            spec.prism_spin_from.get_or_insert(from);
            spec.prism_from.get_or_insert_with(|| from.max(1));
        } else if !closed {
            spec.prism_from.get_or_insert_with(|| from.max(1));
        }
    }
}

fn dmx_byte(value: gdtf::values::DmxValue) -> u8 {
    let bits = 8u32.saturating_mul(u32::from(value.bytes().get()));
    // The largest value `bits` bits can hold, as the `f64` denominator of
    // a 0..=max ratio. `checked_shl` (rather than bare `<<`) turns an
    // implausibly wide `bits` into 0 instead of a shift-amount panic; a
    // 0..=-1 range collapsing to a 0 max only matters if `value.value()`
    // is also 0 there, which it is — so the ratio below is still 0/0,
    // same as it always was for an empty range.
    let max = f64_of_u64(1u64.checked_shl(bits).unwrap_or(0).saturating_sub(1));
    byte_of_f64((f64_of_u64(value.value()) / max) * 255.0)
}

// ── library ──────────────────────────────────────────────────────────────

/// A wheel with its masks uploaded.
pub struct LoadedWheel {
    pub spec: WheelSpec,
    /// One handle per slot, `None` for an open slot.
    pub images: Vec<Option<Handle<Image>>>,
}

impl LoadedWheel {
    fn upload(spec: WheelSpec, images: &mut Assets<Image>) -> Self {
        let handles = spec
            .slots
            .iter()
            .map(|slot| slot.as_ref().map(|mask| images.add(mask.to_image())))
            .collect();
        Self {
            spec,
            images: handles,
        }
    }
}

/// Every profile's gobo wheel, keyed by normalized fixture-type name,
/// plus the built-in wheel for fixtures with no profile.
#[derive(Resource, Default)]
pub struct GoboLibrary {
    by_type: HashMap<String, LoadedWheel>,
    fallback: Option<LoadedWheel>,
}

impl GoboLibrary {
    /// The wheel for a fixture type, or the built-in one.
    #[must_use]
    pub fn wheel(&self, fixture_type_name: Option<&str>) -> Option<&LoadedWheel> {
        fixture_type_name
            .and_then(|n| self.by_type.get(&normalize(n)))
            .or(self.fallback.as_ref())
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.by_type.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.by_type.is_empty()
    }
}

/// The same directories `GdtfLibrary::load_default` reads, in the same
/// order, so the wheel found for a fixture type is the one whose
/// geometry is drawn.
#[must_use]
pub fn gdtf_files() -> Vec<PathBuf> {
    let candidates = [
        PathBuf::from("data/gdtf"),
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data/gdtf"),
    ];
    let Some(dir) = candidates.into_iter().find(|d| d.is_dir()) else {
        return Vec::new();
    };
    let is_gdtf = |p: &Path| p.is_file() && p.extension().and_then(|e| e.to_str()) == Some("gdtf");
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return Vec::new();
    };
    let mut files = Vec::new();
    let mut subdirs = Vec::new();
    for path in entries.filter_map(|e| e.ok().map(|e| e.path())) {
        if path.is_dir() {
            subdirs.push(path);
        } else if is_gdtf(&path) {
            files.push(path);
        }
    }
    files.sort();
    subdirs.sort();
    for sub in subdirs {
        let Ok(entries) = std::fs::read_dir(&sub) else {
            continue;
        };
        let mut nested: Vec<_> = entries
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| is_gdtf(p))
            .collect();
        nested.sort();
        files.extend(nested);
    }
    files
}

fn fixture_type_name(path: &Path) -> Option<String> {
    let file = std::fs::File::open(path).ok()?;
    let gdtf = GdtfFile::new(file).ok()?;
    gdtf.description
        .fixture_types
        .first()
        .and_then(|t| t.name.as_ref().map(std::string::ToString::to_string))
}

fn normalize(s: &str) -> String {
    s.chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

fn load_gobo_library(mut library: ResMut<GoboLibrary>, mut images: ResMut<Assets<Image>>) {
    for path in gdtf_files() {
        let Some(name) = fixture_type_name(&path) else {
            continue;
        };
        match wheel_from_gdtf(&path, None) {
            Ok(Some(spec)) => {
                library
                    .by_type
                    .insert(normalize(&name), LoadedWheel::upload(spec, &mut images));
            }
            Ok(None) => {}
            Err(e) => eprintln!("viz: gobo wheel of {}: {e}", path.display()),
        }
    }
    library.fallback = Some(LoadedWheel::upload(
        WheelSpec::builtin_default(),
        &mut images,
    ));
}

// ── entities ─────────────────────────────────────────────────────────────

/// On an emitter whose fixture has a gobo wheel: which wheel, and where
/// its spins have got to.
#[derive(Component)]
pub struct GoboProjector {
    /// Normalized fixture-type name, or `None` for the built-in wheel.
    pub fixture_type: Option<String>,
    /// Accumulated wheel spin, radians.
    pub spin: f32,
    /// Accumulated prism rotation, radians.
    pub prism: f32,
}

/// One of the projector's decals: facet 0 alone without a prism, all
/// three with one.
#[derive(Component)]
pub struct GoboFacet(pub usize);

/// Hangs a projector off every new emitter of a fixture with a gobo
/// wheel channel.
#[expect(
    clippy::needless_pass_by_value,
    reason = "Bevy system params (Res, Option<Res>) are taken by value by the system signature itself; see docs/ops/clippy.md"
)]
fn attach_gobo_projectors(
    mut commands: Commands,
    venue: Res<VenueRes>,
    gdtf: Option<Res<GdtfLibraryRes>>,
    emitters: Query<(Entity, &BeamEmitter), Added<BeamEmitter>>,
) {
    for (entity, emitter) in &emitters {
        let Some(record) = venue.0.fixtures.get(emitter.fixture) else {
            continue;
        };
        let has_wheel = venue.0.patch().get(emitter.fixture).is_some_and(|p| {
            p.map
                .channels
                .iter()
                .any(|(_, a)| matches!(a, Attribute::GoboWheel { .. }))
        });
        if !has_wheel {
            continue;
        }
        let manufacturer = record.manufacturer.as_deref().unwrap_or("");
        let model = record.model.as_deref().unwrap_or("");
        let fixture_type = gdtf
            .as_ref()
            .and_then(|lib| lib.0.as_ref())
            .and_then(|lib| lib.find(manufacturer, model))
            .map(|f| normalize(&f.fixture_type_name));
        commands.entity(entity).insert(GoboProjector {
            fixture_type,
            spin: 0.0,
            prism: 0.0,
        });
        for facet in 0..PRISM_FACETS {
            commands.spawn((
                GoboFacet(facet),
                ClusteredDecal::default(),
                Transform::default(),
                Visibility::Hidden,
                Name::new(format!("{} gobo {facet}", record.name)),
                ChildOf(entity),
            ));
        }
    }
}

/// The width of the projected pattern at the surface it lands on: the
/// beam's full field at `reach` metres.
#[must_use]
pub fn pattern_width(reach: f32, field_half_angle_deg: f32) -> f32 {
    2.0 * reach * field_half_angle_deg.to_radians().tan()
}

/// The local transform of facet `facet`'s decal under its emitter: a cube
/// `width` across, `DECAL_DEPTH_FACTOR * reach` deep, its near face at the
/// gate, projecting down the emitter's -Z.
///
/// With the prism in, every facet is thrown `PRISM_SPREAD_DEG` off axis in a
/// direction `prism_angle + facet * 120°` round the beam; the gobo's own
/// spin is applied about the beam axis inside that.
// `Vec3`/`Quat` composition below is float, component-wise math with no
// integer overflow to guard against — `arithmetic_side_effects` fires
// on any operator-overloaded type, not just the primitive integers the
// lint is really about (see docs/ops/clippy.md, and the same precedent
// in `gdtf_geometry.rs::collect_beams`).
#[expect(
    clippy::arithmetic_side_effects,
    reason = "Vec3/Quat composition is float and cannot panic or overflow"
)]
#[must_use]
pub fn facet_transform(
    facet: usize,
    reach: f32,
    width: f32,
    spin: f32,
    prism_in: bool,
    prism_angle: f32,
) -> Transform {
    use core::f32::consts::TAU;
    let depth = reach * DECAL_DEPTH_FACTOR;
    let spin_rot = Quat::from_rotation_z(spin);
    let rotation = if prism_in {
        let phi = prism_angle + f32_of_usize(facet) * TAU / f32_of_usize(PRISM_FACETS);
        Quat::from_rotation_z(phi)
            * Quat::from_rotation_x(PRISM_SPREAD_DEG.to_radians())
            * Quat::from_rotation_z(-phi)
            * spin_rot
    } else {
        spin_rot
    };
    Transform {
        translation: rotation * Vec3::new(0.0, 0.0, -depth * 0.5),
        rotation,
        scale: Vec3::new(width, width, depth),
    }
}

/// A forced wheel byte (and prism byte) for every projector, from
/// `IGNITION_VIZ_GOBO=<byte>[,<prism byte>]` — so a still can show a
/// gobo without a cue that selects one.
fn forced_bytes() -> Option<(u8, Option<u8>)> {
    let raw = std::env::var("IGNITION_VIZ_GOBO").ok()?;
    let mut parts = raw.split(',');
    let gobo = parts.next()?.trim().parse().ok()?;
    let prism = parts.next().and_then(|p| p.trim().parse().ok());
    Some((gobo, prism))
}

/// Points every projector's decals down its beam, sized to the surface
/// the beam lands on, showing the slot the wire selected.
#[expect(
    clippy::type_complexity,
    reason = "Bevy query tuples name their own component set; a type alias would move the shape away from its one call site"
)]
#[expect(
    clippy::needless_pass_by_value,
    reason = "Bevy system params (Res) are taken by value by the system signature itself; see docs/ops/clippy.md"
)]
// `Vec3`/`Quat` composition below (the beam direction, the facet
// transform's translation) is float, component-wise math with no
// integer overflow to guard against — `arithmetic_side_effects` fires
// on any operator-overloaded type, not just the primitive integers the
// lint is really about (see docs/ops/clippy.md).
#[expect(
    clippy::arithmetic_side_effects,
    reason = "Vec3/Quat composition is float and cannot panic or overflow"
)]
fn update_gobo_projectors(
    time: Res<Time>,
    venue: Res<VenueRes>,
    live: Res<LiveDmx>,
    library: Res<GoboLibrary>,
    mut forced: Local<Option<Option<(u8, Option<u8>)>>>,
    mut projectors: Query<(
        &BeamEmitter,
        &EmitterState,
        &GlobalTransform,
        &Children,
        &mut GoboProjector,
    )>,
    mut facets: Query<(
        &GoboFacet,
        &mut Transform,
        &mut Visibility,
        &mut ClusteredDecal,
    )>,
) {
    let forced = *forced.get_or_insert_with(forced_bytes);
    let throw = BeamThrow::for_venue(&venue.0);
    let dt = time.delta_secs();

    for (emitter, state, global, children, mut projector) in &mut projectors {
        let wheel = library.wheel(projector.fixture_type.as_deref());
        let live = live.0.get(emitter.fixture).and_then(|o| o.as_ref());
        let (gobo_byte, prism_byte) = match forced {
            Some((g, p)) => (Some(g), p.or_else(|| live.and_then(|l| l.prism))),
            None => (live.and_then(|l| l.gobo), live.and_then(|l| l.prism)),
        };

        let mut image = None;
        let mut prism_in = false;
        if let (Some(wheel), Some(byte)) = (wheel, gobo_byte) {
            if let Some(slot) = wheel.spec.slot_for_byte(byte) {
                image.clone_from(wheel.images.get(slot).unwrap_or(&None));
            }
            match wheel.spec.spin_for_byte(byte) {
                Some(speed) => projector.spin += speed * dt,
                None => {
                    if let Some(frac) = live.and_then(|l| l.gobo_rotation) {
                        projector.spin = frac * core::f32::consts::TAU;
                    }
                }
            }
            if let Some(p) = prism_byte {
                prism_in = wheel.spec.prism_in(p);
                if let Some(speed) = wheel.spec.prism_spin_for_byte(p) {
                    projector.prism += speed * dt;
                }
            }
        }
        let lit = state.color.is_some() && image.is_some();

        let origin = global.translation();
        let direction = (global.rotation() * Vec3::NEG_Z).normalize_or_zero();
        let reach = throw.reach(origin, direction).max(0.1);
        let width = pattern_width(reach, state.field_half_angle_deg);

        for child in children.iter() {
            let Ok((facet, mut transform, mut visibility, mut decal)) = facets.get_mut(child)
            else {
                continue;
            };
            let shown = lit && (facet.0 == 0 || prism_in);
            let wanted = if shown {
                Visibility::Visible
            } else {
                Visibility::Hidden
            };
            if *visibility != wanted {
                *visibility = wanted;
            }
            if !shown {
                continue;
            }
            *transform = facet_transform(
                facet.0,
                reach,
                width,
                projector.spin,
                prism_in,
                projector.prism,
            );
            if decal.base_color_texture != image {
                decal.base_color_texture.clone_from(&image);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gdtf(name: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../data/gdtf")
            .join(name)
    }

    /// r[verify viz.gobo-raster] - a real profile's wheel art is extracted from the archive
    #[test]
    fn lixada_wheel_art_is_extracted() {
        let spec = wheel_from_gdtf(
            &gdtf("Lixada@Mini_Gobo_Moving_Head@pm-040220252203.gdtf"),
            None,
        )
        .unwrap()
        .expect("Lixada mode has a Gobo1 channel");
        assert!(spec.has_art);
        assert_eq!(spec.slots.len(), 8);
        let art_count = spec.slots.iter().filter(|s| s.is_some()).count();
        assert_eq!(
            art_count, 7,
            "seven slots carry PNGs, the placeholder is open"
        );
        assert!(spec.slots[7].is_none());
        // The wheel's default byte, 20, is gobo 3.
        assert_eq!(spec.slot_for_byte(20), Some(2));
        assert_eq!(spec.slot_for_byte(0), Some(0));
        assert_eq!(spec.slot_for_byte(57), Some(7));
        let mask = spec.slots[0].as_ref().unwrap();
        assert_eq!(mask.size, 128);
        let open = mask.open_fraction();
        assert!(
            open > 0.05 && open < 0.7,
            "a gobo is partly open, got {open}"
        );
        assert!(spec.prism_from.is_none());
    }

    /// r[verify viz.gobo-raster] - the transparent-ground convention reads the same as the black-disc one
    #[test]
    fn uking_wheel_reads_open_slot_and_prism() {
        let spec = wheel_from_gdtf(
            &gdtf("UKing@ZQ02341_150W_Big_Steel_Gun_LED_Moving_Head_Beam@2024-R001.gdtf"),
            None,
        )
        .unwrap()
        .unwrap();
        assert!(spec.has_art);
        assert!(spec.slots[0].is_none(), "slot 1 is Open");
        assert_eq!(spec.slot_for_byte(0), Some(0));
        assert_eq!(spec.slot_for_byte(45), Some(5), "byte 45 is the star");
        let star = spec.slots[5].as_ref().unwrap();
        assert_eq!(star.size, 256);
        // Centre of the star is open; the corner (transparent) is blocked.
        assert!(star.openness(128, 128) > 0.9);
        assert!(star.openness(2, 2) < 0.1);
        assert_eq!(spec.prism_from, Some(1));
        assert_eq!(spec.prism_spin_from, Some(128));
        assert!(spec.prism_in(40));
        assert!(!spec.prism_in(0));
    }

    /// r[verify viz.gobo-raster] - a profile without art gets the built-in set
    #[test]
    fn generated_profile_gets_builtins() {
        let spec = wheel_from_gdtf(
            &gdtf("generated/ZKYMZL@Mini_Gobo_Moving_Head_Light_11ch@ignition.gdtf"),
            None,
        )
        .unwrap()
        .unwrap();
        assert!(!spec.has_art);
        assert!(spec.slots[0].is_none());
        assert!(spec.slots[1..].iter().all(std::option::Option::is_some));
        assert!(spec.spin_for_byte(0).is_none());
        assert!(spec.spin_for_byte(150).is_some());
    }

    /// r[verify viz.gobo-raster] - eight built-ins, each a different, partly open pattern
    #[test]
    fn builtin_set_is_eight_distinct_masks() {
        let set = builtin_masks();
        assert_eq!(set.len(), 8);
        let mut fractions = Vec::new();
        for (name, mask) in &set {
            assert_eq!(mask.size, GOBO_SIZE);
            assert_eq!(
                mask.rgba.len(),
                usize_of_u32(GOBO_SIZE.saturating_mul(GOBO_SIZE).saturating_mul(4))
            );
            let open = mask.open_fraction();
            if *name == "open" {
                assert!(open > 0.75, "open is the whole gate, got {open}");
            } else {
                assert!(
                    open > 0.05 && open < 0.7,
                    "{name} is partly open, got {open}"
                );
            }
            // Outside the gate is always blocked.
            assert!((mask.openness(0, 0) - (0.0)).abs() < 1e-6);
            fractions.push(mask.rgba.clone());
        }
        fractions.sort();
        fractions.dedup();
        assert_eq!(fractions.len(), 8, "no two built-ins are the same image");
    }

    /// r[verify viz.gobo-raster] - the decal is the beam's field at the surface it lands on
    #[test]
    fn decal_matches_beam_angle() {
        let reach = 6.0;
        let half = 12.5_f32;
        let width = pattern_width(reach, half);
        assert!((2.0 * reach).mul_add(-half.to_radians().tan(), width).abs() < 1e-5);
        let t = facet_transform(0, reach, width, 0.0, false, 0.0);
        assert_eq!(t.scale, Vec3::new(width, width, reach * DECAL_DEPTH_FACTOR));
        // Near face at the gate, projecting down -Z past the surface.
        let near = t.scale.z.mul_add(0.5, t.translation.z);
        let far = t.scale.z.mul_add(-0.5, t.translation.z);
        assert!(near.abs() < 1e-5);
        assert!(far < -reach);
        // Spin turns the pattern about the beam, not the beam itself.
        let spun = facet_transform(0, reach, width, 1.0, false, 0.0);
        let axis = spun.rotation * Vec3::NEG_Z;
        assert!((axis - Vec3::NEG_Z).length() < 1e-5);
    }

    /// r[verify viz.gobo-raster] - three prism facets, equally spread, equally spaced
    #[test]
    fn prism_triplet_geometry() {
        use core::f32::consts::TAU;
        let (reach, width) = (5.0, 1.0);
        let axes: Vec<Vec3> = (0..PRISM_FACETS)
            .map(|f| facet_transform(f, reach, width, 0.0, true, 0.3).rotation * Vec3::NEG_Z)
            .collect();
        for axis in &axes {
            let off = axis.angle_between(Vec3::NEG_Z).to_degrees();
            assert!(
                (off - PRISM_SPREAD_DEG).abs() < 0.01,
                "facet is {off} deg off axis"
            );
        }
        for i in 0..PRISM_FACETS {
            let a = axes[i];
            let b = axes[(i + 1) % PRISM_FACETS];
            // Projected onto the plane across the beam, neighbours sit 120 deg apart.
            let (pa, pb) = (a.truncate(), b.truncate());
            let sep = pa.angle_to(pb).abs().to_degrees();
            assert!(
                (sep - 360.0 / f32_of_usize(PRISM_FACETS)).abs() < 0.1,
                "{sep}"
            );
        }
        // Rotating the prism turns the triplet round the beam.
        let turned =
            facet_transform(0, reach, width, 0.0, true, 0.3 + TAU / 4.0).rotation * Vec3::NEG_Z;
        let sep = axes[0]
            .truncate()
            .angle_to(turned.truncate())
            .abs()
            .to_degrees();
        assert!((sep - 90.0).abs() < 0.1, "{sep}");
    }

    #[test]
    fn builtin_default_wheel_steps_eight_bytes() {
        let spec = WheelSpec::builtin_default();
        assert_eq!(spec.slots.len(), 8);
        assert_eq!(spec.slot_for_byte(0), Some(0));
        assert_eq!(spec.slot_for_byte(7), Some(0));
        assert_eq!(spec.slot_for_byte(8), Some(1));
        assert_eq!(spec.slot_for_byte(255), Some(7));
    }
}
