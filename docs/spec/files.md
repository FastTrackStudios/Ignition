# File formats

Two files, and the line between them is the whole design.

```
Rockstars Norco.igv        the room:  what fixtures exist, where they are,
                                      what they are patched to, what the
                                      room looks like, where the screens are
Bye Bye Bye.ig             the show:  cues, recipes, triggers — and not one
                                      DMX address anywhere in it
```

A show is written once and plays at Norco, Riverside and Vegas. That is the
requirement everything below serves, and it is not achieved by a clever file
layout — it is achieved by the show being *unable* to name anything
venue-specific. If a show could say "channel 12", it would be a Norco show
forever, and no amount of format design would rescue it.

Which means the interesting question is not "what goes in which file". It is
**what vocabulary the two agree on**, and what happens when a venue does not
speak all of it.

## The two files

r[files.venue]
An **`.igv`** MUST describe one room and nothing about any performance: its
fixtures and their types, each fixture's position and orientation, its DMX
patch, the room's geometry, its screens and their canvases, and its props.
Today this is the seven JSON files under `data/venues/<name>/`; the format is
their consolidation, not a new model.

r[files.show]
An **`.ig`** MUST describe one performance and nothing about any room: its cue
list, its recipes, its triggers, and the song timing they are written against.
An `.ig` MUST NOT contain a DMX address, a universe, a fixture id, or a
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
A set of **required roles** MUST be defined that every venue is expected to
supply, so a show can rely on them existing. A venue missing a required role
MUST still load — a rig is often half-patched — but MUST report the gap.

Without a required set, "portable" means only "did not crash": a show naming
`Back Wash` at a venue that calls the same bar `Rear Wash` would play with that
bar dark and nothing said. The vocabulary is the interface, and an interface
nobody declared is one nobody can implement.

r[files.compatibility-check]
It MUST be possible to check a show against a venue **without running it**, and
the check MUST report every name the show uses that the venue does not provide.
This is `unresolved()` generalised from cues to the whole show, and it is what
makes "will this work in Vegas" answerable on the plane rather than at
soundcheck.

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

r[files.show.many-per-song]
Several shows MUST be able to reference one song, so a different room or a
different look on a different night is a second `.ig` rather than a fork of the
first. Nothing may assume a one-to-one binding.

## Format mechanics

r[files.text-and-diffable]
Both formats MUST be text, and MUST be readable and diffable. A show is edited
by a person and by a generator, sometimes on the same day, and a binary format
makes "what did the generator change" unanswerable. JSON today; the requirement
is the property, not the syntax.

r[files.versioned]
Both formats MUST carry a version. A file from an older version MUST load or be
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
