# Focus

Where a moving light points, and how a *group* of them points together.

Today a focus preset is a name and a point:

```json
{ "name": "Vocal Centre", "target": { "x": 0.0, "y": -3.44, "z": 1.55 } }
```

Every fixture given that preset aims at that one spot. That is correct and
useful — it is how a key light works — and it is also the *only* thing the
current model can say. It cannot express eight movers fanned across the stage,
or all of them parallel and pointing out over the audience, or each one taking a
different position from a list. Those are the looks that make a moving-light rig
worth having, and none of them are "aim here".

grandMA3 separates the two ideas cleanly. A position can be **Pan/Tilt** — an
angle each fixture holds — or **XYZ**, a coordinate in the room that the console
converts to angles per fixture. "Instead of using Pan and Tilt values to point a
fixture to a spot on stage, the same position can also be described by a
coordinate with X, Y, and Z values. The grandMA3 system recalculates
automatically the XYZ coordinate into Pan/Tilt values for the fixture."

Those are not two representations of one thing; they answer different questions,
and the distinction is the whole spec. Convergent looks are coordinates.
Parallel looks are angles. A rig fanned across the audience is neither on its
own — it is an angle *rule* evaluated per fixture.

Prior art: [XYZ](https://help.malighting.com/grandMA3/2.0/HTML/xyz.html),
[Stages](https://help.malighting.com/grandMA3/2.2/HTML/patch_stage.html),
[MArker Fixture](https://help.malighting.com/grandMA3/2.0/HTML/xyz_marker.html).

## The two kinds of aim

r[focus.two-kinds]
A focus preset MUST be able to express either a **point** in the room or an
**orientation** per fixture, and MUST say which it is. A point makes beams
converge; an orientation makes them parallel. Storing an orientation as a point
"very far away" is not equivalent — it converges at that distance and the error
is largest for the fixtures furthest apart, which is exactly the rig-wide look
being asked for.

r[focus.point]
A **point** focus MUST be stored as a room coordinate and resolved to pan/tilt
per fixture from that fixture's own position. This is the existing behaviour and
MUST keep working unchanged.

r[focus.orientation]
An **orientation** focus MUST be stored as a direction — pan and tilt angles, or
an equivalent unit vector — applied to every fixture in the selection. Eight
movers on an orientation focus point the same way regardless of where they hang,
which is the "all parallel, facing out" look.

r[focus.partial-axes]
A focus MAY constrain only some axes. A focus that fixes X and Z but leaves Y
free means "aim at this height and this position across the stage, at whatever
depth you are" — which is how a rig lights a line across the stage rather than a
point on it. An unconstrained axis MUST be resolved per fixture, not defaulted to
zero.

## The room

r[focus.stage-space]
Coordinates MUST be interpreted in a declared **stage space** with an origin and
extent, in metres. MA3's default is 30 m wide and deep with zero in the middle
and 15 m of height; Ignition's MUST come from the venue rather than a constant,
because the venue file already knows the room and a second source of truth for
where the stage is would drift from it.

r[focus.relative-origin]
A focus MAY be expressed **relative** to a named origin rather than to the stage
— a downstage-centre marker, a drum riser, a performer position. MA3 calls the
origin a MArker fixture. Moving the origin MUST move every focus expressed
against it, which is what makes "the vocal spot" survive the band standing
somewhere else on a different stage.

r[focus.units]
Coordinates MUST be metres and angles MUST be degrees, everywhere, with no
per-venue unit setting. The venue data, the fixture survey and the visualizer
already agree on metres; a unit flag would only create a way for them to
disagree.

## Patterns — a focus for a group, not a fixture

r[focus.pattern]
A focus preset MUST be able to describe a **pattern** across a selection rather
than a single aim shared by all of it. A pattern is evaluated per fixture, in
the selection's own order, and produces that fixture's aim.

r[focus.pattern.fan]
A **fan** MUST interpolate an aim between two endpoints across the selection —
the first fixture takes the start, the last takes the end, the rest are spaced
between. Endpoints MAY be points or orientations, and both endpoints MUST be the
same kind. This is MA3's MAtricks *from/to* layer generalised to position:
values are distributed across the selection rather than applied identically.

r[focus.pattern.parallel-out]
A **splay** MUST orient each fixture outward from a named axis or origin, so the
rig opens away from centre. The angle per fixture MUST be derived from that
fixture's real position, so the look survives a fixture being re-hung and does
not need re-authoring per rig.

r[focus.pattern.per-fixture]
A focus MAY carry an explicit aim **per fixture**, which is the selective scope
from [colour](color.md) applied to position — "each mover its own direction",
authored deliberately. Fallback for a fixture with no entry MUST follow the same
order as colour: own value, then type value, then the preset's shared value.

r[focus.pattern.order-is-the-selection]
A pattern MUST take its ordering from the selection and MUST NOT define its own.
"Left to right" is an X-ordered selection, not a `direction: left` field on the
focus. This is the same seam the chases already use, and duplicating ordering
inside focus would let a cue's chase and its focus disagree about which end of
the rig is first.

## Resolution and conflict

r[focus.resolve-at-output]
A focus MUST resolve to pan/tilt at output time against the fixture's live
position, never be baked at author time. A fixture that is re-surveyed or
re-hung MUST then aim correctly with no change to the show.

r[focus.unreachable]
A fixture that cannot reach its resolved aim — outside its pan or tilt range —
MUST clamp to its nearest reachable orientation and MUST be reportable as
unreachable. Silently wrapping to the other side of the yoke is worse than
clamping: it is a fixture pointing at the audience during a ballad.

r[focus.point-beats-orientation]
Where a point focus and an orientation focus are both live on one fixture, the
later cue MUST win outright rather than the two being blended. MA3 has an
equivalent rule — calling absolute XYZ "knocks out absolute PanTilt values" —
and for the same reason: an average of "aim at the singer" and "point out over
the audience" is a beam aimed at neither.

r[focus.straight-line]
When a fixture moves between two **point** focuses, it SHOULD travel so the beam
moves in a straight line across the room rather than each axis interpolating
independently. MA3 notes this as the reason to use XYZ: "when fixtures move from
one position to another, they do it in a straight line from position A to B when
using XYZ values." Independent pan/tilt interpolation makes the beam bow, which
is visible on a slow move across a stage.

## Shapes in the room

The catalogue of mover patterns is written in pan/tilt degrees, which means a
circle is a different circle at every venue: its size and even its shape
depend on where each fixture hangs. grandMA3's XYZ layer draws the shape
*in the room* and lets each fixture solve its own angles.

r[focus.delta]
A recipe MUST be able to apply a **relative offset in metres** to a fixture's
focus (`FocusDelta`), added to whatever point the cascade has aimed it at
before that point is resolved to pan/tilt. A fixture with no point focus (an
orientation, or nothing) MUST ignore the delta rather than treat it as a
point.

r[focus.orbit-in-metres]
A movement effect MAY be authored as a path in metres — a 2 m circle on the
floor around the drummer — and MUST then produce that same path at every
venue, each fixture solving its own angles per frame. This is the portable
form of a mover pattern; a pattern in degrees is a look for one hang.

r[focus.marker-moving]
A named focus origin (`r[focus.relative-origin]`) MUST be updatable at run
time, so a focus expressed against `Vocal` follows the singer when a tracker
or an operator moves the marker. Everything expressed against it moves in the
same frame.

r[focus.magic]
A focus preset MAY be a set of **keyframes** — up to five aims placed along the
selection's order — from which every unit's aim is interpolated (MA3's MAgic
presets). This generalises the two-point fan: five hand-set positions across
a truss land on twelve, sixteen or twenty movers with no per-fixture values.
