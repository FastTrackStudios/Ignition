---
title: The song
type: concept
order: 5
stage: The song
blurb: Where cues come from when the desk can read the session the band is playing to.
---

# The song

This is the part no other console can do, so it is the part worth the most
explaining.

## The song map

A song arrives from the DAW project as a **song map**: sections with lengths
in bars, and a tempo map. Everything downstream is positioned against it. A
[[recipes-cues-effects|cue]] is "at the top of CH 1", or "4 bars into VS 2" —
never "at 2 minutes 14".

Positions being musical is not a convenience. It is what lets an arrangement
change without invalidating the show: cut eight bars out of the bridge and
every cue after it is still in the right place, because none of them was
measured from the start of the tape.

## The chart

The `HITS` track in the session is a chart of what the band is actually
playing, and it reads in three registers:

- **Kick** and **Snare** notes become pulses.
- **Low**, **Medium** and **High** notes become hits — intensity tiers, not
  instruments. A "High" hit is a big one, whatever hit it.
- A **Connected** note spanning several hits becomes a **figure**: a phrase,
  whose **moments** land on **zones** — thirds of the stage. Two or three
  moments make a cutout; more make a bump run.

So the chart is not a MIDI-to-DMX map. It is a description of the
arrangement's shape, and the show that comes out of it is written in the same
terms a lighting designer would use watching the band.

## What that buys

The generator writes a first pass: section looks, sustained
[[recipes-cues-effects|effects]] that run under them, hits on the charted
accents, figures across the zones. It is a draft — a real designer moves
things, and the file it produced is an ordinary show file that can be edited
by hand. Regeneration is non-destructive: your edits survive the song being
re-generated around them.

## Transport

The desk follows whatever clock is available: the DAW directly, MTC, Art-Net
timecode, LTC off an audio input, or a tap-tempo clock when there is no
timecode at all. Whichever it is becomes the `Song` speed master, so every
[[recipes-cues-effects|effect]] in the show is locked to it — which is the
claim [[what-ignition-is|the whole project rests on]].

---

Previous: [[recipes-cues-effects|Recipes, cues and effects]] · Next: [[running-it|Running it]] · Up: [[a-show-end-to-end|A Show, End to End]]
