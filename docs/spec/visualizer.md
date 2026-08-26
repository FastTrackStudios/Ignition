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
