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

r[recipes.relative-sums]
Where two recipes apply a **relative** value — a delta — to the same attribute
on the same fixture, the results MUST **sum**.

This is a deliberate departure from last-wins, and it is not a softening of it:
relative and absolute are answering different questions. An absolute value says
what an attribute *is*, and two answers to that must be resolved by choosing.
A delta says what to *add*, and two additions compose by definition — "+30% for
the chorus" and "+50% for this hit" together mean +80%, not +50%.

Applying last-wins to deltas was a real defect, found on stage: a bump laid over
a running chase replaced the chase instead of adding to it, so hits landing on
already-chased fixtures produced no visible lift at all — and once the bump's
one-shot envelope settled at zero it went on owning the slot, leaving the chase
dead for the rest of the section. See `cue::tests`.

r[recipes.cascade]
Direct values MUST outrank recipe output. The full order, highest first: a
direct value on the cue; a recipe on the cue; a direct value on a referenced
preset; a recipe on that preset. Exactly one absolute value survives per
attribute; relative values are then summed on top of whatever won.

r[recipes.blocking-resets]
A blocking cue MUST start from empty layers, discarding inherited tracking
*and* accumulated relative values. Without this, summed deltas would grow
without bound across a show; with it, accumulation is naturally scoped to a
section, which is also how an operator reasons about it.

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
