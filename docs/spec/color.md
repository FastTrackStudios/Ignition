# Colour

What a colour *is* in Ignition, and what a colour preset can say.

Today a colour preset is a name and three floats:

```json
{ "name": "Open White", "red": 1.0, "green": 1.0, "blue": 1.0 }
```

That is one colour, applied identically to every fixture that receives it. It
cannot express "the ambers, warmer at the edges", it cannot say what to do when
the fixture has amber and lime emitters as well as RGB, and it cannot be
recalled against a rig it was not written for.

grandMA3's answer to all three is that a preset is not a value — it is a
**rule with scope**, and the scope is the interesting part. The requirements
below take that model. They do not take MA3's data layout, its pool numbering,
or its command syntax; those are console ergonomics, and Ignition is not a
console.

Prior art, and where the ideas come from:
[Presets](https://help.malighting.com/grandMA3/2.2/HTML/presets.html),
[Using the Color Picker](https://help.malighting.com/grandMA3/2.0/HTML/operate_color_picker.html),
[Gels Pool](https://help.malighting.com/grandMA3/2.2/HTML/operate_gel_pool.html).

## Colour as a value

r[color.space-independent]
A colour MUST be stored in a device-independent form, not as a set of emitter
levels. Emitter levels are a property of the fixture that happens to be
producing the colour, and a preset written against a five-emitter wash is
meaningless to a three-emitter par. The stored form is the *intent*; turning
intent into emitter levels is the fixture's job, and MUST happen at output time
against that fixture's actual emitters.

r[color.model]
The stored form MUST carry, at minimum, a linear RGB triple. It SHOULD also be
expressible as HSB, and the two MUST be interconvertible without loss beyond
float precision — MA3's three colour systems are interlinked, "adjusting the
colors in RGB also moves the CMY and HSB faders", and an operator who nudges
brightness expects hue to survive it.

r[color.gel]
A colour MAY be named as a **gel** — a manufacturer and a swatch number, e.g.
Lee 201. A gel reference MUST resolve to a colour value through a swatch table,
and MUST keep the reference rather than only the resolved value, so a show says
"Lee 201" where that is what was meant. This is MA3's Gels Pool: a library
"of gel colors from different manufacturers".

r[color.quality]
Where a fixture has more emitters than the colour model needs, the preset MAY
carry a **quality** hint in `0..=1` describing how to mix: 1 favours the
narrow-band emitters that match the target most exactly, 0 favours using as many
emitters as possible. MA3 exposes this as the Q fader — "Q at 100 results in a
kind of small-band mixing … 0% results in a broadband mix that uses as many
emitters as possible". Absent the hint, a fixture MUST pick its own default
rather than fail.

## Scope — the part that makes a preset a rule

r[color.scope]
Every colour preset MUST declare a scope, and the scope MUST be one of
**universal**, **global**, or **selective**. These are MA3's three preset modes
and they answer one question: *when this preset meets a fixture it was not
written for, what happens?*

r[color.scope.universal]
A **universal** preset carries one value that applies to any fixture whatsoever.
"Open White" is universal. A universal preset MUST NOT store per-fixture or
per-type variation; if it needs to, it is not universal.

r[color.scope.global]
A **global** preset carries one value per *fixture type*, so the same named
colour can mean slightly different emitter targets on a par and on a wash — the
two rendering as the same colour to the eye being the entire point. Where
fixtures of one type disagree, the global value MUST be their average and the
divergent fixtures MUST be stored selectively, which is MA3's rule verbatim: the
global data is "determined by average, and selective data will be added for the
other fixtures which have divergent data".

r[color.scope.selective]
A **selective** preset carries a value per *fixture*. This is what makes a
multi-colour preset possible: "Rainbow" is one preset that gives fixture 1 red,
fixture 2 orange, fixture 3 yellow. A selective preset recalled onto a fixture
it has no entry for MUST fall back — global value if there is one, otherwise the
first selective value — rather than leaving that fixture unchanged, because a
half-applied colour reads as a rig fault.

r[color.scope.fallback-order]
The fallback order MUST be: the fixture's own selective value, then its type's
global value, then the preset's universal value, then the first selective value
in the preset. Every colour preset MUST therefore produce a value for every
fixture it is applied to. A preset that cannot is a defect in the preset, not a
condition to handle at output.

## Multi-colour presets

r[color.multi]
A preset MUST be able to hold *several* colours and a rule for distributing them
across a selection. This is the thing the current single-triple model cannot
express at all, and it is how a rainbow, a two-tone split, or a warm/cool wash
is one recallable object rather than five hand-written recipes.

r[color.multi.distribution]
The distribution rule MUST be declared, not inferred from the count. At minimum:
`cycle` (repeat the list across the selection in order), `spread` (interpolate
between the entries so a two-entry preset is a gradient), and `block` (divide the
selection into as many contiguous runs as there are entries). Inferring the rule
from the number of colours would make adding a colour silently change the look.

r[color.multi.order]
Distribution MUST follow the selection's own order, not patch order. The
selection already knows how to order itself spatially — see
[groups](groups.md) — so "red to blue, left to right" is a two-colour `spread`
over an X-ordered selection, with no colour-specific notion of direction.

## Recall

r[color.recall-by-reference]
A cue MUST store a *reference* to a preset, never a resolved colour. Editing the
preset MUST change every cue that refers to it, with no re-save and no
re-authoring — MA3's rule that "updating a preset automatically propagates
changes to all referencing cues and presets". A show that stored resolved values
would need re-authoring every time a gel changed.

r[color.embedding]
A preset MAY refer to another preset. Nesting MUST be bounded (MA3 allows ten
levels) and a cycle MUST be detected and reported rather than recursing. The
point is that a rig change is repaired in one intermediate preset rather than in
every show that uses it.

r[color.unresolved-is-visible]
A reference that does not resolve — a missing preset, a missing gel, a cycle —
MUST be reported by the show's load-time check, naming the cue and the
reference. It MUST NOT abort the load: the rest of the show still runs, which is
what makes the check safe to run on a rig that is half-patched.

## Colour intent and emitters

r[color.intent]
A colour MUST be storable as a device-independent **intent** beyond RGB — a
CIE xy chromaticity with luminance, or a correlated colour temperature — so
that "warm white at 3200 K" is one value on a par, a wash and an LED wall.

r[color.emitter-solve]
Turning intent into emitter levels MUST use the fixture type's emitter data
(GDTF carries it) when present: a five-emitter wash reaches the intent with
amber and lime, a three-emitter par with what it has, and the two match to the
eye. The `quality` hint of `r[color.quality]` steers narrow-band against
broadband mixing. Fixtures without emitter data MUST fall back to the RGB
triple.

r[color.cct]
A colour preset MAY be a colour temperature, and a fixture with a white
emitter or a CTO MUST prefer it over an RGB approximation.
