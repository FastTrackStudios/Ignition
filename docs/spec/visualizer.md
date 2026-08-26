# Visualizer

The visualizer is a fixture-level simulation, not a picture of the cue list.

r[viz.driven-by-dmx]
The visualizer MUST render from the **DMX bytes** the engine outputs — the same
universes a rig would receive — decoded through each fixture's channel map,
never from the engine's attribute values directly. A visualizer that reads
attributes cannot show a patch mistake, a curve, a multipatch, or a fine
channel; one that reads bytes shows exactly what the lights would.

r[viz.export]
The visualizer MUST be able to render a show to a **video file** offline —
frame by frame against the song's clock, at a chosen size — so a look can be
reviewed away from the desk and sent to someone who has no console.

r[viz.gobo-raster]
A fixture with a gobo selected SHOULD render that gobo's pattern in its beam
and on the surface it lands on. (Deferred until the beam material supports a
projected texture; noted, not built.)

r[viz.gdtf-meshes]
A fixture with a GDTF profile MUST be drawn with that profile's **real
geometry**: a `<Model>` that ships a 3D file (`models/gltf/*.glb`, or failing
that `models/3ds/*.3ds`) renders that mesh, and a `<Model>` that names one of
the spec's standard primitives (Base, Yoke, Head, Scanner, Conventional and
their 1.1 variants; Cube, Cylinder, Sphere, Pigtail) renders the spec's own
primitive mesh, not a box. Every mesh is scaled to the Model's declared
Length/Width/Height (X/Y/Z), as the spec requires, in the venue's fixture
material — the file's own materials and textures are ignored. A file that
cannot be decoded falls back to a box of the declared size rather than
vanishing.

r[viz.gdtf-generated]
A fixture with no manufacturer GDTF MAY be given a **generated profile**
(`tools/make_gdtf.py`, from a spec JSON plus a base `.gdtf` whose 3D geometry
it borrows). A generated profile MUST carry the venue's console name as its
fixture-type `Name`, verbatim, so the visualizer matches it the same way it
matches a real file; its DMX modes, wheels and emitters MUST come from the
spec, never from the base, and every mode MUST import through
`gdtf_import::import_channel_map`.

r[viz.gdtf-aliases]
The visualizer's fixture library MUST load `data/gdtf` and its immediate
subdirectories (`data/gdtf/generated`) together, and MUST resolve a patched
fixture's model string to a profile in this order: (1) the model string equals
a fixture-type name, (2) `data/gdtf/aliases.json` maps the model string to a
fixture-type name, (3) a normalized substring match in either direction, as a
last resort. Names compare case- and punctuation-insensitively. When a
downloaded and a generated profile share a name the generated one (in the
subdirectory) MUST win, deterministically. A missing library directory MUST
not prevent the visualizer opening. Every fixture patched in a shipped venue
MUST resolve to a profile that draws at least one real mesh.

r[viz.emitter-at-beam-node]
A GDTF fixture's light MUST originate at its `<Beam>` node — the profile's
own statement of where the light leaves the fixture, "usually the position of
the lens" — and fire along that node's -Z, the spec's beam direction, for
every profile: the emitter is the `<Beam>` entity in the transform tree, so
every joint and offset above it applies. No other node is an emitter: a
`<Geometry>` with no model (a DMX socket, a power inlet) draws nothing and
emits nothing.

r[viz.one-emitter-tree]
Every emitter MUST be a node of its fixture's transform tree, placed by that
tree alone: the point emitter is the profile's `<Beam>` entity, a bar's strip
emitter is a child of the node its cells hang from, placed from the cells'
own local poses, and the beam cone, wedge, spill light and glow faces are
children of the emitter with identity transforms. Aim is
mount x pan joint x tilt joint x beam-node local, composed by transform
propagation — no code recomputes an emitter's world pose from offsets, a
guessed yoke split or a pre-rotation. Only a fixture with no profile at all
takes the QLC+ placeholder path, whose emitter is the guessed head.

r[viz.profile-optics]
The resolved profile's `<Beam>` node is the single source of truth for how
wide a fixture's light is: `BeamAngle` is the bright shaft (the 50% edge,
the spill's inner cone), `FieldAngle` the spill's outer cone (the 10% edge),
and a profile with no wider `FieldAngle` is read as a field of twice its
beam. The patch's `beam_angle_deg` is consulted only for a fixture with no
profile, as its beam with the same assumed field. A generated profile MUST
carry the spec's `optics.beam_angle_deg` / `field_angle_deg`, writing
2 x beam when the datasheet publishes no field.

r[viz.beam-reach]
A beam MUST NOT end in mid-air. Its throw is sized by the room's real
surfaces (`Venue::room_extent`), not the object-centre bounds; a spill
light's range reaches comfortably past every surface; the haze volume fills
the whole room; and the drawn cone fades to nothing over its last stretch
rather than ending in a lit cap.

r[viz.exposure]
A spill light MUST be fed the fixture's real peak candela (lumens over its
field cone) — Bevy spreads a spot light's lumens over the full sphere
whatever its cone, so raw lumens made a 1.7-degree beam and a 60-degree par
equally bright per direction — scaled by one exposure dial that is a plain
multiplier on real photometry. Candela is capped so a beam fixture reads as
a hard shaft in the same frame as a par without flooding the haze.

r[viz.bar-emitters]
A profile with four or more `<Beam>` nodes on one line, firing the same way
square to that line, is a **bar** and MUST be drawn as one linear source, not
a row of point sources: the emissive face is the whole strip (one face per
cell, tiling from end to end, each carrying its cell's colour), the shaft is
one wedge — a frustum of rectangular section whose near face is the strip
and whose sides open by the beam angle — and the spill is one wide light,
never a cone per cell.

r[viz.body-glow]
A fixture's housing MUST NOT emit light of its own by default: the real
fixtures are black, lit only by the rig around them. A **fixture body glow**
option, off unless asked for (`viz --body-glow`, the studio's GLOW key,
`IGNITION_BODY_GLOW=1`), lets a lit fixture's housing glow the colour it is
putting out, as a way of reading which fixtures are on.

r[viz.performance-budget]
The studio's viewport MUST hold **120 frames per second at 5120×1440** on the
reference GPU (an RTX 4080) on the benchmark cue (`data/songs/benchmark.json`
at Norco: every mover in a fast figure, every par on a chase and a rainbow,
the bars on a strip chase, the beams strobing, the canvases on `proc:rainbow`,
hazers up), measured through the studio's own embedded route by
`viz --bench`. The frame is spent by rule, not by fixture count:

The benchmark cue has **every light on the whole time**: every par and bar
on a bed of at least half, with the chases riding on top of it and never
taking a fixture to black — a frame measured with the pars chased to black
is a frame measured with most of the rig off.

- **Shadow maps** go to at most `SHADOW_BUDGET` spill lights, the brightest
  per direction (peak candela) among those that cut a shaft **and move**. A
  shadow map is for the silhouette a sweeping beam cuts out of a person; a
  fixed wash's pool lands where it always lands, and it never gets one,
  whatever the budget has room for. A wash keeps its pool and its cone in
  the haze.
- **Volumetric light** (the fog raymarch) goes to at most
  `VOLUMETRIC_BUDGET` spill lights, ranked by brightness alone. A cut never
  splits a run of equal fixtures: a type is in or out as a whole. A
  shaft-cutter over the budget keeps its pool and **shows in the air as the
  hand-drawn additive cone** — a par's light must be visible in the air, or
  the par reads as dead. The cone is cheap by design: two taps of value
  noise for its grain, not four octaves of simplex, and it is drawn on the
  haze camera at the haze's fraction of the picture, never at full size.
  `IGNITION_PAR_CONES=0` turns the cones off, for comparing.
- **The haze is marched at a fraction of the picture's size** on a camera of
  its own and composited back — added, with the room behind it dimmed by the
  same transmittance the in-camera fog would have applied. The fraction is
  chosen so the haze camera stays under a fixed pixel budget
  (`HAZE_PIXEL_BUDGET`) whatever the viewport; a still keeps full size. The
  drawn cones and the bars' wedges ride the same camera and reach the
  picture through the same composite.
- **Fixture housings cast no shadow and block no shaft**: a par's body is a
  hand's width across and hangs above every beam. Only the room, the risers,
  the props and the people are occluders.
- **The render world runs on its own thread** in the studio (Bevy's
  pipelined rendering), so a paint costs the main world's update plus the
  wait for the previous frame's render, not the two added together. The
  target texture reaches the host through a mailbox the render thread
  posts to, never through a readback or a copy.
- **Nothing per-frame allocates or uploads for a rig standing still**: a
  bar's cell materials change only when the colour does; a wedge is rebuilt
  only when its length does; a procedural canvas rasters on a worker and
  uploads only a fresh frame.
- **One mesh asset per fixture model**, shared by every fixture of that
  type, so the engine batches them.
