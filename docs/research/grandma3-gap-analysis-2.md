# grandMA3 → Ignition capability comparison, second pass

**Status**: research, 2026-08-26, after commit fa9206f on `spec-and-engine`. Re-verifies every item of [grandma3-gap-analysis.md](grandma3-gap-analysis.md) against the code, adds what the first pass missed, and names where Ignition is ahead.

# grandMA3 v2.2 vs Ignition — second capability comparison

**Branch** `spec-and-engine` @ `fa9206f`. Ground truth: `python3 tools/spec_coverage.py --uncovered` is **empty**; `--untested` lists 51 ids (all in the "declared/vocabulary" class — `song.*`, `triggers.*`, `playback.stack`, `effects.timing.uniform`… — none of them a capability gap). All file:line cites below are from `grep -n` on this checkout, not from the first doc. Paths are relative to the repo root; `core/` = `crates/ignition-core/src`, `viz/` = `crates/ignition-viz/src`, `song/` = `crates/ignition-song/src`, `studio/` = `apps/ignition-studio/src`.

One correction to the domain skill's "What is built": everything it claims checks out **except** that the skill's "video export in `ignition-viz/video`" (carried over from the old doc's Table 1 #28) is wrong — `viz/video/{mod,hap,ffmpeg}.rs` is *decode* only; there is no encoder or frame-capture path. Conversely, one thing I initially had reported as missing is built: straight-line XYZ fades (`core/cue.rs:1510,1589`, verified `cue.rs:3987`).

---

## 1. Status of every item from the first analysis

**Table 1 (console feature set)**

| # | Item | Status now | Evidence |
|---|---|---|---|
| 1 | DMX output | **MISSING** | Whole-tree scan: only `UdpSocket::bind` + `recv_from` in `viz/dmx.rs:244,254`; sACN/Art-Net listeners `dmx.rs:201,242`; `artnet_protocol`/`sacn` deps (`viz/Cargo.toml:25,36`) used for parse only. No `send_to`, no serial. `show.rs:93,177` writes into the same in-process `DmxUniverses` buffer the listeners fill. |
| 2 | Move-in-Black | **PARTIAL** | `Mib{mode,fade_beats,delay_beats}` `core/cue.rs:315-328`, `MibMode::{Early,Late,None}` `:301-313`; tests `cue.rs:3021,3052,3069`. Missing: `UponGo`, `Defined`/target, preference 0–100, MultiStep running/paused, sequence-level Force modes, `Hold` dimmer value. Generator (`song/bin/authorshow.rs`) emits zero `mib` keys — verified against `data/songs/bye-bye-bye.json`. |
| 3 | Per-attribute fade/delay + fans | **BUILT** | `CueTiming{dimmer_in,dimmer_out,color,position,beam,delay:ClassDelays}` `cue.rs:256-269`; `CueFan{delay,fade}` `cue.rs:291-297`; tests `cue.rs:2786,2815,2885,2925,2950`. Not per-fixture individual times (MA3's level 3) — per attribute *class* only. |
| 4 | Priority levels + multiple sequences | **BUILT (class model)** | `Class{Show,Look,Movers,Song}` `core/playbacks.rs:31`; `Playbacks::output` `:115` — higher class wins, dimmer HTP within a class, absent keys fall through; tests `:208-285`. No numeric priority, no Soft LTP, no Off-when-overridden (by spec design). |
| 5 | Grid + MAtricks X/Y/Z | **BUILT** | `Trick::OnAxis(Axis, Box<Trick>)` `core/tricks.rs` enum; `Grid::from_rig_in_order` `tricks.rs:424`; `Selection::Layout` `core/selection.rs:317`; `Timing.phase_spread_y/z_deg` `core/step.rs:290,295`; tests `tricks.rs:1460,1482,1404`, `step.rs:911`, `recipe.rs:3798`. |
| 6 | Colour engine | **PARTIAL** | `Intent{Rgb,Xy,Cct,Gel}` `core/color.rs:56-77`; constrained emitter solve `color.rs:568,596` with Q (`:816` test); GDTF emitters `viz/gdtf_import.rs:143`. Missing: named colour spaces (PLASA/Rec.2020 — GDTF `<ColorSpace>` parsed in `gdtf-vendored` but unconsumed), mix-vs-wheel preference, and **intent is lost at the cue boundary**: `CueValue{chan,attr,value:f32}` `cue.rs:38-42`; `viz/show.rs:168` rebuilds `Intent::Rgb` from three floats. Gel book is 16 hard-coded swatches `color.rs:327-456`. |
| 7 | Preset modes / embedded / Recast | **PARTIAL** | `Scope{Universal,Global,Selective}` `core/preset.rs:58`, `resolve_for` `:129`, tests `:487-538`. Embedded chains only for splits (`walk_split` `:360`, depth 10); a `Ref<ColorPreset>` is one hop. No Recast verb — refs resolve at output (`recipe.rs:700,806`) so the *effect* is automatic; `Scope::Global` averaging when authoring not built. No preset-stored timing. |
| 8 | Assert / Release / Break / Cue-only / Shield | **PARTIAL** | `assert` `cue.rs:124`, `cue_only` `:129`, `release: Vec<Attribute>` `:135`, `block` `:78`; tests `:3093,3120,3153,2401`. Missing: Break (per-attribute filter), Tracking Shield ↑0/>0, Tracking Distance, X-Assert. |
| 9 | Trig types + Command | **BUILT / PARTIAL** | `Trig::{Go,At,Follow{beats},Sound{band}}` `cue.rs:349-361`; `commands: Vec<String>` `:153` — opaque strings, host drains via `drain_commands` `:839`; no command delay, no BPM trig type. |
| 10 | Speed masters ×16 / Learn / scale / Sound BPM | **BUILT mostly** | `SpeedMasters` `step.rs:126`; `Speed::{Hz,Bpm,Secs,Master,Scaled}` `step.rs:137`; per-fader `speed_scale` `programmer.rs:63`; sound BPM → `Command::Tap` `studio/sound.rs:336`. Missing: Learn (tap-average), Half/Double speed keys, Speed-from-Rate. |
| 11 | Group master modes | **BUILT** | `MasterMode` `programmer.rs:89-110`, `apply_masters` `:918`; tests `:1284-1428`. |
| 12 | Executor key/fader functions, pages | **BUILT mostly** | `KeyAction::{Flash,Toggle,Swap,Kill,Black}` `programmer.rs:132-152`; pages + pickup `:207,657-679,546-566`. Missing: Temp, Pause, Load, Go−, manual X/XA/XB crossfade, Swap/Kill protect. |
| 13 | Timing masters / Program Time / Exec Time | **PARTIAL** | `program_time_beats` `programmer.rs:225` (tests `:1759-1805`), global `rate` `:267`. No per-playback Exec Time or timing-master pool. |
| 14 | Freeze / Blind / Preview | **BUILT** | `blind` `:233`, `preview_output` `:737`, test `:1817`; `CuePlayer::freeze` `cue.rs:1727` (test `:4076`). No MA3-style *programmer-over-executor* freeze, no Park. |
| 15 | XYZ / stages / markers / PSN | **BUILT except PSN** | `FocusDelta` `recipe.rs` enum, `resolve_focus_delta` `focus.rs:290`; `StageSpace` `focus.rs:202`; `Reach` clamp `:152-182`; straight-line fades `cue.rs:1510`; movable markers `viz/playback.rs:85` + overlay drag `viz/overlay.rs:216`. PSN: only the vendored GDTF stub. |
| 16 | Bitmap / pixel mapping | **BUILT** | `CanvasRecipe` `core/canvas.rs:83`, procedurals `:38,372` (rainbow/wipe/noise/bands/sparkle), `BitmapChannel` `:228`, `RecipeApply::Canvas` `recipe.rs:206`; video decode behind `hap`/`ffmpeg` features `viz/video/mod.rs:186`. Missing: NDI, X/Y/zoom/rotate *as live attributes*. |
| 17 | Timecode | **PARTIAL** | Sources: MTC `song/timecode.rs:309`, Art-Net TC `:423`, LTC decoder `:493/659`, TC→bars `song/transport.rs:112` → `music.rs:165`. Missing: timecode *shows* (tracks/events). Ignition's cue positions in bars already are the "track" — see §3. |
| 18 | MIDI/OSC/DMX-in/DC remotes | **BUILT (MIDI, OSC)** | `studio/remote.rs` `Binding` enum `:105-142` (faders, keys, masters, rate/size/speed/program-time, blind/highlight, go, pages); tests `:621-845`; `data/profiles/remote.json`. Missing: DMX-in remote, analog, MSC, MIDI/OSC *feedback out*. |
| 19 | Cue parts | **MISSING** (deferred `docs/spec/cues.md:183`) | Subsumed by #3 + several recipes per cue. |
| 20 | Sync / Morph / Transition | **BUILT** | `Cue.morph` `cue.rs:144`, test `:3192`; shared clock. No per-cue Transition curve column (9 MA3 curves) — fades are linear at the cue level. |
| 21 | Stomp / explicit freeze | **BUILT** | `CuePlayer::freeze` `cue.rs:1727` = MA3 `Capture`. |
| 22 | Highlight / Lowlight / Solo | **BUILT** | `programmer.rs:236-240,874`, `:286-351`; tests `:1855,1882,1446`. |
| 23 | Worlds / filters | **MISSING** (by design) | |
| 24 | Random generator | **BUILT** | `Random{attr,low,high,level_var,speed_var,attack,decay,seed,absolute}` `recipe.rs:232-258`, hash `roll` `:260`; tests `:3320,3351`. Missing vs MA3: phase variance, ratio/ratio-variance, "random start". |
| 25 | Align modes/curves | **BUILT** | `FanShape::{Linear,FromFirst,FromLast,CentreOut,EndsIn}`, `Curve::{Linear,Sine,Slow,Fast}` `tricks.rs:972-1000`; test `:1777`. |
| 26 | MAgic presets | **BUILT** (focus only) | `FocusKeyframes(Vec<Ref<Vec3>>)` `recipe.rs:104`; tests `recipe.rs:3192`, `tricks.rs:1817`. Colour keyframes: `Colors{Spread}` two-point only. |
| 27 | Fixture types / subfixtures / multipatch | **PARTIAL** | GDTF `gdtf_import.rs:50`, OFL `ofl_import.rs`, 16-bit `show.rs:40-66`. Domain model is flat (`FixtureInfo` `selection.rs:35`); geometry tree only in the renderer (`gdtf_geometry.rs`). No multipatch (`Patch` `venue.rs:299` is 1:1), no DMX curves, GDTF meshes not loaded (`gdtf_import.rs:10-17`). |
| 28 | 3D viewer | **BUILT** | Beams `viz/beam.rs`, bloom `app.rs:496`, props `props.rs`, `ViewPreset` `view.rs:21`. Missing: gobo raster, cameras in venue JSON, video export. |
| 29 | Layout view / linearize | **PARTIAL** | `Selection::Layout` data only `selection.rs:317`, `Grid::for_selection` `tricks.rs:487`; flagged non-portable by `igcheck` (`show_file.rs:656`). No view. |
| 30 | Macros / Lua | **MISSING** | `studio/command.rs` is a typed message enum. |
| 31 | Sessions / multi-user / views | **MISSING** | |
| 32 | RDM | **MISSING** | |
| 33 | Agenda | **MISSING** | |
| 34 | Library-by-name (no clone-by-value) | **BUILT** | `RecipeRef::{Inline,Named,Bundle}` `recipe.rs`, `Profile.bundles` `profile.rs:153`, `tricks_ref` `recipe.rs:377`; `igcheck` `UnknownEffect` `show_file.rs:733`. |
| 35 | Relative positions / ordinals / regeneration | **BUILT** | `Position::nth` `music.rs:336`, `resolve` `:359`; `CueList::resolve_positions` `cue.rs:392`; `draft.rs:26-100` `--merge/--edits`; tests `music.rs:629-692`, `draft.rs:132-179`. Missing: a `generated` marker (merge identity is the cue *name*, `draft.rs:56-77`). |

**Table 2 (expressiveness)**

| # | Item | Status | Evidence |
|---|---|---|---|
| 1 | Accel/Decel | BUILT | `Ease::Curve{accel,decel}` `step.rs:42`; tests `:1120-1155` |
| 2–3 | Delay/fade fans | BUILT | `CueFan` `cue.rs:291` |
| 4 | Random generator | BUILT | `recipe.rs:232` |
| 5 | Bundles | BUILT | `profile.rs:153`, `RecipeRef::Bundle` |
| 6 | Morph | BUILT | `cue.rs:144,3192` |
| 7 | Stacked relatives | BUILT | `Recipe.stack` `recipe.rs:365`; test `cue.rs:3355` |
| 8 | XYZ phaser | BUILT | `FocusDelta` + metre orbit; tests `cue.rs:3476,3507` |
| 9 | 3-axis tricks | BUILT | above |
| 10 | Invert per attribute | BUILT | `Trick::Invert(InvertStyle)`; tests `tricks.rs:1709`, `recipe.rs:3259` |
| 11 | Step transforms | BUILT | `step::transform::{reverse_time,flip,scale_axes,swap_axes,rotate_deg}` `step.rs:530-583`; test `:1336` |
| 12–16 | Master modes / highlight / speed scale / align / MAgic | BUILT | above |
| 17 | Canvas | BUILT | above |
| 18 | Sound-driven *values* | **PARTIAL** | Bands computed (`sound.rs:47`) and stored (`viz/playback.rs:29`) but the only engine consumer is `Trig::Sound` `cue.rs:360`; no band→attribute modulator |
| 19 | DelayToPhase | **MISSING** (falls out of `CueFan` but nothing links a delay fan to phase) | |
| 20 | Cue Command | PARTIAL | opaque strings |

Untested-but-built worth flagging: `song/transport.rs` has no test module at all (TapClock, SourceTransport, DawSource); `viz/playback.rs` `load/tick_playback/operator_keys` untested.

---

## 2. New gaps (not in the first list)

Pages read this pass (all `https://help.malighting.com/grandMA3/2.2/HTML/`): `phaser.html`, `phaser_editor.html`, `generator.html`, `matricks.html`, `matricks_transform.html`, `operate_align.html`, `operate_selection.html`, `presets.html`, `presets_recipes.html`, `recipes.html`, `keyword_recast.html`, `keyword_park.html`, `keyword_capture.html`, `keyword_stomp.html`, `keyword_default.html`, `keyword_temp.html`, `keyword_clone.html`, `operate_programmer.html`, `operate_color_picker.html`, `cue_mib.html`, `cue_timing.html`, `cue_sequence_sheet.html`, `cue_sequence_settings.html`, `cue_playback.html`, `cue_tracking_shield.html`, `keyword_fadercrossfade.html`, `executor_assign.html`, `executor_configurations.html`, `masters_grand.html`, `masters_playback.html`, `masters_selected.html`, `masters_speed.html`, `group_master.html`, `sound_viewer.html`, `xyz.html`, `xyz_marker.html`, `patch_stage.html`, `patch_add_multipatch.html`, `fixture_types.html`, `ft_build.html`, `dmx_ethernet.html`, `dmx_ethernet_sacn.html`, `dmx_ethernet_artnet.html`, `remote_inputs_osc.html`, `remote_inputs_midi.html`, `remote_inputs_dmx.html`, `worldfilter.html`, `timecode.html`, `timecode_events.html`, `bitmap.html`, `layouts.html`, `patch_3d_viewer.html`, `network_session.html`, `agenda.html`. 404: `matricks_interleave.html`.

Values: **P** portable show, **G** generated cues, **B** busking, **V** visualizer.

| # | MA3 capability (page) | Ignition status | Value | What it takes |
|---|---|---|---|---|
| N1 | **Park / Unpark** — pin a fixture, attribute or DMX channel at a value above every playback (`keyword_park.html`) | MISSING (`Playback.enabled` is the only kill switch) | **High (B)** — "that mover's tilt motor is screaming, park it at 50" is a nightly busk need; also the safe way to hold a house-light channel | `Programmer.parked: BTreeMap<(ChanId,Attribute),f32>` applied last in the fold, before DMX; **S** |
| N2 | **Fixture default values + Release-to-default; Cue Zero mode** (`keyword_default.html`, `cue_sequence_settings.html`) | MISSING — a released attribute falls through to whatever is below, and nothing below means 0 | **High (P,G)** — a cue that releases zoom on a spot must land on that spot's *default* zoom, which differs per model; a generated first cue needs Cue-Zero semantics | `FixtureProfile.defaults` from GDTF `Default` per channel function (already in gdtf-vendored) used as the floor of `Playbacks::output`; **S–M** |
| N3 | **Sequence-level MIB policy + Hold** — Enabled/Never/Force Early/UponGo/Late; `Hold` dimmer value; MIB preference 0–100; MultiStep running/paused (`cue_mib.html`, `cue_sequence_settings.html`) | PARTIAL (see Table 1 #2) — and the **generator never emits MIB** | **High (G)** — the model exists but a generated `bye-bye-bye.json` has zero MIB fields, so movers still swing visibly into every chorus | (a) `authorshow` sets `mib: Early` on every cue whose position recipe changes; (b) `MibMode::UponGo`, `preference`, `MultiStep`; **S** for (a), **M** for (b) |
| N4 | **Cue Transition curves** — Linear/Slow/Slow+/Fast/Fast+/SCurve/Swing± per cue (`cue_timing.html`) | MISSING at cue level — `Ease::Curve` exists on *steps* only | Med (G) — a "Swing" arrival on a mover reposition looks designed; a cue fade today is a straight line | reuse `Ease` on `CueTiming` (`ease: Ease` per class); **S** |
| N5 | **Individual per-fixture timing (level 3) + Individual Timing "Normalized"** (`cue_timing.html`, `cue_sequence_sheet.html`) | MISSING — timing is per attribute *class*; fans give a gradient but not arbitrary per-fixture times | Low-Med (G) — fans cover 90% of generated use | `Cue.timing_overrides: Vec<(Selection, CueTiming)>`; **S** |
| N6 | **Tracking Shield ↑0 / >0 and Break** (`cue_tracking_shield.html`, `cue_sequence_sheet.html`) | MISSING | Med (G) — when a human edits one generated cue, shield is what stops the edit reaching the next "lights come up" cue; `cue_only` is the blunt form | `Cue.break_: Vec<Attribute>`; shield as a store-mode in `authorshow --edits` merge; **S** |
| N7 | **Speed-master Learn / Half / Double keys; Rate1/Speed1 reset** (`executor_assign.html`, `masters_speed.html`) | PARTIAL — Tap exists (`viz/playback.rs:553`), no learn-averaging, no half/double key | Med (B) — a tap key that *averages* is the difference between usable and jittery; ×½/×2 keys are how a busker rides a breakdown | `KeyAction::{Learn,HalfSpeed,DoubleSpeed}` on the `Tap` master; **S** |
| N8 | **Temp fader/key, Pause, Load, Go−, manual X/XA/XB crossfade** (`executor_assign.html`, `masters_selected.html`) | MISSING (Flash/Toggle/Swap/Kill/Black built) | Med (B) — Temp (on-with-fade-times while held) is the musical version of Flash; manual X-fade is theatre, low for the goals | `KeyAction::Temp`, `CuePlayer::pause/resume`; **S**. Skip X/XA/XB |
| N9 | **Playback masters ×50 (inhibitive per sequence) + Grand Master** (`masters_playback.html`, `masters_grand.html`) | MISSING — masters are per *role* only (`programmer.rs:279`); no grand master | Med (B) — "pull the whole song list down under the look list" and a physical GM are both expected on any surface | `Playback.master: f32` scaled in `Playbacks::output`; `Programmer.grand: f32` last; **S** |
| N10 | **Selected-fixture master / Dimmer-of-selection encoder** (`masters_selected.html`) | MISSING — masters key on role names | Low-Med (B) | `Master` keyed by `Selection`; **S** |
| N11 | **Sound channels as *values*** — 11 bands + inverses feeding any attribute; Sound Fade master (`sound_viewer.html`) | PARTIAL — bands reach `viz/playback.rs:29` and stop | Med (B) — bass-driven blinder level with no chart is the support-act case | `Speed`-like `Source::Band(name)` inside `RecipeApply::Delta`/`Random.high`; smoothing = Sound Fade; **M** |
| N12 | **DMX-in remotes, analog/DC, MSC; MIDI/OSC feedback out** (`remote_inputs_dmx.html`, `remote_inputs_midi.html`, `remote_inputs_osc.html`) | MISSING (input MIDI/OSC only) | Low-Med (B) — OSC *feedback* is what makes a TouchOSC/X-Touch surface show fader positions; DMX-in as remote is a house-desk-triggers-Ignition path | OSC out of `Playhead` state; **S**. MSC: skip |
| N13 | **Colour spaces + Constant Brightness + Mix/Wheel preference** (`operate_color_picker.html`) | MISSING — no gamut model, no wheel arbitration | Med (P) — "Prefer Mix Color, wheel as backup" is what makes a gel-only spot and an RGBW wash both hit "Congo Blue" | consume GDTF `<ColorSpace>`; wheel-slot nearest-xy fallback in `viz/show.rs:140`; **M** |
| N14 | **Intent carried to output** — CueValue as a bare float (`presets.html` "preset link in the absolute layer") | MISSING — `cue.rs:38`; `show.rs:168` re-derives | **High (P)** — the whole point of `Intent::Cct/Gel` is lost the moment a cue is cooked; a CCT 3200K on a 6-emitter wash becomes whatever RGB the 3-float round trip gives | `CueValue::Color{intent}` variant or a side-table `(chan) -> Intent` alongside `values`; solve at output; **M** |
| N15 | **Multipatch** (`patch_add_multipatch.html`) | MISSING (`Patch` 1:1) | Med (P,V) — four house pars on one address is normal in a small venue; today the venue must lie about fixture count | `Patch.mirrors: Vec<DmxAddress>`; **S** once DMX out exists |
| N16 | **Subfixture selection (Down/Up) + strict selection mode** (`operate_selection.html`, `recipes.html`) | MISSING in domain model | Med (P) — bars and cells: `strip` effects address whole fixtures | `FixtureInfo.parent`, `Selection::Down`; **M** |
| N17 | **DMX/attribute curves, physical from/to per channel function** (`ft_build.html`) | MISSING — linear encode | Low-Med (V,P) — dimmer curves matter for HTP between a 0–100 LED and a lamp | per-attribute LUT in `channel_map.rs`; **S** |
| N18 | **Generator extras** — phase variance, ratio, ratio variance, random start (`generator.html`) | PARTIAL | Low (G) | fields on `Random`; **S** |
| N19 | **Preset-stored timing (fade/delay in a preset)** (`presets.html`) | MISSING | Low (G) — cue timing covers it | skip |
| N20 | **Recipe line `Selection Mode` Normal/Strict, `Lock`, Merge vs Overwrite cook** (`recipes.html`) | PARTIAL — `Recipe.enabled` yes; cook is always overwrite-into-cue | Low | `Cook::Merge` flag; **S** |
| N21 | **Wrap-around, Restart mode, Master-Go mode, Auto Start/Stop** (`cue_sequence_settings.html`) | PARTIAL — Auto-start/stop on fader is built (pickup); wrap/restart are positional (`seek`) | Low (B) | `CueList.wrap: bool`; **S** |
| N22 | **Sequence Speed Scale / Rate Scale stepped** | BUILT (`Fader.speed_scale`) | — | — |
| N23 | **NDI into a bitmap** (`bitmap.html`) | MISSING | Low | skip |
| N24 | **Session failover / backup** (`network_session.html`) | MISSING | Low | text files + git |
| N25 | **Video export / render-to-file** (old Table 1 #28 claimed it) | MISSING — decode only | Low-Med (V) — a rendered preview per song is a good deliverable | frame capture + ffmpeg encode behind the `ffmpeg` feature; **M** |
| N26 | **Gobo raster in the viewer** (`patch_3d_viewer.html`) | MISSING | Low (V) | **M** |

---

## 3. Where Ignition is ahead of MA3

- **Spatial selections from real positions.** `Selection::Where{Half,Within,Near,Covers}` / `Order{Axis,Distance}` (`core/selection.rs:132-237`), grid derived from the rig (`Grid::from_rig_in_order`). MA3's selection grid is explicitly *not* the 3D position: `operate_selection.html` — "the grid establishes spatial relationships between fixtures independent of their actual 3D Viewer positions"; you build it by hand or via `layouts.html` + linearize. Ignition needs no layout view.
- **Profile-based portability.** `Profile{roles,…}` (`core/profile.rs:111`) + `VenueProfile` binding (`:205`), proven by `tests/portability.rs:118` (one recipe, two rigs) and `tests/profile_binding.rs:74`. MA3's answer is `keyword_clone.html` — copy values fixture-to-fixture, with "all presets referenced by the destination objects are automatically cloned", i.e. by-value duplication per venue.
- **Song-map cues.** `Position::nth(section, ordinal, bars)` (`core/music.rs:336`), tempo-map aware (`cue.rs:3859`). MA3 only has seconds: `cue_sequence_sheet.html` Trig Time, and `timecode.html` events on a running time counter. A re-arranged bridge breaks an MA3 timecode show and not an Ignition one.
- **Triggers/holds as a bounded layer.** `TriggerBus` (`core/trigger.rs:101`, `MAX_LIVE=32` `:115`, own clock `:92`). MA3 has nothing between a cue and a Temp key; hits are cues or macros.
- **Procedural canvases with a shared clock.** `canvas.rs:38` (rainbow/wipe/noise/bands/sparkle) at `cycles_at(secs, masters)`. `bitmap.html` sources are "images, gobos, symbols, or videos" plus NDI — no generated content, and playback speed is a Speed Master on a *video's* frame rate.
- **Static compatibility check.** `igcheck` (`viz/bin/igcheck.rs`, `show_file.rs:564` findings: Undeclared, VenueGroup, UnknownEffect, FixtureIdentity, MetreCoordinate, AudienceOriented). MA3 has no offline "will this show file work on that rig" — `keyword_recast.html` and `keyword_import.html` act on a loaded show.
- **Generated shows with non-destructive regeneration.** `authorshow` (`song/bin/authorshow.rs`) + `draft.rs:56` merge. MA3's nearest is `presets_recipes.html` auto-cook, which regenerates *values* for a selection, never a cue list from a chart.
- **Seekable, deterministic effects.** `Random.roll` is a pure hash of (seed, unit, k) (`recipe.rs:260`); MA3's `generator.html` "Random Start: Yes" is explicitly non-reproducible and its phasers free-run from Go.

---

## 4. Revised top 10 (effort S/M/L)

1. **DMX output — sACN + Art-Net transmit** — **M**. Still nothing outranks it: every row above is invisible until a socket sends. `sacn` 0.11 has a sender; `artnet_protocol` builds `ArtDmx`. Include per-universe priority (0–200), unicast list, "send if idle" and a 25–44 Hz keep-alive per `dmx_ethernet_sacn.html`/`_artnet.html`. Put it beside `viz/dmx.rs` reading the same `DmxUniverses`.
2. **Carry colour intent through `CueValue`** (N14) — **M**. The portability promise is half-kept until "Warm" stops being three floats at `viz/show.rs:168`.
3. **Generator emits MIB + `UponGo`/preference/`Hold`** (N3) — **S + M**. The engine has `Mib`; the shipped show has none. Cheapest visible win for generated cues.
4. **Fixture defaults as the release floor + Cue Zero** (N2) — **S–M**. Without it release/cue-only semantics are wrong on any fixture whose rest state isn't 0.
5. **Park/Unpark** (N1) — **S**. Nightly busking safety, trivial in the fold.
6. **Multipatch + DMX curves** (N15, N17) — **S + S**, after #1. Small venues need both on day one of real output.
7. **Grand master + per-playback masters; Temp/Pause/Learn/½/×2 keys; OSC feedback** (N7, N8, N9, N12) — **S each**. Finishes the 8-fader surface.
8. **Sound bands as values + Sound Fade** (N11) — **M**. The no-chart busking case.
9. **Mix-vs-wheel preference + colour spaces** (N13) — **M**. Makes gel-only spots participate in colour presets.
10. **Cue transition curves + Break/Shield** (N4, N6) — **S + S**. Polish for generated lists; both reuse existing types.

Dropped from last time's list because they are now built: XYZ deltas, per-attribute timing/fans, assert/cue-only/library-by-name, 3-axis tricks, accel/decel/align/invert, multiple playbacks, group-master modes/keys/pages/MIDI. Deliberately still off: cue parts, worlds/filters, Lua, sessions, agenda, layout view, X/XA/XB, NDI.