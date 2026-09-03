---
title: Recipes, cues and effects
type: concept
order: 4
stage: The model
blurb: A recipe is a template that is cooked against the live rig; a cue is what changes.
---

# Recipes, cues and effects

## A recipe is a template

A **recipe** is a [[roles-selections-tricks|selection]], some steps, timing,
and [[roles-selections-tricks|tricks]]. It is not a list of fixtures and
values — it is a template, and it is **cooked** against whatever is actually
in the rig, every frame.

That is what makes a show survive a rig change. Add a ninth mover to the floor
group and every recipe that selects that group now covers it, with nothing
re-authored. Cooking establishes *coverage* — which attributes on which
fixtures — and reports per cue whether it came out `ok`, `empty`, or `mixed`.
An `empty` cue is a cue that will do nothing in this room, and you find that
out before the room is full.

One step is a look. Two or more steps is an effect.

## Absolute and relative are different layers

`Dimmer`, `Color`, `FocusPoint` and `Raw` **set** a value. `Delta`
**modulates** whatever is underneath it. They live on separate layers, which
is why an effect laid over a look can move the intensity without touching the
colour the look established.

When several sources want the same attribute, the cascade runs highest first:

> direct value on the cue › recipe on the cue › preset direct › preset recipe

One absolute survives. One relative is added on top.

## Effects are phasers

An effect is a recipe with two or more steps. The steps carry values; the
*timing* is uniform across all of them:

- **speed** — in `Hz`, `Bpm`, `Secs`, or from a named **master**
- **measure** — beats per loop
- **phase** — spread across the selection
- **play mode** — `Forward`, `Reverse`, `Bounce`, `Build`, `Negative`
- **width**, **transition**, **ease** — including explicit accelerate/decelerate

The speed master is the part that matters here. `Song` is
[[the-song|the transport's tempo map]]. Point every effect at it and the whole
rig breathes with the band, and stays in phase with itself across cues,
because they share one clock.

## Cues track

A **cue** has a name, a fade, values, recipes, an optional `block`, and a
position in [[the-song|bars]]. It states **only what it changes**; everything
else tracks forward from the cue before it — and because what tracks is the
*source*, an inherited chase keeps moving rather than freezing on the frame it
was inherited at.

`block` starts a cue from empty layers. Section cues block, so sections can
reorder — a verse can follow either chorus and still look right. Accent cues
(the ones named with a leading `·`) do not block; they are decorations on the
section they sit inside.

## Seeking, not firing

The player is never told "fire cue 9". It is asked **"what is the state at bar
43?"** — and it rebuilds. Jump backwards and it rebuilds from the top of the
list; scrub and it reconstructs state without re-performing the transients
that happened along the way.

That is what makes rehearsing possible. Drop the playhead anywhere in the song
and the rig is correct, immediately, with no cue-stack bookkeeping.

## Triggers are the hits

A **trigger** is a moment [[the-song|the song]] fires: a position, a one-shot
recipe, and a name, living in the cue list beside the cues. Crossing the
position fires it once. Stopped, nothing fires. Seeking locates rather than
performs. Two triggers at the same instant sum.

The standard shape is a bump — `Level`, `White`, `ColorBoost`, `Burst`,
falling over about half a beat. A hit charted in the song's MIDI and a flash
key under an operator's finger produce the *same object*, which is why they
behave the same way against everything else running.

## The priority stack

Nothing merges at the DMX layer. Sources are ranked by kind, highest first:

1. the operator's hand
2. masters and solo (scaling only)
3. flashes
4. faders
5. triggers
6. the cue player — transient, then sustained, then absolute

A transient beats a sustained effect regardless of which took first. That one
rule is why a hit reads over a chase instead of fighting it.

---

Previous: [[roles-selections-tricks|Roles, selections and tricks]] · Next: [[the-song|The song]] · Up: [[a-show-end-to-end|A Show, End to End]]
