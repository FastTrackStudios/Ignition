# Norco full patch + group export — 2026-08-24

**Source**: live OSC read of the Eos Nomad running on `voyager`
(`tailscale`/`ssh voyager`), show `Popstars_Playground_VERIFIED_20260823.esf3d`
open at the time of the pull. This is the direct answer to "get the full
channel universe addressing list" — read straight from the console rather
than reverse-engineered from the show file (see
`docs/domain/norco-showfile-extraction-notes.md` for why that path was a
dead end for address data specifically).

**How it was pulled**: eos-toolkit's own `eosdump.py`, read-only, exactly as
designed — connects over OSC TCP 3032 to Nomad on `127.0.0.1` (i.e. from
`voyager` itself), queries counts and then every record. Ran a **patched
copy** (not the checked-in one) to work around a real bug: `eosdump.py`'s
cue-list enumeration (`eosdump.py:339`) builds `lists` from every OSC reply
path starting `/eos/out/get/cuelist/`, including the *count* reply itself
(`/eos/out/get/cuelist/count`) — `b.split("/")[5]` on that path yields the
literal string `"count"`, which then gets queried as if it were a cue-list
number and fails after 3 retries, aborting the whole dump before it writes
anything. One-line fix: filter to `.isdigit()` paths only. Not applied to
the original file on `voyager` — worth a genuine fix upstream in eos-toolkit
when convenient, flagging here so it's not lost.

Nothing was written to the console. `eosdump.py` is read-only by
construction (no `--allow-control`/`--allow-destructive` flags exist for it,
unlike `eos_mcp.py`).

## Full patch (71 channels, 69 patched)

Global address is the flattened DMX address Eos itself reports (universe 2
starts at 513); `Univ`/`Addr` is that same address split into
universe/local-address, cross-checked against every value
`norco-rig-facts.md` had already documented by hand (90–101, 85–88) — all
matched exactly. Channels 19 and 98 are the documented unpatched phantoms
(`address: 0`) — no universe/address, included for completeness.

| Chan | Manufacturer | Model | Patched | Univ | Addr | Global | Label |
|---|---|---|---|---|---|---|---|
| 1 | Uking | Par | yes | 1 | 1 | 1 |  |
| 2 | Uking | Par | yes | 2 | 1 | 513 |  |
| 3 | Uking | Par | yes | 1 | 8 | 8 |  |
| 4 | Uking | Par | yes | 1 | 15 | 15 |  |
| 5 | Uking | Par | yes | 2 | 8 | 520 |  |
| 6 | Uking | Par | yes | 2 | 15 | 527 |  |
| 7 | Uking | Par | yes | 1 | 22 | 22 |  |
| 8 | Uking | Par | yes | 1 | 29 | 29 |  |
| 9 | Uking | Par | yes | 2 | 22 | 534 |  |
| 10 | Uking | Par | yes | 2 | 29 | 541 |  |
| 11 | Uking | Par | yes | 1 | 36 | 36 |  |
| 12 | Uking | Par | yes | 1 | 43 | 43 |  |
| 13 | Uking | Par | yes | 1 | 50 | 50 |  |
| 14 | Uking | Par | yes | 1 | 57 | 57 |  |
| 15 | Uking | Par | yes | 2 | 36 | 548 |  |
| 16 | Uking | Par | yes | 2 | 43 | 555 |  |
| 17 | Uking | Par | yes | 2 | 50 | 562 |  |
| 18 | Uking | Par | yes | 2 | 57 | 569 |  |
| 19 | Uking | Par | **no** | — | — | — |  |
| 20 | Uking | Par | yes | 1 | 64 | 64 |  |
| 21 | Uking | Par | yes | 1 | 71 | 71 |  |
| 22 | Uking | Par | yes | 1 | 78 | 78 |  |
| 23 | Uking | Par | yes | 1 | 85 | 85 |  |
| 24 | Uking | Par | yes | 1 | 92 | 92 |  |
| 25 | Uking | Par | yes | 1 | 99 | 99 |  |
| 26 | Uking | Par | yes | 2 | 64 | 576 |  |
| 27 | Uking | Par | yes | 2 | 71 | 583 |  |
| 28 | Uking | Par | yes | 2 | 78 | 590 |  |
| 29 | Uking | Par | yes | 2 | 85 | 597 |  |
| 30 | Uking | Par | yes | 2 | 92 | 604 |  |
| 31 | Uking | Par | yes | 2 | 99 | 611 |  |
| 32 | Uking | Par | yes | 1 | 106 | 106 |  |
| 33 | Uking | Par | yes | 1 | 113 | 113 |  |
| 34 | Uking | Par | yes | 1 | 120 | 120 |  |
| 35 | Uking | Par | yes | 1 | 127 | 127 |  |
| 36 | Uking | Par | yes | 2 | 106 | 618 |  |
| 37 | Uking | Par | yes | 2 | 113 | 625 |  |
| 38 | Uking | Par | yes | 2 | 120 | 632 |  |
| 39 | Uking | Par | yes | 2 | 127 | 639 |  |
| 40 | Uking | Par | yes | 1 | 134 | 134 |  |
| 41 | Uking | Par | yes | 1 | 141 | 141 |  |
| 42 | Uking | Par | yes | 1 | 148 | 148 |  |
| 43 | Uking | Par | yes | 1 | 155 | 155 |  |
| 44 | Uking | Par | yes | 2 | 134 | 646 |  |
| 45 | Uking | Par | yes | 2 | 141 | 653 |  |
| 46 | Uking | Par | yes | 2 | 148 | 660 |  |
| 47 | Uking | Par | yes | 2 | 155 | 667 |  |
| 48 | Uking | Par | yes | 2 | 162 | 674 |  |
| 50 | Chauvet | SlimPAR Tri 7 IRC 7ch | yes | 1 | 162 | 162 |  |
| 51 | Chauvet | SlimPAR Tri 7 IRC 7ch | yes | 1 | 169 | 169 |  |
| 52 | Chauvet | SlimPAR Tri 7 IRC 7ch | yes | 2 | 169 | 681 |  |
| 53 | Chauvet | SlimPAR Tri 7 IRC 7ch | yes | 2 | 176 | 688 |  |
| 80 | Riukoe | Mini Gobo Moving Head Light 11ch | yes | 1 | 176 | 176 | Movers All |
| 81 | Riukoe | Mini Gobo Moving Head Light 11ch | yes | 1 | 187 | 187 | Movers All |
| 82 | Riukoe | Mini Gobo Moving Head Light 11ch | yes | 2 | 183 | 695 | Movers All |
| 83 | Riukoe | Mini Gobo Moving Head Light 11ch | yes | 2 | 194 | 706 | Movers All |
| 85 | Betopper | 150W LED Beam Moving Head Light | yes | 2 | 358 | 870 | Movers All |
| 86 | Betopper | 150W LED Beam Moving Head Light | yes | 2 | 382 | 894 | Movers All |
| 87 | Betopper | 150W LED Beam Moving Head Light | yes | 2 | 394 | 906 | Movers All |
| 88 | Betopper | 150W LED Beam Moving Head Light | yes | 2 | 370 | 882 | Movers All |
| 90 | Rockville | Rockstrip 252 7ch | yes | 1 | 260 | 260 |  |
| 91 | Rockville | Rockstrip 252 7ch | yes | 1 | 240 | 240 |  |
| 92 | Rockville | Rockstrip 252 7ch | yes | 2 | 220 | 732 |  |
| 93 | Rockville | Rockstrip 252 7ch | yes | 1 | 276 | 276 |  |
| 94 | Rockville | Rockstrip 252 7ch | yes | 2 | 283 | 795 |  |
| 95 | Rockville | Rockstrip 252 3ch | yes | 2 | 260 | 772 |  |
| 96 | Rockville | Rockstrip 252 3ch | yes | 2 | 267 | 779 |  |
| 97 | Rockville | Rockstrip 252 7ch | yes | 2 | 270 | 782 | ???? |
| 98 | Betopper | 150W LED Beam Moving Head Light | **no** | — | — | — | Movers All |
| 100 | Chauvet | Hurricane Haze 1DX | yes | 1 | 303 | 303 |  |
| 101 | Chauvet | Hurricane Haze 1DX | yes | 2 | 302 | 814 |  |

## Groups (112, full channel membership)

Saved in full at `data/venues/norco/groups.json` (target number, label,
channel-range list exactly as Eos reports it — see eos-toolkit's own caveat
that this is the OSC Number Range format, compressed to sorted ranges, so it
is a *set*, not necessarily the original recorded *order* used by step
effects). `data/venues/norco/group-names.txt` (the earlier string-extracted
name-only list) is now superseded by this — kept only as a record of how it
was found.

Highlights relevant to the 2026-08-24 field measurements
(`norco-field-measurements-2026-08-24.md`):

| # | Label | Channels | Relevance |
|---|---|---|---|
| 20/99 | `Drums` | `42-46, 50-53` | confirms the drum-kit-convergence pars (42/46) with no change needed |
| 94 | `Pars Middle` | `19-30` | best candidate for "the center 12 pars" — spatially centred, 12 channel numbers |
| 93 | `Pars Ends` | `1-6, 43-48` | the opposite of Middle — the extreme outer pars |
| 87–90 | `Pars Qtr 1`–`4` | `1-12` / `13-24` / `25-36` / `37-48` | numeric quarters, not spatial — ruled out as a match for "center 12" |
| 56–65 | `OH Outer`/`Inner`, `BM Outer`/`Inner`, `*Pair` | `80,83` / `81-82` / `85,88` / `86-87` / … | the mover sub-groupings the operator built after the "outer movers sit upright" fix — confirms that correction is fully reflected in the console's own group structure, not just the raw patch |
| 41–45 | `Back Left`/`Centre`/`Right`, `Back L+C`/`C+R` | `40-42` / `43-45` / `46-48` | candidates for the "back row," though the dictated "2 and 4" split doesn't cleanly match any single one of these |

Full list is in the JSON — worth a skim next time a par-group question comes
up, since 112 names is more than this doc's spot-checks covered.

## What changed in `fixtures.json`

Rebuilt entirely from this live pull rather than patched incrementally:

- **Position/orientation for all 71 channels now come from the console's
  live `augment3d_position`/`augment3d_beam`**, not the show file's saved
  state. 26 of 71 channels had moved >5cm since the file was last saved —
  most dramatically the four downstage pars (1/2/3/4-ish), which moved
  several metres. This supersedes both the original `working.a3d` extraction
  *and* the field-measurement-driven floor-mover correction from earlier
  today (which used the wrong wall reference — see
  `norco-field-measurements-2026-08-24.md`'s superseded note).
- **New fields**: `manufacturer`, `model`, `patched`, `universe`, `address`,
  `global_address`, `beam_angle_deg`, `label`, `gel` — all straight from the
  live patch, not inferred.
- **`size`** (the fixture's rendered bounding box) is kept from the original
  extraction — the live OSC patch doesn't carry a physical size, only
  position/rotation/beam angle.
- Channels 19 and 98 are now **included** (previously dropped during the
  original extraction) with `patched: false` and a `(0,0,0)` position —
  matches eos-toolkit's own warning that an addressless fixture "is harmless
  on stage and *not* harmless in a visualiser — it still renders as
  geometry." Left in the data deliberately for completeness; `ignition-viz`
  now skips `patched: false` fixtures when building the scene rather than
  render a stray marker at the room origin (`scene.rs`).
