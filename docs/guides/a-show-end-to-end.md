---
title: A Show, End to End
type: guide
order: 0
stage: Start here
blurb: The front door — every part of Ignition, and the order they happen in.
---

# A Show, End to End

This guide is a vault, not a manual. Each page is one concept, the pages link
to each other, and you can read them in order or follow the links. There is a
[graph](/guide/graph) of how they connect, if you would rather see the shape
before you read the words.

Start with [[what-ignition-is|What Ignition is]] if you have not met the
project. Otherwise, here is the whole thing in the order it happens.

## Before the show

Someone builds a **venue** — this room's fixtures, where they hang, what they
are patched to. It implements a **profile**, which is the list of roles a rig
has to provide. Those are two of [[the-four-files|the four files]], and the
line between them is what lets a show travel.

## Writing the show

A designer programs in [[roles-selections-tricks|roles and selections]], never
in channel numbers, and states values as [[recipes-cues-effects|recipes]] —
templates that are cooked against whatever is actually in the rig. Cues state
only what changes and track forward from each other.

Or the show writes its first draft itself, from [[the-song|the song]]: the DAW
session's sections, tempo map and hit chart become section looks, sustained
effects and accents on the bar line.

## Running the show

The player is never told to fire a cue. It is asked what the state is at a
bar, and it rebuilds — so you can drop the playhead anywhere and the rig is
right. Sources are ranked by kind, hits beat chases, and nothing merges at the
DMX layer.

## Seeing it

The visualizer is the room in 3-D — beams in real haze, GDTF meshes, gobos,
the venue's screens playing their content. It is where you focus fixtures, and
it will render a show to a video file frame by frame against the song's clock.
That is what the clip on the front page is.

[[running-it|Running it]] is how to build and drive all of the above.