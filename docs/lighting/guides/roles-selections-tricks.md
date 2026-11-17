---
title: Roles, selections and tricks
type: concept
order: 3
stage: The model
blurb: How you say which fixtures — without ever saying which fixtures.
---

# Roles, selections and tricks

## Role

A **role** is a job a rig plays. `Key` is whatever lights the faces. `Wash` is
whatever covers the stage. `Movers` is whatever moves. Focus roles (`Vocal`,
`Drums`) name a *place* something is aimed at; canvas roles (`Main`) name a
video surface.

The [[the-four-files|profile]] declares the roles. The venue binds each one. A
show only ever speaks in roles.

## Selection

A **selection** is an ordered expression that resolves to fixtures:

- `Role("Movers")`, `Group("Floor")`, `Tag("upstage")`, `Model("SlimPAR")`
- set algebra over any of those — union, intersection, difference
- `Where(…)` — a spatial predicate, including `Covers`, which asks where the
  beam actually *lands* rather than where the fixture is hung
- `Order(…)` — by axis, or by distance from a point

**Order is not decoration.** Order is meaningless for "turn these on" and is
the entire content of a [[recipes-cues-effects|chase]]: one hand-authored
eight-step chase plus four differently-ordered selections is four distinct
chases. Which fixture is "first" is a question only the selection's order
answers.

## Trick

A **trick** sub-selects or spreads across a selection. These are grandMA3's
MAtricks, and the profile ships twenty-four named sets of them.

| Trick | Does |
|---|---|
| `Block(n)`, `Group(n)` | take every nth, in blocks or interleaved |
| `Wings(n)` | fold the selection into n symmetrical wings |
| `Mirror` | pair it with itself, reversed |
| `Shuffle(seed)` | a deterministic scramble |
| `Shift(n)`, `Reverse` | rotate, or turn round |
| `Fan { from, to }` | spread a *value* across the selection rather than sub-selecting it |
| `OnAxis(axis, trick)` | apply a trick along one axis of the rig's real 3-D grid |

Every trick returns a selection, so they compose. And crucially, phases are
handed to *units*, not to fixtures — so a trick that groups eight fixtures
into four pairs produces four phases, not eight.

## Focus

Focus is a **point** (everything converges on it) or an **orientation**
(everything runs parallel). Patterns fan, splay, or go per-fixture. It
resolves to pan and tilt at output time from the fixture's live hang, in
metres and degrees.

The hang — where a fixture is rigged and how — is `Placement`, and it is never
the aim. Live pan/tilt is attribute state resolved at showtime. Conflating the
two is a bug that has already happened, at this exact venue.

---

Previous: [[the-four-files|The four files]] · Next: [[recipes-cues-effects|Recipes, cues and effects]] · Up: [[a-show-end-to-end|A Show, End to End]]
