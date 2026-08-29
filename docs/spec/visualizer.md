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
reviewed away from the desk and sent to someone who has no console. The
export follows the show's camera cut (`r[viz.camera-cuts]`): the cues'
`camera …` commands drive the programme camera on the same clock the
lighting runs on, so an exported video is the cut programme, and
`--camera <preset>` names the camera it opens on.

r[viz.gobo-raster]
A fixture with a gobo selected MUST render that gobo's pattern on the surface
its beam lands on, as a **clustered decal** hung off the emitter — a child of
the `<Beam>` node in the fixture's own transform tree (`r[viz.one-emitter-tree]`),
so pan and tilt reach it by propagation and it is aligned with the beam by
construction. The decal paints black wherever the gobo is closed, masking the
spill light to the pattern; it is sized every frame to the beam's field angle
at the surface the beam reaches (`BeamThrow`), so the pattern is the beam's
width where it lands. The wheel comes from the profile: the `Gobo1` channel's
`ChannelSet`s map bytes to `WheelSlotIndex`, and each slot's `MediaFileName`
is the PNG in the `.gdtf` archive (opaque white is open; black and transparent
are blocked, which reads both manufacturers' conventions the same). A profile
that names a gobo wheel but ships no art gets a built-in set of eight
procedural patterns (dots, bars, breakup, star, spiral, radial, tri, open); a
fixture with a `GoboWheel` channel and no profile gets the same set at uniform
eight-byte steps. A `Prism1` channel in its "in" range splits the pattern into
three decals thrown a few degrees off axis, 120 degrees apart, turning when
the prism-rotation range is engaged; the spill light itself is not split.
On the screen-space march, the fog does not read decals, so the shaft in the
haze stays unbroken and only what lands on a surface carries the pattern. On
the froxel path (`r[viz.haze-is-volumetric]`) the injection reads the same
decals, so a gobo MUST also appear **in the air**: each froxel takes the
stencil of the lamp whose gate the decal hangs from, sampled with the beam's
radius at that depth so the pattern converges on the gate rather than running
down the beam as a cylinder.

r[viz.gdtf-meshes]
A fixture with a GDTF profile MUST be drawn with that profile's **real
geometry**: a `<Model>` that ships a 3D file (`models/gltf/*.glb`, or failing
that `models/3ds/*.3ds`) renders that mesh, and a `<Model>` that names one of
the spec's standard primitives (Base, Yoke, Head, Scanner, Conventional and
their 1.1 variants; Cube, Cylinder, Sphere, Pigtail) renders the spec's own
primitive mesh, not a box. Every mesh is scaled to the Model's declared
Length/Width/Height (X/Y/Z), as the spec requires. A GLB is loaded by Bevy's
own glTF loader (`bevy_gltf`) straight from the `.gdtf` zip, through a
`gdtf://` asset source, so it keeps the materials, tangents and node
hierarchy the file ships; the load is asynchronous, and until it lands the
part is a box of the declared size, swapped for the scene without touching
the fixture's emitter. A 3DS carries no materials and is drawn in the venue's
fixture material. A file that cannot be decoded falls back to a box of the
declared size rather than vanishing.

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
own local poses, and the spill light and glow faces are children of the
emitter with identity transforms. Aim is
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
the whole room; and a shaft in the haze ends only where the fog's own
extinction and the light's inverse-square decay end it.

r[viz.exposure]
Lights carry their real photometry and the camera decides the picture.
A spill light MUST be fed the fixture's real peak candela (lumens over its
field cone) — Bevy spreads a spot light's lumens over the full sphere
whatever its cone, so raw lumens made a 1.7-degree beam and a 60-degree par
equally bright per direction — with nothing scaled and nothing capped: a
150 W beam fixture is its datasheet's million candela. The level is the
camera's: every main camera carries a physical `Exposure` at one stage
EV100 (`STAGE_EV100`, chosen so a 36 W par's 180 lux at 3 m reads as a soft
wash on a black deck and the beam saturates through its core), the haze
camera carries the same one so the fog is at the level the room is, the
ambient fill is lux under the same exposure, and the operator's
`--exposure` is stops on that EV, not a multiplier on the lights.

r[viz.bar-emitters]
A profile with four or more `<Beam>` nodes on one line, firing the same way
square to that line, is a **bar** and MUST be drawn as one linear source, not
a row of point sources: the emissive face is the whole strip (one face per
cell, tiling from end to end, each carrying its cell's colour), and the spill
is one wide volumetric light, never a cone per cell — in the haze it shows as
one wide fan from the strip.

r[viz.body-glow]
A fixture's housing MUST NOT emit light of its own by default: the real
fixtures are black, lit only by the rig around them. A **fixture body glow**
option, off unless asked for (`viz --body-glow`, the studio's GLOW key,
`IGNITION_BODY_GLOW=1`), lets a lit fixture's housing glow the colour it is
putting out, as a way of reading which fixtures are on.

r[viz.haze-is-volumetric]
Light in the air comes from Bevy's volumetric fog and nothing else: one
`FogVolume` filling the room, and every emitter's spill light a
`VolumetricLight` — pars, movers and bars alike, with real occlusion by the
room and the people in it. No beam mesh, cone or wedge is ever drawn. What
shows in the air is decided by the haze in the room, not per fixture:

- **The hazers put the haze there.** A fixture whose model names it a hazer
  or fogger emits no light; its output channel drives `HazeLevel`, the
  room's haze as a fraction of a normally hazed room. The level settles
  toward the hardest-working hazer with a first-order lag — seconds to
  build, longer to fall, since haze lingers — and never below a residual
  (`HAZE_RESIDUAL`): a hazed room does not clear, and a show that never
  touches its hazers is lit in that residual, not in clean air. A venue with
  no hazer patched is taken as normally hazed.
- **The operator's `--haze` dial multiplies the level.** The fog's density
  is dial × level × one calibration constant, and the same density sets the
  occluders' extinction on the haze camera, so the composite dims the room
  exactly as much as the fog lights it. At a dial of zero, or with no haze
  in the room, nothing shows in the air — only the pools on surfaces.
- **The haze is uneven and it drifts.** A hazed room is not a
  homogeneous medium: the output hangs in banks, thins where the air
  moves, and crosses the stage all night. The room's density is
  therefore multiplied by a tileable 3D noise texture scrolled slowly
  over time (`FogVolume::density_texture` and `density_texture_offset`),
  so a beam brightens and fades along its length instead of reading as a
  clean bar. The texture MUST average one — both renderers multiply by
  it, so any other average silently rescales the room and every
  calibration above it. `IGNITION_HAZE_ANIMATE=0` returns the room to
  one flat density, and `IGNITION_HAZE_SWING`, `IGNITION_HAZE_BANKS` and
  `IGNITION_HAZE_DRIFT` shape it. What the air *looks* like is settings,
  not a quality tier: these live in one resource that may change while
  the show runs, and they are listed with every other dial in
  `docs/ops/visualizer-dials.md`.
- **Haze is a gain on scatter, not on extinction:** the fog's
  `light_intensity` lifts what the air scatters toward the camera so a par's
  cone reads at a density that leaves a twenty-metre house visible through
  it; extinction stays physical.

Two implementations of that fog exist and MUST agree about the room's
brightness — and until they demonstrably do, the screen-space raymarch is
what ships. The other is a **froxel grid**: a frustum-aligned voxel grid,
lit once per froxel rather than once per pixel per step, integrated front to
back and sampled at each pixel's own depth. `IGNITION_FROXEL=1` selects it. The froxel path MUST carry the same lighting the march does —
the same clustered light walk, spot cone, distance attenuation and shadow
map — and MUST additionally read the gobo decals (`r[viz.gobo-raster]`), so
a stencil shows in the air and not only where the beam lands. Because a
beam's bright core is thinner than a froxel, the grid is sampled at more
than one point per froxel and blended with the previous frame's grid,
reprojected through the camera's motion; without both a beam renders as a
dashed line down its own length.

r[viz.quality-presets]
The live picture is chosen by name from a ladder — `potato`, `low`,
`medium`, `high`, `ultra` — set with `IGNITION_QUALITY` and defaulting to
`medium`, which is what the studio has always rendered and must stay so
byte for byte. Every dial remains overridable on its own — on the froxel path
`IGNITION_FROXEL_GRID`, `IGNITION_FROXEL_SAMPLES` and
`IGNITION_FROXEL_HISTORY`; on the march `IGNITION_FOG_STEPS`,
`IGNITION_HAZE_PIXELS` and `IGNITION_FOG_SCALE`; and either way
`IGNITION_TAA`, `IGNITION_SSR`, `IGNITION_SSAO` — and a per-dial
variable outranks the preset: a tier is for choosing a picture, an override is for
finding out what one dial costs on one GPU.

How far a beam *reads* is a look decision, not only a physical one. The
fog's distance attenuation carries an exponent — `IGNITION_HAZE_DILUTION`,
two being the physical inverse square — and lowering it carries a shaft
further than its own falloff would, which is what a real room does for
reasons a homogeneous medium does not model. It applies to the fog
alone; surfaces stay physical either way.

On the froxel path the cost is carried by the grid's shape and its samples
per froxel: width buys a beam's *sharpness*, since the grid is filtered and
a tile spanning sixteen screen pixels gives a beam a sixteen-pixel edge,
while depth and samples buy its *continuity*. On the march, two dials carry
the cost and they multiply — the raymarch's step count
and how many pixels the haze camera may have — and they trade against
exactly one thing: a mover's shaft staying a solid cone rather than
breaking into a string of dots. A third dial exists only to serve that
trade and is not a choice: the ray-start jitter is sized in *steps*, not
metres, so a coarser march is dithered proportionally. Fixed, it dithers
a long step by a fraction of itself and the step boundaries come back as
rings — which reads as needing more steps when what is needed is more
dither. The ladder only climbs: no rung is
coarser than the one below it on either dial, or in what it switches on.
An unknown name is a warning and the default, never a failure to open.

r[viz.post-processing]
The picture is Bevy's post-process stack and nothing hand-rolled, and every
feature in it is an explicit `RenderQuality` switch, set separately for a
live view and a still:

- **Auto exposure** adapts like an eye, not a light meter: Bevy's
  `AutoExposure`, whose compensation curve holds a full chorus at the stage
  exposure and opens up part of the way (`AUTO_EXPOSURE_GAIN`) toward a
  darker frame, never more than `AUTO_EXPOSURE_RANGE_STOPS` either way — a
  blackout stays black and a verse stays a verse. A flash is met in about
  half a second and a drop to one par takes a couple of seconds to open up
  to; a still lands on its level in one frame, and an export adapts at
  the eye's pace against its own frame time, pre-rolled so its first
  frame is already adapted. It is on by default,
  `--auto-exposure off` fixes the stage exposure, and it never meters the
  overlay, which is drawn after tonemapping. It runs on the main camera
  only: the haze composite is added into the main camera's HDR frame before
  its tonemapping, which is where the adaptation lands, so fog and room are
  adapted together.
- **Temporal anti-aliasing** on every camera (MSAA stays off; the deferred
  deck and the ambient occlusion do not multisample), which is also what
  averages the haze camera's per-frame jitter into a smooth cone.
- **Screen-space reflections** on the stage deck, the one material with a
  wet-look roughness (`DECK_ROUGHNESS`) and the one that takes the deferred
  path; **screen-space ambient occlusion** for the room. Both live and
  still.
- **Depth of field**, focused on the point the camera looks at, in a still
  or an export only: a blurred house is a photograph, not an operator's
  view.
- **Tonemapping** is `TonyMcMapface`, and a `--grade` is a `ColorGrading`
  preset on top of it, neutral by default.

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
- **Volumetric light** (the fog raymarch) has no budget: every emitter's
  spill is in the fog (`r[viz.haze-is-volumetric]`), and the frame pays for
  it at the haze camera's size, not the picture's. On the reference GPU the
  whole Norco rig — 59 shaft-cutters and the bars — marches at 128 steps in
  under the budget; if a rig ever cannot, the fallback is fewer steps with
  jitter, never a drawn cone.
- **The haze is marched at a fraction of the picture's size** on a camera of
  its own and composited back — added, with the room behind it dimmed by the
  same transmittance the in-camera fog would have applied. The fraction is
  chosen so the haze camera stays under a fixed pixel budget
  (`HAZE_PIXEL_BUDGET`) whatever the viewport; a still keeps full size.
- **Fixture housings cast no shadow and block no shaft**: a par's body is a
  hand's width across and hangs above every beam. Only the room, the risers,
  the props and the people are occluders.
- **The render world runs on its own thread** in the studio (Bevy's
  pipelined rendering), so a paint costs the main world's update plus the
  wait for the previous frame's render, not the two added together. The
  target texture reaches the host through a mailbox the render thread
  posts to, never through a readback or a copy.
- **Nothing per-frame allocates or uploads for a rig standing still**: a
  bar's cell materials change only when the colour does; the fog volume's
  density is written only when the haze moves; a procedural canvas rasters
  on a worker and uploads only a fresh frame.
- **One mesh asset per fixture model**, shared by every fixture of that
  type, so the engine batches them.

## Cameras

r[viz.camera-presets]
A venue MUST be able to carry any number of **named camera presets** — an
eye, a look-at point, a vertical field of view and an optional depth-of-field
focus distance — in `data/venues/<venue>/cameras.json`. A preset is a place
in *that* room ("Drum cam" is where the drums are), so it lives with the
venue, never with a song. A venue with no file gets the three auto-framed
views (`house`, `stage`, `top`) as presets, so nothing is required of it.
The ten names the shipped shows cut between — `Wide`, `Singer`, `Drums`,
`Guitar`, `Bass`, `Keys`, `Side stage`, `Super wide`, `Flat front`, `Bird's
eye` — are the vocabulary a venue is expected to bind, the way it binds the
profile's roles; a song naming one a venue lacks is a warning, not a crash.

r[viz.camera-favourites]
Ten presets sit on the number keys, `1`–`9` and `0` for the tenth, **per
operator**: the operator file's `cameras.favourites` lists them in key order,
and an operator with no such key gets the venue file's own `favourites`
list. Pressing a key in the windowed visualizer cuts the programme camera to
that slot; the studio sends the same as `Command::Camera { Slot(n) }`.

r[viz.camera-setups]
A venue file MAY name **setups** — a named list of N presets, one per slot —
and ships `two`, `four` and `eight`. A setup is what a cut list addresses: a
show authored for the eight-camera setup cuts between those eight, and a
venue's `eight` says which of its presets stand in for each. Slot numbers in
a cue resolve through the chosen setup when one is active, else through the
favourites.

r[viz.camera-cuts]
A cue's `commands` MAY carry `camera <slot|preset> [in <beats>] [after
<beats>] [for <beats>]`. When the cue goes live the programme camera cuts to
the target; `in` dissolves over that many beats — a **linear tween of eye,
look-at and field of view on the song clock**, instant at zero; `after`
delays the cut from the cue's moment; `for` is a punch-in that returns to the
camera it left after that many beats. The transport-synced cut is therefore
**part of the song file** — there is no separate camera timeline to keep in
step with the cues, and an export (`r[viz.export]`) renders exactly the cut
the studio showed, since the same commands drive both.

r[viz.camera-birdseye]
The bird's-eye preset (`"ortho": true`) MUST be a **true top-down
orthographic** view from above the ceiling with every room object whose name
contains `Ceiling` hidden while it is active, so the XY plot of every
fixture and prop reads at true scale — the plan the venue was built from,
not a perspective of the roof.

r[viz.programme-view]
The visualizer MUST be able to render a second, **programme** camera to its
own texture beside the main view: the number keys and the cues' `camera …`
commands move the programme camera, and the main view stays on a *wide*
preset of its own (selectable), so an operator can dock the whole rig and
the cut side by side or on different monitors. The programme camera is
spawned only while something shows it — a Programme pane, or a canvas
sampling it — and a lone viewport pays nothing for it; with no programme
camera the main view takes the cuts itself.
