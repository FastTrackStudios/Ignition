---
name: ignition-program-song
description: Program a song's light show for Ignition — turn a song map and hit chart into a portable cue list (`data/songs/<song>.json`) written against the profile's roles, with section looks, sustained effects, hits and figures, then hold it to the cue-design guide with the lint and prove it cooks on a real venue. Use when asked to program, light, design cues for, or regenerate the show for a song, or to improve how an existing show looks.
---

# Program a song

**Edit with the actions, not by hand**: the `ignition-program` skill and
`docs/lighting/guides/programming.md` cover `IGNITION_CUE_*`,
`IGNITION_SECTION_*`, `IGNITION_FILL_SECTIONS`, `IGNITION_FILL_HITS` and the
library. What follows is what a good show is; write it through them, and
name palette slots (`Primary`, `Accent`…) rather than colours.

The deliverable is a `CueList` JSON in `data/songs/` that plays at every venue
implementing the profile. Every rule below exists because a show that names a
fixture, a channel or a metre is a show for one room. Load `ignition-domain`
first if the vocabulary (role, recipe, trick, transient, block) is not already
in context. The worked example is `crates/ignition-song/src/bin/authorshow.rs`
— *Bye Bye Bye* — whose module doc carries a section-by-section table of
which engine feature each section leans on.

## 1. Read the vocabulary you may use

Open `data/profiles/ignition.ig-profile` and list, verbatim:

- **roles** — group roles (`Key`, `Wash`, `Back` required; `Movers`, `Beams`,
  `Bars`, `Floor`, `Audience`, `Drums`, `Spot`, `Haze`… optional), focus
  roles (`Vocal`, `Stage`, `Audience` required; `Band`, `Drums`, `Back Wall`,
  `House` optional), canvases (`Main`, `Side Left`, `Side Right`).
- **colours** — the named colour presets a recipe may reference by string,
  and **splits** — named multi-colour palettes (`Ocean`, `Fire`, `Congo
  Split`, `Two Tone`…) recalled with `{"Split": "Ocean"}`; a split is one
  object for a split look, distributed `cycle` / `spread` / `block` across
  the selection, and its distribution travels with its name.
- **tricks** — 24 named sub-selections (`odds`, `evens`, `pairs`, `mirror`,
  `paired odds`, `centre out`, `four wings`, `scatter`…). Put one on an
  inline recipe as `tricks_ref` (by name, resolved from the profile pool at
  cook) or on a named effect as `tricks` (copied in, since a name carries no
  pool). The engine's own tricks — `Mirror`, `Invert(Pan|Tilt|PanTilt|All)`,
  `OnAxis(Y|Z, …)` — go inline on any recipe.
- **effects** — 125 library effects with a family and an `about` line in
  `effect_notes` (intensity, movement, one-shot, colour, beam, strip) and 12
  **bundles** (`verse bed`, `chorus drive`, `outro fade`…) that take several
  as one. Pick by reading the `about`; it says where the effect belongs.
  Check the library's `measure` (beats per loop) and `once` — some entries
  loop faster than the guide allows in a verse and want a `bars` override.
- **looks** — the profile's named static scenes (`verse bed` *Bed*, `chorus
  full` *Full*, `punt` *Punt*, `blackout` *Safe*), the same scenes the
  studio's LOOK keys hold. A cue may open on one (`look_ref("verse bed")` →
  `{"look": "verse bed"}`) and state what differs on top. **macros** —
  `drop`, `build 8`, `breakdown`, `end` — the two-key moves; a cue fires one
  through `commands: ["macro drop"]`. **protected** roles — `House Lights`
  — that a blackout, a rig drop, the black key, a `Safe` look and the grand
  master never touch. **speed_routing** — the family → master table the
  busk pages route by (movers `Tap` ×½, beams `Tap` ×2, the rest `Song`).

Done when: you have the lists and know which roles are optional. A show may
use an optional role, but must still read as a show where it is absent.

## 2. Read the song

**A show is two things, kept apart.** The song's **events** say what the
music does and nothing about light: sections, every chord change, every
pushed chord, every snare and kick that is really played, landings, stops,
the band out. They are the song map's markers (`IGNITION_SONG_MAP_SET`),
and they belong with the session. The **show** maps kinds of event to
generic moves from the library, section by section (`IGNITION_FILL_HITS`
with `kind`, `target`, `bump`, `depth`, `section`). Another show, another
room or another palette maps the same events another way. Nothing about one
song goes into the library; when a generic move is missing, add it to the
library under a name that fits any song.

A show that holds one effect per section and changes at the section line is
not finished. What happens inside a section comes from the events.

### The three sources, and what each is for

Read all three before designing anything, and make them agree on bar
numbers first (the chart's bar N is the project's bar N+1 after a count-in;
a section that is 18 bars in the set and 16 in the chart is a finding to
settle, not to average).

1. **The tempo map.** `lintshow <set.als> "<Song>" <song.json> --grid
   out.json` writes where every bar starts in seconds, through every tempo
   change. Never place anything from a constant tempo: a song that slows
   for its last third drifts by bars.
2. **The isolated drums.** Split the session's own reference with Studio's
   `song-stems` skill (11 stems; check the stems are of this arrangement by
   length). Chart the kit on the map's grid
   (`examples/shows/know_me_chart.py`: onset strength per kit piece on the
   eighth-note grid, loudness per bar). Read from it, per section:
   - where the **snare** really is (a half-time backbeat is on three, not
     on two and four) and where the **kick** is: these become `snare` and
     `kick` events, one per hit that is there, so a fill, a dropout and a
     stop are followed without being programmed;
   - **crashes** on a downbeat (landings and phrase tops), **tom runs**
     (pickups), bars where the kit is **silent** (stops, the band out);
   - **loudness per bar**: which sections are really louder than their
     neighbours, and where a build starts.
   Cymbals are the least reliable stem: confirm a crash by its bar position
   before a cue leans on it.
3. **The chart** (Keyflow `.kf`, with its `.lrc`). It is the only source
   for harmony and for what the band was told:
   - **chord changes** with their beat, and **pushes** (a chord written
     with `'` lands on the and of four of the bar before): `chord` and
     `push` events;
   - the **b7 or turnaround** that ends a line, a **walk-up**, a **stab**
     rhythm (`2m_8 1/3_4.`): where a section turns inside itself;
   - **written directions**: "Bass out", "Band in (no drums)", "Drums in",
     "Band out on 3", "Ring out", a fermata, a tempo change, who solos
     (a keytar solo wants the pool on the keys, not the guitarist), who
     talks over an outro (faces stay up);
   - **lyric cues**: where the voice enters a section (often beat 2), and
     the lines the song is about.

Where the chart and the stems disagree, look at the hits: the chart said
the band is out on three of bar 107 and the kit's last hits were there,
where a loudness curve alone had the song ending two bars early.

### No automatic stepping

An effect that turns lights on one after another, or swaps them, on a grid
(`build`, `chase`, `mirror chase in/out`, `chase eighths`, `alternate`,
`strip chase`, `sparkle`, a `wave` rolling tower to tower) is a claim that
the music does that. It almost never does, and the owner's verdict on a
show full of them was that it looks bad: "we only want to intentionally do
stuff like that, not just automatic".

- A layer of fixed lights (wash, fill, floor, bars, blinders, back) **holds
  still** inside a cue. What moves it is the song's events: all of it
  together on a kick, a snare, a chord change, a stab.
- A stepped move is **placed by hand** for a moment that has those steps in
  it (a walk-up's two stabs, a written hit figure), as events at those
  beats, never as a loop.
- A held chord gets a held look. Read the bar before putting motion in it.
- Smooth travel on moving heads, and organic flicker that claims no rhythm
  (`candle`, `fire flicker`), are fine where the section wants them.

### What else makes it land

- **A fade is an arrival.** A cue's fade ends on the beat it is for; a
  pushed chord's cue is placed on the and of four.
- **Movers move in black**: a focus change is written on the phrase start
  and the engine pre-positions dark heads; do not sweep lit beams to a new
  focus mid-phrase.
- **Vocal activity** (the vocals stem) says when the singer sings: key
  light up for a line, a touch down in a turnaround.
- **The reference's other stems** (bass drop-outs, a guitar or keys line
  entering) are section-internal moments as real as the drums.
- **Texture against accents.** Small level-only lifts (0.3 or less) on the
  song's own hits are texture and may be every bar; white, blinders and
  anything deeper are accents and stay thinned (rules 30 and 30a of the
  guide).
- **Prove timing by playing it**: a still does not fire a hit that falls
  inside a bar. Export a few bars (`viz --export out.mp4 --from-bar A
  --to-bar B`) and read the frames, or play it in the room.

```
cargo run -q -p ignition-song --bin songmap -- <project.rpp>
cargo run -q -p ignition-song --bin chart   -- <project.rpp>
```

The authoring path is `authorshow`: `cargo run -p ignition-song --bin
authorshow -- <project.rpp> > data/songs/<song>.json` reads the sections and
the `HITS` MIDI track (the authored chart) from the DAW project and writes the
cue list; `data/songs/<song>.hits.json` is only the detected draft. Edit the
`author()` function's per-section looks, not the JSON, when the show is
generated.

Write down, per section: name, kind (Intro / Verse / Pre / Chorus / Bridge /
Breakdown / Break / Outro / Count-in — `generate::kind_of` reads it from the
name), start bar, length in bars, and the energy relative to its neighbours.
Note every figure (a Connected note spanning hits) with its moment count.

Done when: every bar of the song belongs to exactly one section, and you can
say for each section whether it is louder or quieter than the one before.

## 3. Design the looks — one per section

Read `docs/domain/cue-design-guide.md` first — it is the professional
baseline (home/depart/return, one idea per section, the energy curve, colour
ownership, movement discipline) and ends in a 32-rule lint checklist the
show is judged against. `ignition_song::lint` implements the portable ones
(step 6). The rules below are the guide's short form for authoring:

- **Contrast is the show.** Each boundary between sections of different kind
  changes at least two of: level (Δ ≥ 0.2), dominant hue, how many layers
  are lit, whether the movers move. Levels by section: intro 0.2–0.4, verse
  0.3–0.55, pre 0.55–0.7, chorus ≤ 0.72, bridge 0.3–0.6, breakdown 0.1–0.4,
  **last chorus 1.0 and it is the only cue at full** — hold one thing back
  from chorus 1 (beams, floor, blinders, strobe, the fastest chase) and spend
  it there; CH 2 = CH 1 + one addition. The section before the last chorus
  is the darkest since the intro. The lint's energy is
  `0.5·level + 0.3·(lit layers/8) + 0.2·(counted effects/3)`; CH 1 ≤ 0.7,
  CH 2 ≤ 0.85, CH-last is the maximum by ≥ 0.05.
- **Two hues + white per song.** The *home* hue is the verse/pre/bridge; the
  **chorus owns its hue** and it appears in no verse cue (the last two bars
  of a pre excepted). A third hue lives only in the intro/outro or the
  bridge/breakdown — never in the vocal core. The palette folds into
  families (`lint::family`): cold (Sky…Cyan), violet (Lavender…Congo,
  Magenta, Pink), warm (Gold…Red), white. ≤ 2 families per role per cue.
  The key is white at **one** colour temperature all song (`Warm White`).
- **Snap when the music snaps.** Into a chorus ≤ 1 beat (`timing.color = 0`,
  `timing.dimmer_in = 1`); out of a chorus into a verse or bridge 4–8 beats.
  Effect periods are 4, 2, 1 or ½ bars only — position effects ≥ 2 bars in
  V/BR/INTRO/OUT, ≥ 1 in CH, ½ only in the last chorus. Strobes and
  blinders: last chorus only, ≤ 4 bars, ≤ 2 bursts; a `strobe riser` is the
  one exception — a one-shot over the last bar of a pre or breakdown that
  the chorus ends. Rainbows: never in V/BR, ≤ 4 bars.
- **Library first.** Reach for an effect by name (`effect("dark chase",
  Some(wash_lr()), None)`) or a bundle (`bundle("verse bed")`) before
  writing a step table; `bars` overrides the loop (`effect("random strobe",
  Some(beams()), Some(0.5))`) and is written as the `bars` **effect
  parameter**. Use `with_trick(…, "paired odds")` for a
  profile trick and `tricked(…, vec![Trick::OnAxis(Axis::Z,
  Box::new(Trick::Invert(InvertStyle::Pan)))])` for an engine one. Two
  relatives on one attribute is a `stack: true` inline recipe under a named
  one (`stacked_nod`) — the pair counts as one effect.
- **Looks first, then the difference.** Where a profile look *is* the
  section — the verse bed under a first verse, the blackout under a break,
  the punt under `safe` and `reset`, `chorus full` under a run-out — open the
  cue on it and say only what this song changes. The lint reads the look's
  recipes as the cue's, so a look that brings a bundle (`verse bed` brings
  `breathe` + `circle breathe`; `chorus full` brings `chase` + `windmill` +
  `two colour chase`) counts against the movement budget and the
  one-position-effect rule: do not restate what the look already runs, and
  do not open a held-back chorus (CH 1) on `chorus full`.
- **Effect parameters on a reference** are the fader's three, meaning the
  same thing: `param(effect("breathe", None, None), "depth", 0.5)` halves
  the swing for that cue alone; `param(…, "duty", 0.25)` makes a chase's
  first step a quarter of the cycle — a point of light is a point; `bars`
  is the loop. The library recipe is never rewritten; the file carries
  `"params": {"depth": 0.5}`.
- **Attribute filter on a reference** — `filtered(effect("rainbow",
  Some(bars()), None), AttrFilter::COLOUR)` — lets two effects share a role:
  the rainbow owns colour, `strip chase` owns intensity. Emits outside the
  filter are dropped at resolution and withdraw nothing.
- **Speed: the show stays on `Song`.** The busk pages route speed by the
  effect's *family* (`profile.speed_routing`: movers half the `Tap`, beams
  double it) because a fader has no song to follow. A cue is synced to the
  song, so every effect in a show runs on the `Song` master and the show-side
  form of routing is a per-recipe scale — `at_speed(…, Speed::Scaled {
  master: "Song", scale: 2.0 })` on the run-out's `chase eighths`. Never
  route a cue to `Tap`.
- **Macros from cues.** `c.commands = vec!["macro drop"]` on the last
  chorus's downbeat runs the profile's drop (strobe burst, blinders, fly
  out, chorus look, four beats, release) exactly as the DROP key would;
  `"macro end"` on the last cue. The studio drains the song's commands each
  frame and starts the macro; `osc …` lines are logged for the host. Keep a
  hand-authored fade beside `end` — the macro is "two beats, then black" at
  program time, not the song's S-curve.
- **House lights.** `House Lights` is optional and **protected**: set it
  once (Count-In, `look(house(), 0.3, "Warm White")`) and restate it on the
  end cue; nothing between — the break's blackout look, the drop's release,
  the end's blackout, a rig drop from the desk — takes it away. A room that
  binds it keeps its house through the show's black; a room that does not
  loses nothing.
- **The engine has more than looks.** A `RecipeApply::Canvas` drives a
  fixture attribute from a `proc` picture on a canvas grid (the count-in's
  noise on the bars); `FocusKeyframes` aims a line of movers through several
  focus roles; `FocusFan` between two; `Cue.fan` is a delay fan — a static
  look that wipes with no phaser; `Trig::Follow { beats }` fires a lift a
  fixed distance after the previous take (give it an `at` too so the clock
  lands it in the same place); `Cue.commands` carries `osc …` for the host;
  `timing.ease.position = CurveName::Swing.ease()` is how a verse's movers
  arrive; `"macro <name>"` in the same list runs a profile macro. Each is
  worth one section; none is worth every section.
- **Move in black and per-class timing are derived, not typed.** After the
  list is built, `ignition_song::set_mib` flags every cue whose focus for a
  mover differs from the last cue that aimed it (`mib.mode = Early`, fade 2
  beats, preference 80 on sections and 30 on `·` lifts); `set_class_timing`
  snaps colour into choruses and drifts position over a bar in verses and
  bridges. Author the aims; do not hand-set `mib`.
- **`Key` on the vocal**, 0.5–0.8 whenever someone is singing, never
  chased, strobed or accented — a generator (`candle`, `tv flicker`) is
  texture and allowed; a step table is not. Backlight whenever the key is
  up. Movers aim by focus role, never a point; one sustained position
  effect per role per cue; the movers run an effect in ≤ 60 % of the bars
  (dark movers do not count).
- **Dark is a value.** Set `Dimmer 0.0` on layers a section does not use;
  sections block, so what is unset goes out — but say it, so a reader knows.
  Ship a `safe` cue (positioned under the count-in so the clock never lands
  on it and GO can) and a `reset` cue after the end.

Done when: every section has a look with `Key`, `Wash`, `Back` stated, and no
two adjacent sections of different kind are the same.

## 4. Place the accents

- Hits are **triggers** (`CueList.triggers`), never cues — `docs/spec/
  triggers.md`. A charted High hit is a `white pop` bump (`BumpKind::White`);
  a Medium hit a `Level` bump; fall 0.45 beats. **Thin them to the guide**:
  one per bar in a chorus, pre or intro; one per two bars in a verse; none
  in a breakdown until its last bar; none in a break (its accent is the
  `negative flash` on the downbeat, from `section_triggers`); never in a bar
  a figure already owns. Strongest wins, a downbeat over an off-beat.
- Kick/Snare become the section's **pulse**: a looping 8-step `Delta` on the
  floor and back (kick, low and warm) and the wash (snare), depth 0.1–0.3,
  `measure 4.0`, no spread; never both on one role.
- A **figure** of 2–3 moments is a cutout: recipe 1 cuts `everything()` —
  every stage layer *but the key* — by −0.95, recipe 2 lifts the zone; the
  zone's fixtures take both. Zones are `Where::Covers` on the wash at face
  height, left→centre→right in performer's terms. Longer figures are bump
  runs across zones. `hold: true` on every figure moment except the last.
  The figures in a bar count as that bar's one accent, however many.

Done when: every kept hit is a trigger, every figure has one (or two, for a
cutout) per moment, and no trigger sets a colour or an absolute level.

## 5. Write the file

Shape (see `data/songs/bye-bye-bye.json` for a full example):

```json
{ "name": "Song", "cues": [
  { "name": "CH 1", "fade_secs": 0.25, "values": [], "block": true,
    "at": { "section": "CH 1", "bars": 0 }, "resolved": { "bar": 23 },
    "timing": { "dimmer_in": 1.0, "color": 0.0 },
    "recipes": [
      { "target": { "Role": "Wash" },
        "steps": [ { "apply": [ { "Dimmer": 0.72 }, { "Color": "Gold" } ] } ] },
      { "target": { "Order": { "of": { "Role": "Movers" }, "by": { "Axis": ["X", "Asc"] } } },
        "apply": { "FocusFan": { "from": "Vocal", "to": "Audience" } } },
      { "effect": "dark chase", "target": { "Order": { "of": { "Role": "Wash" }, "by": { "Axis": ["X", "Asc"] } } } }
    ] } ] }
```

Hard rules, checkable by grep on the file:

- `values` is empty on every cue (`r[cues.recipes-not-values]`); hits are in
  `triggers`, not `cues` (`r[triggers.are-not-cues]`).
- No `"Chans"`, no `"Group"` naming a venue group, no `universe`, no
  coordinates in metres except inside a `Where` zone (`r[files.no-fixture-identity]`).
- Every `Color` / `Split` / `FocusPoint` string is in the profile's lists.
- Cues sorted by `at`; blocking cues before accents at the same position.

Done when: the file parses (`serde_json`) and the greps above are clean.

## 6. Lint it

```
cargo run -q -p ignition-song --bin authorshow -- <project.rpp> --lint
```

prints the energy curve (energy, level, lit layers per section cue) and every
finding against the guide, by rule number, and exits non-zero on any. The
rules it holds a list to: 1 (every section has a blocking downbeat cue,
sections block and accents do not, recipes not values, roles only), 2, 4, 5,
6, 7, 8, 9, 10, 11, 12, 13, 14, 16, 17, 18, 19, 20, 21, 22, 23 (MIB on every
re-aim), 25, 26, 27, 28, 30, 31. Rules that need a venue or the player (3,
15, 24, 29, 32) are not here. `authorshow`'s `the_show_passes_the_design_lint`
test runs the same check on a synthetic arrangement and, when the project is
on the machine, on the real chart — so `cargo test -p ignition-song` fails on
a regression.

Done when: `0 finding(s)`, and the energy line reads as the curve you meant.

## 7. Prove it on a room

Without a GPU:

```
cargo run -q -p ignition-viz --bin igcheck -- data/shows/sunday.ig-show   # exit 0
cargo test -p ignition-viz --test figure_reveal --test dmx_loopback --test library_cooks
cargo test -p ignition-song
cargo test -p ignition-core show_file
```

`igcheck` holds every name in the show to the profile and reports what the
venue leaves unbound; `figure_reveal` plays figure 0's cutout through the
running PRE chase at Norco; `dmx_loopback` round-trips a chorus frame through
the bytes.

With one:

```
cargo run -p ignition-viz --bin viz -- --venue data/venues/norco \
    --cuelist data/songs/<song>.json --snapshot /tmp/<song>-b23.png --bar 23
```

- The loader prints cook status per cue: any cue reported dead (selects
  nothing) is a role the venue does not bind or a name outside the profile.
  Fix the show, not the venue.
- Snapshot the first bar of every section and one hit. A section still
  should be visibly different from its neighbours; a hit still should differ
  from the bar it lands in.
- Run the same load against `data/venues/riverside` — the second room exists
  to catch a show that only works at Norco (it has no beams, floor, drums or
  canvases; every one of those layers must be optional in the design).

Done when: igcheck is clean, the tests pass, and the lint is empty. Report
the section-by-section design with the feature each section demonstrates,
the lint result, and anything the model made awkward; leave the JSON as the
artefact.
