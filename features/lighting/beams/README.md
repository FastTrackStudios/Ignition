# ignition-beams

Every beam in the air, drawn analytically. It replaces Bevy's volumetric
fog in the visualizer: there is no grid, no march step and no dependence on
earlier frames, so a beam cannot bead or block at any pan or tilt speed or
strobe rate. The plan it follows is step 3 of
`research/ignition-visualizer/REPORT.md` (2026-11-18).

## How a beam is drawn

1. **A hull decides which pixels run.** One instanced 24-sided truncated
   cone per beam, built in the vertex shader from the vertex index, a
   little larger than the beam and starting at the lens's real width. Its
   back faces are drawn, so a pixel runs once per beam whether the camera
   is outside the cone or in it.
2. **The pixel finds its exact segment.** It intersects its view ray with
   the cone itself (a quadric cut at the lens plane and the far cap), as
   intervals, and cuts the result at the first surface in the depth
   buffer. The hull never bounds the segment.
3. **It integrates in closed form.** Single scattering from a point source
   at the beam's *virtual apex*, `lens radius / tan(edge)` behind the lens.
   The inverse square from the apex is integrated exactly (an arctangent)
   on each of 4 to 16 equal pieces of the segment; each piece is weighted
   at its middle by the radial profile, a Henyey-Greenstein phase term,
   extinction and an end fade. Every term is continuous.
4. **Beams add.** Into an `Rgba16Float` air target, each with its own soft
   knee. A composite pass adds that to the view, paling the brightest
   parts to white instead of clipping them in the beam's colour, and
   dithers below one output step.

Per-beam data is a storage buffer of seven `vec4`s (`GpuBeam`, `Beam` in
`shaders/scatter.wesl`).

## Using it

```rust
app.add_plugins(BeamsPlugin);
commands.spawn((Camera3d::default(), Hdr, Msaa::Off, BeamCamera::default()));
commands.spawn((Beam { flux: 9_000.0, ..default() }, Transform::from_xyz(0.0, 7.0, 0.0)));
```

A `Beam` shines along its entity's −Z from its origin, as a Bevy
`SpotLight` does, so the two can share an entity (the spot light lights
the surface the beam lands on; this crate draws only the air).

- **Brightness is photometric.** `Beam::flux` is the fixture's luminous
  flux in lumens (GDTF `LuminousFlux`); its on-axis candela is that flux
  over the solid angle its profile covers (`math::peak_candela`), so a
  zoomed-in beam is brighter by exactly what it narrows. Candela ×
  scattering per metre × the integral is nits, and the camera's `Exposure`
  turns nits into the frame. `Haze::scattering_at_full` is the one number
  that says how bright the air is.
- **The profile** is 1 on the axis, ½ at the GDTF beam angle, ⅒ at the
  field angle and zero at 1.15 × the field, where the hull ends.
- **Gobos** come from one texture array with mips (`BeamGobos`,
  `gobo_image`). Focus and frost blur a gobo by mip level; frost also
  widens the field.

## Tiers and the governor

| Tier | Pieces | Gobo in the air |
|---|---|---|
| low | 4 | none |
| medium | 6 | one sample where the ray passes closest to the axis |
| high | 8 | 8 samples along the segment |
| ultra | 16 | 16 samples |

The stepped gobo only multiplies the closed-form light by a ratio of two
sums with the same weights, so it is between 0 and 1 and exactly 1 with no
gobo: it cannot make the beam itself noisy. Each sample's mip level is
chosen from how far the gobo moves across the stretch of ray the sample
stands for, so the samples are averages, not points.

`BeamGovernor` steps a camera's quality from the measured frame time: down
the tiers, then, as a last resort, to a half-resolution air target with a
depth-aware upsample. It climbs back slowly and only with headroom.

## What is not built yet

- **Occluders inside a beam.** A performer or truss in front of a beam
  hides it (the depth cut); one *inside* a beam does not cut the shaft
  behind it. The report's per-beam occluder map is not here.
- **Haze that is not uniform.** No density noise; `Beam::haze_response`
  is a per-beam lever only.
- **Prisms** have no helper: a prism is one `Beam` per facet, which the
  data model allows, and the caller makes them.
- **Colour** is whatever linear RGB the caller gives; nothing here reads
  emitter spectra or filters.
- **MSAA and sub-viewports.** The camera must have `Msaa::Off` (a multisampled
  camera gets no beams) and must cover its whole target. A
  temporally jittered projection is not followed, so with TAA the depth
  cut is off by under a pixel.

## Wiring it into the visualizer

Not done here: it follows `feat/bevy-fork-gpu-frames` landing.

1. Delete this crate's `[workspace]`, `[workspace.lints]` and `[profile]`
   tables, add it to the root `members`, and take `bevy` from the root
   `[workspace.dependencies]` (same revision already).
2. `ignition-viz` depends on `ignition-beams` and adds `BeamsPlugin`. The
   room camera gets `BeamCamera` (and `BeamGovernor`), behind
   `IGNITION_BEAMS=1` until the A/B is done.
3. Where `spawn.rs` gives an emitter a `SpotLight` and `VolumetricLight`,
   it gets a `Beam` on the same entity. Each frame, where the fixture's
   state is written to the light, it is written to the `Beam` too: the
   beam and field angles at the current zoom, `BeamRadius`,
   `LuminousFlux`, the colour, dimmer × strobe, the gobo slot and
   rotation, frost. The spot light's intensity becomes
   `peak_candela(..) × 4π` so pools and shafts agree.
4. The gobo images already loaded for the decals are packed into one
   array with `gobo_image` and set as `BeamGobos`.
5. The hazers' level drives `BeamCamera::haze.density`.
6. Then delete: `FogVolume`, `VolumetricFog`, `VolumetricLight`, the haze
   camera and its occluder twins in `haze.rs`, `vendor/bevy_pbr`, and the
   shadow maps that existed only so movers could be in the fog.

## Proving it

`examples/truss` is a dark room with three trusses of moving heads (beam
fixtures of 1.8°, spots, washes). `--headless --out DIR` renders the
scenes that break a marched fog to PNG with no window; `--bench` measures
the pass's GPU time. `docs/lighting/research/analytic-beams.md` has the
captures and the measured cost.

```sh
cargo test --release
cargo run --release --example truss
cargo run --release --example truss -- --headless --out target/tmp/beams
```

## Provenance and licence

GPL-3.0-or-later, as Ignition is (`LICENSE` at the repository root).

Parts of the shaders are ported from **ASLS Studio**
(<https://github.com/ASLS-org/studio>, GPL-3.0, read at `88f276f`),
copyright its authors:

| From | What | Where it is here |
|---|---|---|
| `src/plugins/visualizer/air_beams.js`, `AIR_FRAGMENT` | The cone cut at the lens with its apex behind it; the arctangent integral of the inverse square over equal pieces, each weighted by profile, phase, extinction and end fade at its middle; the Henyey-Greenstein phase; the closest-approach point; the soft and the stepped gobo; the per-beam soft knee | `shaders/scatter.wesl`, `shaders/beams.wesl`, mirrored in `src/math.rs` |
| `air_beams.js`, `COMPOSITE_FRAGMENT` | The depth-weighted four-tap upsample of a reduced air buffer | `shaders/composite.wesl`, `air_at` |
| `src/plugins/visualizer/beam_shading.js`, `ROLLOFF_GLSL`, `createConeGeometry` | The roll-off towards white; the hull ring pushed out by `1 / cos(π / segments)` | `shaders/composite.wesl`, `roll_off`; `shaders/beams.wesl`, `RING_SCALE` |

Changed from ASLS, on purpose:

- The segment is found by intersecting intervals (slab, then quadric), not
  by classifying crossings with a step past each one, and the quadric's
  coefficients are built from explicit radial parts so a narrow beam far
  away keeps its precision in single floats.
- No softening of the inverse square (`k / (1 + k d²)`): with the apex
  behind the lens the distance is never zero. The arctangent difference
  is taken as one `atan2`, exact as the ray nears the apex's line.
- Intensity is the fixture's flux over its profile's solid angle
  (exponent 2, in candela), not `(flux / 5000)^0.5` with exponent 1.5 and
  gains set by eye. The phase function is per steradian, not relative to
  a side view.
- The profile passes through GDTF's beam and field angles instead of
  `1 − smoothstep(0.55, 1, r)`.
- Extinction is along the light's path as well as the view ray's.
- The stepped gobo's samples are prefiltered by footprint, and its
  jitter can be turned down to none.
- The roll-off leaves the brightest channel alone unless asked to
  compress, because a tonemapper follows it here.
