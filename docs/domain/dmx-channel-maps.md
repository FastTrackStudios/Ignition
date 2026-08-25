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
| Uking Par | 7ch | **Fully confirmed** — Open Fixture Library `uking/par-light-b262.json` | Real layout: Dimmer, R, G, B, Strobe, Mode, Hue Selection/Speed — **no White channel** (an earlier estimate had guessed one; corrected 2026-08-24, see Slice 8) |
| Chauvet SlimPAR Tri 7 IRC 7ch | 7ch | Footprint confirmed (name + chan 50→51 spacing) | Layout estimated: Dimmer, R, G, B, Strobe (macro/speed channels not modelled). No OFL/GDTF profile found for this exact model. |
| Riukoe Mini Gobo Moving Head 11ch | 11ch | Footprint confirmed (name + chan 80→81 spacing) | Layout estimated: Pan, Tilt, Colour wheel, Gobo wheel, Shutter, Dimmer. **Riukoe has no manufacturer entry in OFL at all** — checked directly, not just unsearched. |
| Betopper 150W LED Beam Moving Head | 12ch | Footprint confirmed (sorted-address spacing) | Layout estimated: Pan, Tilt, Dimmer, Strobe, Colour wheel, Gobo wheel. **Betopper has no manufacturer entry in OFL at all** — checked directly. |
| Rockville Rockstrip 252 7ch | 7ch | Footprint confirmed (name) | Layout estimated, **flagged suspect**: used the same "generic 7ch par = Dimmer/R/G/B/White/Strobe/Program" template the Uking Par entry did — and that template turned out wrong when checked against a real profile. OFL only has "rockpar50" for Rockville, not this model, so this one can't be checked the same way. |
| Rockville Rockstrip 252 3ch | 3ch | Footprint confirmed (name) | Layout estimated: bare R, G, B, no dimmer channel |
| Chauvet Hurricane Haze 1DX | 1ch | **Fully confirmed** — OFL `chauvet-dj/hurricane-haze-1dx.json` | Real layout: a single Haze channel, no separate fan-speed channel. Corrected 2026-08-24 from an earlier 2ch guess (see Slice 8). |

## What to do next time at the rig

Cycle each fixture type's dimmer/color/pan/tilt from the console one
channel at a time and watch which physical channel does what — the
fastest way to convert an "estimated" row above to "confirmed," and the
only real option left for Riukoe/Betopper (no-name brands with no entry
in either OFL or GDTF-land) and Rockville's actual Rockstrip 252 (OFL only
has a different Rockville model).

For Chauvet SlimPAR: worth one more direct check — gdtf-share.com or
Chauvet's own site might have a real GDTF/OFL profile for the exact
"Tri 7 IRC" variant even though the generic search here didn't find one;
`gdtf_import.rs` (Slice 4) is ready to pull it in the moment a `.gdtf`
file for it turns up. The Open Fixture Library route (Slice 8) is
generally the faster path to try first — plain JSON via `serde_json`,
no zip/XML pipeline, and it's what actually resolved the Uking Par and
Hurricane Haze corrections above.
