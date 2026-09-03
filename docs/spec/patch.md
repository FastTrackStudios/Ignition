# Patch and fixture types

Bringing a room up: describing a fixture *type* once, and telling the
venue which of them hang where and on what addresses.

Everything above this spec reaches the rig through roles
(`r[files.no-fixture-identity]`), which is what makes a show portable.
This is the layer where that stops being true and a fixture is a real
object at a real address, and it is deliberately the *only* such layer.

`r[profile.setup-cost-is-the-metric]` is the requirement this whole
surface answers: the measure of a profile is how long it takes a new
room to implement it, and a console template that advertises patch,
groups and presets in about thirty minutes is the bar. A patch that can
only be edited in a text editor does not clear it.

## Fixture types

r[patch.type-is-data]
A fixture type MUST be **data on disk**, not code — one document per
model under `data/fixtures/`, readable and writable by the studio. A
type that requires a recompile to add cannot be added by the person
standing in the room, and a model with no entry has no channel map and
never lights: this project has had twenty-four of a forty-fixture rig
dark for exactly that reason.

r[patch.type-identity]
A type's identity is its **console name** — the string a venue's
`fixtures.json` writes in `model`, and the `FixtureType` `Name` of any
generated GDTF profile for it (`r[viz.gdtf-generated]`). It MUST be
unique across the library. Renaming it unpatches every fixture that used
it, so a rename MUST be offered as a rename *with its venues updated*,
never as a silent edit.

r[patch.type-aliases]
A type MUST be able to declare other model strings that resolve to it.
One OEM head is sold under four brand names, and a room that bought it
as a Lixada must not need a second document to patch it.

r[patch.type-resolution]
A venue's `(manufacturer, model)` MUST resolve to a type by exactly one
rule, shared with the visualizer's geometry matching
(`r[viz.gdtf-aliases]`): exact console name, then declared alias, then
the shared alias table, then a normalised substring in either direction
as a last resort. Comparison ignores case and punctuation. Two different
rules for one question is how a fixture is drawn as one model and
addressed as another.

r[patch.type-modes]
A type MUST carry its **modes** — named channel charts, in the manual's
own order — because one fixture is several fixtures depending on which
mode its DIP switches select, and the footprint is what decides whether
the next fixture's address is free.

r[patch.type-vocabulary]
A channel MUST be nameable in the manual's own words (`Warm White 3`,
`Dimmer (per LED section)`, `Colour Selection`) and resolved to an
engine attribute on the way in, never at authoring time. A person
copying a chart out of a PDF should be transcribing, not translating; a
translation step at the keyboard is where a channel map goes wrong.

r[patch.type-unknown-channel]
A channel whose name resolves to nothing MUST still occupy its byte and
MUST be carried through under its own name. Dropping it shortens the
footprint, and a fixture short by one byte bleeds into whatever is
patched after it.

r[patch.type-confidence]
A type MUST record how much of it is known — chart from a manual, from a
listing, or inferred — per document and, where they differ, per mode.
"The channel order on this fixture is a guess" is something an operator
needs *before* the fixture does something unexpected. The patch sheet
MUST show it.

r[patch.type-sources]
A type MUST carry the sources its facts came from. The next person to
doubt a channel order needs the manual, not a commit message.

r[patch.type-interchange]
The library MUST import from **GDTF**, **QLC+ `.qxf`** and **Open
Fixture Library** JSON, and MUST export GDTF and `.qxf`. Nobody should
retype a chart that already exists, and a type authored here should be
usable on the house desk.

r[patch.type-not-geometry]
A fixture type does NOT carry what the fixture looks like. Meshes,
yokes and beam nodes stay with GDTF (`r[viz.gdtf-meshes]`), matched on
the same console name. A type with no GDTF profile MUST patch, address
and output normally; it draws as a box.

## The patch

r[patch.sheet]
The studio MUST present the patch as a **channel-ordered sheet**, one
row per fixture, with at least: channel, name, type, mode, universe and
address, label and gel. Label and gel are the operator's own words for a
fixture and MUST be editable there — they are already in the venue file
and have never been visible.

r[patch.sheet.columns]
The sheet MUST offer a **condensed** and a **full** column set. Patching
forty fixtures needs six columns; auditing one needs twenty, and a sheet
that always shows twenty cannot be read at a rig.

r[patch.sheet.filter]
The sheet MUST be filterable by fixture type, universe, tag,
**unpatched** and **conflicting**. The last two are not filters over a
property, they are the two questions asked while patching, and they MUST
be reachable in one action.

r[patch.address]
Addressing MUST accept the console idiom: a count, a target, and a
universe — `10@20`, `10@2.20` — and MUST offer **next free address** as
the default target, computed from the live occupancy.

r[patch.conflict]
Two fixtures whose channel spans overlap within one universe MUST be
reported as a **conflict**, on both rows and on the universe's occupancy
view, and MUST NOT be silently accepted. A conflict is not an error that
blocks the edit — a patch is often briefly wrong on the way to being
right — but it MUST be visible until it is resolved.

r[patch.occupancy]
A universe MUST be viewable as its 512 channels, showing what occupies
each and what is free. "Where does this fit" is a spatial question and a
list of addresses answers it badly.

r[patch.unpatched]
A fixture MAY be **unpatched**: present in the room, positioned, part of
groups and selections, with no address. A prop, a spare, a fixture whose
node has not arrived. Unpatching MUST NOT delete the fixture.

r[patch.multipatch]
A fixture MUST be able to occupy several addresses — see
`r[files.venue.multipatch]`. Four house pars on one address are one
fixture in the show and four on the wire, and a venue that cannot say so
must lie about its fixture count.

r[patch.curves]
A patched fixture's type MAY carry a per-attribute output curve
(`r[files.venue.dmx-curves]`), and the studio MUST be able to author
one. A dimmer that is not linear is corrected here, so HTP between two
different types compares like with like.

r[patch.pick]
Clicking a fixture in the visualizer MUST select its row, and selecting
a row MUST identify the fixture in the visualizer. The room and the
sheet are two views of one thing, and matching them up by counting is
the slowest part of patching a rig nobody has patched before.

## Editing a venue

r[patch.writes-the-venue]
Patch edits MUST be written back to the venue's own files, in the same
text form they were read in (`r[files.text-and-diffable]`), preserving
every key the build did not understand (`r[files.additive-evolution]`).
A venue edited by the studio and a venue edited by hand MUST be the same
kind of artifact.

r[patch.explicit-save]
A patch edit MUST NOT reach disk until it is saved, and unsaved state
MUST be visible. Patching is exploratory — an address is tried and
abandoned — and a file that changes under every keystroke cannot be
diffed or reverted.

r[patch.orientation-is-whole]
A fixture's hang is one fact with two spellings in the file — Euler
angles and a quaternion — and writing one without the other MUST be
impossible. The loader reads the quaternion, so an angle-only edit is
silently ignored, which is a bug this project has already shipped once
(`crates/ignition-viz/src/bin/aimwash.rs`).

r[patch.venue-layer]
A venue MAY carry a **venue-local layer**: a separate file in the venue
directory holding per-fixture overrides — a repatched address, a swapped
model, a fixture out of service — that the base venue does not know
about. It exists for the same reason `r[profile.venue-layer]` does: the
night-of change is real whether or not there is a place to put it, and
without a sanctioned place it is committed into the room's own
description and stays there.

This is a *different* artifact from the show's `.ig-local`
(`r[profile.venue-layer]`), which overrides cues. One overrides the
room, the other overrides the song, and folding them together would mean
a repatch had to name a song.

r[patch.venue-layer.optional]
The base venue MUST be complete and playable with no layer. A layer
adjusts a room that already works; it is never where part of the room
lives.

r[patch.venue-layer.visible]
The studio MUST show which fixtures are being overridden by a layer, and
the compatibility check MUST report that a venue carries one. A room
that behaves differently from its own file is a fact somebody needs
before the night.

## Bringing a room up

r[patch.insert]
Adding fixtures MUST be one action taking a type, a **quantity**, a
start channel, a start address and an offset per fixture — defaulting to
the type's footprint — because rigs come in bars of eight and patching
them one at a time is the thirty minutes.

r[patch.new-venue]
Creating a venue from nothing MUST be possible in the studio: a
directory, a manifest (`r[files.directory-or-archive]`), a room, and
then patch. A product whose first step is "hand-write six JSON files"
has no first step.

r[patch.derived-groups]
Groups and their ordering MUST stay derived from real fixture positions
(`r[profile.spatial-grid-is-derived]`), so patching a fixture into the
room is what puts it in the right place in every selection — never a
second structure to maintain that can disagree with the patch.
