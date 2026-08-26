---
name: ignition-domain
description: Ignition's lighting domain — what a profile, venue, ignition file and show are; roles, recipes, cooking, tricks, effects/phasers, cues, tracking, triggers, the playback priority stack, and how grandMA3 terms map onto Ignition's. Load before designing, reviewing or explaining anything in crates/ignition-core, ignition-song or the show/venue data, or when a question uses console vocabulary (preset, palette, group, phaser, MAtricks, cue part, HTP/LTP, executor).
---

# Ignition domain

The specs in `docs/spec/` are the source of truth; this is the map. When a
rule matters, open the file and cite the `r[…]` id — code and tests carry the
same ids (`tools/spec_coverage.py` shows what is covered).

## The four files

```
Rockstars.ig-profile     interface   — roles a rig must provide (Key, Wash, Back…)
Norco.ig-venue           implementation — this room's fixtures, patch, geometry, bindings
Bye Bye Bye.ignition     a song's show — cues, recipes, triggers, written against the profile
Sunday 14th.ig-show      the night — a profile, a venue, an ordered list of songs
Bye Bye Bye@Norco.ig-local  venue layer — sanctioned home for one room's overrides
```

**Portability is the core concern.** A show can never name a fixture, channel,
universe or coordinate — only *roles*, *colour roles*, *focus roles* and
*canvases* the profile declares. The venue binds each role to a selection, a
point, a colour. Checking is static and two-sided: show→profile, venue→profile.
`profile.md`, `files.md`, `default-profile.md`.

Today's data: `data/profiles/ignition.ig-profile` (roles, 28 colours, 10 colour
splits, 24 named tricks, 125 effects with notes — `effects/{intensity,movement,colour,beam,strip,oneshot}.rs`), `data/venues/<name>/` (a directory of JSON: fixtures,
patch, groups, room, screens, props, palettes, profile bindings, areas),
`data/songs/<song>.json` (a bare `CueList`), `data/shows/*.json` (small demo
lists).

## Concepts, and the word to think with

| Word | Means | Spec |
|---|---|---|
| **role** | A job a rig plays (`Key`, `Wash`, `Movers`, focus `Vocal`, canvas `Main`). Shows reach the rig only through roles. | profile.md |
| **selection** | An ordered expression: `Role`, `Group`, `Tag`, `Model`, set algebra, `Where` (spatial predicate incl. `Covers` = where the beam *lands*), `Order` (by axis/distance). Order is the only authority on "which fixture is first". | groups.md |
| **trick** | Sub-selection and spreading: `Block(n)`, `Group(n)`, `Wings(n)`, `Mirror`, `Shuffle(seed)`, `Shift(n)`, `Reverse`, plus `Fan{from,to}` for spreading a value; 24 named sets in the profile. Every trick returns a selection; units, not fixtures, get phases. MA3's MAtricks. | tricks.md |
| **recipe** | A template: selection + steps + timing + tricks. **Cooked** against the live rig every frame — never a frozen fixture list. One step = look, two+ = effect. | recipes.md |
| **cook** | Establishing *coverage* (which attributes on which fixtures) — values are re-resolved every frame. Cooked status per cue: ok / empty / mixed. | recipes.md |
| **cascade** | Per attribute, highest first: direct value on cue › recipe on cue › preset direct › preset recipe. One absolute survives; one relative is added. | recipes.md |
| **absolute / relative** | `Dimmer`, `Color`, `FocusPoint`, `Raw` set; `Delta` modulates whatever is beneath. Separate layers. An effect over a look uses `Delta` and leaves colour alone. | recipes.md, effects.md |
| **effect (phaser)** | Steps with width/transition/ease; timing uniform across steps: speed (`Hz`/`Bpm`/`Secs`/`Master`), measure (beats per loop), phase spread across the selection, play mode (`Forward`/`Reverse`/`Bounce`/`Build`/`Negative`), `once`. Waveforms are authoring sugar. | effects.md |
| **speed master** | Named tempo source. `Song` = transport tempo map, `Tap` = tap tempo. Drives every recipe uniformly (past MA3). | effects.md |
| **one-shot / transient** | `once: true`: runs from its take time, withdraws when done. Bumps (`Level`/`White`/`ColorBoost`/`Burst`, fall = 0.45 beats) are the standard shape; a charted hit and a flash key are the same object. | effects.md, recipes.md |
| **sustained** | A looping recipe — chase, pulse, wave. Runs on the shared show clock so two cues stay in phase. | effects.md |
| **cue** | name, fade (arrival time), values, recipes, `block`, `at: Bars`. Sections block; accents (`·`-named) do not. | cues.md |
| **tracking** | A cue states only what changes; the *sources* track forward, so an inherited chase keeps moving. `block` starts from empty layers. | cues.md |
| **seek / locate** | The player is asked "what is the state at bar 43" — never "fire cue 9". Backwards rebuilds from the top; replay rebuilds state without performing transients. | cues.md, triggers.md |
| **trigger** | A hit the song fires: `at` + one-shot recipe + name, in `CueList.triggers`. Crossing fires once; stopped fires nothing; seek locates; simultaneous triggers sum; a ringing trigger holds off sustained effects on its keys (`CuePlayer::output_under`). | triggers.md |
| **stack** | Priority, highest first: operator's hand › masters/solo (scale only) › flashes › faders › triggers › cue player (transient › sustained › absolute). **Transient beats sustained regardless of take order.** Nothing merges at DMX. | playback.md |
| **song map** | Sections with lengths in bars, from the DAW project. Positions are bars/beats, never seconds; "4 bars into CH 1". | song.md |
| **chart** | The `HITS` MIDI track: Kick/Snare → pulses; Low/Medium/High → hits (intensity tiers, not instruments); a Connected note spanning hits → a **figure** whose **moments** land on **zones** (thirds of the stage); 2–3 moments = cutout, more = bump run. | song.md |
| **colour preset / split** | Intent (linear RGB, optionally gel), scope universal / global / selective (specified), and **splits** — named multi-colour entries (`ColorSplit`, `RecipeApply::Split`) distributed `cycle`/`spread`/`block`, nestable, cycles reported. Recalled by reference. | color.md |
| **focus** | A **point** (converge) or an **orientation** (parallel); patterns fan / splay / per-fixture; resolved to pan/tilt at output from the live hang, in metres and degrees. | focus.md |
| **area** | Where the talent stands — venue-owned, performer's left/right, distinct from a focus point. | profile.md |

## grandMA3 → Ignition

| MA3 | Ignition |
|---|---|
| Preset (Dimmer/Position/Color/… pools) | Colour preset, focus preset, role — recalled by reference |
| Group (with grid + selection order) | `Selection`, ordered; grid derived from real positions |
| MAtricks / Align | Tricks / spread |
| Phaser (steps, width, transition, accel/decel, speed, measure, phase) | Effect = recipe with ≥2 steps; same layers, `ease` for accel/decel |
| Recipe line (group + preset + MAtricks + phase spread), Cook | Recipe (selection + steps + tricks), cooked every frame |
| Cue part precedence "highest part wins" | Last-wins by take order within a layer |
| Priorities Super/Prog/Swap/HTP/…/LTP | Fixed stack by *kind of source*; HTP only for dimmer between equal playbacks |
| Temp / Flash executor, timecode event | Trigger (song) / flash (hand) — same bump object |
| Stage, MArker fixture | Venue stage space, relative-origin focus |
| Speed master ×16, Speed Scale | Named masters (`Song`, `Tap`), rate on the programmer |
| Stomp | Absolute layer last-wins; relative layer separate and continues |
| Clone / fixture-type exchange | Not needed — the show never held fixture identity |

## Where the code is

`crates/ignition-core`: `selection.rs`, `tricks.rs`, `recipe.rs` (+`expand_recipe`, cook status), `step.rs` (timing), `effects.rs` (library), `bump.rs`, `cue.rs` (player, cascade, tracking, seek), `trigger.rs` (bus), `programmer.rs` (busking layers), `profile.rs`, `preset.rs`, `focus.rs`, `music.rs`. `crates/ignition-song`: song map from the DAW, hits, chart, generate, `bin/authorshow.rs` (how *Bye Bye Bye* is written). `crates/ignition-viz`: venue loading, playback loop, DMX encode, visualizer. Design history: `docs/domain/cue-building-architecture.md`, `docs/domain/musical-time-cues.md`. Professional cueing rules and the lint checklist: `docs/domain/cue-design-guide.md`.

## What is built (2026-08-26)

Everything the grandMA3 gap analysis (`docs/research/grandma3-gap-analysis.md`) listed except **DMX output** — deliberately last. Highlights, with the type to reach for:

- **Shapes in the room**: `RecipeApply::FocusDelta(Vec3)` (metres), `orbit_m`, `FocusFan`, `FocusKeyframes`, `FocusSplay`, `FocusPerFixture`, `FocusAxes`, `FocusRelative{origin}`; movable markers via `Show.focus_overrides`; straight-line XYZ fades; `PanTiltRange`/`Reach` clamping; `StageSpace`.
- **Effects model**: `Ease::Curve{accel,decel}`, `RecipeApply::Random{..}` (pure per-unit generator), `Trick::Invert(style)`, `Trick::Mirror`, `Trick::OnAxis(axis, trick)` on a 3-axis `Grid` binned from real positions on Y and Z with selection order along X (`Grid::from_rig_in_order`; `Selection::Layout` overrides), `Timing.phase_spread_y/z_deg`, `Fan{shape,curve}` + keyframes, `Recipe.stack` (summing relatives), `Cue.morph`, `Speed::Scaled`, `Show.size/speed_scale` (one SIZE/RATE for every effect), `CuePlayer::freeze`, `step::transform::{reverse_time,rotate_deg,scale_axes,swap_axes,flip}`, bundles (`Profile.bundles`), `RecipeRef::{Inline,Named,Bundle}` in cues, `Recipe.enabled`, `tricks_ref` to the profile's named tricks.
- **Cues**: per-attribute in/out fades and delays (`Cue.timing`), delay/fade fans (`Cue.fan`), move-in-black (`Cue.mib`), `assert`, `cue_only`, `release`, `Trig::{Go,At,Follow,Sound}`, `commands`, same-position takes, relative positions (`Position` in `Cue.at`/`Trigger.at`, `CueList::resolve_positions`), tempo-map-aware timing (`Show.tempo`).
- **Playback**: `Playbacks` (several players by `Class`, HTP dimmer within a class), `Master{mode}` positive/negative/scaling/additive, `KeyAction::{Flash,Toggle,Swap,Kill,Black}`, fader pages with pickup, `program_time_beats`, `blind` + `preview_output`, highlight/lowlight, per-fader `speed_scale`; studio: page strip, key modes, PROG/BLIND toggles, MIDI/OSC (`remote.rs`, `data/profiles/remote.json`, features `midi`/`osc`), sound-in beat + bands (`sound.rs`, feature `sound`).
- **Colour**: `color::Intent{Rgb,Xy,Cct,Gel}`, emitter solve against fixture emitters (GDTF `Emitters` parsed), preset `Scope{Universal,Global,Selective}` resolved per fixture, named `ColorSplit`s.
- **Canvases**: `canvas::CanvasRecipe` procedural sources (`proc:rainbow`, `proc:{json}`), `BitmapChannel` driving any attribute from grid position (`RecipeApply::Canvas`), clips and procedural on one clock.
- **Files**: `.ig-show` (`ShowFile`), the `.ignition` header (`ShowDocument{profile,song}`), `.ig-local` venue layers, `venue.ig-venue` manifests, and `igcheck <show.ig-show>` — the static check from `r[files.compatibility-check]`.
- **Transports**: `TransportSource` (DAW, MTC, Art-Net timecode, LTC decoder, `TapClock`), non-destructive regeneration (`authorshow --merge --edits`).

Still unbuilt: DMX output (next), per-axis fans, `Scope::Global` averaging when *authoring*, intent carried through `CueValue` (the solve re-derives it from RGB). Check `python3 tools/spec_coverage.py --uncovered` before assuming either way.
