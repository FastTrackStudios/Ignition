# Cue-building architecture — decisions needed

**Status**: **decided** 2026-08-25 — see "Decisions taken" at the foot.
Feeds `DOMAIN.md`'s Recipes / Effects sections.
**Date**: 2026-08-25.
**Reading order**: [`grandma3-recipes-and-phasers.md`](../research/grandma3-recipes-and-phasers.md)
first — this doc assumes it and only argues about what to build.

`DOMAIN.md` already describes the target model. What exists in
`ignition-core` today diverges from it in three places, and only one of
them is a small fix. This is the honest gap, then the decisions.

## Where the code actually is

| `DOMAIN.md` says | `ignition-core` does |
|---|---|
| A recipe is stored into a cue and cooked against a live selection | `expand_cue_list` resolves recipes **at load time** and discards them; `Cue` holds a flat `Vec<CueValue>` |
| Resolution priority is a 4-layer cascade | Last-write-wins across a flat value list |
| A phaser is a recipe with ≥2 steps | `EffectRecipe` is a separate type with a separate player, running beside cues rather than inside them |
| `SelectionExpr` = group \| tag filter \| from-value | `RecipeTarget` = group name \| explicit channel list |
| Steps carry absolute **and** relative values | Only absolute |
| Speed in BPM/Hz/sec, slaveable to a speed master | `rate_hz: f32`, no master |
| `Cue.block` from day one | No `block` field |

The first row is the one that matters. Everything else in this document
follows from it, because **a recipe that has already been flattened into
values is not a recipe any more** — it cannot re-cook when a group gains
a fixture, cannot report cooked status, cannot be edited in place, and
leaves no layers for a cascade to cascade over. Baking at load time is
not a shortcut on the way to the model; it is the one thing that
forecloses it.

## Decision 1 — when does a recipe resolve?

**A. Load time (today).** Cheap, already works, and every downstream
consumer sees plain values.

**B. Cook on demand, store the output next to the recipe** — MA3's actual
model. The cue holds both the template and its last cooked result; the
cooked values are what plays, and re-cooking is an explicit operation.

**C. Resolve every frame from the recipe, store nothing.** The cue holds
only templates. Output is a pure function of (recipe, selection, time).

**Decided: C for now, but the model must not foreclose B** — Ignition is
not targeting lightweight console hardware forever, and caching is the
answer at a rig an order of magnitude larger than Norco. Resolve every
frame today because it is simpler and this rig is small; keep resolution
behind a single call so a cache can be introduced later without the
callers knowing. Concretely: nothing outside the resolver may hold a
resolved value across frames, and resolution must be a pure function of
(layers, selection, time) so a memo table is a legal optimisation rather
than a rewrite.

The reasoning for C over B *today*:

C is what "content that regenerates rather than gets baked" actually
means, and it is *simpler* than B — no cached-output invalidation, no
stale-cook bugs, no question about what happens when a group changes
under a cooked recipe. The reason MA3 needs B is hardware: an 8000-fixture
rig cannot re-resolve every selection every frame on a console CPU. At
this rig's scale — 71 fixtures, ~100 channels — re-resolution is free,
and we should spend that freedom on a simpler model rather than copy a
constraint we do not have.

The one thing B buys that C does not is somewhere to put a hand tweak
after cooking ("that one par is too hot"). That is exactly what the
cascade's top layer is for, so it comes back in Decision 2 rather than
being lost.

C also changes what "cooked status" means, for the better: it stops being
a property of a stale cached result and becomes a live fact about the
current resolve — green/red/open computed on the spot.

## Decision 2 — the cascade, and what a cue holds

Adopt MA3's four layers verbatim, because their *order* is the whole
feature and there is no reason to invent a different one:

```rust
pub struct Cue {
    pub name: String,
    pub fade_secs: f32,
    /// Layer 1 — direct values on the cue. Hand tweaks and recorded
    /// output. Always win.
    pub values: Vec<CueValue>,
    /// Layer 2 — recipes on the cue, resolved at output time.
    pub recipes: Vec<Recipe>,
    /// Does not track from its predecessor. Required before song
    /// sections can reorder; cheap now, a retrofit later.
    pub block: bool,
}
```

and a `Preset` carries the same pair, supplying layers 3 and 4. Per
`(ChanId, Attribute)`, the first layer with a value wins and the rest are
discarded — never merged, never averaged. One value per attribute per
frame, resolved by an order the operator can state out loud.

This is also where the hand tweak from Decision 1 lands: recording a
tweak writes a layer-1 `CueValue` over a layer-2 recipe, and deleting it
lets the recipe show through again. That is the behaviour an operator
already expects from a console, obtained for free from the ordering.

## Decision 3 — do Recipe and Effect become one type?

The research is unambiguous: *"A Recipe with one step is an ordinary
parametric value template. A Recipe with two or more steps is a Phaser."*
Today they are two types, two players, two authoring formats, and an
effect cannot live inside a cue at all — it runs in a parallel stream
that a cue cannot start, stop, or fade.

**Decided: unify.**

```rust
pub struct Recipe {
    pub selection: Selection,
    pub steps: Vec<Step>,               // 1 = static look, 2+ = phaser
    pub timing: Timing,                 // ignored when steps.len() == 1
}

pub struct Step {
    pub values: Vec<(Attribute, Value)>,
    pub width: f32,       // this step's share of the cycle
    pub transition: f32,  // fraction of width spent moving vs. holding
}

pub enum Value {
    Absolute(f32),
    /// Modulates whatever a lower layer already set — how "pulse the
    /// intensity but leave the colour alone" works without the phaser
    /// needing to know what the colour is.
    Relative(f32),
    Color(Ref<ColorPreset>),
    Focus(Ref<Vec3>),
}

pub struct Timing {
    pub speed: Speed,
    pub measure: f32,           // beats per full loop of the step list
    pub phase_spread_deg: f32,  // spread across the selection, in order
    pub phase_offset_deg: f32,
    pub direction: Direction,
}
```

Unifying costs one thing worth naming: the current `Waveform` (sine,
triangle, ramp) is a *continuous* shape, and a step table is discrete
with interpolation. A sine is expressible as two steps at 100%
transition, but that is a worse way to say "sine".

**Keep `Waveform` — demote it to a step-table constructor.** A show file
can still say `{"waveform": "Sine", "size": 0.4, "base": 0.6}`; it
expands to steps on load. One runtime, one cascade, one player, and the
ergonomic spelling survives. This is the same authoring-form/runtime-form
split `expand_cue` already uses and GDTF/OFL import established.

`Relative` is what makes this worth doing rather than just tidy. Today an
intensity chase over a coloured wash has to restate the colour, because
the effect player's output overwrites. With relative values the phaser
says "−40% dimmer" and the colour underneath is simply not its business.

## Decision 4 — how much of a selection expression?

`Selection` must be an expression, and it must be **ordered**, because
phase spread is defined by position in the selection.

```rust
pub enum Selection {
    Group(String),
    Chans(Vec<ChanId>),
    /// `fixtures.json` already carries tags — this is free data we are
    /// not using.
    Tag(String),
    Model(String),
    Union(Vec<Selection>),
    Intersect(Vec<Selection>),
    Except(Box<Selection>, Box<Selection>),
}
```

**Decided: all of the above except MA3's `<From Value>`** (a
programmer-workflow affordance that only makes sense with MA3's command
line), **plus spatial selection — filtering and, more importantly,
*ordering* by real position.**

`Tag` is the cheapest win here: the venue's fixtures are already tagged,
so `Tag("mover")` needs no new data and gives recipes that survive a
re-patch.

Spatial is the one no other console in this class can do, because it is
the one thing Ignition uniquely has: every fixture's real hung XYZ. A
left-to-right chase on a real console means knowing, by hand, which
channel numbers happen to run left to right, and re-doing that knowledge
every time the rig moves. Here it is a property of the room:

```rust
pub enum Axis { X, Y, Z }

/// Which fixtures — a predicate on real position.
pub enum Where {
    /// Half-space: keep fixtures on one side of a plane. "Stage left
    /// half" is `Half { axis: X, cmp: Gt, at: 0.0 }`.
    Half { axis: Axis, cmp: Cmp, at: f32 },
    /// Inside an axis-aligned box — a zone of the room.
    Within { min: Vec3, max: Vec3 },
    /// Within a radius of a point.
    Near { at: Vec3, radius: f32 },
}

/// What order — the half that makes effects spatial rather than
/// patch-order accidents.
pub enum Order {
    /// However the underlying selection produced them (group order,
    /// which `DOMAIN.md` already says is meaningful).
    Native,
    /// Sorted along an axis. `Axis(X, Asc)` is a left-to-right chase;
    /// `Axis(Y, Desc)` sweeps downstage to upstage.
    Axis(Axis, Dir),
    /// Sorted by distance from a point — a centre-out bloom, or a
    /// ripple away from wherever the singer is standing.
    Distance { from: Vec3, dir: Dir },
    Reverse,
}
```

Both are nodes in the expression tree rather than fields beside it, so
they compose: `Order::Axis(X, Asc)` applied to
`Where::Half { Z, Gt, 2.0 }` applied to `Group("Washers")` is "the
ceiling washers, left to right". Ordering matters because phase spread
(Decision 3) is defined by position *in the selection* — which is
precisely why sorting the selection by real position is what turns a
generic chase into a directional one.

## Decision 5 — where time comes from

Ignition's stated differentiator is beat-locked content, so speed cannot
stay `rate_hz: f32`.

```rust
pub enum Speed {
    Hz(f32),
    Bpm(f32),
    Secs(f32),
    /// Slaved to a named tempo source — tap tempo, a fader, or (the
    /// point) the session tempo map from the FastTrackStudio side.
    Master(String),
}
```

**Decided: build all four now.** The first three are unit conversions
and cost nothing. `Master` is the seam that makes "cues that follow the
song" possible without a redesign, and leaving it out is how a
`rate_hz` field becomes load-bearing again.

**And this is where we deliberately go past MA3.** On grandMA3 a speed
master can drive a *phaser*, but a recipe-driven effect cannot be
slaved to one — the two systems only partly meet. Because Decision 3
makes a phaser *be* a recipe rather than a neighbouring object type,
`Speed::Master` applies to every recipe uniformly, with no special case
and no second code path. One tap-tempo source drives every effect in
the show, whether it was authored as a step table or generated from a
group. That is not a coincidence of the design; it is the main reason
to prefer one type over two.

## Decision 6 — what tracking means once cues hold recipes

The subtle one, and the reason to decide it before writing code.

Today `CuePlayer` tracks a flat `HashMap<(ChanId, Attribute), f32>`: a
cue that does not mention a channel leaves the previous value in place.
If cues hold recipes that are live functions of time, the tracked thing
can no longer be a number — a phaser left running from three cues ago
must keep moving.

**Decided: track the layer stack, not the values.** `CuePlayer`
holds the set of currently-active layers (direct values and recipes),
carried forward by the same tracking rule; each frame it resolves that
stack through the cascade to get output. A cue that does not mention a
group does not touch its layers, so its phaser keeps running — which is
what an operator means by "tracking" once effects exist.

Fades then need care in exactly one place: crossing between two cues when
both sides are time-varying. The from-side stack has to stay alive for
the duration of the fade and be resolved every frame alongside the
to-side, with the crossfade applied to the two resolved outputs. That is
one extra stack and one extra resolve per frame, and it is what makes a
phaser fade *in* rather than snap.

## Staging

Each step is independently shippable and leaves the tree working:

1. **Cascade + `block`.** `Cue` gains `recipes` and `block`; resolution
   moves from load time to output time. Existing shows keep working —
   they simply have an empty layer 2. This is the step that unlocks the
   rest; nothing else should start before it.
2. **`Selection`.** Widen `RecipeTarget`, add `Tag`/`Model`/set algebra.
   Pure addition, no behaviour change to existing shows.
3. **Unify `Recipe` and `EffectRecipe`.** Steps, `Value::Relative`,
   `Waveform` demoted to a constructor. Retire `EffectPlayer`.
4. **`Speed` and speed masters.** Tempo sources as a registry.
5. **Cooked status** surfaced in the loader and, later, the cue-list UI.

The order is not arbitrary: 1 is a precondition for 3 and 6, and 2 is a
precondition for the phase-spread semantics in 3 being meaningful.

## Deliberately not decided here

- **Cue parts.** MA3 subdivides a cue into parts with their own fade
  times, and the cascade is defined per part. Nothing in this rig needs
  parts yet, and adding them later is a layer inside a layer rather than
  a redesign — but the cascade should be written so a part boundary can
  be inserted without re-ordering it.
- **MAtricks / relative value ranges across a selection** ("fan the tilt
  from −20 to +20 across these eight"). Real and wanted, but orthogonal:
  it is a value *generator* over an ordered selection, and it plugs into
  `Value` once `Selection` is ordered.
- **Stomp.** Needs a programmer to stomp into, which needs Decision 2 to
  exist first.

## Decisions taken (2026-08-25)

| # | Decision |
|---|---|
| 1 | Resolve per frame **for now**; keep resolution a pure function behind one call so caching can land later without touching callers. Ignition is not a lightweight-hardware target long-term. |
| 2 | MA3's four-layer cascade verbatim; `Cue` gains `recipes` and `block`. |
| 3 | `Recipe` and `EffectRecipe` unify; `Waveform` demoted to a step-table constructor; `Value::Relative` added. |
| 4 | Full selection algebra **plus spatial filter and spatial ordering**. No `<From Value>`. |
| 5 | `Speed::{Hz,Bpm,Secs,Master}` — and unlike MA3, a speed master drives **every** recipe, not only hand-authored phasers. |
| 6 | Tracking carries the layer stack, not resolved values; both sides of a fade resolve every frame. |
