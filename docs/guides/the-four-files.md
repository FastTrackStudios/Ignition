---
title: The four files
type: concept
order: 2
stage: The model
blurb: Profile, venue, show and night — and the line between them that makes a show portable.
---

# The four files

Everything in Ignition is one of four documents. The split is the whole
architecture, so it is worth learning before anything else.

| File | Is | Holds |
|---|---|---|
| `Rockstars.ig-profile` | the interface | the roles a rig must provide, colour presets, named tricks, the effect library |
| `Norco.ig-venue` | the implementation | this room's fixtures, patch, geometry, screens, and the bindings from role to selection |
| `Bye Bye Bye.ignition` | a song's show | cues, recipes, triggers — written against the profile |
| `Sunday 14th.ig-show` | the night | a profile, a venue, and an ordered list of songs |

Read the first two as an interface and its implementation, because that is
exactly what they are. The profile declares `Key`, `Wash`, `Movers`, a focus
[[roles-selections-tricks|role]] called `Vocal`, a canvas called `Main`. The
venue says what each of those *is* in this building: which fixtures, at which
point in space, at which address.

## Why a show may not name a fixture

Because if it could, it would only ever play in one room.

A show reaches the rig through roles and nothing else — never a fixture, never
a channel, never a universe, never a coordinate. That is a rule the checker
enforces statically, in both directions: show against profile ("every role you
use is declared"), and venue against profile ("every role you must provide is
bound"). `igcheck` runs it.

The payoff is that the show file you wrote for a room with eight movers plays
in a room with four, and in a room with sixteen, with no re-authoring —
because what you wrote was "the movers", and the venue is what knows how many
there are.

## The fifth file

`Bye Bye Bye@Norco.ig-local` is the escape hatch, and it is deliberately a
*separate file*: the sanctioned place for one room's overrides to one song.
Anything you would be tempted to hard-code into the show goes here instead,
and travels no further than the venue it belongs to — which is
[[what-ignition-is|the whole reason the line is drawn]] where it is.

## Fixture profiles come from GDTF

Fixture types are GDTF — the industry's format, with a geometry tree and named
attributes rather than a channel offset table. Everything upstream (palettes,
[[recipes-cues-effects|recipes and effects]], the visualizer) programs against
attributes like `Dimmer`, `Pan`, `ColorAdd { Red }` and `ColorWheel { slot }`;
the patch resolves attribute to bytes at output time, per fixture, per mode.

This is not academic. The Norco rig has eight movers with colour *wheels*
sitting in a library that is otherwise all RGB LED wash. A colour effect
targeting "everything with a colour attribute" has to do something different
on each — which it can, because the two are different attribute variants and
not two spellings of one.

---

Previous: [[what-ignition-is|What Ignition is]] · Next: [[roles-selections-tricks|Roles, selections and tricks]] · Up: [[a-show-end-to-end|A Show, End to End]]
