# grandMA3 Recipes and Phasers — how they actually work, and what to steal

**Status**: research / decision input, feeding `docs/domain/DOMAIN.md`.
**Date**: 2026-08-24.

MA3's "Recipe" is the feature the [landscape doc](lighting-console-landscape.md)
(§8, §1 "Recipes") flagged as the closest thing on the market to Ignition's own
differentiator — content that regenerates rather than gets baked. This doc goes
one level deeper: what a Recipe actually stores, how it resolves, and how it
relates to Phasers (MA3's effects engine), so `ignition-core`'s design can
borrow the *mechanism*, not just the marketing description.

Sources: [MA3 help — Recipes](https://help.malighting.com/grandMA3/2.4/HTML/recipes.html),
[MA3 help — Recipe Editor](https://help.malighting.com/grandMA3/2.4/HTML/recipe-sheet.html),
[MA3 help — Phasers](https://help.malighting.com/grandMA3/2.4/HTML/phaser.html),
[MA3 help — Phaser Editor](https://help.malighting.com/grandMA3/2.2/HTML/phaser_editor.html),
[MA forum — recipe-friendly position phasers](https://forum.malighting.com/forum/thread/68367-how-to-create-recipe-friendly-position-phasers/),
[MA forum — phaser basics](https://forum.malighting.com/forum/thread/68294-phaser-basics/).

## Recipes

### What it stores

A Recipe is a **parametric template**, not a value: a selection expression
plus attribute values (and optionally MAtricks parameters, fade/delay/speed/
phase per-attribute) that get *resolved against whatever selection is live
when the recipe is applied* — "cooked," in MA's terminology — rather than
against a fixed, pre-recorded fixture list.

### Where it lives

A recipe can be stored into, and cooks independently at, four different
places:

- **Cue parts** — with `Merge` or `Overwrite` semantics against whatever the
  cue part already contains
- **Presets** — cooks automatically
- **The programmer** — live, before it's recorded anywhere

### Resolution priority — the part that matters for correctness

For any single attribute on any single fixture, MA3 resolves through a
**four-layer cascade**, highest priority first:

1. A direct value stored on the cue part itself
2. A recipe's output, stored on the cue part
3. A direct value stored on a referenced preset
4. A recipe's output, stored on the preset

Only one value survives per attribute — the console picks the highest layer
present and discards the rest. This is what makes "the cue overrides part of
the preset it's built on" a predictable, orderable operation instead of an
undefined merge.

### Status feedback — a UX detail worth stealing outright

Every cooked recipe carries a status marker in the cue/preset sheet:

- **Green pot** — cooked successfully, values are live
- **Red pot** — cooked and failed (e.g. the selection resolved to nothing)
- **Open pot** — still a template, not yet cooked against anything
- **Orange pot** — a cue containing a mix of cooked recipe data and
  direct/uncooked values

This is a cheap, legible way to answer "is this cue's content actually going
to do what I think it does" at a glance — exactly the kind of workflow
affordance the landscape doc (§4) flags as the *actual* hard part of a console,
as opposed to the maths.

### Selections that travel with the rig

The documented pattern: build a **template preset with ranged/relative
values** applied to a **flexible selection** (a group, a fixture-type filter,
or literally `<From Value>` — "select whatever fixtures are set in the values
column of this recipe line"). Because the selection is an expression, not a
frozen list:

- Add a fixture to the group → every recipe that selects that group covers it
- Re-hang or re-patch a fixture → the recipe still resolves correctly, because
  it never stored the fixture's identity as data
- The same recipe travels between rigs with different fixture counts, which is
  explicitly one of MA's stated use cases

### Recipes vs. Phasers — they're not the same axis

A Recipe with **one step** is an ordinary parametric value template. A Recipe
with **two or more steps** *is* a Phaser — "Phasers can now be created as
recipes" is MA3's own framing. So in MA3's model, "how many steps" is the
switch between "static templated look" and "dynamic effect," not two separate
object types with separate authoring tools. Phaser recipes show a violet
marker in the sheet to distinguish them from plain (single-step) recipes.

## Phasers

### The core distinction from a classic chase engine

A Phaser is a **multi-step value container living inside a single cue or
preset**, not a sequence of discrete external cues. Instead of "cue 1, cue 2,
cue 3, each a snapshot," a phaser holds N steps *as one object* and
continuously interpolates through them.

### Step-level data

Each step carries, per attribute:

- **Absolute values** ("dimmer = 50%") and **relative values** ("dimmer =
  -20%") — stored and playable **simultaneously**, not as a type choice per
  step. This is what lets a phaser modulate brightness *relative to whatever
  colour a cue already set*, the same "step effects modulate intensity, colour
  stays untouched" separation Eos gets by defaulting On/Off states to
  intensity (`docs/research/lighting-console-landscape.md` companion material,
  eos-toolkit `effects-model.md`) — MA3 achieves the same outcome more
  explicitly, as a first-class absolute/relative split per value.
- **Width** — the fraction of one beat from the start of this step to the
  start of the next (i.e. this step's *duration*, not a global tempo)
- **Transition** — what fraction of `width` is spent moving toward the step's
  value vs. holding it
- **Accel/Decel** — curve shaping per step, either Proportional or Free
  (free-form spline) mode

### Phaser-level layers — shared across every step

- **Speed** — playback rate, expressed in BPM, Hz, or seconds-per-beat (an
  operator picks whichever unit matches how they think about the song — worth
  copying directly, since Ignition's whole differentiator is BPM-native cues)
- **SpeedMaster** — a phaser can slave its speed to a speed-master executor,
  so one fader/tap-tempo source drives every phaser bound to it at once
- **Phase** — a 0–360° offset **per fixture, per attribute** — this is the
  mechanism behind "fixtures 1-2-3-4 chase in sequence" or "movers fan out
  symmetrically": one phaser definition, N different phase offsets, applied by
  the fixture's position in the selection (or explicitly per fixture)
- **Measure** — how many beats one full loop of the step list takes; combined
  with Speed this is what fixes the phaser's real-world duration

### Playback model

Steps do not "jump" between discrete states the way a classic chase does.
Given `width`/`transition`/accel/decel per step and a shared `speed`, the
console **continuously interpolates** — width/transition together describe a
hold-then-move (or move-then-hold) curve within each step's slice of the
cycle, and the whole thing free-runs at `speed`/`measure` rather than being
retriggered.

### Authoring shortcut worth copying

Holding a dedicated "Step" key while tapping through several presets in
sequence **auto-creates a step per preset**, inheriting only the attributes
that changed between taps. This turns "build an 8-step chase" into "select
step 1's look, hold Step, tap step 2's look, tap step 3's look, ..." — no
separate step-table editor required for the common case. This is exactly the
kind of workflow-speed win the landscape doc (§4) identifies as the hardest,
least glamorous part of the whole project.

### Stomp — freezing a live phaser into a static look

`Stomp` resolves whatever a running phaser's output happens to be *right now*
into a single static step-1 value and discards the rest of the step table.
Useful both as a design tool (grab a moment out of a generative effect and
keep just that) and operationally (a static preset recalled onto a running
phaser needs defined blend behaviour — Stomp is part of how MA3 defines it).

## What this means for `ignition-core`

Maps directly onto the `Recipe`/`Effects`/`Phasers` section of
[`docs/domain/DOMAIN.md`](../domain/DOMAIN.md):

1. **`Recipe.selection` must be an expression** (group ref, tag filter, "from
   value"), never a frozen fixture list — this is the entire mechanism, not a
   detail.
2. **Resolution priority needs to be an explicit, ordered cascade**
   (direct-on-cue > recipe-on-cue > direct-on-preset > recipe-on-preset), not
   an ad hoc merge — copy MA3's four layers verbatim as a starting point.
3. **A phaser is a recipe with ≥2 steps**, not a separate object type. This
   keeps the object model small and matches "single-step template" and
   "generative effect" as points on one spectrum, which is also exactly what
   the section-driven auto-cue differentiator needs: a section can start as a
   1-step recipe (a static colour from `Section.color`) and gain steps later
   without changing type.
4. **Split absolute/relative per attribute at the step level**, and keep
   **phase/speed/measure as layer-level, not step-level** — this is what makes
   "8 fixtures phase-offset around one definition" cheap instead of requiring
   8 authored step tables.
5. **Steal the cooked-status marker** (green/red/open/mixed) for the cue-list
   UI from day one — it is a small feature with an outsized effect on operator
   trust, exactly the "workflow speed is the hardest part" lesson from the
   landscape doc.
6. **`Speed` in BPM/Hz/seconds** as a first-class unit choice, and wire it to
   the session tempo map the same way `SpeedMaster` wires to an executor — this
   is the direct implementation path for "beat-locked phasers" in the landscape
   doc §8.
