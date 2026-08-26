# Groups and selection

Which fixtures a thing applies to, and in what order.

Ignition already has more here than it has for colour or focus. `Selection` is
an algebra — union, intersect, except, by tag, by model, by spatial predicate —
and `Order` can sort a selection by axis or by distance from a point. That is
what makes "a chase travelling left to right" a property of the *selection*
rather than a direction field on the effect.

What is missing is the layer above: a way to take a selection and address
*parts* of it — every other fixture, four blocks of three, two mirrored wings —
without writing a new selection for each. grandMA3 calls this MAtricks, and it
is the reason one recipe there covers looks that would be a dozen here.

Prior art: [MAtricks](https://help.malighting.com/grandMA3/2.0/HTML/matricks.html),
[MAtricks Groups](https://help.malighting.com/grandMA3/2.0/HTML/matricks_group.html),
[Groups](https://help.malighting.com/grandMA3/2.2/HTML/qsg_group.html),
[Group Masters](https://help.malighting.com/grandMA3/2.0/HTML/group_master.html).

## What a group is

r[groups.order-is-data]
A group MUST store its fixtures' **order**, not just their membership. MA3 is
explicit that groups store "the fixture selection, the grid information, and the
fixtures' selection order". Order is what an effect phases along; a group that
returns a set makes every effect over it arbitrary.

r[groups.order-is-stable]
Resolving the same group against the same rig MUST produce the same order every
time. A group backed by a set or an unordered map is a defect even when it
happens to iterate consistently, because the show's look then depends on
iteration order.

r[groups.derived]
A group MAY be defined by a rule rather than a fixture list — "everything tagged
wash above 2 m" — and MUST then pick up a fixture added to the rig later without
being re-authored. This already exists as `Selection` and is the reason a
generated show survives a re-patch.

## Sub-selection and spreading

Both moved to [tricks.md](tricks.md), which is the single authority on
addressing part of a selection and on distributing a value across one. They
began here, and splitting them out is the point: they turned out to be
properties of *how a value meets a selection*, which is a bigger idea than
groups and is carried by recipes rather than by group definitions.

## Group masters

r[groups.master]
A group MAY carry a **master** level that scales or limits its fixtures'
intensity independently of any cue. This is the busking layer's grip on a rig
that a cue list is otherwise driving.

r[groups.master.modes]
Master behaviour MUST be declared, and MUST distinguish at least **scaling**
(output is multiplied — a fixture at 50% under a master at 50% outputs 25%) from
**limiting** (output is capped, not scaled). MA3 additionally separates positive
(HTP) and negative (LTP) limiting and an additive master that merges HTP with
playback rather than constraining it. Conflating scaling with limiting is the
common bug: it makes a master at 100% either a no-op or a full-on, depending on
which was meant.

## Consistency with the rest of the system

r[groups.one-ordering-authority]
Selection order MUST be the only source of "which fixture is first". Effects,
colour distribution and focus patterns all take their ordering from it, and MUST
NOT carry direction fields of their own. Two authorities on direction is how a
cue ends up with a chase running left while its colours spread right.

r[groups.resolution-is-live]
Selections MUST resolve against the rig at output time, never be frozen at
author time. A cue written for "wash above 2 m" MUST cover a wash hung tomorrow.
The cue player already relies on this — cooking a cue fixes *coverage*, and the
values are re-resolved every frame.
