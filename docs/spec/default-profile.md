# The default Ignition profile

The profile Ignition ships with, and the one a rig should implement unless it
has a reason not to. Anyone can write their own — see
[profile.md](profile.md) — but a default that is actually good is what stops
everyone writing one, and a vocabulary only one venue speaks is not a
vocabulary.

## The principle: roles are layers, subdivision is Tricks

Norco currently defines **127 groups**. Reading them is the whole argument:

```
Front Wash   Mid Wash   Wide Wash   Back Wash   Strips   OH Movers   Drums …
Pars Odd   Pars Even   Pars Qtr 1   Pars Qtr 2+4   Pars 1st Half   Pars Ends
Front L+C   Wide + Mid R   Left All   Centre All   Chase L to R   Chase Ctr Out
```

The first line is roles. Everything after it is *subdivision* — odds and evens,
quarters, halves, left and right, chase orders — hand-authored one at a time
because there was no way to say them. That is what [Tricks](tricks.md) exist to
express: `Pars Odd` is `Group(2)`, `Pars Qtr 3` is `Block(n)`, `Chase L to R` is
an X-ordered selection, and none of them need a name.

So the profile declares **layers**, and everything spatial is derived.

r[default.layers-not-positions]
The default profile MUST declare functional layers, never spatial subdivisions
of them. `Key` is a role; `Key Left` is `Key` with a Trick. A profile that
declares positions makes every rig with a different fixture count unable to
implement it, and makes the show unable to say anything the profile did not
anticipate.

r[default.small]
The required set MUST stay small enough that a modest rig can implement it
honestly. Every required role is a barrier to adoption, and a profile only a
large rig can satisfy has failed at being a default.

## Required layers

Three, because a show that lacks any of them is not a show.

r[default.key]
**`Key`** — light on the performers' faces, from the front. Required because
without it the audience cannot see who is playing, and no amount of colour
elsewhere substitutes.

r[default.wash]
**`Wash`** — the main colourable surface on stage. This is what carries the
song's mood, and it is the layer a busking operator spends most of the night on.

r[default.back]
**`Back`** — anything behind the band: back wall, cyc, upstage bar. Required
because separation between the band and the background is the single largest
difference between a rig that reads as designed and one that reads as switched
on.

## Optional layers

Present in most rigs, absent in some, and a show MUST run either way.

r[default.optional-layers]
The default profile SHOULD declare these as optional: **`Movers`** (anything that
pans and tilts), **`Beams`** (hard-edged movers, where they are a separate
package from wash movers), **`Bars`** (pixel strips and battens — the accent
layer), **`Floor`** (uplight and floor package), **`Audience`** (blinders and
house wash), **`Drums`** (the drum special), **`Spot`** (a solo or follow), and
**`Haze`**.

r[default.optional-is-not-second-class]
A show MUST be able to build a complete look from optional layers and still run
where they are missing — the look is simply thinner. Anything that would *fail*
without an optional layer belongs in the required set instead.

## Focus roles

Focus roles are required in a way colours are not, because a coordinate is
meaningless outside the room that defines it.

r[default.focus-required]
The default profile MUST require: **`Vocal`** (the lead position), **`Stage`**
(the whole performance area), and **`Audience`**. These are the three a show
cannot avoid needing, and a room that cannot say where its singer stands cannot
host a show written for one.

r[default.focus-optional]
It SHOULD declare as optional: **`Band`** (the upstage area), **`Drums`**,
**`Back Wall`**, and **`House`** (deep audience). A show naming one that is
absent leaves those fixtures at their prior aim, per
`r[files.graceful-degradation]`.

## Colour

Colour is the exception, and the reason is worth stating: **RGB is portable in a
way a coordinate is not**. Per `r[color.space-independent]`, a colour is stored
as intent and rendered by whatever emitters a fixture has, so a show naming a
literal colour is not naming anything venue-specific.

r[default.colour-roles-are-semantic]
Colour roles exist for *consistency*, not portability. `Warm` means "this show's
warm", so that changing it changes every cue that used it — the same reason a
stylesheet has a variable. A show MAY use literal colours freely.

r[default.colour-defaults-ship]
The profile MUST carry **default values** for its colour roles, and a venue MUST
inherit them unless it overrides. Implementing a venue is then binding the
groups, which genuinely differ per rig, and inheriting the colours, which mostly
do not. A profile that made every venue re-specify `Open White` would be
charging rent for nothing.

r[default.colour-set]
The default set SHOULD be: **`Open`** (no colour), **`Warm`**, **`Cool`**,
**`Deep`** (a saturated low-light colour), and **`Hot`** (the brightest, most
saturated accent). Five, because they name the *jobs* colour does in a show
rather than the colours themselves, and a sixth would start naming hues.

## Canvases

r[default.canvas-main]
A profile MUST declare **`Main`** as the primary video surface, required only of
venues that have screens at all. A show says "play this on `Main`" and the venue
decides whether that is one panel or three of unequal width with gaps between
them — see `r[files.venue.canvases]`.

r[default.canvas-optional]
**`Side Left`** and **`Side Right`** SHOULD be optional, for rooms with
independent flanking surfaces. Ignition's own lyric screens are these.

## What this asks of a venue

r[default.implementation-cost]
Implementing the default profile MUST be achievable by binding three required
groups, three required focus points, and — where the room has screens — one
canvas. Everything else is optional or inherited. That is the number that
decides whether a second venue happens, and it is deliberately small.

r[default.norco-is-the-proof]
Norco's 127 groups MUST collapse to this vocabulary plus Tricks. If they do not,
either a role is missing from the profile or a Trick is missing from the
language — and either way the gap is a defect in this spec rather than a reason
for the venue to declare 127 groups.
