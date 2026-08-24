# Projection mapping: how Resolume does it, and the plan for Ignition

**Status**: research / decision input, feeding `docs/domain/DOMAIN.md`
("Screens and MappingLayers").
**Date**: 2026-08-24.
**Goal stated by the user**: "add projection mapping like in resolume to this
software, so that our visualizer has full video output, for things like TVs
which has content mapped to them, and then it can all be inside of the same
system." The Norco venue's five real TVs
([`docs/domain/norco-venue-reference.md`](../domain/norco-venue-reference.md))
are the concrete first target.

Sources: [Resolume — DMX/Lumiverse](https://www.resolume.com/support/en/dmx),
[Resolume — Advanced Output](https://www.resolume.com/support/en/advanced-output),
[Resolume — Output Transformation](https://resolume.com/support/en/output-transformation),
[Resolume forum — Advanced Output](https://resolume.com/forum/viewtopic.php?t=8854).

## What Resolume's model actually is

Resolume separates three layers cleanly, and the separation is the whole
design:

```
Composition        the video content — layers of clips, effects, mixed live
      |
Slices              physical/virtual outputs the composition is routed to —
      |              a DVI/HDMI run to a projector, an HDMI run to an LED
      |              screen, a capture-card feed, a DMX pixel run, Syphon/
      |              Spout — each with its own transform
      v
Physical reality    the actual projector/screen/fixture in the room
```

A **Slice** is one physical output plus a geometric transform onto it. Content
is authored once, in composition space; the slice decides where in the room it
lands and what shape it has to conform to. Multiple slices can read from the
same composition, cropped and warped differently — that's how one show maps
across N projectors, or a projector plus an LED screen plus a strip of DMX
pixels, without re-authoring content per output.

### Warping, concretely

Each slice's shape is edited as a **mesh** of points:

- **Perspective warp** — the four large corner points; moves the quad with
  perspective correction, for surfaces the camera/eye sees at an angle
  (exactly the Norco flare TVs, which sit angled into the room rather than
  flat on a wall — see below)
- **Linear warp** — four smaller points nested inside the perspective corners;
  adjusts without perspective correction, for fine local correction
- **Poly slice** — an arbitrary mesh, more points, no Bezier — for genuinely
  irregular surfaces (not needed for flat TVs, relevant later for anything
  curved or faceted)
- Per-slice **brightness/contrast/RGB** trim, multi-selectable, to balance
  mismatched displays showing the same content

### DMX pixel-mapping (the Lumiverse) — the other half of "full video output"

Separately, Resolume can turn video into DMX: a **Lumiverse** is a virtual set
of DMX universes inside the app. Pixel-fixture presets (LED bars, tubes, etc.)
are positioned in composition space; the app samples pixel colour under each
fixture's position and outputs it as DMX to a real Art-Net/sACN node. This is
the same idea as a Slice, just with a DMX sink instead of a video sink at the
end — and it's the piece that would let an Ignition `Screen` and a pixel-mapped
LED batten be the *same kind of routing decision*, differing only in the
output adapter.

## Where this maps onto Ignition's domain

Per `docs/domain/DOMAIN.md`'s "Screens and MappingLayers" section, treat a
`Screen` as a physical output surface with the same `Placement` (position +
orientation) concept a `Fixture` already has — which the Norco extract
confirms is sufficient: every TV in `screens.json` is fully described by
`position`, `eulers`, and `size`, no new positional primitive required.

```rust
Screen {
    name: String,
    placement: Placement,          // same type fixtures use
    pixel_size: (u32, u32),
    output: OutputRoute,           // physical display, capture-card feed, or
}                                  // a DMX pixel run via ignition-io

MappingLayer {
    composition_ref: CompositionId,   // what content
    slices: Vec<Slice>,               // where it lands, per screen
}

Slice {
    screen: ScreenId,
    mesh: WarpMesh,                   // corner points (perspective) or a
}                                     // full poly mesh
```

The Resolume-equivalent pieces:

| Resolume concept | Ignition equivalent |
|---|---|
| Composition (layers of clips) | `MappingLayer`'s content graph — out of scope for this doc, belongs to `ignition-video` |
| Slice | `Slice` — one screen + one warp mesh |
| Advanced Output routing | `OutputRoute` — which physical display/window/capture path a slice's rendered result goes to |
| Lumiverse (DMX pixel sampling) | An `OutputRoute::DmxPixelRun` variant on the same `Slice`/`Screen` concept — video and DMX are both just sinks |

### Why this belongs *inside* Ignition rather than beside it

The pitch in the README and the landscape doc is "no second application to
sync." Concretely, that buys three things a Resolume-plus-console rig can't
have:

1. **`Screen` is selectable like a `Fixture`.** A `Group` can contain both
   lighting channels and screens; a `Cue`/`Recipe` can set "wash → warm amber,
   `TV - Flare SL`/`SR` → a matching gradient" as one atomic write, because
   they're both attributes on members of one selection — not a lighting cue
   plus an OSC message to a second app hoping the timing lines up.
2. **Song-structure-driven generation (landscape doc §8) covers video too.**
   A `Recipe` seeded from `Section.color`/`section_type` can target a screen's
   background-colour or content-select attribute exactly the way it targets a
   wash fixture's colour — "chorus 2 gets the bigger content cue" is the same
   mechanism as "chorus 2 gets the bigger lighting cue."
3. **One patch, one show file.** The Norco venue's five TVs already have real
   `Placement` data sitting next to its 69 lighting fixtures in one extracted
   show — there is no reason for that same room to be described twice, once in
   an Eos-shaped file and again in a `.avc` Resolume composition.

### The Norco TVs as the concrete test case

From `screens.json` (see the venue reference doc for the full table): three
TVs sit in a row on the upstage wall (flat, roughly parallel to the audience
— straightforward **perspective-warp slices**, likely close to a rectangular
quad each), and two more sit on the stage-side flares, angled into the room
(`TV - Flare SL`/`SR`, rotated — these are the case that actually needs
perspective correction, not just placement, because the camera/eye sees them
at an angle). That mix — some near-flat, some genuinely angled — makes this a
good first real-world test of the warp-mesh model rather than a trivial one.

### Rendering path

This is downstream of the open question in the landscape doc §7.1 (how a wgpu
surface composes with Dioxus/Blitz). A `Screen`'s rendered output is, at
minimum, a **render-to-texture pass**: render the mapped composition/layer
into an offscreen texture sized to the slice's warp mesh, then composite that
texture into the main 3D visualizer scene at the screen's `Placement` (so the
operator sees, in the 3D preview, what's actually showing on the physical TV)
**and** simultaneously route it to the screen's real `OutputRoute` — a second
window/display surface for a real HDMI-connected TV, or a DMX pixel run for an
LED surface. Both consumers read the same rendered texture; only the sink
differs. This reuses whatever solution §7.1 lands on for getting a wgpu
surface into the visualizer in the first place — it is not a separate
rendering problem, it's the same one applied twice.

## What this changes in the roadmap

Landscape doc §9 didn't scope video/projection mapping — this is new scope,
not a reprioritisation. Suggested placement: **fold into Phase 2** (the
differentiator phase) rather than bolting it on later, specifically because
the payoff (`Screen` as a selectable member alongside `Fixture`, driven by the
same `Recipe`/`Cue` machinery) only exists if the domain model treats them
uniformly from early on — retrofitting it after `ignition-core`'s `Cue`/
`Group`/`Recipe` types assume "channel = light" would mean a second class of
special-cased code, the exact trap the landscape doc's §3 warns about (an
architectural decision made wrong is the expensive kind of mistake). Concretely:

- **Phase 0 spike**: add loading `screens.json` alongside `fixtures.json`
  (already done — see `apps/ignition-engine`) and render each screen as a flat
  quad at its `Placement` in the same wgpu scene as the beam cones. No warp
  mesh, no video yet — just "the room's screens exist as objects the
  visualizer knows about."
- **Phase 1 MVP**: `Screen` is patchable like a `Fixture` (shows up in Groups,
  the patch panel); `ignition-video` doesn't exist yet, so a screen's content
  is a placeholder colour/gradient attribute only.
- **Phase 2 differentiator**: real `MappingLayer`/`Slice`/warp-mesh editor;
  `ignition-video` gains media decode/playback; `Recipe`s can target screen
  content attributes from song sections, same mechanism as lighting.
- **Phase 3 console credibility**: DMX pixel-run output route (the
  Lumiverse-equivalent), for LED surfaces that aren't a literal screen.
