# Tricks

Addressing *part* of a selection, and spreading a value *across* one.

grandMA3 calls this MAtricks. Ignition calls it **Tricks** — same idea, without
carrying another vendor's initials into our type names.

The thing to understand before reading the requirements: this is not an effect
system, and building it as one is the mistake it exists to avoid. Every
requirement here describes *how a value meets a selection*. A delay fan on a
static cue is a wipe with no phaser running at all, and if spreading were a kind
of effect that look would be unreachable.

Prior art and worked examples:
[docs/research/grandma3-matricks.md](../research/grandma3-matricks.md).

## The grid

r[tricks.grid]
A selection MUST be addressable as a **three-axis grid** (X, Y, Z), not only as
an ordered list. A truss of pars uses one axis and behaves exactly like a list;
a pixel matrix uses two; a rig with depth uses three. Every Trick below MUST be
expressible per axis.

r[tricks.grid.from-space]
Grid axes MUST derive from the fixtures' real positions in the room by default,
not from patch order. The rig already knows where every fixture is, and an axis
that means "the order they were patched in" would make the same Trick produce a
different look on a re-patch.

r[tricks.grid.explicit-override]
A selection MAY carry an explicit grid layout that overrides the spatial
derivation, for rigs whose logical arrangement is not their physical one — a
matrix hung in a spiral, say. Where both exist the explicit layout MUST win, and
the fact that it is overriding MUST be inspectable.

r[tricks.grid.degenerate-axes]
An axis along which a selection does not vary MUST behave as a single position
rather than as an error or an empty result. A line of pars addressed on Y is one
row, and a Trick on Y is then a no-op — not a selection of nothing.

## Sub-selection

r[tricks.block]
**Block(n)** MUST divide the selection into contiguous runs of `n` and treat
each run as one unit. Ten fixtures at `Block(2)` gives five units: (1,2) (3,4)
(5,6) (7,8) (9,10). Everything downstream MUST see the units, so a phase spread
over a blocked selection produces one phase per block, and the fixtures within a
block move together.

r[tricks.group]
**Group(n)** MUST deal fixtures round-robin into `n` sub-selections. Ten
fixtures at `Group(2)` gives (1,3,5,7,9) and (2,4,6,8,10).

r[tricks.block-is-not-group]
`Block` and `Group` MUST be distinct operations, not one with a flag.
`Block(5)` and `Group(2)` over ten fixtures both produce two sub-selections of
five, and they look nothing alike — halves of the rig against odds and evens.
Expressing one as a variant of the other puts half the available looks out of
reach.

r[tricks.wings]
**Wings(n)** MUST divide the selection into `n` parts with alternate parts
mirrored, so a symmetric rig is driven from one definition and opens outward
from centre.

r[tricks.shuffle]
**Shuffle(seed)** MUST reorder a selection pseudo-randomly, and the same seed
over the same fixture count MUST always produce the same order. A random look
that cannot be recalled has been discovered, not programmed.

r[tricks.shuffle.axes]
Shuffling one axis MUST NOT silently disturb the others. Where a multi-axis
shuffle needs to preserve cross-axis alignment, that MUST be an explicit choice
rather than a side effect. (grandMA3 has Auto/Linked/Unlinked modes here; their
documented behaviour could not be confirmed, so this is ours to define rather
than to copy — see the research note.)

r[tricks.shift]
**Shift(n)** MUST reposition the selection within its grid, so a pattern can be
moved along the rig without re-authoring it.

r[tricks.composable]
Every Trick MUST return a selection, so Tricks compose with each other and with
the existing algebra — union, intersect, except, filter, order. A sub-selection
understood only by effects would be a second, weaker selection language.

## Spreading a value

r[tricks.spread]
A value MAY be distributed across a selection by giving a **from** and a **to**:
the first unit takes `from`, the last takes `to`, the rest are spaced between.
Spreading MUST follow the selection's own order.

r[tricks.spread.attributes]
Spreading MUST be available for at least **phase**, **delay**, **fade** and
**speed**, and SHOULD be available for any scalar attribute. These are the four
grandMA3 exposes, and each produces a distinct look from the same underlying
mechanism.

r[tricks.spread.not-an-effect]
Spreading MUST be a property of how a value meets a selection, not a kind of
effect. A static look with a delay fan and a running chase with the same fan
MUST share one mechanism. This is the requirement the rest of the file exists to
protect: implemented as an effect type, the static case becomes impossible, and
a static cue with a delay fan on it is a large part of what makes a rig look
programmed rather than chased.

r[tricks.spread.blocks-are-units]
Where a Trick has grouped fixtures into units, spreading MUST distribute across
**units**, not fixtures. Five blocks of two get five phases, not ten.

## Mirror

r[tricks.mirror]
**Mirror** MUST make values symmetric about the selection's centre, composing
with whatever Block, Group and Wings are in force rather than mirroring
independently of them.

r[tricks.mirror.odd-centre]
With an odd number of units the centre unit MUST be handled explicitly rather
than falling out of the arithmetic. grandMA3 names it an *edge fixture* and has
it follow only some of the mirrored values — in a mirrored circle of eleven,
"the edge fixture in the center will only tilt but not pan." Naive mirroring
either double-counts the centre or leaves it doing something incoherent, and
both are visible.

## Where Tricks live

r[tricks.on-the-recipe]
A recipe MUST be able to carry its Tricks directly, in the same object as its
selection and its values. Tricks are not a separate stage a value passes
through; in grandMA3 they are columns on the recipe line, and that is why one
recipe there covers what needs a dozen here.

r[tricks.shared-or-inline]
Tricks MUST be expressible both **by reference** to a shared, named Tricks
object and **inline** on the recipe. The reference is how a rig-wide layout
change is one edit across every recipe using it; inline is how one recipe
deviates without polluting the shared object. Supporting only the inline form
means a layout change is a sweep through the whole show.
