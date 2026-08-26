# Canvases and pixel content

A canvas is a surface a venue declares — three back-wall TVs as one, a pixel
wall, a batten of cells — that a show addresses by role name
(`r[files.venue.canvases]`). The video path already plays a clip onto it. This
spec is the other half: **generated** content, and content that drives
fixtures rather than pixels. grandMA3 calls this Bitmap fixtures and Bitmap
channels; the idea is that a picture moving across a grid is an effect.

r[canvas.grid]
A canvas MUST expose its members as a two-axis grid derived from their real
positions (`r[tricks.grid.from-space]`), so "a wipe from left to right across
the wall" means the same thing on a 3-panel and a 64-cell wall.

r[canvas.procedural]
A canvas MUST be able to show **procedural** content — a gradient, a wipe, a
noise field, a scrolling band, a sparkle — authored as a recipe against the
canvas role, timed like any effect, and resolved per member from its grid
position. No file is needed for a colour sweep.

r[canvas.bitmap-channels]
The content's brightness or hue at a member's position MUST be applicable to
**any attribute** of that member — dimmer, colour, tilt, zoom — so a video's
brightness can drive a row of movers' tilt, which is what makes a canvas a
fixture-grid effect rather than a screen.

r[canvas.on-the-stack]
Canvas content MUST enter the same stack as every other value
(`r[playback.stack]`): a canvas recipe is a recipe, cooked and layered like the
rest, and a hit above it still wins.

r[canvas.clip-is-a-source]
A clip playing on a canvas MUST be one kind of source among the procedural
ones, on the same clock, so a show can crossfade from a clip to a gradient
and a lyric layer can sit over either.

## Implementation notes

The procedural sources, the `CanvasRecipe` (`source` + ordinary recipe
`Timing`), and the bitmap-channel mapping live in
`ignition_core::canvas`. Everything there is a pure function of
`(u, v, cycles)`, seeded and deterministic, so the viz, the cooker and a
test paint the same picture at the same song position.

**Where the recipe engine hooks in.** A canvas recipe over a fixture
selection should be expanded with
`canvas::sample_for_grid(&recipe, &bitmap_channel, &cells, cycles)`, where
`cells` is each member's `(ChanId, u, v)` from the rig grid
(`r[tricks.grid.from-space]` — `Grid::from_rig` in `tricks`) and `cycles`
is the same `Timing::cycles` the engine would hand a phaser. The result is
a list of `(ChanId, Attribute, value)` to emit onto the stack like any
other recipe output (`r[canvas.on-the-stack]`).

**Content strings.** In the viz, a canvas's content is a still or a clip
by file extension, or procedural when it starts with `proc:` — either a
built-in name (`proc:rainbow`, `proc:wipe`, `proc:noise`, `proc:bands`,
`proc:sparkle`) or a JSON `CanvasRecipe` literal (`proc:{"source":…,
"timing":…}`). Procedural frames are rasterised at most 320 px wide at the
canvas's own aspect, on the same `CanvasClock` as a clip, and go through
the same slice and cover-fit path.
