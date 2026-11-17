# What's (and isn't) extractable from the raw `.esf3d`

**Date**: 2026-08-24. Attempted to get the full channel/universe/DMX-address
list directly from `Popstars_Playground_VERIFIED_20260823.esf3d` rather than
ask for it, since the file is already on disk. Partial win, one real wall —
recorded here so nobody re-attempts the same dead end.

## What worked: `showdat.dat` is not opaque — it has readable UTF-16LE text

eos-toolkit's `file-format.md` calls `.esf3d`/`.esf2` a closed binary format
and says not to build on it, in the specific context of the `working.a3d`
Augment3d scene (where that's well-demonstrated — see
`docs/domain/norco-venue-reference.md`'s "file route is a dead end" note).
`showdat.dat` — the actual show data (patch, cues, groups) — turns out to be
*partially* readable: it's mostly a packed binary structure, but every label,
fixture-manufacturer name, and group name in the show is stored as a plain
UTF-16LE string inside it. A straightforward scan (two-byte-aligned printable
runs, high byte `0x00`) pulls them straight out — no format documentation
needed, since it's just text extraction, not structural parsing.

Two useful things came out of that:

1. **The fixture library catalogue** — every manufacturer/model referenced by
   the show, each keyed by a GUID: `Uking`/`Par`, `Rockville`/
   `Rockstrip_252_3ch` and `_7ch`, `Riukoe`/`Mini_Gobo_Moving_Head_Light_11ch`,
   `Lixada`/`Mini_Gobo_Moving_Head_11ch`, `Betopper` (with `Color_Wheel`/
   `Gobo_Wheel` sub-entries), `Chauvet`/`Hurricane_Haze_1DX`, plus a `Generic`
   `Dimmer` and some in-progress `Custom` profiles. This matches
   `norco-rig-facts.md`'s documented manufacturer list exactly — confirms
   nothing was missed, but doesn't add new information over what was already
   written down.

2. **Every group name defined on the console** — 114 of them, not just the
   ~10 documented in `norco-rig-facts.md`. Saved verbatim in
   `data/venues/norco/group-names.txt`. This is the genuinely useful find:
   it includes exactly the kind of spatial subdivisions the 2026-08-24 field
   measurements describe — `OH Outer`/`OH Inner`, `BM Outer`/`BM Inner`,
   `Pars Odd`/`Even`/`3rd`/`4th`/`5th`/`6th`/`8th`, `Pars Split A`/`B`,
   `Pars Qtr 1`–`4` (and `1+3`/`2+4`), `Front Left`/`Centre`/`Right` (and
   `L+C`/`C+R`), the same pattern for `Mid`/`Wide`/`Back`, `Left Deep`/
   `Right Deep`/`Centre Deep`, `Outer Ring`/`Inner Core`. Whichever of these
   the operator actually used to build "the center 12 pars," "the back row,"
   etc. is very likely named in this list.

## What didn't work: fixture_uid doesn't join against the library GUIDs

Tried the obvious shortcut: `fixtures.json`'s `fixture_uid` per channel
(pulled from `Scene/Patch.json` during the original extraction) against the
GUID-keyed library above, hoping to get manufacturer/model per channel for
free. **All 69 lookups missed.** These are two different UID namespaces —
Augment3d's `fixture_uid` identifies a 3D *model* in the visualizer's own
catalogue; the GUIDs in the fixture-library strings identify an Eos DMX
*personality*. Same physical fixture, two unrelated ID systems, no shared
key. Not pursued further — per-channel manufacturer is already documented in
`norco-rig-facts.md`'s address table for the interesting channels, and
extending it to all 71 channels doesn't need this join.

## The real wall: DMX universe + address per channel is genuine binary, not text

This is what was actually asked for, and it isn't in the string table at
all — patch addresses are small integers stored in `showdat.dat`'s packed
binary records, not as text. Getting them out means reverse-engineering an
undocumented, versioned, proprietary record format with no spec and no
existing parser (eos-toolkit's own `docs/file-format.md` says exactly this
about the format generally). That's a different order of task than string
extraction, it's exactly the kind of guess eos-toolkit's whole design
philosophy ("after every write, read the state back," never trust
unverified data) argues against attempting blind, and getting it wrong would
produce a confidently-wrong address table that's worse than no table.

**The tool that actually solves this already exists and is the right one**:
eos-toolkit's `eosdump.py` reads the full patch (channel, universe, address,
fixture type) over OSC from the *running* console/Nomad, and writes
`show.json` (structured) + `show.md` (readable). That's a live read against
the real console, not file archaeology — reliable by construction, since
it's the same protocol the console itself uses to answer.
