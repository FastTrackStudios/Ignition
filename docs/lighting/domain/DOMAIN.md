# Ignition domain model

Modelled the way ASLS Studio's `src/models/DMX/` is modelled — a small,
legible object graph — but taking grandMA3's architecture as the target
instead of ASLS's/QLC+'s absolute-scene model, per the gap analysis in
[`docs/research/lighting-console-landscape.md`](../research/lighting-console-landscape.md)
§3–4. Grounded throughout by the real Norco venue and rig, both from
[eos-toolkit](https://github.com/Codys-Wright/eos-toolkit)'s docs and from the
extracted show file in [`norco-venue-reference.md`](norco-venue-reference.md).

## The object graph

```
Show
 ├─ Patch                    chan → Fixture instance (type + placement + address)
 ├─ FixtureLibrary           imported GDTF/QLC/OFL profiles, one internal shape
 ├─ Groups                   named, ORDERED fixture selections
 ├─ Palettes                 referenced, single-category (Intensity/Focus/Color/Beam)
 ├─ Presets                  referenced, multi-category, no effects (see below)
 ├─ Recipes                  parametric selection+value templates (see below)
 ├─ Effects / Phasers        step lists with layered timing (see below)
 ├─ CueLists                 tracking cues, each an ordered list of Cues
 ├─ Screens                  physical video-mapping surfaces (see below)
 ├─ MappingLayers            video content routed onto Screens
 └─ Surfaces                 physical control surfaces (faders/buttons) bound
                              to Executors
```

This is deliberately closer to Eos/MA's shape than to ASLS's/QLC+'s. The
reasons are architectural, not aesthetic — see below.

### Fixture, Attribute, Placement — the abstraction layer

The single most important decision in the project (landscape doc §1.3, §3).
A fixture is:

```rust
Fixture {
    chan: ChanId,
    fixture_type: FixtureTypeRef,   // -> FixtureLibrary entry (GDTF-shaped)
    placement: Placement,           // world position + mounting orientation
    address: DmxAddress,            // universe + start channel
}
```

`FixtureType` carries a **geometry tree** (GDTF's model) and a set of
**attributes** — `Attribute::Dimmer`, `Attribute::Pan`, `Attribute::ColorAdd
{ Red }`, `Attribute::ColorWheel { slot }`, etc. (see
`crates/ignition-proto/src/lib.rs`). Everything upstream — palettes, presets,
recipes, effects, the visualizer — programs against attributes, never against
raw DMX channel offsets. The patch resolves attribute → bytes at output time,
per fixture, per mode.

This is not academic: the Norco rig has 8 moving heads with **colour wheels**,
not RGB mixing, sitting in a library that is otherwise all-RGB LED wash
(`norco-rig-facts.md`). Any model that lets a colour effect target "all
fixtures with a colour attribute" needs `ColorAdd` and `ColorWheel` to be
*different* attribute variants so the resolver can do the right thing per
fixture, rather than every colour effect silently no-oping on the movers the
way it does on the real console today (`effects-model.md`: "RGB colour... no —
movers have colour WHEELS, not mixing").

`Placement` is the **hang** — where the fixture is rigged and how — never the
**aim**. Live pan/tilt is attribute state, resolved at showtime; it is not
part of `Placement`. Conflating the two is a documented, already-happened bug
at this exact venue (see `norco-venue-reference.md`).

### Groups — ordered, not just sets

A `Group` is an **ordered** list of channels. Order is meaningless for
selection but is the entire content of a step-based effect: "channel order
comes from the group" (`effects-model.md`). One hand-authored 8-step chase
plus N differently-ordered groups produces N distinct chases. Ignition's
`Group` type should expose reordering as a first-class operation, not bury it
behind "edit the group, re-save."

### Palettes and Presets — referenced, never copied

This is gap #1 from the landscape doc. A `Palette` (Intensity/Focus/Color/
Beam — Eos's four categories, cleaner than MA's monolithic preset pools for a
volunteer operator) and a `Preset` are objects other things **point at**, not
values that get copied in. A cue or a recipe stores a palette/preset
*reference*; re-editing the palette moves every cue that uses it. This is the
mechanical difference between "usable for one show" and "usable for a
building."

Presets never carry effects — only Recipes and Cues do (see the container
table below, lifted directly from `building-a-show.md`, verified against the
real console):

| Container | Channels | Levels | Colour | Effects | Notes |
|---|---|---|---|---|---|
| Group | yes | — | — | — | order matters |
| Palette | yes | one category | | | |
| Preset | yes | all categories | yes | **no** | |
| Recipe | yes (template) | yes | yes | yes | parametric, see below |
| Cue | yes | yes | yes | yes | the show |

### Recipes — the generative layer

See [`docs/research/grandma3-recipes-and-phasers.md`](../research/grandma3-recipes-and-phasers.md)
for the full research. In this domain model, a `Recipe` is:

```rust
Recipe {
    selection: SelectionExpr,        // a group, a tag filter, or <From Value>
    values: Vec<(Attribute, ValueExpr)>,
    phaser: Option<PhaserSpec>,      // optional — a recipe with 2+ steps IS a phaser
}
```

A recipe is **stored into** a cue, preset, or the programmer, and is
**resolved ("cooked")** against whatever selection is live at resolve time —
it is a template, not a value. This is what makes it survive a rig change: add
a ninth mover to the Norco floor group and every recipe using that group's
selection now covers it, with no re-authoring. Resolution priority (direct
cue value > cue recipe > preset value > preset recipe) matters for predictable
behaviour when a cue overrides part of a preset it's built on.

This is also **the mechanism for §8's differentiator** in the landscape doc:
a `Recipe` whose `SelectionExpr` is "the group for tonight's mover rig" and
whose `values` are seeded from a song `Section`'s `color` and `section_type`
is exactly how "cues that write themselves" gets implemented, not a separate
bolt-on system.

### Effects / Phasers — layered, not just stepped

ASLS's effect model (waveform + freq/min/max/phase/direction-spread per
channel) is a reasonable *shape* for a step but the wrong *architecture* —
MA3's Phaser separates **step values** (absolute/relative, per attribute) from
**phaser-level layers** (speed, speed-master, phase offset, measure) that
apply uniformly across all steps. See the research doc for the full model.
Practically: Ignition's effect step table stores per-attribute
absolute-or-relative values; a phaser wraps 2+ steps with the shared timing
layers; a 1-step "phaser" degenerates to what Eos calls an Absolute effect.

Attribute directions (`Forward`/`Reverse`/`Bounce`/`Build`/`Negative`/
`RandomGroup`/`RandomRate`, `effects-model.md`) are properties of how a phaser
plays its step list, not separate effect types — six looks from one effect
definition, exactly as Eos does it.

### Cues and tracking

A `CueList` is an ordered list of `Cue`s. Each cue stores only what it
*changes*; unset attributes track forward from the previous cue on that
channel. `Block` marks a cue as self-contained (does not track from its
predecessor) — required for any structure where sections can reorder (song
sections, verse/chorus repeats), per `building-a-show.md`'s "blocks make acts
reorderable." `Assert`/cue-only and Mark (dark moves before the cue plays) are
Phase 3 per the landscape doc's roadmap, but the `Cue` type should have a
`block: bool` field from day one — retrofitting tracking onto absolute scenes
is exactly ASLS's and QLC+'s bind.

### Screens and MappingLayers — projection mapping as a first-class citizen

See [`docs/research/projection-mapping-resolume.md`](../research/projection-mapping-resolume.md).
A `Screen` is a physical output surface — the same `Placement` concept a
fixture uses (position + orientation in the room), plus a pixel size and an
output route (a window on a physical display output, or a capture-card feed
to a real TV). A `MappingLayer` routes composed video content onto one or more
screens, positioned/warped to match. The Norco venue's five TVs
(`norco-venue-reference.md`) are the concrete first target: they already have
real `Placement` data extracted from the show file, so a `Screen` is a
`Placement` plus `{width_px, height_px}` — no new positional concept needed.
The point of building this inside Ignition rather than beside it: a `Screen`
can be a member of a `Group` and referenced from a `Cue` exactly like a
`Fixture` can — "chorus 2 the TVs go white" is the same kind of cue data as
"chorus 2 the wash goes white."

### Surfaces and Executors

A physical control surface (fader bank, button grid — same territory as FTS's
`crates/input` + `features/surfaces/daw-csi`) binds physical controls to
`Executor`s, each pointing at a `CueList`, a `Recipe`/submaster, or a raw
`Group`+`Attribute` fader. This is the Norco rig's fader map
(`norco-rig-facts.md`: 37 faders, FX/white/front-mid-back/movers/cans/chases/
LCR/haze/blackout) reduced to data — Ignition's surface layer should be able
to *load* that exact map as a test case once `ignition-io`/surfaces work
starts.

## What's deliberately not modelled yet

- **Colour science / gamut mapping** — Eos's Colour Path. Needed once mixed
  RGB+wheel+CMY rigs need to match a single named colour; the Norco rig's
  wheel-only movers make this visible immediately, but the model (§ above)
  already keeps `ColorAdd`/`ColorWheel` distinct, which is the precondition.
- **RDM discovery** — Phase 3+.
- **GDTF geometry-tree sub-fixtures/pixels** — `FixtureType` above references
  "a geometry tree" but the tree itself isn't modelled until GDTF import
  starts (`crates/ignition-fixtures`, not yet created).
- **Multi-user conflict resolution** — deferred to whatever FTS's CRDT
  doc-sync layer already provides once Ignition takes `architect` as a
  dependency; not re-invented here.
