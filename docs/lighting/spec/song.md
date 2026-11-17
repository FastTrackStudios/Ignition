# Song

What belongs to the music, as opposed to the room or the vocabulary.

```
Bye Bye Bye.RPP            the DAW project: tempo, regions, the HITS track, audio
Bye Bye Bye.lrc            lyrics, timed in seconds by whoever wrote them
bye-bye-bye.hits.json      what the detector heard — a draft, never the authority
Bye Bye Bye.ignition       the show written against all of the above
```

A **venue** ([files.md](files.md)) says what the room has. A **profile**
([profile.md](profile.md)) says what a show may name. The **song** is the third
thing: the shape of the music itself — where the chorus is, how long it runs,
where the band hits — and it belongs to neither of the other two. A chorus is
eight bars at Norco and eight bars in Vegas.

The one rule everything below serves is that the song is measured in **bars**,
not seconds. A cue at 61.196 s breaks when the tempo changes, when a section is
cut, or when the band takes the last chorus twice. A cue four bars into `CH 1`
does not. The design this formalises is
[musical-time-cues.md](../domain/musical-time-cues.md).

## Musical position

r[song.position]
A position in a song MUST be a `Bars { bar, beat }`, both counted from 1 the way
musicians count: `{ bar: 22, beat: 1 }` is the downbeat of bar 22. `beat` MAY be
fractional, so the *and* of 3 is `beat: 3.5` rather than a tick unit nobody says
out loud. A beat of 0, or a beat past the bar's signature, is not a position.

r[song.position.never-seconds]
Everything at song level MUST be addressed in bars and beats, never in seconds.
Seconds are a property of one recording at one tempo; bars are a property of the
music. Where a source only knows seconds — an audio file, an `.lrc`, a DAW
playhead — the conversion MUST happen at import or at the moment of use, and the
stored form MUST be the musical one. Seconds MUST NOT be cached in the show
either — a cached second is a second copy of the tempo, and it is the copy
that will be wrong after a tempo edit.

r[song.tempo-map]
A song's tempo MUST be a **map** — an ordered list of `(position, bpm, time
signature)` points — never a single number, even though most songs have one
entry. A song that ritards is then a data problem, not a redesign. Tempo MUST be
fractional: a project at 86.28 BPM read as 86 is two thirds of a second out by
the last chorus, which was a real bug. Conversion across several points MUST
accumulate segment by segment and MUST be invertible to within a nanobeat.

## The song map

r[song.map]
A song map MUST hold the song's name, its tempo map, and its **sections** in
order, each with a name, a start position and a length in bars. That is the
whole shape of a song as lighting cares about it; audio, tracks and mix are the
DAW's business.

r[song.map.imported]
The song map MUST be imported from the DAW project — today an RPP, in general
the `.daw` format read through the `daw` crate — and MUST NOT be authored a
second time by hand. Moving a section in the DAW moves the lighting with it
precisely because the lighting never had its own copy of where the section was.

r[song.map.sections-from-regions]
Sections MUST be read from the project's **regions**, not its markers. A region
has an end, and a section's *length* is the thing lighting cares about — "the
chorus is eight bars" is a statement about a region. A region with no end, or
with zero length, is not a section.

r[song.map.bar-boundaries]
A section SHOULD start on a bar boundary and run a whole number of bars. Every
section of the song this was built against does, which is the evidence that bars
are how the material is shaped. A section that does not MUST still load — the
importer reports it rather than rounding it silently, since a section that is
really 7.5 bars is a fact about the arrangement someone should know.

r[song.map.kind]
A section's **kind** MUST be inferred from its name, from the set `Count-in`,
`Intro`, `Verse`, `Pre-chorus`, `Chorus`, `Break`, `Bridge`, `Breakdown`,
`Outro`, `Other` — reading the first run of letters, case-insensitively, so
`VS 1`, `Verse 2` and `V3` are all verses and `IN A` is an intro. An arranger
already wrote the name; nobody should tag a section twice for the lights to
know what it is. Collisions (`Breakdown` before `Break`, `BR` as bridge) MUST
resolve the way `kind_of` does, and a name nothing matches is `Other`, never an
error.

## Positions are relative to sections

r[song.relative-position]
A show SHOULD express every position relative to a section — "4 bars into
`CH 1`", "the last bar of `PRE`" — and MUST NOT need to name an absolute bar
number to place a cue. Re-arranging the song then moves the lighting with it:
cut two bars from the verse and the chorus cue still lands on the chorus. The
resolved `Bars` is what the engine runs on; the relative form is what the author
wrote and what survives an edit.

r[song.relative-position.resolved-on-load]
Relative positions MUST resolve against the song map on load, not at authoring
time, so a show file written against last week's arrangement resolves against
this week's. A show that stores only the resolved bar has quietly become a show
for one arrangement. (Today the relative form lives in a `<song>.positions.json`
sidecar beside the show, applied by `ignition_song::reposition`; the cue file
still carries the resolved bar so an older player keeps working.)

r[song.relative-position.duplicate-names]
A section name that occurs more than once — `PRE` and `PRE` again before `CH 2`
— MUST be addressable by ordinal: the second `PRE`, not only the first. Lookup by
name alone finds the first occurrence and leaves the second unreachable, which
is why the show once placed the second pre-chorus as "8 bars into `VS 2`" — a
workaround that broke the moment the verse changed length.

## The hit chart

Two sources describe where the band hits. One is heard; one is decided. They are
not equal.

r[song.hits.detected]
Detection MAY produce hits from the audio: each with a snapped position, its
original seconds, a strength 0..1 against the strongest hit in the song, a band
(`Low`, `Mid`, `High`) and the dynamic level at that moment. A per-bar dynamics
curve MAY accompany them, sampled at each bar's midpoint, because a *cue* is
written at bar resolution and a show does not need seventeen thousand numbers to
know the second chorus is bigger than the first.

r[song.hits.grid-snapped]
Detected hits MUST be snapped to a musical grid, eighths by default. A detected
onset carries about 20 ms of window latency and a few more of human timing; left
raw, a chase built from it drifts either side of the beat all song, which reads
worse than being consistently wrong. Snapping MUST be done in beats from the
song start, so a hit just before a barline snaps forward onto the next downbeat
rather than onto a beat 5 that does not exist. Sixteenths find hi-hat noise;
quarters lose the off-beat stabs pop choruses are built on.

r[song.hits.detection-is-a-draft]
Detected hits are a **draft**. Where a chart exists, the chart MUST be the
authority and detection MUST NOT be consulted for anything the chart covers. The
detector finds a thousand onsets in a three-minute song and is right about
nearly all of them, which is exactly the problem: a hi-hat is a real onset and
wants no cue. Deciding which handful of hits carry the song is a musical
judgement, made in a MIDI editor with the track playing, not with a threshold.

r[song.chart]
The chart MUST live in the DAW project on a MIDI track named `HITS` (matched
case-insensitively), so it is edited where the music is edited and re-read on
every import — nothing to pass, nothing to keep in sync. An absent track is not
an error: most projects have no chart, and a show without one is still a show.

r[song.chart.class]
A note's pitch MUST be its **class**: `48 Kick`, `50 Snare`, `60 Low`,
`72 Medium`, `84 High`, `96 Connected`. Pitches outside the schema MUST be
ignored rather than lit — the track is a place a person works, and a stray note
must not flash the room. Note positions MUST be converted through the tempo map
from quarter-notes, so a tempo change carries the chart with the music.

r[song.chart.class-is-intensity]
Classes are **intensity tiers, not instruments**. `Kick` does not mean "every
kick drum" and never did; it is the softest thing in the vocabulary, placed
where a light touch is wanted. Reading the chart as a drum transcription leads
to lighting a `Kick` low and heavy because that is where a bass drum sits, when
what was asked for was the gentlest accent. Each class MUST carry a fixed weight,
and the two soft tiers MUST stay well under the band hits — a snare on every
backbeat at hit weight is a rig that never stops flashing. Velocity MUST be
carried but MUST NOT scale intensity: hand-entered values drift for reasons that
are not musical, and a hit that needs to be bigger is a bigger class.

r[song.chart.pulse]
`Kick` and `Snare` hits MUST become **pulses**: the one-bar pattern each class
plays inside a section, folded across the section's bars and kept where it lands
in most of them, rendered as one looping effect locked to the song's tempo and
living *in* the section's look. A snare two to the bar for fifty-six bars is
three hundred cues saying "flash on two and four"; on a console that is one
running effect. A section whose chart carries none gets no pulse, and the fold
threshold is what keeps a turnaround fill from repeating through every bar.

r[song.chart.hit]
`Low`, `Medium` and `High` hits MUST become **hits**: one-shot events, one per
charted note, each carrying a shape that puts itself out. They happen a few
dozen times, each one means something, and each earns a cue. A hit MUST NOT also
receive a pulse, or one note flashes twice.

r[song.chart.figure]
A `Connected` note spanning several hits MUST make a **figure**: one musical idea
whose members are the hits it overlaps. Membership MUST be by overlap, not exact
position, because a hand-drawn note starts a little before the hit it opens on
and an exact test finds nothing. Members on the same grid position MUST
collapse into one **moment** at the strongest of their intensities — a snare and
a crash on one eighth are one event. A list of hits can only ever flash on hits;
knowing three hits are one phrase is what lets them travel across the stage, and
that information is in what the band meant, not in the waveform.

r[song.chart.figure.zones]
A figure's moments MUST be addressed across **zones** of the stage — the *n*th
of *count* equal slices of the rig's width, left to right — selected by where the
light *lands* at face height, not where the fixture hangs. Front washes aim at
the centre from wherever they are hung, so a zone cut by fixture position lit
the wrong third of the stage. Zones are cut from the room's width rather than
from whichever fixtures matched, so "stage left" means the same place for a
two-moment figure and a six-moment one.

r[song.chart.figure.cutout-or-bump]
A figure of two or three moments MUST be a **cutout** — kill the rig and reveal
one zone per moment — and a longer figure MUST be a **bump run**, adding to the
running look one zone at a time. A stage carved into halves or thirds, arriving
one piece at a time, reads far harder than the same pieces added; six cuts in
six eighths is a strobe with extra steps, and the shape stops being legible once
there are more pieces than the eye can hold.

r[song.chart.accents-are-additive]
Hit and figure cues MUST be non-blocking and MUST sort after a blocking cue at
the same position. A section usually starts on a crash, so the section cue and
that crash land on one downbeat, and a blocking cue placed after the accent
would wipe it. Whether the crash reads on top of the new look or vanishes into it
must not come down to whether a sort happened to be stable.

## What the song does not own

r[song.no-room]
Song data MUST NOT name a fixture, a channel, a patch address or a coordinate in
metres. A zone is "the left third of the stage", resolved against whichever room
the show plays in; the moment it became "channels 3–7" the chart would be a
Norco chart. See `r[files.show]` and `r[files.no-fixture-identity]`.

r[song.no-role-binding]
Song data MUST NOT bind a role. The chart says a hit happens and how big it is;
which role plays it — the wash, the bars, the movers — is the show's decision,
written in the profile's vocabulary and answered by the venue. A song that knew
its hits went on "Back Wall Pars" would be a song for one profile. Nor MAY it carry a colour value: a show refers
to a profile's colour *roles*, and the venue answers them per
`r[profile.venue-binds]`.

## Lyrics

r[song.lyrics]
Lyrics MAY be attached to a song from an `.lrc`, and every line (and word, where
the file is enhanced) MUST be placed at a `Bars` position through the tempo map
on import, so the lyric screens are driven by the same clock as the lights and
seek with them. The original seconds MAY be kept alongside, because the file said
so and round-tripping through bars only adds error. The file's own `[offset:]`
MUST be applied exactly once. A line MUST hold until the next one, and a timed
blank MUST be kept as a clear rather than dropped, or the last line of a verse
hangs on the TVs through the instrumental.

## Generation

r[song.generate]
A first-draft cue list MUST be derivable from a song map and a profile alone:
one cue per section, at the section's start, blocking, with recipes chosen by
the section's kind and written against the profile's roles. A section named
`CH 1` that runs eight bars is already most of a cue; this makes it one.

r[song.generate.fades-in-beats]
Generated fade times MUST be authored in **beats** by section kind — a chorus
lands in a quarter beat, a verse arrives over four — and converted through the
tempo map at generate time. A chorus that snaps in one beat snaps at 200 BPM
too; a chorus that fades in has already missed.

r[song.generate.recipes-not-channels]
The generator MUST emit recipes against roles and selections, never direct
channel values and never a `Chans` target. This is what makes the draft survive
a rig change: the chorus targets "the wash, left to right", and adding a fixture
to the truss puts it in the chorus. The test `the_draft_is_recipes_not_channels`
holds this.

r[song.generate.is-a-draft]
The generated list is a **starting point** a person or an agent edits, not the
product. What the generator cannot do is know the song: the blackout before the
last chorus, the build in the pre, the lift four bars into the second verse are
the parts a human adds, and a tool that presented its draft as finished would
teach people to distrust the desk. The draft MUST be re-derivable at any time
without destroying those edits. (`authorshow --merge`, with the cues to keep
named in `<song>.edits.json`.)

r[song.camera-cuts]
A show MAY carry its **camera cut** in the cues' `commands`
(`r[viz.camera-cuts]`), and `authorshow --cameras <setup>` writes one: verses
on the singer, pre-choruses across the side stage and the guitar, choruses
wide and then super wide, the break on the drums, the breakdown on keys and
bass, the outro flat at the lip and the last cue from the bird's eye, with
figures and high hits as one-to-two-beat punch-ins to the drum cam. The cuts
name the standard presets, never a venue's own — the venue's `cameras.json`
says where "Drums" is (`r[song.no-room]`). A cue that exists only to carry a
cut is a `·` accent cue with no recipes, so it never blocks and the lighting
reads exactly as it did without it.

## Transport

r[song.transport.position-per-frame]
The engine MUST be given a musical position per frame and MUST NOT care who
computed it. Ignition may own a transport, take one from a DAW over the network,
or follow MIDI clock; timecode is one more way to derive a `Bars`, not a
different model. Deciding the source before the musical model existed would
have been deciding it blind. Where Ignition owns the transport, the tempo map
MUST come from the same project in the same pass as the audio, so the tempo the
cues resolve at is by construction the tempo the audio runs at.

r[song.transport.stopped-fires-nothing]
A stopped transport MUST fire nothing. Position is only meaningful while the
music moves; a stopped playhead sitting on a hit is not a hit happening, and a
rig that flashes every frame it sits on bar 23 has confused *where* with *when*.
See `r[triggers.crossing-fires]`.

r[song.transport.seek-is-a-locate]
A seek MUST be a **locate**, not a performance of the span crossed. Jumping from
bar 10 to bar 43 asks "what is the state at bar 43?" and resolves it from the
top of the list; it does not fire the thirty cues in between. This is the whole
reason loops, restarts and "take it from the last chorus" work. See
`r[cues.position]` and `r[cues.seek]`.

## Two ways to run one list

r[song.two-ways]
The same cue list MUST be runnable clocked — driven by a musical position — and
by hand — driven by GO — and the two MUST be the same list, not an automated
version and a manual backup that drift apart. Losing backing tracks must not
mean losing lighting. A cue carries an optional `at` for the clock and a list
position for the person, and lands in the same state either way; the mechanics
of `Cue.at` are specified in the cues spec, not here. See
[musical-time-cues.md](../domain/musical-time-cues.md).

## Other transports

r[song.transport.sources]
The musical position MAY come from a **timecode** source — LTC on an audio
input, MIDI timecode, Art-Net timecode — mapped through the tempo map to bars,
so a band running playback from a laptop with LTC runs the same show as one in
the DAW. A source MUST report when it is lost, and the player MUST hold rather
than run free.

r[song.transport.follow]
With no transport at all, a list MUST still run on time through cue follows
(`r[cues.trig]`) against the `Tap` master.
