---
title: What Ignition is
type: concept
order: 1
stage: Start here
blurb: One library of looks, effects and tricks that plays in any room, in a console, a visualizer and a video mapper that are one application.
---

# What Ignition is

Ignition is a lighting-and-visual control system: a DMX console that speaks
sACN and Art-Net, a GPU-accelerated 3D visualizer, and video/projection
mapping, in one Rust application that runs on the desktop and in a browser
from the same codebase.

It exists because of a gap nobody else can close. FastTrackStudio — the
sibling project — already owns the *musical* structure of a show: song
sections, a tempo map, chord and hit detection, setlists. No lighting console
on the market has access to any of that. Ignition's long bet is cues that fall
out of the song, because the desk and the band are reading the same session.

That bet only pays if the console underneath it is a good console first. So
Ignition is built to the shape of grandMA3 rather than to the shape of a scene
list: referenced palettes, [[roles-selections-tricks|ordered selections]],
tracking cues, and
[[recipes-cues-effects|parametric recipes and phaser-style effects]].

## The goal: one library, any room

Ignition is built around **one library that adapts to any rig and any
venue**, and around people and agents who know that library well enough to
program and busk with it by name.

- **One library.** Colours, palettes, splits, effects (mover patterns,
  intensity, colour, beam, strip and matrix families, one-shots), tricks,
  looks, bundles and the busk pages ship once, in the profile
  (the profile). A set or a song adds to it or
  overrides an entry; it never starts its own.
- **Any room.** Nothing in the library, or in a show written with it, names a
  fixture, an address or a coordinate. It names **roles**, the jobs a rig
  plays (`Key`, `Wash`, `Back`, `Movers`). A venue says which of its fixtures
  play each role, so the same show and the same busking hands work on two
  light trees in a classroom and on a touring rig.
- **Fluency.** A show is programmed by reaching for the library first: an
  effect by name, a trick preset on it, a palette slot for its colour. A value
  typed by hand into one song is the exception, and a good one is promoted
  into the library. The same holds live: the pages a person busks from are
  the library's, so what they learn in one room is what they play in the next.

What follows from it: a change that makes a show depend on one room, or adds
a second place where effects or colours live, works against the goal; a
change that makes the library richer, easier to learn or easier to bind to a
new room works for it.

The three claims below are what that goal rests on.

## The three claims

**Portability.** A show never names a fixture, a channel, a universe or a
coordinate. It names [[roles-selections-tricks|*roles*]] — `Key`, `Wash`,
`Movers` — and the venue binds each role to something in the room, which is
what [[the-four-files|the four files]] are for. The same show file plays in a
different building because there was never anything building-specific in it.

**The visualizer is a tool, not a demo.** Beams in real volumetric haze, GDTF
meshes for the actual fixtures, gobo projection, the venue's video screens
playing their actual content. It is the thing you point-and-click focus
fixtures in, the way Augment3d and MA 3D are used — and it is what the video
on the front page is: a real offline render of a real show file, frame by
frame against the song's clock.

**Musical time.** Positions are bars and beats, never seconds. A cue lands "4
bars into CH 1"; a hit lands on `27.1.00`. Speed up [[the-song|the song]] and
the chase speeds up with it, because the effect's clock *is* the transport's
tempo map.

## Where it is

Pre-alpha, in the honest sense: it runs, it is not done. The domain model, the
file formats, the cue engine, the effects library, the visualizer and DMX
output are built; the specs in `docs/spec/` are the source of truth and carry
requirement ids that the code and the tests both cite —
[[running-it|Running it]] says where to start reading.

---

Next: [[the-four-files|The four files]] · Up: [[a-show-end-to-end|A Show, End to End]]
