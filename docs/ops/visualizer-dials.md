# The visualizer's dials

Every knob that changes what the visualizer costs or what it looks like,
in one place. All of them are environment variables read at startup; the
haze *look* dials are also a live resource (`haze_texture::HazeLook`),
so anything holding it can move them while a show runs.

The rule throughout: **a per-dial variable outranks the preset.** A tier
is for choosing a picture; a single variable is for finding out what one
dial costs on one GPU, or for judging one decision by eye.

## Which renderer

| Variable | Default | What it does |
| --- | --- | --- |
| `IGNITION_FROXEL` | off | `1` selects the froxel grid instead of the screen-space march. The march is what ships: the grid is roughly three times cheaper, does not bead a beam and carries gobos and shadows in the air, but as it stands it renders a room with no beams in it — see `docs/domain/froxel-volumetrics.md`. |

## Cost — the froxel grid

Sized by `IGNITION_QUALITY` (`potato`, `low`, `medium`, `high`, `ultra`;
`medium` by default), and each dial overridable on its own.

| Variable | Medium | What it buys |
| --- | --- | --- |
| `IGNITION_FROXEL_GRID` | `400x225x96` | Width buys a beam's **sharpness** — the grid is filtered, so a tile spanning sixteen screen pixels gives a beam a sixteen-pixel edge. Depth buys its **continuity**. Width is the more expensive half and usually the better buy. |
| `IGNITION_FROXEL_SAMPLES` | `2` | Points lit per froxel. A beam's bright core is thinner than a froxel, so one sample renders it as a dashed line. This resolves that *within* one frame, which is what a still needs. |
| `IGNITION_FROXEL_HISTORY` | `0.9` | How much of last frame's grid survives. The other half of the same problem, and the half that costs nothing — but too much smears behind a moving mover. `0` disables it. |

At 5120x1440 on the benchmark cue, with effects and mover effects
running, gobos and shadows on:

    potato  240x135x64   x1   173 fps
    low     320x180x96   x1   158 fps
    medium  400x225x96   x2   126 fps
    high    480x270x72   x2   127 fps
    ultra   640x360x96   x2    90 fps

against **55 fps** for the march at medium. High is not a typo: it
spends its budget on width rather than depth slices.

## Look — the haze

These say what the room looks like, not what it costs, which is why they
are not on the quality ladder.

| Variable | Default | What it does |
| --- | --- | --- |
| `IGNITION_HAZE_ANIMATE` | on | `0` returns the room to one flat density everywhere — what every snapshot before this was made with. |
| `IGNITION_HAZE_SWING` | `0.75` | How far the density swings either side of average, 0..1. Below about 0.4 a beam is still a clean bar; past 0.85 it reads as speckle rather than as air. |
| `IGNITION_HAZE_BANKS` | `6` | How many banks of haze fit across the room. Higher is finer, and past about 12 the structure is too small to read as haze hanging in the air. |
| `IGNITION_HAZE_DRIFT` | `1.0` | How fast the banks move, as a multiple of the built-in rate. Slow on purpose: anything quick reads as smoke rather than as the room's air. |
| `IGNITION_HAZE_DILUTION` | `2.0` | The exponent the fog's distance attenuation falls off with. Two is the physical inverse square; lower carries a beam further than physics does, which is what grandMA3 exposes as dilution. It touches the fog alone — surfaces stay physical either way. Around 1.2 to 1.4 is where a shaft starts reading the way one does in a photograph. |

Note that the density texture is a **multiplier that must average one**.
Both renderers multiply the room's density by it, so a texture averaging
anything else silently rescales the haze and every calibration above it
— which is why it is stored as half floats rather than bytes, and why a
first cut centred on 0.75 dimmed the benchmark cue by eighteen per cent.

## The march — what ships

| Variable | What it does |
| --- | --- |
| `IGNITION_FOG_STEPS` | Raymarch steps. |
| `IGNITION_HAZE_PIXELS` | The pixel budget for the reduced-size haze camera. |
| `IGNITION_FOG_SCALE` | Forces a divisor for that camera; `0` uses the budget, `1` marches on the camera itself. |

## Everything else

| Variable | What it does |
| --- | --- |
| `IGNITION_QUALITY` | The preset name. An unknown name warns and uses the default. |
| `IGNITION_TAA`, `IGNITION_SSR`, `IGNITION_SSAO` | Individual post-process switches. |
| `IGNITION_VIZ_GOBO=<byte>[,<prism>]` | Forces a gobo wheel byte on every projector, so a still can show a gobo without a cue that selects one. |
| `IGNITION_SHADOWS`, `IGNITION_SHADOW_MAP` | Shadow casting, and the shadow map's resolution. |

## Judging these

Three cues exist for it. `data/songs/beam-tests.json` is the matrix —
one cue per case: vertical, crossed, aimed at the house, long and short
throws, a single beam, a par's mouth, and the whole rig at once. Render
the lot through both renderers with `just beam-matrix`, which
contact-sheets them so an artefact can be seen in the case that
provokes it rather than in whichever picture happened to be open. It
earned itself immediately: it showed the froxel path rendering a room
with no beams in it, which one picture had not.

`data/songs/beam-alias.json` is crossed
near-vertical movers — the picture that exposes beading, because a
near-vertical beam seen from the house is crossed by the camera ray
rather than followed along it. `data/songs/benchmark.json` is the worst
case for cost: effects, pulses and mover effects all running.

A still cannot show drift or ghosting. The haze look and the history
weight both need judging live, with the movers running.
