# File formats

Four files, and the lines between them are the whole design.

```
Rockstars.ig-profile       the interface a rig must satisfy
Rockstars Norco.ig-venue   one room's implementation of it
Bye Bye Bye.ignition       one song's light show, written against the profile
Sunday 14th.ig-show        the night: a profile, a venue, and the songs
```

The four concepts and the relationships between them are specified in
[profile.md](profile.md); this file covers what each format holds and how the
files behave. Extensions are written out rather than abbreviated: these are
files a person picks from a folder at a desk, in a hurry, and `.igv` against
`.igs` is a distinction nobody makes correctly under pressure.

A show is written once and plays at Norco, Riverside and Vegas. That is the
requirement everything below serves, and it is not achieved by a clever file
layout — it is achieved by the show being *unable* to name anything
venue-specific. If a show could say "channel 12", it would be a Norco show
forever, and no amount of format design would rescue it.

Which means the interesting question is not "what goes in which file". It is
**what vocabulary they agree on** — which is what a profile declares — and what
happens when a venue does not speak all of it.

## The files

r[files.venue]
An **`.ig-venue`** MUST describe one room and nothing about any performance: its
fixtures and their types, each fixture's position and orientation, its DMX
patch, the room's geometry, its screens and their canvases, and its props.
Today this is the seven JSON files under `data/venues/<name>/`; the format is
their consolidation, not a new model.

r[files.show]
An **`.ignition`** MUST describe one performance and nothing about any room: its cue
list, its recipes, its triggers, and the song timing they are written against.
An `.ignition` MUST NOT contain a DMX address, a universe, a fixture id, or a
coordinate in metres. Those are properties of a room, and a show that carries
them has silently become a show for one room.

r[files.no-fixture-identity]
A show MUST NOT reference a fixture by identity — not by channel, not by patch
position, not by index into the venue's fixture list. Every reference to
"which lights" MUST go through a selection expression. This is the single rule
that makes portability possible; the rest of this file is its consequences.

## The contract between them

r[files.vocabulary]
A venue MUST publish a **vocabulary**: the named groups, colours and focus
points it provides. A show MUST consume that vocabulary by name. The venue says
what "Back Wash" *is* at this address; the show says what happens to it.

r[files.required-roles]
The required vocabulary MUST be declared by a **profile**, not fixed in the
software and not left to convention — see [profile.md](profile.md). A venue
missing a required role MUST still load, since a rig is often half-patched, but
MUST report the gap.

Without a declared set, "portable" means only "did not crash": a show naming
`Back Wash` at a venue that calls the same bar `Rear Wash` would play with that
bar dark and nothing said. An interface nobody declared is one nobody can
implement.

r[files.compatibility-check]
It MUST be possible to check compatibility **without running anything**, and the
check MUST report every gap by name. With a profile in the middle this is two
independent checks — show against profile, venue against profile — rather than
one per show-and-venue pair, which is what makes "will tonight's set work in
Vegas" answerable on the plane rather than at soundcheck. See
`r[profile.check-is-static]`.

r[files.graceful-degradation]
A show naming something a venue lacks MUST play everything else. A missing
group means that recipe covers nothing; a missing colour falls back per
`r[color.scope.fallback-order]`; a missing focus point leaves those fixtures at
their prior aim. A show that refuses to run because one venue has no follow
spot is worse than a show with no follow spot.

r[files.capability-over-name]
Where a show can express its intent as a **capability** rather than a name, it
SHOULD. "Every wash above 2 m" is portable to a rig nobody has surveyed for
named groups; "Center Washers" is not. Named roles are for what a
capability cannot express — which is mostly *intent*, like which bar is the key
light.

## Venue contents

r[files.venue.fixtures]
A venue MUST carry, per fixture: its type, its position and orientation in the
room, its DMX patch, and whatever tags classify it. Position MUST be in metres
in the venue's own stage space — see `r[focus.stage-space]`.

r[files.venue.room]
A venue MUST carry the room's geometry — enough to render it and, more
importantly, enough for spatial selection and focus resolution to mean anything.
A `Where::Within` predicate is only portable because both venues agree on what
their coordinates mean.

r[files.venue.screens]
A venue MUST carry its screens: each panel's position, size and orientation, and
which **canvas** it belongs to. Slices are derived from real panel geometry
rather than stored, because a stored slice would be wrong the moment a panel
moved — see `crates/ignition-viz/src/canvas.rs`.

r[files.venue.canvases]
Canvas *names* are part of the venue's vocabulary, exactly as group names are. A
show says "play this on `main`"; the venue decides that `main` is three panels
of unequal width with gaps between them. A show MUST NOT know how many panels a
canvas has.

r[files.venue.assets]
A venue MAY reference external assets — room models, fixture geometry. The
reference MUST resolve relative to the venue, so a venue is movable and
copyable as a unit.

## Show contents

r[files.show.cues]
A show MUST carry its cue list with each cue's musical position, fade, recipes,
and blocking flag — the existing `CueList`, which already contains no
venue-specific data.

r[files.show.triggers]
A show MUST carry its triggers separately from its cues. They are different
kinds of thing: a cue is a look the operator takes, a trigger is an event the
song fires. Folding triggers into cues is what produced a cue list half of whose
entries were machine-generated hits nobody would ever press GO on.

r[files.show.song-binding]
A show MUST record which song it is written against — enough to find the
project and to know its tempo map — without embedding the project. The audio and
the arrangement belong to the DAW session; duplicating them here creates two
sources of truth for where bar 33 is.
Today this is a header on the cue-list document — `version`, `profile`, and
`song: { project, name }` — sitting beside `cues` and `triggers` in the same
JSON (`ignition_core::show_file::ShowDocument`). Every header key is optional,
so a file written before the header existed still loads, and a reader that only
knows `CueList` still reads a file written with it.

r[files.show.many-per-song]
Several shows MUST be able to reference one song, so a different room or a
different look on a different night is a second `.ignition` rather than a fork of the
first. Nothing may assume a one-to-one binding.

## Format mechanics

r[files.text-and-diffable]
Every format MUST be text, and MUST be readable and diffable. A show is edited
by a person and by a generator, sometimes on the same day, and a binary format
makes "what did the generator change" unanswerable. JSON today; the requirement
is the property, not the syntax.

r[files.versioned]
Every format MUST carry a version. A file from an older version MUST load or be
rejected with a message naming the version — never load half-understood, which
is how a show plays with its effects silently missing.

r[files.additive-evolution]
Unknown fields MUST be preserved on load and written back on save. A venue
edited by an older build MUST NOT lose the screens that build knew nothing
about. This is what makes it safe to open a file on a console that has not been
updated yet.

r[files.directory-or-archive]
A venue MAY be a directory with a manifest rather than a single file, since it
references assets. An archive form for transport is a packaging concern, not a
format one, and the directory MUST remain the editable form — a venue that can
only be edited by unpacking and repacking will drift from what is in git.
The manifest is `venue.ig-venue` in the directory: `{ "version", "name",
"profile", "files": { optional overrides of the JSON file names }, "assets" }`.
A directory with no manifest is the same venue with every default.
