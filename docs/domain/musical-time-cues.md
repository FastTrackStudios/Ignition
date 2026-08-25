# Musical-time cue lists — generated, synced, and hand-runnable

**Status**: design, being built.
**Date**: 2026-08-25.
**Reads on from**: [`cue-building-architecture.md`](cue-building-architecture.md).

## The requirement, stated as a constraint

> Losing backing tracks must not mean losing lighting.

Everything below follows from that one sentence. A show that only exists
as an offset from a running audio file is a show that dies with the
audio file. So the same cue list has to be playable two ways — driven by
a clock, or driven by a person — and the two must be *the same list*,
not an automated version and a manual backup that drift apart.

The second constraint is how positions are named:

> The chorus is eight bars, not thirty-five seconds.

A cue at 61.196 s is a cue that breaks when the tempo changes, when a
section is cut, or when the band takes the last chorus twice. A cue at
bar 22 does not.

## Three decisions

### 1. Cues are addressed in musical time, and *also* ordered

```rust
pub struct Cue {
    // ... values, recipes, block ...
    /// Where this cue sits in the song, if it belongs to one.
    pub at: Option<Bars>,
}
```

Both, not either. `at` is what a clock uses; list order is what a human
uses. A cue with no `at` is still perfectly playable — that is every
show written so far. A cue *with* one can still be reached by pressing
GO, and lands in the same state either way.

This is the whole "manually runnable" requirement, and it costs one
optional field because the cue engine was already ordered.

### 2. Sync is position-addressed, never event-driven

The player is asked *"what is the state at bar 43?"*, not told *"fire
cue 9 now"*.

That distinction is the entire reason loops, restarts and section jumps
work. An event-driven player that has been told to fire cues 1..9 has no
way to answer "we jumped back to bar 22" except by replaying history. A
position-addressed one just resolves again.

The primitive already exists: `CuePlayer::jump_to_end_of` reconstructs
tracked state from the top of the list. Seeking to a musical position is
that, with the index found by position instead of given.

It is also why this is a **clock sync rather than timecode**. Timecode
says "we are 1:41.86 into the show". A clock says "we are at bar 51,
beat 1". The second survives a tempo change; the first is a promise
about wall time that a live band does not make. Timecode can arrive
later as *one more way to derive a musical position* — it does not
change the model.

### 3. The song map is data, and it comes from the project

A song is a list of sections with lengths in bars:

```text
Count-In  2    IN A  4    IN B  4    VS 1  8    PRE  4    CH 1  8
Break     1    VS 2  8    PRE   4    CH 2  8    BR   5    Breakdown 4
CH 3      12   Outro 2                                    = 74 bars
```

That is the real map of *Bye Bye Bye*, read out of its REAPER regions —
and every section lands exactly on a bar boundary, which is the evidence
that arranging in bars is how the material is actually shaped. The map
is not authored twice: it is imported from the project file, so moving a
section in the DAW moves the lighting with it.

Not REAPER-specific. RPP is what today's file happens to be; the format
this reads is the `.daw` project format, which works in REAPER *and* in
daw-standalone.

## What this makes possible later

- **Lighting as a DAW sub-project.** Once cues carry musical positions,
  a sub-project holding recipes is just another track in the same
  timeline — it seeks with everything else because it is addressed the
  same way.
- **Generated cue lists.** A section named `CH 1` with a known length is
  enough to generate a starting cue list; hits, once charted, become
  cues at their own bar positions. `keyflow`'s chart generation is the
  eventual source, and it produces exactly this shape.
- **Video on the same spine.** A clip is a thing that starts at a bar
  and runs for a number of bars. Nothing above is about lights.

## Deliberately deferred

- **Where the clock comes from.** The model needs a musical position
  per frame and does not care who computes it. Ignition can own a
  transport, or take one from `daw-standalone` over the network, or
  follow MIDI clock. Deciding that *before* the musical model exists
  would be deciding it blind — and taking a git dependency on the
  FastTrackStudio tree is a real cost that should be paid for a reason,
  not on the way to one.
- **Tempo maps with more than one tempo.** The engine takes a map, not
  a number, from the start; it just has one point in it today. Songs
  that ritard are a data problem, not a redesign.
- **Hits.** Cues at arbitrary bar/beat positions already express them.
  What is missing is a way to *author* them, which is the chart.
