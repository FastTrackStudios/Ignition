# Effects

An effect is a [recipe](recipes.md) that moves. `r[recipes.steps-are-the-switch]`
already says the only thing that separates a look from an effect is a second
step; this file says what the steps mean, where the time comes from, and how a
moving thing sits on top of a still one without wrecking it.

grandMA3 calls this a Phaser: steps with Width, Transition, Accel and Decel;
Speed, SpeedMaster, Phase and Measure shared across the steps; sixteen speed
masters from 0 to 225 BPM. Eos gets six visibly different effects out of one
step table with its play modes. Both are copied here where they earn it and
departed from where Ignition's beat-locked premise wants more.

Prior art:
[docs/research/grandma3-recipes-and-phasers.md](../research/grandma3-recipes-and-phasers.md),
[docs/domain/cue-building-architecture.md](../domain/cue-building-architecture.md)
(Decisions 3 and 5).

## Steps

r[effects.step]
A step MUST carry an **apply** list, a **width**, a **transition** and an
**ease**, and nothing about timing. A step can set several things at once — a
colour *and* a level — because "tap through four looks to build a four-step
chase" is the authoring model, and a look is rarely one attribute.

r[effects.step.width]
Width MUST be a step's share of one cycle **relative to the other steps**, not
a duration. `[1, 1]` and `[3, 3]` MUST be the same effect; `[3, 1]` holds the
first step three times as long. A width in seconds would have to be re-authored
every time the speed changed, and speed changes every song.

r[effects.step.transition]
Transition MUST be the fraction of a step's width spent **moving toward** its
values, the remainder held. `0` MUST snap — a classic chase — and `1` MUST
never stop moving. The blend is 0 the moment a step takes over and 1 once it
has fully arrived, so a step's transition is *into* it from the step before.
This is MA3's Width/Transition pair and it is what makes hold-then-move and
move-then-hold one mechanism rather than two step types.

r[effects.step.ease]
A step MUST carry an ease that shapes its transition; at least **Linear** and
**Sine** MUST exist. This is MA3's Accel/Decel in proportional mode, and it is
the reason a two-step recipe can be a sine rather than a triangle — which in
turn is why waveforms can be sugar instead of a second engine.

r[effects.interpolate]
Between two steps an attribute MUST be interpolated from the outgoing step's
value to the incoming one's. An attribute the outgoing step did not set has
nothing to move from and MUST take the incoming value directly. Interpolating
from "whatever the fixture is doing" would make a step's look depend on the
cue underneath it, and the point of a step is that it says what it means.

## Timing

Timing is a property of the effect, shared across every step. MA3 keeps
Speed, SpeedMaster, Phase and Measure at the phaser level for one reason and
it is the right one: eight fixtures phase-offset around one definition costs
one step table, not eight.

r[effects.timing.uniform]
Speed, measure, phase spread, phase offset, play mode and `once` MUST be
carried once per effect and MUST apply to every step. A per-step speed is a
different effect wearing the same name, and a reader cannot tell which step is
setting the pace.

r[effects.speed]
Speed MUST be expressible in **Hz**, **BPM**, **seconds per beat**, or as a
**named speed master**, and all four MUST resolve to one internal rate. An
operator picks whichever unit matches how they think about the song; the
engine gets one number. See `r[recipes.timing-in-musical-terms]`.

r[effects.speed.default-is-still]
An effect that does not state a speed MUST hold still rather than run at a
tempo picked for the operator. A one-step recipe never asks; a multi-step
recipe that forgot to say is a mistake, and a still chase is a visible one.

r[effects.measure]
Measure MUST say how many **beats** one full loop of the step list takes,
with speed and measure together fixing the real-world duration. The library
authors everything in bars (`measure = bars × 4`) because a lighting idea is
"once a bar" or "twice a bar" and never "every 3.7 beats".

r[effects.phase.spread]
Phase spread MUST say how much of one cycle is distributed across the
selection **in its order**, from 0 (lockstep: a pulse) to 360 (a classic
chase, each fixture visibly offset from its neighbour). The spread MUST land on
the **units** the recipe's Tricks produce, not on raw fixtures — five blocks of
two get five phases (`r[tricks.spread.blocks-are-units]`).

r[effects.phase.values-per-fixture]
Phase is decided per unit; values MUST still be resolved per fixture. A step
can say something fixture-relative — a focus point is a different pan and tilt
for every head aiming at it — so a blocked pair shares a moment and still aims
individually. That is what blocking is supposed to mean.

r[effects.phase.offset]
An effect MUST carry a fixed phase offset applied to every fixture equally.
Two recipes on Pan and Tilt a quarter-cycle apart *are* a circle, and the
offset is how that is said without a dedicated position-effect type.

r[effects.play]
An effect MUST carry a play mode, with at least these five: **Forward** — the
first unit in the selection leads; **Reverse** — the other end of the
selection leads (the spread fraction is mirrored, nothing else changes);
**Bounce** — one cycle runs the step list out and back, 1‑2‑3‑4‑3‑2‑1, so the
wave washes rather than snapping at the wrap; **Build** — each unit arrives
when the cycle passes its position and holds the last step until the wrap, so
the selection fills up and resets; **Negative** — every value is reflected
about that attribute's own swing across the step table, so the travelling
point is the dark one. Eos's docs call Negative "the one most people never
build", and on a full wash it is the gap crossing the stage that no amount of
forward chasing produces.

r[effects.play.build-is-a-mode]
Build MUST be a mode, not a step table. It is a threshold against a unit's
position in the selection rather than a position in the step list, and no
phase shift of a shared waveform can say it.

r[effects.play.is-not-direction]
Play mode MUST NOT be how an author says "left to right". Direction belongs to
the selection's order (`r[recipes.selection-owns-order]`); Reverse exists for
when the order is already the one meant and the other end should lead.

r[effects.play.random]
Eos's **Random** MAY be provided, and if it is, it MUST be a seeded reorder of
the selection (`r[tricks.shuffle]`) rather than a play mode, so the same seed
gives the same look on the next night. There is no Random play mode;
`Trick::Shuffle(seed)` on the recipe is the whole feature.

r[effects.once]
An effect MUST be able to run its step list once and hold on the last step.
See `r[recipes.one-shot]` for why, and `r[recipes.one-shot-clock]` for which
clock it runs on. Holding, not stopping: a one-shot that wrapped would strobe,
and one that vanished would leave the fixture wherever the last frame caught
it.

## Speed masters

r[effects.masters.registry]
Speed masters MUST be a named registry of tempos, and an effect MUST reference
one by name. MA3 numbers sixteen of them; a name is what lets a library effect
say `Song` and mean it in any show.

r[effects.masters.song]
A master named **Song** MUST be driven by the transport's tempo map whenever a
transport is present. It is the master the library is written against, and it
is what makes "one cycle per bar" one cycle per bar of *this* song, with a
tempo change carrying every effect in the show.

r[effects.masters.tap]
A master named **Tap** MUST be driven by tap tempo. Busking is not playback: a
support act, a change-over and a worship set nobody sequenced have no song
master, and four taps is the oldest tempo source there is. The busked entries
in the library are the same recipes slaved to `Tap` rather than a second
library — an effect that behaved differently depending on where its tempo came
from would be two effects wearing one name.

r[effects.masters.unknown]
An effect slaved to a master nobody has set MUST NOT freeze, and MUST NOT go
silent about it. The engine runs it at an ordinary fallback tempo (120 BPM) and
reports the missing master against every cue that asked for it.

The obvious rule — stop, so a frozen chase reads as "not wired up" — was the
first one written and it was wrong in a way only a one-shot exposes. Freezing a
loop is merely still. Freezing a **one-shot** parks it at its lift: a flash key
fired before a transport loaded holds the rig bright with no way to release
it, precisely the stuck light one-shots exist to make impossible. A master set
to zero MUST be treated the same way, since "stopped" is not a tempo anybody
programs. The mistake is still reported; it is no longer reported by leaving a
light on.

r[effects.masters.uniform]
A speed master MUST drive every recipe the same way, with no second code path.
This is Decision 5, and it is where Ignition deliberately goes past MA3: there
a speed master can drive a phaser but not a recipe-driven effect. Because a
phaser here *is* a recipe, one tap source drives everything in the show
whether it was authored as a step table or generated from a section.

r[effects.masters.scale]
MA3's Speed Scale — a global multiplier over every master — MAY exist, and if
it does it MUST be operator state on the programmer, not a field on any
master. It is `r[recipes.rate]` applied to all of them at once. It travels on
the `Show` as `speed_scale`, so the cue player and the faders read one number.

## Waveforms

r[effects.waveform.is-sugar]
A named waveform — at least **Sine**, **Triangle**, **Square**, **RampUp**,
**RampDown** — MUST be authoring sugar that expands to a step table on load.
The runtime MUST have one representation: steps. "Sine" is a worse thing to
write as two fully-eased steps than as the word, and it is how every other
console spells it; but a second engine is how a cascade rule gets implemented
twice and differently.

r[effects.waveform.starts-low]
A waveform MUST start its cycle at the low end of its swing and rise. Because a
transition moves *into* a step, the table lists the high step first; that is
correct, not backwards, and it is what every other console means by a sine.

r[effects.waveform.ramp-snaps]
A ramp's snap-back MUST be a step of near-zero width (2% of the cycle) rather
than a true discontinuity. It reads as instant and keeps the model to one
mechanism.

## Absolute and relative

r[effects.modulates-with-delta]
An effect that modulates a look MUST write **Delta** values, never absolute
ones. A Delta says how much to add or take away and leaves everything it did
not name alone, so a chase runs over whatever colour the cue set instead of
replacing it. An absolute effect is a look that happens to move; a relative one
is an effect. The layering rules are `r[recipes.relative-is-a-separate-layer]`
and `r[recipes.relative-leaves-colour-alone]`.

r[effects.delta-ends-at-nothing]
A relative one-shot MUST end at zero. Held on its last step, a Delta of zero
contributes nothing, which is the same as not being there; a one-shot that
settled anywhere else would leave the rig a little brighter every time it fired,
and a song's worth of snares would ratchet the whole show up.

r[effects.size-scales-the-swing]
`r[recipes.size]` MUST scale a relative effect's swing about zero and an
absolute waveform's swing about its base, and MUST NOT move the base. At size
zero a Delta effect is exactly absent and an absolute one sits at its base
value. Size travels on the `Show` and is applied where every recipe is
expanded, so a fader and a cue-player effect scale the same way.

## The library

r[effects.library.profile-ships-it]
A profile MUST be able to ship named effects, and the default profile MUST
ship the default library (the fifty-one in `effects.rs`, baked into
`data/profiles/ignition.ig-profile`). A vocabulary of roles without the
effects that use them is half a profile.

r[effects.library.by-name]
A show MUST reference a library effect by **name**, not by copying its steps.
The library is the one place a chase is defined, and a venue or a person adding
their own MUST be able to do so as data without touching the engine.

r[effects.library.roles-only]
A library effect MUST select by **role** — `Wash`, `Movers`, `Bars`, a union of
roles — and MUST NOT name a fixture, a group or a venue. That is what the whole
Profile/Venue/Trick apparatus was for: `chase` on `Wash` works in Norco,
Riverside and a room nobody has built yet.

r[effects.library.missing-role-is-empty]
A library effect whose role this venue does not implement MUST resolve to no
fixtures and be reported, not fail. A rig without movers is a rig where the
orbits do nothing, and a show that would not load there is a show that cannot
be run anywhere smaller than the room it was written in. See
`r[recipes.status.selects-nothing-is-not-an-error]`.

r[effects.library.categories]
The library MUST cover at least: intensity chases and pulses; movers (orbits
and Lissajous figures, written as Pan and Tilt in one recipe a quarter-cycle
apart); strobes; sparkle; colour cycles; focus moves; zoom and breathe; and
whole-rig accents. Every entry MUST be slaved to a named master and every
intensity entry MUST be relative except where its name says otherwise.

## Bumps

r[effects.bump]
A bump MUST be a standard one-shot effect with a named shape — at least
**Level**, **White**, **ColorBoost** and **Burst** — built from a selection, a
kind and a depth. It snaps up and falls back on its own; a looping bump is a
strobe.

r[effects.bump.shape]
A bump's lift MUST snap (transition 0) and its fall MUST transition the whole
way over a wider step. A hit that eases in has already missed the moment it was
for; a fall that snapped would be a second hit. **White** MUST drive every
emitter, because adding to red alone tints rather than flashes; **ColorBoost**
MUST touch only the level, because adding white to a saturated look washes it
out, the opposite of boosting it.

r[effects.bump.fall-beats]
A bump's fall MUST be measured in beats against `Song`, and the default
(`FALL_BEATS`, just under an eighth) MUST let a figure written in eighths clear
each flash before the next arrives. Longer smears hits together; much shorter
ticks rather than punches.

r[effects.bump.one-object]
A charted hit fired by the transport and an operator's flash key MUST produce
the same object. A snare that flashes the rig and an operator flashing on the
same snare should be indistinguishable in the output and stay that way when
either side changes; two implementations drift within a week, and the drift
shows up as "the chart feels different from playing it by hand", which nobody
can debug.

r[effects.bump.is-not-held]
A flash key MUST fire a one-shot, not hold a state. Held-down-stays-up needs a
release to arrive, and it does not when a hand slips, a note-off drops or a
charted hit has no matching end. The cost is that holding the key does not
hold the light, and for a rig driven mostly by a chart that is the right trade.

## Live control

r[effects.live-control-on-programmer]
Size and rate MUST live on the programmer, not the recipe — see
`r[recipes.size]`, `r[recipes.rate]` and `r[recipes.live-control-is-not-stored]`.
A library entry MUST NOT grow a timid and a bold spelling to work around their
absence.

## Phase and sync

r[effects.sync.shared-clock]
A looping effect MUST run on the shared show clock, so two cues carrying the
same chase stay in phase across the cut between them. This is MA3's Sync, and
it is what `r[recipes.one-shot-clock]` protects from the other side.

r[effects.sync.follows-the-song]
When a transport is present the show clock MUST be the song's position, so
cycle position *is* position in the bar. A snare pulse written on beats two and
four then lands on beats two and four rather than on whatever beat the app
launched on; a scrub moves the clock with it, so effects arrive already in the
phase they would have had; and a stopped song stops the pulse. Fades are
untouched — two seconds is two real seconds whether or not a song is playing.

r[effects.sync.pure-function]
Every play mode MUST be a pure function of the clock. Nothing in an effect may
keep state between frames, or a seek, a re-cook or a second player would
produce a different picture from the same show at the same moment.

r[effects.phase.in-selection-order]
Phase MUST be counted in the selection's order (`r[recipes.selection-owns-order]`),
with unit 0 at the leading end. A phase counted in patch order would put the
chase in a different place on every re-patch.

## Stomp

r[effects.stomp]
An absolute value arriving on an attribute an effect is modulating MUST take
the absolute layer outright (`r[recipes.absolute-last-wins]`) and MUST leave
the relative layer running. The effect keeps modulating the new base; it does
not stop, and it is not averaged with what stomped it. MA3's Stomp freezes a
running phaser into a static value; Ignition gets the operator's intent — "this
is the look now" — from the cascade, and keeps the effect's intent — "and it
still moves" — because the two live in separate layers
(`r[recipes.relative-is-a-separate-layer]`).

r[effects.stomp.freeze-is-explicit]
Freezing an effect's current output into a static look MUST be a deliberate
authoring verb, not a side effect of recalling a value over it. It is a useful
design tool — grab a moment out of a generative effect and keep only that — and
an appalling thing to have happen by accident mid-song. `CuePlayer::freeze`
is that verb: it returns the current output as direct values and leaves
playback running.

## Curves, randomness, and stacking

r[effects.step.accel-decel]
A step MUST be able to carry an **accel** and a **decel** (each −1…+1, MA3's
−100…+200 normalised) shaping how the move out of the previous value starts
and how the arrival at this one ends. A sine is accel −1 / decel −1; a snap is
transition 0; "hit hard, fall soft" is accel +1 / decel −1. The two existing
eases MUST remain expressible as sugar.

r[effects.random]
A recipe MUST be able to apply a **random generator** per unit: a level range,
a variance on that range and on speed, and an attack/decay — fire, candle, an
electrical fault, a sparkle whose density is a fader. It MUST be a pure
function of (seed, unit, time) so a seek, a replay and a second machine agree
frame for frame. A shuffled step table is not this: every unit there runs the
same shape.

r[effects.invert]
A Trick MUST be able to **invert** a relative value's sign per unit and per
attribute style — pan only, tilt only, both, or every attribute — so two halves
of a rig circle in opposite directions from one recipe. Mirroring the
selection (`r[tricks.mirror]`) changes *which* fixture takes a value; this
changes the value.

r[effects.relative-stack]
Two sustained relative recipes on one attribute MAY be declared **stacking**,
in which case their contributions sum instead of the later winning. A slow
tilt wave under a fast shiver is one idea that cannot be one recipe. Default
remains last-wins (`r[recipes.relative-last-wins]`); stacking MUST be opt-in
on the recipe so nothing already authored changes.

r[effects.morph]
A cue MAY be flagged **morph**, in which case a recipe it replaces is
crossfaded *value by value* over the cue's fade rather than swapped at take:
a circle melts into a figure-eight, a four-step chase into an eight. Both
sides are pure functions of time, so this is one extra evaluation per frame,
not a new engine.

r[effects.bundle]
The library MUST be able to ship a named **bundle** — several recipes taken as
one — so a single name carries "pan sweeps every two bars, dimmer pulses every
beat, colour steps every bar". Timing stays uniform per recipe
(`r[effects.timing.uniform]`); the bundle is how one attribute set gets
several clocks.

r[effects.align]
Spreading a value across a selection MUST support the four Align shapes — from
first to last, first fixed, last fixed, and **centre-out** / **ends-in** — and
a curve (linear, sine, slow, fast). A centre-out zoom fan and an ends-in tilt
"V" are half of what reads as designed rather than chased.

r[effects.step-transforms]
Authoring MUST provide pure transforms over a step table — reverse in time,
rotate and scale a two-axis path, swap axes, flip one axis — so an ellipse
tilted thirty degrees is a circle with two transforms, not a new table.
