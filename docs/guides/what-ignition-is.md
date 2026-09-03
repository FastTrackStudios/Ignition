---
title: What Ignition is
type: concept
order: 1
stage: Start here
blurb: A console, a visualizer and a video mapper in one application — and why that is one application.
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
