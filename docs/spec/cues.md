# Cues and cue lists

The show, as a sequence of looks. A cue is what an operator takes; a cue list
is the order they take them in; tracking is the rule that a cue only has to say
what *changes*.

This spec is about the list and the player. What a cue *contains* is the
cascade in [recipes.md](recipes.md); how competing values resolve is
[playback.md](playback.md); the song positions cues are addressed by are in
[song.md](song.md); the hits that fire *between* cues are
[triggers.md](triggers.md).

Prior art: [Tracking](https://help.malighting.com/grandMA3/2.2/HTML/cue_tracking.html),
[Cue Timing](https://help.malighting.com/grandMA3/2.2/HTML/cue_timing.html),
[Store Cue](https://help.malighting.com/grandMA3/2.0/HTML/cue_store.html),
[Move In Black](https://help.malighting.com/grandMA3/2.2/HTML/cue_mib.html),
and [docs/domain/musical-time-cues.md](../domain/musical-time-cues.md).

## What a cue is

r[cues.shape]
A cue MUST carry: a number, a name, a fade time, its direct values, its
recipes, a blocking flag, and an optional musical position. Nothing else is a
property of a cue; timing that varies per attribute belongs to the recipe that
sets it, and a subdivision of a cue into separately-timed pieces is a
subdivision into recipes (`r[cues.parts-are-recipes]`).

r[cues.number]
A cue MUST carry a **number** that is stable under insertion, distinct from its
index in the list. Numbers MUST be able to be fractional, so a cue inserted
between 5 and 6 is 5.5 and every note, cue sheet and spoken call that said
"six" still means the same cue. A list whose cues are addressed by index
renumbers half the show every time somebody adds a look, which is how a
rehearsal note stops matching the desk.

r[cues.note]
A cue MAY carry a **note** of arbitrary length, distinct from its name. The
name is what fits in a column and gets called over comms; the note is why the
cue is the way it is — "wait for the bass drop, MD holds the bar if the crowd
goes" — and it is the first thing lost when the only text field is one line
wide. A note MUST NOT affect output.

r[cues.appearance]
A cue MAY carry an **appearance**: a colour, and optionally a short label,
used only to draw it. A hundred-cue list is scanned, not read, and section
boundaries the eye can find are worth more than any single column of data.
Appearance MUST NOT affect output.

r[cues.recipes-not-values]
A cue written by a person or a generator SHOULD consist of recipes, and MUST be
able to consist of recipes only. Direct values are layer 1 of the cascade — the
hand tweak, the recorded output — and a show whose cues are mostly direct values
has stopped being portable. *Bye Bye Bye* has zero direct values in 105 cues,
and that is the target, not an accident.

r[cues.fade-is-arrival]
A cue's fade time MUST describe arriving at it — how long the move *into* this
cue takes — matching Eos and MA3, where the time on a cue is its own in-time.
A fade on the cue being left is the other convention and it is the one
operators from any console will get wrong.

r[cues.fade-in-beats]
A generated cue's fade SHOULD be authored in beats and converted to seconds
through the tempo map when the list is written or played. A fade of "two beats"
is the same gesture at any tempo; a fade of 1.4 s is a gesture for one tempo.

## Tracking

r[cues.tracking]
A cue MUST only state what changes. Any attribute a cue does not mention MUST
hold whatever the previous cue left it at. A forty-cue song that re-states
every fixture in every cue cannot be edited: changing the chorus colour means
changing it in twelve places.

r[cues.tracking-carries-sources]
What tracks forward MUST be the *source* of each attribute's value — the recipe
or the direct value — not the value it last produced. A chase left running by a
cue three cues back keeps moving because the player is still asking the chase,
not because it remembered a number. See `r[recipes.cook-fixes-coverage]`.

r[cues.block]
A blocking cue MUST start from empty layers, discarding everything tracked and
every relative value stacked beneath, per `r[recipes.blocking-resets]`. What a
blocking cue does not set goes *out*, faded over the cue's own time rather than
snapped.

r[cues.sections-block]
A cue that begins a song section SHOULD block. Sections are the unit a song can
be rearranged in; a chorus recalled after a bridge must not inherit the bridge's
leftovers, and a section that blocks is a section that can be looped in
rehearsal from any starting point.

r[cues.accents-do-not-block]
A cue that is an accent on a section — a lift, a hit, a widening — MUST NOT
block, and SHOULD be visibly named as an accent (the show uses a leading `·`).
It adds to the look that is running; a blocking accent would be a section with
a very short name.

## Fades

r[cues.both-sides-resolve-live]
During a fade, both the outgoing and the incoming layer stacks MUST be resolved
every frame and the crossfade applied to the two resolved outputs. A phaser
being faded *out of* keeps moving while it goes; a snapshot of it freezes,
which is visible on any slow crossfade between two effects.

r[cues.fades-stack]
Firing GO mid-fade MUST push a new fade over the one in flight rather than
discarding it; output is the fold of every live fade, oldest first. The stack
MUST be bounded, with the oldest forced to finish past the bound. Eight is the
current bound, reachable only by firing GO faster than fades complete several
times over.

r[cues.arrived-collapses]
Once a fade has fully arrived, everything beneath it MUST be dropped. A fully
arrived stage contributes nothing from what it covered, and keeping it is a
leak over a three-hour service.

r[cues.fade-is-wall-time]
A fade MUST run in real seconds regardless of whether a song is playing. The
show clock that drives effects follows the transport; the fade clock does not.
A two-second fade is two seconds while the band vamps, and it is two seconds
while the operator scrubs.

## Musical position

r[cues.position]
A cue MAY carry a musical position, and a list MUST be playable whether or not
its cues have one. `at` is what a clock uses; list order is what a person
pressing GO uses; both MUST land in the same state.
[docs/domain/musical-time-cues.md](../domain/musical-time-cues.md) is the
argument; this is the rule.

r[cues.seek]
Seeking to a position MUST land in the state that playing forward to that
position would have reached. The player is asked "what is the state at bar 43",
never told "fire cue 9 now". This is what makes a loop, a restart and a jump
in rehearsal all the same operation, and it is why a seek backwards rebuilds
from the top of the list.

r[cues.seek-is-cheap-when-still]
Seeking every frame to a position inside the current cue MUST be free. The
transport reports a position every frame; a seek that re-fired the cue would
restack a fade sixty times a second.

r[cues.replay-does-not-perform]
Replaying cues to reach a position MUST rebuild their tracked state without
performing their transients. A one-shot on a replayed cue is stamped as long
finished, so it contributes nothing; a looping recipe is unaffected because it
runs on the shared clock. Without this, reaching the second moment of a figure
re-fires the first, and by the third the whole rig flashes together — which
reads as nothing happening at all.

r[cues.one-shot.stamp-travels-with-recipe]
The time a one-shot was taken MUST be stored *with* the recipe it belongs to,
in the same entry, never in a parallel structure keyed by index. Active recipes
are renumbered whenever superseded ones are dropped, and a stamp held in a
parallel list is not renumbered with them — after the first compaction every
one-shot reads some other recipe's stamp, judges itself finished, and withdraws
before it has been seen. From the stage that was a figure whose reveal never
landed while the chase beneath went on as if nothing had fired.

r[cues.seek-keeps-the-clock]
A seek MUST NOT disturb the show clock. A phaser mid-cycle should not restart
because the operator moved the playhead; the clock is driven by the transport
when there is one, and by wall time when there is not.

r[cues.unpositioned-are-invisible-to-seek]
A cue without a position MUST be invisible to seeking and still reachable by
GO. A list can mix positioned cues with hand-only ones — a blackout the operator
keeps at the end — and the clock simply never picks the unpositioned ones up,
though replaying past one still applies it.

## Order

r[cues.sorted-by-position]
A generated list MUST be sorted by position, with blocking cues before accents
at the same position. A section cue and the hit on its downbeat are meant to
land together, and the section has to be under the hit, not over it.

r[cues.same-position-is-one-take]
Cues at the same position SHOULD be taken as one frame's work. A zero-fade
accent taken during a section's fade otherwise truncates that fade, per
`r[cues.arrived-collapses]` — the accent has arrived, so everything beneath it
is dropped, including the section still fading in. The player therefore folds
a cue that shares its predecessor's position into that predecessor's stage:
the accent's own keys take the accent's timing, everything else keeps the fade
it was already on.

## Status

r[cues.cooked-status]
Every cue MUST report, before it is fired, what its recipes resolve to on the
current rig, per `r[recipes.status.visible-per-cue]`. The list is where an
operator looks to answer "will this cue do what I think", and a cue that
selects nothing on this rig is the most common way a portable show goes wrong.

r[cues.dead-cue-warns]
A cue none of whose recipes cover any fixture MUST be reported on load, by
name, and MUST NOT abort the load. See
`r[recipes.status.selects-nothing-is-not-an-error]`.

## Parts

r[cues.parts-are-recipes]
A cue's **recipes ARE its parts**. A console that stores values needs a
separate part object to hold a second set of fade times; a cue that stores
recipes already has an ordered list of named pieces, and MUST NOT grow a second
ordering axis beside it. The recipe's index is the part number, and
`r[recipes.absolute-last-wins]` — which takes grandMA3's "highest cue part
number wins" as its source — is already the precedence rule parts need.

r[cues.recipe.name]
A recipe on a cue MAY carry a **name** and a **note**. This is the half of
parts that is documentation: "movers swing in" beside "house to half" tells the
next person what the cue is doing far better than one opaque row, and it gives
the list something to label a sub-row with.

r[cues.recipe.timing]
A recipe MAY carry its **own fade, delay and ease per attribute class**,
overriding the cue's for every key it covers. This is the half of parts that is
timing, and it is the reason parts exist at all: a cue-wide class fade cannot
say "*these* movers over five seconds, everything else over one" — only a
per-piece one can. A recipe that says nothing here MUST take the cue's timing
exactly as it does today.

r[cues.recipe.timing-precedence]
Where several timings could apply to one key, the more specific MUST win, in
this order: a pre-position's own timing (`r[cues.mib.timing]`), then a named
selection's override (`r[cues.timing-overrides]`), then the recipe's own, then
the cue's class timing. A fan (`r[cues.fan]`) adds its delay on top of whichever
won, because a fan is a spread across a timing rather than a timing of its own.
Where two recipes on one cue both cover a key and both carry timing, the later
MUST win, per `r[recipes.absolute-last-wins]` — the same rule that decides its
value decides its fade.

## Timing per attribute

r[cues.timing.per-attribute]
A cue MUST be able to carry separate **in** and **out** fades for intensity and
separate fades for colour, position and beam, in beats. Colour snaps while the
dimmer fades over two beats while the movers drift over a bar is the single
largest difference between a cue that reads as programmed and one that reads
as generated. The scalar fade MUST remain as the default for every class.

r[cues.delay]
A cue and a recipe MUST be able to carry a **delay** in beats before their fade
begins, per attribute class.

r[cues.fan]
A recipe's fade and delay MUST be spreadable across its selection as a **fan**
(`r[tricks.spread]`), so a static cue wipes: every fixture arrives at the same
red, just later, with no phaser running. A fade fan gives depth in time — front
row snaps, back row drifts.

r[cues.delay-to-phase]
Where a recipe has both a delay fan and a phase spread, the phase MUST be
measured from each unit's own start, so a wipe and a chase on one selection
agree about which fixture is first.

## Move in black

r[cues.mib]
The player MUST **pre-position** a fixture whose intensity is about to rise
from zero: its position, colour and beam values for the coming cue are applied
while it is still dark, so the change is never seen. A generator says "movers
to Drums at CH 2" cold; without this every chorus opens with a visible swing.

r[cues.mib.mode]
Pre-positioning MUST be selectable per cue among **early** (as soon as the
fixture is dark), **late** (as late as its own fade allows before the
intensity rises), and **none**; the default SHOULD be late, which keeps the
rig still until it must move.

r[cues.mib.timing]
A pre-position MUST have its own fade and delay, distinct from the cue's, and
MUST never touch intensity.

## Editing generated lists

r[cues.assert]
A cue MAY **assert**: values it would only track are re-stated with this cue's
timing, so a look reused after a bridge arrives with its fade rather than
already being there. This is how a generator reuses "the chorus look" without
blocking.

r[cues.cue-only]
A change to one cue MAY be marked **cue-only**, so it does not track into the
cues after it. A human fixing one generated cue must not silently edit the
rest of the song.

r[cues.release]
A cue MAY **release** an attribute — stop asserting it — so a lower playback or
the rest value takes over, per `r[playback.release-falls-through]`.

r[cues.morph]
A cue MAY morph its recipes from its predecessor's, per `r[effects.morph]`.

## Triggering and commands

r[cues.trig]
A cue MUST be able to be triggered by GO, by musical position, by a **follow**
(N beats after the previous cue was taken), or by a **sound** event; follows
are how a list runs on time with no transport, and MUST chain.

r[cues.command]
A cue MAY carry **commands** the host executes when the cue is taken — send
OSC, start a clip, fire a macro — separate from its lighting content, so the
show file stays a light show and the integration lives at its edge.

## Arrival curves and finer timing

r[cues.transition-curve]
A cue's fade MUST be shapeable per attribute class with a curve — linear,
slow, fast, s-curve, swing — reusing the step ease. A mover reposition that
swings into place reads as designed; a straight line reads as generated.

r[cues.timing-overrides]
A cue MAY carry timing overrides for a **selection** within it, so one fixture
or a small group takes its own fade and delay while the rest of the cue keeps
the class timing. Fans cover the gradient case; this covers the exception.

r[cues.break]
A cue MAY **break** tracking for a set of attributes: values from before it do
not track through it for those attributes, even though it does not set them.

r[cues.shield]
When a generated cue is edited, the edit MUST be able to be **shielded** from
the cues after it — a store mode that stops the change at the next cue where
the intensity comes up from zero (shield ↑0) or is above zero (shield >0).
This is the surgical form of cue-only.

r[cues.wrap-and-restart]
A list MAY **wrap** from its last cue to its first, and MUST declare how it
**restarts** when re-enabled: from the first cue, from the current cue, or from
the next.

r[cues.mib.preference]
A cue MAY carry a **preference** (0–100) for how good a moment it is to
pre-position into, and pre-positioning MUST choose the best-rated dark window
when several are available; **upon-go** (move with the next take after the
fixture goes dark) MUST be a selectable mode beside early and late.

r[cues.mib.hold]
A fixture MAY be marked **hold** so it is never pre-positioned — a light that
must stay where it is while dark, for a reveal.

r[cues.mib.multistep]
Whether a running effect keeps running or pauses while a fixture is dark and
being pre-positioned MUST be selectable.

r[cues.cook-merge]
Cooking a recipe into direct values MUST support **merge** (keep direct
values the recipe does not cover) as well as **overwrite**.

r[cues.generator-emits-mib]
A generated list MUST set pre-positioning on every cue whose position recipe
differs from the previous cue's for a fixture that is dark at the boundary.
The engine having MIB does nothing if the generator never asks for it.

## Deferred

Per-part move-in-black, per-part snap delay, and grandMA3's *allow duplicates*
— one attribute holding a value in several parts at once — are not modelled.
Each is a field that would attach to a recipe under `r[cues.parts-are-recipes]`
rather than a change to the shape, and nothing in the current rig asks for one.
