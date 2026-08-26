---
name: ignition-program-song
description: Program a song's light show for Ignition — turn a song map and hit chart into a portable cue list (`data/songs/<song>.json`) written against the profile's roles, with section looks, sustained effects, hits and figures, then prove it cooks on a real venue. Use when asked to program, light, design cues for, or regenerate the show for a song, or to improve how an existing show looks.
---

# Program a song

The deliverable is a `CueList` JSON in `data/songs/` that plays at every venue
implementing the profile. Every rule below exists because a show that names a
fixture, a channel or a metre is a show for one room. Load `ignition-domain`
first if the vocabulary (role, recipe, trick, transient, block) is not already
in context.

## 1. Read the vocabulary you may use

Open `data/profiles/ignition.ig-profile` and list, verbatim:

- **roles** — group roles (`Key`, `Wash`, `Back` required; `Movers`, `Bars`,
  `Floor`, `Drums`, `Audience`, `Spot`, `Haze`… optional), focus roles
  (`Vocal`, `Stage`, `Audience` required; `Band`, `Drums`, `Back Wall`, `House`
  optional), canvases.
- **colours** — the named colour presets a recipe may reference by string,
  and **splits** — named multi-colour palettes (`Fire`, `Ocean`, `Sunset`,
  `Two Tone`…) recalled with `{"Split": "Fire"}`; a split is one object for a
  split look, distributed `cycle` / `spread` / `block` across the selection.
- **tricks** — 24 named sub-selections (`odds`, `evens`, `pairs`, `thirds`,
  `quarters`, `mirror`, `centre out`, `ends in`, `four wings`, `scatter`…);
  put them on a recipe's `tricks` so one effect plays as a pattern.
- **effects** — 125 library effects with a family and an `about` line in
  `effect_notes` (intensity 38, movement 26, one-shot 21, colour 17, beam 12,
  strip 11). Pick by reading the `about`; it says where the effect belongs.

Done when: you have the four lists and know which roles are optional. A show
may use an optional role, but must still read as a show where it is absent.

## 2. Read the song

Load the song map and chart. Today the authoring path is
`crates/ignition-song/src/bin/authorshow.rs` — `cargo run -p ignition-song
--bin authorshow -- <project.rpp>` reads the sections and the `HITS` MIDI
track (the authored chart) from the DAW project and writes the cue list;
`data/songs/<song>.hits.json` is only the detected draft. Edit the `author()`
function's per-section looks, not the JSON, when the show is generated.

Write down, per section: name, kind (Intro / Verse / Pre / Chorus / Bridge /
Breakdown / Break / Outro / Count-in), start bar, length in bars, and the
energy relative to its neighbours. Note every figure (a Connected note
spanning hits) with its moment count.

Done when: every bar of the song belongs to exactly one section, and you can
say for each section whether it is louder or quieter than the one before.

## 3. Design the looks — one per section

Read `docs/domain/cue-design-guide.md` first — it is the professional
baseline (home/depart/return, one idea per section, the energy curve, colour
ownership, movement discipline) and ends in a 32-rule lint checklist the
show is judged against. The rules below are its short form for authoring:

- **Contrast is the show.** Each section boundary changes at least two of:
  level (Δ ≥ 0.2), colour, which layers are lit, movement. Levels by section:
  intro 0.2–0.4, verse 0.4–0.55, pre 0.55–0.7, chorus 0.7–0.85, bridge
  0.3–0.5, breakdown 0.1–0.4, **last chorus 0.9–1.0 and it is the only peak**
  — hold one thing back from chorus 1 (audience beams, strobe, white, the
  fastest chase) and spend it there. The cue before the last chorus is the
  darkest since the intro.
- **Two hues + white per song**, a third only in the bridge. The chorus owns
  its hue; it appears in no verse. Snap into choruses (fade ≤ 1 beat); fade
  out of them over 4–8 beats. Effect periods are 4, 2, 1 or ½ bars only —
  ½ only at the peak. Strobe/blinders: last chorus or button only, ≤ 4 bars.
- **Library first.** `ignition_core::effects::library()` has 125 curated
  effects in six families; reach for one by name (`effect("mirror chase
  out", …)`) before writing a step table, and use a named trick
  (`tricked(effect(...), profile trick)`) rather than a new selection for
  odds/evens/mirror patterns. Names and notes are in the profile.
- **`Key` on the vocal**, always lit when someone is singing; `Open` or `Warm`.
  Verses: `Key` up, `Wash` low and `Cool`. Pre: build — level rising, one
  sustained effect. Chorus: everything, `Warm`/`Hot`, `Back` saturated,
  `Movers` aimed `Audience`. Break: kill all but one layer. Bridge: a colour
  the song has not used yet. Breakdown: low, one layer, slow movement. Outro:
  fall back to the intro's look.
- **Movers**: aim by focus role (`Stage`, `Vocal`, `Audience`), never a point.
  One movement effect at most per section.
- **One sustained effect per section, two at most**, always `Delta`, never
  restating colour. Speed as `{"Master": "Song"}` with `measure` in beats.
  Direction comes from the selection: `Order{of: Role Wash, by: Axis[X,Asc]}`
  is left-to-right; `Distance{from centre}` is centre-out. Use `tricks` for
  odds/evens/halves rather than new selections.
- **Colour** by name from the profile. Literal RGB is allowed but a named role
  lets the whole show retune.
- **Dark is a value.** Set `Dimmer 0.0` on layers a section does not use;
  sections block, so what is unset goes out — but say it, so a reader knows.

Done when: every section has a look with `Key`, `Wash`, `Back` stated, and no
two adjacent sections are the same.

## 4. Place the accents

- Every charted Low/Medium/High hit becomes a one-shot `Delta` bump on the
  section's lit layers: use `ignition_core::bump::bump(target, kind, depth)` —
  `White` at depth ≥ 0.6 so it reads through colour, `Level` below. Fall is
  0.45 beats; do not lengthen it.
- Kick/Snare become the section's **pulse**: a looping 8-step `Delta` on the
  bar lights (kick) and wash (snare), depth 0.1–0.3, `measure 4.0`, no spread.
- A **figure** of 2–3 moments is a cutout: recipe 1 cuts `everything()` by
  −0.95, recipe 2 lifts the zone — in that order; the zone's fixtures take the
  later recipe. Zones are `Where::Covers` on the wash (where beams land, at
  face height), left→centre→right in performer's terms. Longer figures are
  bump runs across zones.
- Hits and figures are **triggers** (`CueList.triggers`: `{at, recipe, name}`),
  never cues — `docs/spec/triggers.md`. The transport fires them; they sum
  when they coincide and hold off any sustained effect on the fixtures they
  touch. A cutout is two triggers at one position: the cut over everything
  and the lift on the zone, the lift sized cut-plus-level. `hold: true` on
  every figure moment except the last, so the shape stays until the next
  moment moves it and the figure ends by falling; lone hits and last moments
  `hold: false` — a moment, not a state. Lifts, widenings
  and builds an operator would press GO for stay as `·`-named cues.

Done when: every chart hit above the kick/snare tier is a trigger, every
figure has one (or two, for a cutout) per moment, and no trigger sets a
colour or an absolute level.

## 5. Write the file

Shape (see `data/songs/bye-bye-bye.json` for a full example):

```json
{ "name": "Song", "cues": [
  { "name": "CH 1", "fade_secs": 0.25, "values": [], "block": true,
    "at": { "bar": 23, "beat": 1.0 },
    "recipes": [
      { "target": { "Role": "Wash" },
        "steps": [ { "apply": [ { "Dimmer": 1.0 }, { "Color": "Warm White" } ],
                     "width": 1.0, "transition": 0.0, "ease": "Linear" } ] },
      { "target": { "Role": "Movers" }, "apply": { "FocusPoint": "Audience" } },
      { "target": { "Order": { "of": { "Role": "Wash" }, "by": { "Axis": ["X", "Asc"] } } },
        "steps": [ { "apply": [ { "Delta": [["Dimmer", 0.0]] } ], "width": 1.0, "transition": 1.0, "ease": "Sine" },
                   { "apply": [ { "Delta": [["Dimmer", -0.6]] } ], "width": 1.0, "transition": 1.0, "ease": "Sine" } ],
        "timing": { "speed": { "Master": "Song" }, "measure": 4.0,
                    "phase_spread_deg": 360.0, "direction": "Negative", "once": false } }
    ] } ] }
```

Hard rules, checkable by grep on the file:

- `values` is empty on every cue (`r[cues.recipes-not-values]`); hits are in
  `triggers`, not `cues` (`r[triggers.are-not-cues]`).
- No `"Chans"`, no `"Group"` naming a venue group, no `universe`, no
  coordinates in metres except inside a `Where` zone (`r[files.no-fixture-identity]`).
- Every `Color` / `FocusPoint` string is in the profile's lists from step 1.
- Cues sorted by `at`; blocking cues before accents at the same position.

Done when: the file parses (`serde_json`) and the greps above are clean.

## 6. Prove it on a room

```
cargo run -p ignition-viz --bin viz -- --venue data/venues/norco \
    --cuelist data/songs/<song>.json --snapshot /tmp/<song>-b23.png --bar 23
```

- The loader prints cook status per cue: any cue reported dead (selects
  nothing) is a role the venue does not bind or a name outside the profile.
  Fix the show, not the venue.
- Snapshot the first bar of every section and one hit (`--bar N --time T`
  holds the clock at T). A section still should be visibly different from its
  neighbours; a hit still should differ from the bar it lands in.
- Run the same load against `data/venues/riverside` — the second room exists
  to catch a show that only works at Norco.
- `cargo test -p ignition-viz --test figure_reveal` if figures were touched;
  `cargo test -p ignition-core` always.

Done when: zero dead cues on both venues, every section snapshot differs from
its neighbour, and tests pass. Report the section-by-section design and the
snapshot paths; leave the JSON as the artefact.
