# DMX channel maps

Where each fixture-type channel layout in `crates/ignition-viz/src/
channel_map.rs` came from, and how confident it is. See
`docs/research/lighting-console-landscape.md` for the architecture this is
part of (why a channel-map layer sits between `fixtures.json`'s static
mount pose and the live sACN/Art-Net state in `dmx.rs`).

## Two independent facts per fixture type

- **Footprint** (how many consecutive DMX channels one fixture occupies) —
  confirmed for every fixture below by checking the live Eos patch's own
  address spacing between consecutive fixtures of that type
  (`data/venues/norco/fixtures.json`'s `universe`/`address` fields, cross-
  referenced in `docs/domain/norco-patch-and-groups.md`). This is real data,
  not a guess.
- **Per-channel function order** (which byte is dimmer vs. red vs. pan...)
  — estimated from the general class of fixture (RGB(W) par, cheap moving
  head, LED batten, hazer), since Ignition has no real `.qxf`/GDTF fixture
  profile for any of these specific budget models yet. This is the part
  that needs a real fixture manual or DMX-cycling-and-watching-the-console
  session to actually confirm.

## Current fixtures

| Manufacturer/model | Footprint | Confidence | Notes |
|---|---|---|---|
| Uking Par | 7ch | Footprint confirmed (chan 1→3 spacing) | Layout estimated: Dimmer, R, G, B, White, Strobe, Program |
| Chauvet SlimPAR Tri 7 IRC 7ch | 7ch | Footprint confirmed (name + chan 50→51 spacing) | Layout estimated: Dimmer, R, G, B, Strobe (macro/speed channels not modelled) |
| Riukoe Mini Gobo Moving Head 11ch | 11ch | Footprint confirmed (name + chan 80→81 spacing) | Layout estimated: Pan, Tilt, Colour wheel, Gobo wheel, Shutter, Dimmer (fine-resolution pan/tilt bytes reserved in the footprint but not read) |
| Betopper 150W LED Beam Moving Head | 12ch | Footprint confirmed (sorted-address spacing) | Layout estimated: Pan, Tilt, Dimmer, Strobe, Colour wheel, Gobo wheel |
| Rockville Rockstrip 252 7ch | 7ch | Footprint confirmed (name) | Layout estimated: Dimmer, R, G, B, White, Strobe, Program |
| Rockville Rockstrip 252 3ch | 3ch | Footprint confirmed (name) | Layout estimated: bare R, G, B, no dimmer channel |
| Chauvet Hurricane Haze 1DX | 2ch | Not confirmed (only 2 units, different universes, no spacing to check) | Estimated from Chauvet's documented 2ch mode: Haze output (mapped to `Attribute::Dimmer`), Fan speed |

## What to do next time at the rig

Cycle each fixture type's dimmer/color/pan/tilt from the console one
channel at a time and watch which physical channel does what — the
fastest way to convert an "estimated" row above to "confirmed." Or, once
Ignition has a `.qxf`/GDTF importer (see the GDTF/MVR slice in
`docs/research/lighting-console-landscape.md`), pull the real manufacturer
fixture profile for each of these and replace the hand-authored entry in
`channel_map.rs` outright.
