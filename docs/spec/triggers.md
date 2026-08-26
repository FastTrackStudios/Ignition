# Triggers

What fires when the song plays it. A cue is a look the operator takes; a
trigger is an event the song fires. They are different kinds of thing, and
`r[files.show.triggers]` already requires that they be stored apart. This spec
says what a trigger is and how the bus that fires them behaves.

The reason this exists as its own layer rather than as very short cues: a cue
list half of whose entries are machine-generated hits is a list nobody can
read, and a hit modelled as a cue inherits every cue behaviour it does not
want — tracking, replay, fades, a place in the GO order. *Bye Bye Bye* has 105
cues, of which 91 are hits and figures. That is the defect this spec removes.

Prior art: grandMA3 has no direct equivalent — its Temp and Flash executors and
its timecode events together do this job. Ignition's `TriggerBus` in
`crates/ignition-core/src/trigger.rs` is the reference implementation.

## What a trigger is

r[triggers.shape]
A trigger MUST carry a musical position, a recipe, and a name. The recipe
SHOULD be relative and MUST be one-shot: a trigger adds to the look for a
moment and gets out of the way, and a looping recipe here would never stop.

r[triggers.are-not-cues]
Triggers MUST be stored separately from the cue list and MUST NOT appear in
the GO order. An operator running the list by hand presses GO for sections and
lifts; hits are played by the song or by the operator's flash key, never by GO.

r[triggers.from-the-chart]
A song's triggers MUST be derivable from its hit chart — see
`r[song.chart]` — with the class of each hit selecting the bump kind and depth,
and a figure's moments each becoming one trigger against one zone. The chart
is the authority; a trigger list is its rendering against a profile.

## Firing

r[triggers.crossing-fires]
A trigger MUST fire when the playhead crosses its position moving forward, and
MUST fire exactly once per crossing. The window is half-open: a trigger exactly
on the previous position already fired last frame; one exactly on the new
position fires now. Closed at both ends it doubles; open at both it can be
skipped by a frame boundary.

r[triggers.stopped-fires-nothing]
A playhead that has not moved MUST fire nothing. A stopped transport reports
the same position every frame, and silence there must be free — a property of
the crossing rule — rather than a check somebody has to remember to write. No
signal, no light.

r[triggers.seek-locates]
A jump in position MUST be a *locate*, not a performance of the span crossed.
Scrubbing across a chorus fires nothing; treating a jump as a sweep is how a
scrub becomes a strobe. A backwards move is a locate even when it arrives as an
advance, because there is no playing a song in reverse.

r[triggers.locate-clears]
A locate MUST clear whatever is ringing. A hit from before the jump has no
business still decaying after it.

r[triggers.one-sweep-many]
One advance MUST be able to fire several triggers. A frame is sixteen
milliseconds and a fill can put two hits inside one; firing only the first
would drop the second.

## Ringing

r[triggers.own-clock]
A ringing trigger MUST run its envelope from the show time it fired, not from
the shared clock. This is `r[recipes.one-shot-clock]` applied: a hit is timed
from its own start.

r[triggers.simultaneous-sum]
Triggers ringing at once MUST sum their relative values. A kick under a crash
is both, not the louder of the two. This is deliberately the opposite of the
cue player's last-wins, and the reason is the kind of disagreement being
resolved — see `r[playback.triggers-sum]`.

r[triggers.retire]
A trigger whose envelope has finished MUST be retired, and retirement MUST
happen after the frame that read it, not inside the read. A frame may be
rendered twice without changing; a bump on a slow frame must be seen once before
it can be dropped.

r[triggers.bounded]
The set of ringing triggers MUST be bounded, oldest dropped first. A stall or a
scrub could otherwise leave an unbounded list of envelopes nobody will look at
again. Thirty-two is generous for hits that decay in under a beat.

## Holding

r[triggers.hold]
A trigger MAY **hold**: snap to its peak and stay there rather than fall. A
hit that falls away in half a beat reads as a flicker; one that stays reads as
the band having landed somewhere, and a figure's moments then carve the stage
and leave it carved. The hold MUST be a property of the trigger, so a show can
mix held hits with falling ones.

r[triggers.hold.released-by-next]
A held trigger MUST be released by the next trigger to fire — the new hit
replaces the old — except that triggers fired in the same sweep MUST keep each
other, because a cutout is a cut and a lift landing at once.

r[triggers.hold.released-by-cue]
A held trigger MUST be released when a cue is taken. The look has moved on;
the hit was a moment in the look before. A hold MUST NOT depend on any release
event from its source (a note-off, a key-up): those get lost, and a hold that
waits for one is a rig stuck on. See `r[effects.bump.is-not-held]`, which
this refines rather than contradicts — the flash key still falls.

## Where triggers sit

r[triggers.layer]
Trigger output MUST be applied above the cue player and below the programmer —
layer 5 of `r[playback.stack]`. A hit outranks the section it lands on, and the
operator's hand outranks the hit.

r[triggers.transient-class]
Triggers are the song's **transient** class, and MUST obey
`r[playback.transient-over-sustained]`: a hit covers the chase for its
duration and hands it back.

r[triggers.wired]
The trigger bus MUST be driven by the same transport that seeks the cue player,
in the same frame, so a section cue and the hit on its downbeat land together.
(Not yet built: the bus exists and is tested, but `authorshow` still emits hits
as cues and the playback loop does not advance a bus. Migrating is the next
step; the cue-based form keeps working until then.)

## Reporting

r[triggers.visible]
The number of ringing triggers and the name of the last fired MUST be available
to the overlay. "Was that hit fired or did it do nothing" is the first question
asked at every rehearsal, and the answer should be on screen.
