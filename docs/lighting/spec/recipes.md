# Recipes

A recipe is a **template**, not a value: a selection expression, some values,
and the [Tricks](tricks.md) that say how those values meet that selection. It is
resolved — *cooked* — against whatever rig is live when it runs, not against a
fixture list frozen when it was written.

That is the whole reason a generated show survives a re-patch. A recipe never
stores a fixture's identity, so adding a fixture to a group means every recipe
selecting that group covers it, with nothing re-authored.

Prior art:
[docs/research/grandma3-recipes-and-phasers.md](../research/grandma3-recipes-and-phasers.md),
[docs/research/grandma3-matricks.md](../research/grandma3-matricks.md).

## What a recipe is

r[recipes.template]
A recipe MUST store a selection *expression*, never a resolved fixture list. Two
recipes with the same expression MUST cover the same fixtures at output time
even if the rig changed between them being written.

r[recipes.cook-fixes-coverage]
Cooking a recipe MUST establish *which* attributes on which fixtures it covers,
and MUST NOT freeze the values. Coverage is what the cascade needs in order to
know what a cue owns; values MUST be re-resolved every frame so a palette edit
or a moved fixture takes effect without re-cooking.

r[recipes.steps-are-the-switch]
A recipe with **one step** is a static templated look. A recipe with **two or
more** is a phaser. This MUST be the only distinction between the two — not
separate types with separate authoring. grandMA3's own framing is that "Phasers
can now be created as recipes", and collapsing them means every Trick, every
spread and every selection feature works identically for both.

r[recipes.enabled]
A recipe MUST carry an **enabled** flag, so a line can be switched off without
being deleted. A/B-ing a look by deleting and retyping it loses the thing being
compared against.

## Resolution — the part that has to be exactly right

r[recipes.absolute-last-wins]
Where two recipes set an **absolute** value for the same attribute on the same
fixture, the **later** one MUST win outright. No averaging, no merging. This is
grandMA3's rule — "when a Group has multiple recipe lines with different presets
for the same attribute, only the last entry will generate output" — and it is
what makes overriding part of a look a predictable, orderable operation rather
than an undefined blend.

r[recipes.relative-last-wins]
Where two recipes apply a **relative** value to the same attribute on the same
fixture, the **later** one MUST win. Relative values do not sum with each other.
This is grandMA3's rule for both kinds of data — "absolute and relative values in
multiple parts will use the value with the highest cue part number" — and it
means a stab genuinely replaces the chase under it for as long as it runs, which
is what a stab should do.

r[recipes.relative-is-a-separate-layer]
Absolute and relative MUST be **separate layers**, with the relative layer added
to the absolute one at output. grandMA3 "handles storage, editing, and playback
of absolute values and relative values separately", and that separation is what
lets an effect modulate a look without competing with it for the same slot.
Last-wins therefore applies *within* each layer, not across them.

r[recipes.finished-one-shot-withdraws]
A one-shot that has run out MUST **withdraw** from the relative layer, not hold
at its final value. Whatever relative value was underneath MUST then resume.

This requirement is the whole reason the layer keeps more than one entry per
attribute, and it exists because the alternative was a real on-stage defect: a
finished bump that stayed in the layer went on winning at its final value
forever. For an envelope ending at zero that meant the accent silently killed
the chase it landed on for the rest of the section — and read, from the stage,
as the hit doing nothing at all. See `cue::tests`.

r[recipes.cascade]
Direct values MUST outrank recipe output. The full order, highest first: a
direct value on the cue; a recipe on the cue; a direct value on a referenced
preset; a recipe on that preset. Exactly one absolute value survives per
attribute, and exactly one relative value, the latter added to the former.

r[recipes.blocking-resets]
A blocking cue MUST start from empty layers, discarding inherited tracking *and*
the relative values stacked under it. A blocking cue is a complete statement, so
an effect from two sections ago MUST NOT still be waiting underneath to resume
when a bump withdraws.

## Cooked status

r[recipes.status]
Every cooked recipe MUST report a status, distinguishing at least: cooked and
producing output; cooked but resolving to nothing; not yet cooked; and a cue
carrying a mix of cooked recipe output and direct values.

r[recipes.status.selects-nothing-is-not-an-error]
A recipe whose selection resolves to no fixtures MUST NOT abort the show, the
cue, or the load. It MUST be reported. A rig is often half-patched while a show
is being written, and a load that fails on the first empty selection cannot be
run at all during exactly the period it is most needed.

r[recipes.status.visible-per-cue]
Status MUST be inspectable per cue before anything is fired, so "will this cue
do what I think" is answerable by looking rather than by running the show. This
is the affordance that makes a generated show trustworthy: the generator can be
wrong, and the report is how that is caught.

## Timing

r[recipes.timing-in-musical-terms]
A recipe's rate MUST be expressible against a named speed master rather than
only in Hz or seconds, so an effect written as "one cycle per bar" is one cycle
per bar of *this* song. A tempo change MUST carry every such effect with it,
with nothing re-authored.

r[recipes.one-shot]
A recipe MUST be able to run its steps **once and hold**, rather than only
looping. This is what makes a bump expressible as one event with a shape,
instead of two cues — a lift, and a release a beat later. Half the cue names in
a show being "… out" is a cue list that cannot be read at a glance.

r[recipes.one-shot-clock]
A one-shot MUST be timed from the moment its cue was taken, while a looping
recipe MUST run on the shared show clock. The two are not interchangeable: a
loop on a per-cue clock restarts every time a cue is taken, so two cues carrying
the same chase fall out of phase; a one-shot on the shared clock has already
finished before its own cue fired, so it never plays at all.

## Live control

An operator does not only choose *which* effect runs. They ride how big it
is and how fast, continuously, with a hand on a fader — which is a different
thing from editing the recipe, and has to stay different.

r[recipes.size]
An effect's depth MUST be scalable at run time by a **size** control, without
editing the recipe. Size scales the swing an effect applies and MUST NOT change
its shape, its rate or what it targets: at zero the effect is inert and whatever
is underneath shows through unchanged, at one it is as authored.

This is the control a busking operator holds for most of a night. Without it the
only way to make a chase shallower is to author a second chase, and a library
then grows a timid and a bold spelling of everything in it.

r[recipes.size.is-not-intensity]
Size MUST be distinct from a group master or a dimmer. A master scales what the
fixtures *output*; size scales how far the effect *swings*. Halving a master
makes a chase dimmer overall; halving size makes it flatter while the look under
it stays where it was. Conflating them is the common mistake and it makes an
effect impossible to withdraw from a look without dimming the look too.

r[recipes.rate]
An effect's rate MUST likewise be scalable at run time against its speed master.
A tap master already lets an operator set the tempo; rate is how one effect runs
at half or double it without a second recipe.

r[recipes.live-control-is-not-stored]
Size and rate are **operator state**, not part of the recipe. Storing them into
the recipe would make the show file depend on where somebody left a fader, and
two cues sharing an effect would fight over it.

## Composition

r[recipes.tricks]
A recipe MUST be able to carry [Tricks](tricks.md) — sub-selection, spread,
mirror — inline, and MUST also be able to reference a shared Tricks object.
See `r[tricks.shared-or-inline]`.

r[recipes.relative-leaves-colour-alone]
A relative recipe MUST modulate the attributes it names and no others. A chase
that dims says how much to take away; what colour the fixture is doing at the
time is not its business. This is what makes effects layerable over looks
instead of replacing them.

r[recipes.selection-owns-order]
A recipe MUST take direction from its selection's order and MUST NOT carry a
direction field of its own. "Left to right" is an X-ordered selection. Two
authorities on direction is how a cue ends up with its chase running one way and
its colour spread running the other.
