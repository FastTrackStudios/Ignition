# Cue design guide — what makes a generated show read as professional

**Status**: reference, 2026-08-26. Consolidated from console workbooks and
working LDs (sources at the foot); numbers marked *(default)* are
proposed generator defaults where the sources are qualitative. This is
the guide `authorshow` and the `ignition-program-song` skill design
against, and the source of the lint rules at the end.

Section tags: **[ALL] [INTRO] [V] [PRE] [CH] [CH-last] [BR] [BD] [OUT]**.
Units are tempo-relative: `1b` = one beat.

## 1. Structure and density

- **Home, depart, return.** Every song has a visual *home* look (usually
  verse 1), departs from it, and returns. The last chorus is the furthest
  departure. [ALL]
- **One cue per structural section, one big idea per section.** A section
  owns one headline change — a colour, a movement, a layer arriving. The
  rest is support. Do not emphasise every word in the sentence. [ALL]
- **Sub-cues only where the music lifts**: last 2–4 bars of a pre, a bridge
  tail, a breakdown riser. 0–2 per section, none in verse 1. [PRE][BR][BD]
- **Density**: 8–14 section cues + 4–10 lifts for a 3–4 minute song; more
  than ~25 non-accent cues reads as busy *(default)*. [ALL]
- **Fades in beats**: into V1 / V→PRE 2–4b; **into a chorus or drop: snap or
  ≤ ½b** — a chorus that arrives two seconds late is the single biggest
  tell of a generated show; CH→V2 4–8b so the come-down breathes; a build
  is one continuous fade the length of the build (8–16b); outro 8–16b or
  snap on the button. [ALL]
- **Snap when the music snaps, fade when it evolves.** [ALL]
- **Pre-empt the downbeat.** Pros press GO on the "and of 4". Fire section
  cues ε ≈ ⅛–¼ beat early so the look *lands* on the 1. [ALL]
- **Energy curve and headroom**: CH1 ≤ 0.7, CH2 ≤ 0.85, **last chorus = 1.0
  and nothing else is** *(default)*. Hold one thing back from CH1 (audience
  beams, strobe, blinders, full white, or the fastest chase) and spend it in
  the last chorus. The cue right before the last chorus is the darkest since
  the intro. Energy never sags inside a chorus. [CH][CH-last][BR][BD]
- **What changes at each boundary**: INTRO establishes home colour, low
  level, movers on walls/ceiling, no audience light, no key until vocal.
  V1: key on, solid colour, static movers. PRE: one added layer or a level
  build, *no* colour change until the chorus. CH: colour flips to the chorus
  colour, level up ≥ 0.2, a tempo-locked effect appears, movers snap to a new
  focus. V2: home plus *one* thing kept from CH. BR: the third colour or a
  mood change, fewest fixtures, one slow movement, texture. BD: everything
  off but one texture, effects off, build over its length. CH-last: all on,
  audience light, the one strobe/blinder moment. OUT: home decaying; the last
  hit gets the button.

## 2. Colour

- **Two contrasting hues + white per song**; a third hue only in the bridge
  or breakdown. Complementary pairs (red/cyan, blue/amber, green/magenta)
  for maximal section contrast; analogous pairs for songs that never lift.
- **The chorus owns its colour**: every chorus uses it and it appears nowhere
  else except the last 1–2 bars of a pre-chorus build. Verse ≠ chorus hue.
- **Warm/cool** is decided per song (cool verses / warm choruses, or the
  reverse for intimate verses), never per section.
- **Saturation**: deep at low levels (verse, bridge) reads rich; desaturate
  toward white at peaks — peak looks contain ≥ 1 white/open layer.
- **Faces**: key on whenever there is a vocal, 0.5–0.8 (never full — a
  camera clips above), white or near-white at one colour temperature for the
  whole song, never saturated, never accented or chased. Colour lives in
  back/side/cyc. Always some backlight when key is on (separation).
- **No muddy mixes**: an RGB crossfade between non-adjacent hues passes
  through mud. A colour change ≥ 2b on the same fixture goes via black or a
  single-primary path; section snaps are fine.
- **≤ 2 hues per zone per cue.** Two-tone/spread only on ≥ 4-head roles and
  only in choruses; solid hues in verses/bridges. No rainbow anywhere in
  V/BR, and never longer than 4 bars.

## 3. Movement

- **Hold in verses, move on phrase boundaries, run effects in choruses and
  drops.** Movement should point somewhere, not wander. Movers run an effect
  in ≤ 60 % of the song's bars.
- **Aim**: INTRO ceiling/back wall/flyout; V band/stage, tight, static; PRE a
  slow open (zoom out / fan) across the build; CH out to the audience or
  crossed fans, symmetric; BR one asymmetric position, slow; CH-last audience
  + the fastest movement; OUT back to the intro's aim.
- **One sustained movement effect at a time** per role; layer a position
  effect with an *intensity* effect, never with a second position effect.
- **Period ≥ 2 bars** in V/BR/INTRO/OUT, ≥ 1 bar in CH, ≥ ½ bar only in the
  last chorus/drop — and never faster than the fixture can follow.
- **Move in black**: a position change on a fixture coming up from 0 is set
  in the previous cue at intensity 0 (or flagged).
- **Position changes on phrase starts** (bar ≡ 1 mod 4) or on a charted hit,
  never mid-phrase. Symmetry by default; asymmetry as a bridge device.

## 4. Intensity and effects

- **Levels** *(default)*: INTRO 0.2–0.4, V1 0.4–0.55, PRE 0.55–0.7, CH
  0.7–0.85, BR 0.3–0.5, BD 0.1–0.4, CH-last 0.9–1.0, OUT decaying from ≤ 0.6.
  Song-wide range between the dimmest vocal section and the peak ≥ 0.5.
  **Contrast over brightness.**
- **Effect rate is a beat subdivision, never Hz**: cycles of 4b, 2b, 1b, ½b
  only. V/BR ≥ 2b; CH 1–2b; ½b only at the peak.
- **Which effects where**: breathe/offset breathe (2–4 bars, 20–40 % depth)
  in V/BR; sparkle at low duty in PRE/OUT/ballad choruses; chases, can-can,
  ballyhoo in CH/CH-last; a *dark* chase on a full stage (motion without
  brightness); colour chase between the two palette hues in BD/post-chorus;
  gobos/texture in BR and intros.
- **Strobe and blinders are peaks only**: ≤ 4 bars per burst, ≤ 2 bursts per
  song, last chorus / drop / button; ≤ 4 Hz for continuous audience-facing
  strobe; blinders ≤ 2 bars, ≤ 3 hits, fade out ≥ 1b; all strobes on one
  clock.
- **Flicker fatigue**: no ≤ 1b intensity effect runs more than 16 bars on
  one role without ≥ 4 bars static.

## 5. Accents and hits

- **Hit the downbeat, not every hit.** Phrase-start downbeats, stops, fills
  into a chorus, the button. Density ≤ 1 per bar in CH, ≤ 1 per 2 bars in V,
  none during a breakdown build until the drop.
- **Kick → floor/back pulse (low, warm); snare → front/side flash or strobe
  burst.** Never both on the same role.
- **Bump vs cutout**: bump adds (white flash on a role); cutout subtracts
  (everything but key to 0 for ½–1b). Cutouts on stops and rests, bumps on
  hits; on a stage at ≥ 0.85 prefer cutouts — a bump on a full stage is
  invisible.
- **Decay**: bump attack 0, fall 1–2b for a hit, ¼–½b for eighth-note stabs;
  cutout snaps out and fades back over ½–1b. Figures hold moment to moment
  and the last moment falls.
- **Accents ride a separate, higher layer** with their own decay; they never
  restart the effect underneath. Keys are never accented.

## 6. Transitions

- Snap up, fade down. The chorus arrives on the 1; the verse settles over a
  bar.
- **Blackouts are rare, deliberate and short**: 1–2 beats on a stop, ≤ 2 s
  before a reveal, always followed by a big cue. Never black by accident —
  every busker keeps a *punt* look so the stage cannot go dark.
- **Intros from black**: the first cue fires *with* the first note (≤ 1b for
  a downbeat start, 4–8b for a pad); key arrives with the first vocal.
- **The button**: snap to the peak look (or to black), hold 2–4b while the
  sound decays, then black or an 8–16b fade; movers park, backs decay last.
- **Repeats**: CH2 = CH1 + one addition, not a new look. Corresponding
  musical moments get corresponding looks.
- Ship a **`safe`** (punt) cue and a final **`reset`** cue (effects off,
  stage lit).

## 7. Video walls

- Wall content sits inside the song palette (or neutral) — otherwise it is a
  third, uncontrolled colour source. Wall level ≤ stage peak in verses; drop
  it 20–30 % under vocals and raise it for the last chorus. Faces stay in
  their band regardless; add backlight when the wall goes bright behind the
  singer. White content matches the key's colour temperature. If the wall is
  doing tempo motion, movers hold, and vice versa.

## 8. Portability and busking

- The portable unit is **role × attribute**: colour per system, focus per
  system, intensity per system, effect per system with a speed master.
  Presets by reference, never raw values. On faders: key level, the punt
  look, effect speed/size, an audience-light inhibit. In cues: section looks
  and lifts. Every look degrades gracefully when a role is missing.

## Lint checklist — machine-checkable rules for a generated list

Inputs: sections with kind, BPM, per-cue per-role `level` (0–1), `palette`,
`effects[]` with `period_beats`/`fade_beats`, triggers, `energy` (0–1).

**Structure**
1. Every section has ≥ 1 cue; none has > 4 non-accent cues.
2. ≤ 25 non-accent cues for a song ≤ 4:30.
3. Section-entry cues fire ε ∈ [⅛b, ¼b] early.
4. Fade into CH/drop ≤ 1b; fade CH→V ∈ [4b, 8b].
5. Adjacent sections of different kind differ in ≥ 2 of {dominant hue,
   level (Δ ≥ 0.2), active layer count, movement state}.
6. Each chorus ⊇ the previous chorus (same hue, level ≥, layers ⊇).
7. energy(CH1) ≤ 0.7, energy(CH2) ≤ 0.85, energy(CH-last) is the song max and
   no other cue is within 0.05 of it.
8. The cue before CH-last has energy ≤ min over verse cues.
9. Song-wide vocal-section level range ≥ 0.5.
10. A `safe` cue and a final `reset` cue exist.

**Colour**
11. ≤ 3 hues + white; the third only in BR/BD.
12. Chorus hue ≠ verse hue; chorus hue in no verse cue (last 2 bars of PRE
    excepted).
13. Key is white/near-white at one colour temperature all song; never
    saturated.
14. ≤ 2 hues per role per cue.
15. Same-fixture hue changes with fade ≥ 2b go via black or a single-primary
    path.
16. No rainbow/hue-cycle in V/BR; none > 4 bars anywhere.
17. Peak cues contain ≥ 1 white/open layer.

**Faces**
18. `Key ≥ 0.5` in every vocal section; `Key ≤ 0.8` when a camera profile is
    set.
19. Key is never a target of accents, strobe or intensity effects.
20. Back > 0 whenever Key > 0.

**Movement**
21. ≤ 1 sustained position effect per cue; ≤ 1 position + 1 intensity
    effect per role.
22. Position-effect period ≥ 2 bars in V/BR/INTRO/OUT, ≥ 1 bar in CH, ≥ ½
    bar only in CH-last/drop.
23. A position change on a fixture coming from 0 is pre-set in the prior cue
    or flagged.
24. Position changes only at phrase starts or on a charted accent.
25. Movers run a movement effect in ≤ 60 % of bars.

**Effects / accents**
26. Every effect period ∈ {4b, 2b, 1b, ½b}.
27. No ≤ 1b intensity effect > 16 consecutive bars on one role without ≥ 4
    bars static.
28. Strobe only in CH-last/drop/button; ≤ 4 bars per burst; ≤ 2 bursts;
    ≤ 4 Hz continuous when audience-facing; one clock.
29. Blinders ≤ 2 bars per hit, ≤ 3 hits, fade-out ≥ 1b.
30. Accent density ≤ 1/bar in CH, ≤ 1 per 2 bars in V, 0 in a BD build; decay
    ∈ [¼b, 2b]; no accent on a role running a ½b intensity effect; on a
    stage ≥ 0.85 accents are cutouts.
31. All-roles-zero cues only if tagged `blackout`, ≤ 2 bars, followed by a cue
    with energy ≥ 0.7.
32. Wall hue ∈ palette ∪ {neutral}; wall level ≤ stage peak in V; white
    content matches key colour temperature.

## Sources

Mark LaPierre, *Lighting Music Basics 3/4* and the Eos busking template
(mlp-lighting.com); Brad Schiller, *Busking* (Lighting & Sound America, Dec
2024); Mike Graham / Chauvet, *How to master the art of busking*; Rob
Sinclair interview (12songsproject.com); Church Production — *Moving Light
Programming*, *Cueing vs Busking*, *Video and Lighting*, *Lighting mistakes
killing your videos*; Church Stage Design Ideas — *6 common stage lighting
mistakes*, *Colour theory and emotion*; ETC blog *Stage Lighting Design part
6* and *Out-of-control cue transitions*; On Stage Lighting — *Colour mixing
crossfades*, *Cue timing*; ETC Eos help *Beats Per Minute*; Hog 4 help *Mark
Cues*; LimeLightWired — *Programming timecoded shows*, *Introduction to
effects*; Ticket Fairy — *Strobe audits* and *Timecode vs busking*; Vello
Light *Programming chases*; SHEHDS *Live band stage lighting design*.
