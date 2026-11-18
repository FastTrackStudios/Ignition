# grandMA3 MAtricks — how sub-selection actually works, and what to steal

Companion to `grandma3-recipes-and-phasers.md`, which covered what a recipe
stores and how a phaser runs. This covers the third thing, and the one that
turns out to bind the other two together.

The short version, and the reason this document exists: **MAtricks is not a
separate feature that effects happen to use. It is a set of columns on the
recipe line.** A recipe carries its own sub-selection, its own inversion, and
its own from/to timing spread, in the same row as its selection and its values.
That single fact explains why one MA3 recipe covers looks that need a dozen
hand-written recipes in `ignition-core` today.

Sources: [MAtricks](https://help.malighting.com/grandMA3/2.0/HTML/matricks.html),
[Blocks](https://help.malighting.com/grandMA3/2.0/HTML/matricks_block.html),
[Groups](https://help.malighting.com/grandMA3/2.0/HTML/matricks_group.html),
[Shuffle](https://help.malighting.com/grandMA3/2.0/HTML/matricks_shuffle.html),
[Transform](https://help.malighting.com/grandMA3/2.0/HTML/matricks_transform.html),
[Recipes](https://help.malighting.com/grandMA3/2.2/HTML/recipes.html).

## The model: a selection is a grid, not a list

Ignition treats a selection as an ordered list of fixtures. MA3 treats it as a
**three-axis grid**, and every MAtricks property is per-axis: XBlock, YBlock,
ZBlock, XGroup, YGroup, and so on.

For a line of pars the Y and Z axes are trivial and it behaves like a list. For
a matrix of pixels, or a rig with depth, the extra axes are the difference
between "chase along the truss" and "wipe front-to-back". Our `Order::Axis`
already picks *which* axis orders a selection; MA3's grid says a selection can
be ordered on all three at once and addressed on each independently.

**Worth deciding early**: whether Ignition's selections become genuinely
2- or 3-dimensional, or whether we keep one ordering axis and accept that
matrix effects are out of scope. Retrofitting a second axis later means
touching every effect.

## Grid properties

### Block N — contiguous runs, treated as one fixture

> "The blocks function in the MAtricks creates blocks of fixtures of the
> specified size. This treats blocks of fixtures as one fixture."

Ten fixtures, `XBlock 2` → five blocks: (1,2) (3,4) (5,6) (7,8) (9,10).
`XBlock 5` → two blocks: (1–5) (6–10).

Everything downstream sees *blocks*, not fixtures. A phase spread across a
blocked selection gives five phases, not ten, and the pair moves together. This
is how a rig of pixel bars behaves as a rig of heads without authoring a second
selection.

### Group N — round-robin, interleaved

> "It alternates through the selection putting fixtures into each group."

Ten fixtures, `XGroup 2` → group 1 is 1,3,5,7,9; group 2 is 2,4,6,8,10.

**Block and Group are not variants of one operation.** Block 5 on ten fixtures
gives (1–5)(6–10); Group 2 gives (1,3,5,7,9)(2,4,6,8,10). Both produce two
sub-selections of five and they look nothing alike — one is halves of the rig,
the other is odds and evens. Implementing one with a flag on the other is the
obvious mistake and would make half the looks unreachable.

### Wings N — divide and mirror

Divides the selection into N parts with alternate parts mirrored, so a
symmetric rig is driven from one definition and opens outward from centre.

### Width — how much of the grid is used

Constrains the extent an axis spans, so a selection can be laid out into a grid
wider than its fixture count — leaving deliberate gaps.

### Next / Previous

Block and Group both change what "the next sub-selection" means: with
`XBlock 5`, Next steps twice to cross ten fixtures instead of ten times. This is
a programming affordance rather than an output one, but it is the reason these
are *sub-selections* and not just effect parameters — an operator moves through
them by hand.

## Layers — from/to across a selection

The MAtricks window carries **Fade From/To, Delay From/To, Speed From/To,
Phase From/To**. Each distributes a value across the selection rather than
applying it identically: first fixture gets *From*, last gets *To*, the rest
are spaced between.

This is the generalisation of our `phase_spread_deg`. We have exactly one
spreadable quantity, hard-coded into `Timing`; MA3 has four, expressed
uniformly, and they apply to **static looks as well as effects**. A delay fan on
a static cue is a wipe with no phaser involved at all.

That is the piece worth taking most seriously. `r[groups.spread]` in our spec
says distribution is a property of how a value meets a selection, not a kind of
effect — this is where that comes from.

## Shuffle and Shift

Shuffle reorders the selection pseudo-randomly. Per the MAtricks overview page,
each axis takes a value from 0 (none) up to 32,767, and "when repeatedly
selecting the same amount of fixtures, the same shuffle value will result in the
same shuffled selection order" — reproducibility is the whole point, since a
random look you cannot recall is a look you discovered rather than programmed.

Shift repositions the selection within the grid (XShift/YShift/ZShift).

The overview also mentions Auto / Linked / Unlinked shuffle modes, governing
whether shuffling one axis preserves alignment with the others. **I could not
confirm the details** — the dedicated Shuffle page does not describe the modes,
the value range, or Shift, and I am recording that as unknown rather than
guessing. If we implement multi-axis shuffle we will need to work out linked
behaviour ourselves.

## Transform — Mirror

Only two states: **Mirror** and **None**. Mirror makes values symmetric
"depending on the other MAtricks settings, such as Blocks, Groups, and Wings" —
so it composes with the grid rather than being a separate mirroring of its own.

The detail worth stealing is the odd-count handling. With an odd selection the
centre fixture becomes an **edge fixture** that deliberately does not follow
every value — in a mirrored circle of 11, "the edge fixture in the center will
only tilt but not pan." Naive mirroring of an odd count either double-counts the
centre or leaves it doing something incoherent; MA3 names the case and handles
it explicitly.

## The recipe line — where this all actually lives

This is the finding that changes the implementation plan. A recipe line's
columns:

| Group | Columns |
|---|---|
| Selection | `Selection`, `Values`, `Selection Mode` (Normal / Strict), `Filter` |
| MAtricks | `MAtricks` (a reference to a pool object), `X`, `Y`, `Z`, `Group`, `Block`, `Wings`, `Width` |
| Inversion | `InvertStyle`, `PhaserTransform` (Mirror), `Invert`, `Inv`, `InvB`, `InvG`, `InvW` |
| Timing | `Fade`, `Delay`, `Speed`, `Phase` — each with From/To |
| Order | `Shuffle`, `Shift` |
| Admin | `Lock`, `Name`, `Tags`, `Enabled` |

So MAtricks arrives two ways: **by reference** to a shared pool object, or
**inline** as columns on the line. That is a real design decision, not
redundancy — the shared object is how six recipes use one grid layout and all
change together; the inline columns are how one recipe deviates without
polluting the pool.

`Selection Mode` is a wrinkle we have no equivalent for: **Normal** passes
values down to subfixtures, **Strict** applies only to the fixtures actually
selected. That matters for multi-cell fixtures — a bar that is one fixture with
twelve cells.

`Enabled` is worth copying for free: a recipe line that can be switched off
without deleting it is how a look is A/B'd.

## Combination rules

Two rules, both consequential:

1. **"Values stored in the cue part have higher priority than the cue part
   recipe value."** Direct values beat recipe output. This is the four-layer
   cascade already documented in the recipes note.
2. **"When a Group has multiple recipe lines with different presets for the same
   attribute, only the last entry will generate output."** Last line wins, per
   attribute.

Rule 2 is *not* what Ignition does. We now **sum** relative recipes — that was a
deliberate fix, because a bump laid over a running chase was replacing it and
producing nothing. MA3's rule is last-wins because its recipe lines are
generally absolute values competing for one attribute; ours are deltas
composing on top of a look.

**These are compatible if the distinction is explicit**: absolute values are
last-wins, relative deltas sum. Our cascade already separates `tracked` from
`modulated` along exactly that line. It is worth writing into the recipe spec so
the difference is a decision on the record rather than an accident.

## What this means for Ignition

Concretely, in rough dependency order:

1. **Sub-selection as selection combinators.** `Block(n)`, `Group(n)`,
   `Wings(n)`, `Shuffle(seed)` returning selections, so they compose with the
   existing algebra rather than being effect parameters. Covered by
   `r[groups.subselect.*]`.
2. **From/To distribution as a property of a value meeting a selection** —
   phase, delay, fade, speed — usable by static cues, not only phasers.
   `r[groups.spread]`.
3. **A MAtricks-equivalent on the recipe**, available both by reference and
   inline. The reference form is what makes a rig-wide layout change one edit.
4. **Mirror, with the odd-count edge-fixture rule handled explicitly.**
5. **Decide the axis question.** One ordering axis, or a real 2-/3-axis grid.
   Everything above is cheaper to build before that decision than after it.

The thing to resist: implementing these as effect types. Every one of them is a
property of *how a value meets a selection*. Built as effects, a static look
cannot have them — and half of what makes an MA3 rig look programmed rather than
chased is a static cue with a delay fan on it.

## Named MAtricks idioms

What working programmers actually keep as MAtricks pool items, Eos fan/effect
groupings, Hog fan modes and MagicQ FX parts — and what each of them is *for*.
Read on 2026-08-26:

- grandMA3 help, [MAtricks and Shuffle](https://help.malighting.com/grandMA3/2.0/HTML/matricks.html)
  — Blocks, Groups, Wings, Width, Shuffle (0–32767, "matching values produce
  identical shuffled orders"), Shift ("shifted within the selection grid
  positions" per axis, negative goes the other way), Invert per axis, and the
  four layers: Fade, Delay, Speed, Phase from/to.
- grandMA3 help, [MAtricks Transform](https://help.malighting.com/grandMA3/2.3/HTML/matricks_transform.html)
  — Mirror "mirrors values depending on the other MAtricks settings, such as
  Blocks, Groups, and Wings"; an odd count has a "center fixture marked as an
  edge fixture" which "does not follow all values to keep the symmetry" (the
  eleven-fixture circle: "will only tilt but not pan").
- MA forum, [Reverse Selection and Invert Wings](https://forum.malighting.com/forum/thread/8669-reverse-selection-and-invert-wings/)
  — Wings=2 is the "center out" idiom for colour bumps; "1 thru 0" delays get
  normalised, so reversing is done with Invert / grid rotation rather than by
  swapping from and to.
- MA forum, [Modify selection order via MAtricks](https://forum.malighting.com/forum/thread/9084-modify-selection-order-via-matricks/)
  — "MAtricks are not involved in inverting selection orders of fixtures";
  order is the grid's job, MAtricks address the values. Ours differ here on
  purpose: a Trick *is* a reorder, because a selection is the only type.
- MA forum, [Checkerboard Odd/Even Selection](https://forum.malighting.com/forum/thread/9173-checkerboard-odd-even-selection/)
  — odds/evens on a matrix is a MAtricks preset plus `Set + Next` on Y.
- MA forum, [Width and phases in recipes](https://forum.malighting.com/forum/thread/7661-width-and-phases-in-recipes/)
  — the odd/even dimmer chase recipe: width 100%, Groups 2, phase 0 thru 360;
  a "1 in 3" chase is Groups 3.
- Hog 4 help, [Fanning](https://www.etcconnect.com/webdocs/controls/hog4/content/06_FixtureParameters/Fanning.htm)
  — Fan Normal (middle unchanged, ends opposite), From Start, From End, To
  Center (ends "in the same direction"); Segmenting ("fanning is repeated for
  each segment") and Buddying ("'gangs' that all take the same value").
- Eos manual, [Fan](https://www.manualsdir.com/manuals/559096/etc-eos-family-v190.html?page=15)
  — Mirror ("1 to 30%, 2 to 20%, 3 to 10%, 4 to 20%, 5 to 30%"), Repeat
  (1,4,7,10 alike), Cluster (1-3 alike, 4-6 alike…), Random.
- Eos community, [Create odds/evens effect](https://community.etcconnect.com/control_consoles/eos-family-consoles/f/eos-family/39781/create-odds---evens-effect)
  and [Effect direction](https://community.etcconnect.com/control_consoles/eos-family-consoles/f/eos-family/32242/2-9-1-effect-direction)
  — grouping 2 with trail solo; Offset with reverse / mirror in / mirror out.
- ChamSys, [FX Engine](https://secure.chamsys.co.uk/docs/magicq/manual/FX_engine.html)
  and the [manual's FX Parts/Segments page](https://www.manualsdir.com/manuals/760818/chamsys-magicq-user-manual.html?page=115)
  — Parts "2s/3s" repeat the FX every n heads; Segments are "the number of
  adjacent heads that have the same offset"; Spread 0 is everyone together.
- Avolites Titan features page ([Wings](https://www.avolites.com/support/titan-features/))
  — Wings divides the selection into sections where the outer ones are
  "the opposite of" each other and the middle "does not change".

The mapping onto Ignition's vocabulary — the same operation appears under
four names, and it is worth having the table so nobody thinks Eos *Cluster*
and MA *Blocks* are different ideas:

| Idea | grandMA3 | Eos | Hog 4 | MagicQ | Ignition |
|---|---|---|---|---|---|
| contiguous runs move together | Blocks | Cluster | Buddying | Segments | `Block(n)` |
| deal round-robin, pattern repeats every n | Groups | Repeat / grouping | Segmenting | Parts | `Group(n)` |
| symmetric halves from one definition | Wings | Mirror in/out | — | direction symmetric | `Wings(n)` |
| values symmetric about centre | Transform Mirror | Fan Mirror | Fan Normal / To Center | — | `Mirror` |
| seeded random order | Shuffle | Random | — | — | `Shuffle(seed)` |
| move the pattern along the rig | Shift | Offset | — | — | `Shift(n)` |
| run it backwards | Invert X | Reverse | — | Invert | `Reverse` |
| one from/to for phase, delay, fade, speed | layers | Fan | Fan | Spread | `Fan` |

### The named library

The `tricks` key of `data/profiles/ignition.ig-profile`. Every set is a
chain of Trick ops; unit counts are for a twelve-fixture line.

| Name | Ops | Units/12 | Stage use |
|---|---|---|---|
| odds | Group 2 | 2 | the odd/even dimmer chase; unit 0 is the odds |
| evens | Group 2, Shift 1 | 2 | the same chase led by the evens, so two cues alternate |
| pairs | Block 2 | 6 | a two-fixture-wide chase; pairs move together |
| triples | Block 3 | 4 | a three-wide chase — cells on a batten |
| quads | Block 4 | 3 | four-wide; the big slow sweep on a long truss |
| halves | Wings 2 | 12 | historic name for centre out; kept for old shows |
| thirds | Group 3 | 3 | the "1 in 3" stripe; every third fixture steps together |
| quarters | Group 4 | 4 | a four-step comb — every fourth fixture |
| paired odds | Block 2, Group 2 | 2 | checkerboard of pairs: two on, two off |
| paired thirds | Block 2, Group 3 | 3 | the 1-in-3 stripe, two fixtures wide |
| mirror | Mirror | 6 | a symmetric fan: ends in, both sides at once, from one from/to |
| fan from centre | Mirror, Reverse | 6 | the same fan opening from the middle outward |
| mirrored pairs | Block 2, Mirror | 3 | symmetric fan in two-wide steps |
| mirrored wings | Wings 2, Mirror | 3 | each wing's outer end paired with the other's inner end — a crossing fan |
| centre out | Wings 2 | 12 | a chase or colour wave that opens from centre |
| ends in | Wings 2, Reverse | 12 | the same wave running into the middle |
| four wings | Wings 4 | 12 | two symmetric waves side by side on a wide truss |
| paired wings | Block 2, Wings 2 | 6 | centre-out wave two fixtures wide |
| scatter | Shuffle 1741 | 12 | random-looking twinkle, recallable |
| scatter b | Shuffle 907 | 12 | a second scatter so two layers do not line up |
| paired scatter | Block 2, Shuffle 1741 | 6 | random order over pairs — a chunkier sparkle |
| reverse | Reverse | 12 | run any of the above the other way |
| shifted | Shift 1 | 12 | the same pattern one fixture along, for a second cue |
| shifted back | Shift -1 | 12 | one fixture the other way |

### What the one-dimensional model cannot say

Y and Z axes (a matrix checkerboard, a Shuffle on X that leaves rows alone,
grid rotation, Width) wait on the grid decision in item 5 above. Eos's
*inverse channel selection* (the complement of the selection) is a selection
op, not a Trick, and lives in the selection algebra. Hog's asymmetric fans
(From Start / From End) are just which way round from and to are.
