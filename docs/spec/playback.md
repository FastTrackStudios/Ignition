# Playback — priority and merging

What happens when two things want the same attribute on the same fixture at
the same moment. Every other spec describes a *source* of values — a cue, a
recipe, a trigger, a fader, an operator's hand. This one is the single
authority on how the sources are stacked, and it exists because the failure
mode is not "the wrong thing wins": it is that *nobody can say* what should
win, and the rig does something different each night.

grandMA3 answers this with named **playback priorities** — Super, Prog, Swap,
HTP, Highest, High, LTP, Low, Lowest — and one rule inside each: newest wins,
except that dimmer under HTP takes the highest. Ignition keeps the shape and
loses the numbering. Priority here is a *kind of source*, not a setting an
operator can get wrong, because the sources already differ in kind: a hand on a
fader is not a chorus, and a snare is not a chase.

Prior art: [Play Back Cues](https://help.malighting.com/grandMA3/2.0/HTML/cue_playback.html),
[Sequence Settings — Priority](https://help.malighting.com/grandMA3/2.0/HTML/cue_sequence_settings.html),
[Group Masters](https://help.malighting.com/grandMA3/2.0/HTML/group_master.html),
[Programmer](https://help.malighting.com/grandMA3/2.3/HTML/operate_programmer.html).

## The stack

r[playback.stack]
Output MUST be produced by folding a fixed, ordered stack of layers, highest
first:

```
1. the operator's hand      direct values in the programmer
2. masters and solo         per-role scaling — not a value source
3. flashes                  the operator's transient layer
4. faders                   busking recipes, weighted by fader level
5. triggers                 charted hits — the song's transient layer
6. the cue player           the show: transient over sustained over absolute
```

The order MUST be the same for every attribute and every show. A stack an
operator cannot recite from memory is one they cannot predict from the desk.

r[playback.hand-wins]
A direct value in the programmer MUST beat everything beneath it. This is the
one rule that must never have an exception: the operator reaching for a fixture
is the last line of defence against a show doing something wrong, and a layer
that could override that hand would make the desk unsafe.

r[playback.busking-over-show]
Everything in the programmer MUST sit above everything in the cue player. A
busking layer is what an operator uses to *correct* a show in flight; if the
show could override the correction, the correction would need a second layer to
protect it, and so on up.

r[playback.masters-scale]
Masters and solo MUST scale the fixtures' output rather than supplying a value.
A role master at 50% halves whatever the layers beneath produced; it does not
"want" a level of its own and MUST NOT participate in last-wins. Where a fixture
plays two roles with two masters, the lower MUST apply. See `r[groups.master]`.

## Inside the cue player

The cue player is where two cues, or a cue and the effect it inherited, meet.
It resolves to **one absolute value plus one relative value** per attribute,
per `r[recipes.cascade]`. This spec says how the relative one is chosen when
several are live.

r[playback.absolute-layer]
The absolute layer MUST be last-wins by take order — a direct value on the
newest cue over a recipe on it, over anything tracked from an earlier cue. See
`r[recipes.absolute-last-wins]`.

r[playback.relative-classes]
The relative layer MUST distinguish two classes of modulator: **sustained** (a
looping recipe — a chase, a pulse, a wave) and **transient** (a one-shot — a
bump, a cutout, a hit). The two are not the same kind of thing: a sustained
effect *is* part of the look, and a transient is an event that happens *to* the
look.

r[playback.transient-over-sustained]
A transient MUST outrank every sustained modulator on the same attribute for as
long as it is contributing, regardless of which was taken later and regardless
of which layer either lives in — a trigger ringing above the cue player holds
off the chase inside it just as a one-shot taken by a cue does. A hit landing
on a chased wash replaces the chase for a fraction of a beat and hands it
back; the chase does not get a vote about the hit. Without this rule a section
that re-states its chase after a figure has been fired silently outranks every
hit for the rest of the section, which reads from the stage as "the hits
stopped working halfway through the chorus".

r[playback.relative-last-wins-within-class]
Within a class, the later modulator MUST win outright, per
`r[recipes.relative-last-wins]`. Two transients in one cue are ordered by their
position in that cue — which is what makes a cutout expressible as *cut the
rig, then lift the zone* with the zone's fixtures taking the lift.

r[playback.transient-withdraws]
A transient MUST withdraw when its envelope finishes, per
`r[recipes.finished-one-shot-withdraws]`, and the sustained modulator beneath
it MUST resume at the phase it would have reached had nothing happened. The
chase runs on the shared clock and was never stopped; the hit merely covered
it.

r[playback.relative-on-unset-attribute]
A relative value on an attribute nothing has set absolutely MUST be applied to
the attribute's rest value (zero for a level), not dropped. A figure that cuts
"everything" and reveals one zone has to be able to *lift* a layer the section
left dark; dropping the modulation because the key light happened to be unset
makes the same hit behave differently depending on which section it lands in.

r[playback.clamp-at-output]
Level-like attributes MUST be clamped to their range at output, after the stack
is folded, and never before. A cut of −0.95 on a fixture at 0.55 is meant to
reach zero; clamping the relative value on its own would leave the fixture at
0.55 − 0.95 + (whatever the next layer adds) and the arithmetic would depend on
the order the layers were folded.

## Transients from the song and from the hand

r[playback.triggers-sum]
Triggers ringing at the same moment MUST sum, not last-win — see
`r[triggers.simultaneous-sum]`. Two hits landing together are two hits. This
is deliberately the opposite of the cue player's rule, and the difference is
what kind of disagreement is being resolved: last-wins is for two sources
disagreeing about one value, and summing is for one source firing twice.

r[playback.flash-equals-hit]
A flash from the operator's hand and a charted hit of the same kind MUST
produce the same object and the same output — see `r[effects.bump.one-object]`.
Two implementations would drift within a week and the drift would arrive as
"the chart feels different from playing it by hand", which nobody can debug.

## Multiple playbacks

Ignition runs one cue list tonight. It will not always, and the rules for more
than one are decided now so the single-list engine is not built in a way that
forecloses them.

r[playback.playbacks-have-priority]
Every playback (a cue list, a fader carrying a recipe, a trigger bus) MUST
carry a priority class, and the classes MUST be the ones in the stack above.
Two playbacks in the same class resolve by the rule for that class; a higher
class always beats a lower one. There MUST NOT be a numeric priority an operator
sets by hand.

r[playback.dimmer-htp-between-equals]
Where two playbacks of the *same* class both set a fixture's dimmer absolutely,
the higher MUST win (HTP). Every other attribute MUST be latest-takes-precedence
(LTP). This is the one place the two rules coexist, and it is MA3's rule for
its HTP priority verbatim: "the highest intensity value will be used; other
parameters will use LTP". A colour cannot be "highest"; a level can. (Not yet
built — one playback today.)

r[playback.release-falls-through]
When a playback stops asserting an attribute — a cue releases it, a fader
reaches zero, a transient withdraws — the attribute MUST fall through to the
next layer that asserts it, and finally to the fixture's rest value. Nothing
may be left holding a value from a source that has gone.

## Where the rules are applied

r[playback.no-merge-at-dmx]
All merging MUST happen in attribute space, before encoding to DMX. The byte
encoder MUST be a pure function of one resolved value per attribute; it MUST
NOT implement HTP, LTP or priority. Merging bytes is how two fixtures of
different types with the same address footprint come to be merged wrong.

r[playback.output-is-pure]
The folded output MUST be a pure function of (the layer stack, the rig, the
show clock, the operator state). Nothing may cache a resolved value across
frames outside the resolver. This is what makes the stack *inspectable*: a
frame can be re-resolved offline to answer "why was that fixture at 0.3", which
is the question this whole spec exists to make answerable.

r[playback.inspectable]
It MUST be possible to ask, for one attribute on one fixture, which layer won
and what each layer beneath it would have produced. A priority model nobody can
inspect is one nobody can trust, and the reports that led to this spec were all
of the form "it did nothing" — which was three different bugs that looked
identical from the stage.

## Playbacks, keys and pages

r[playback.several-players]
The engine MUST run several cue players at once — a song list, a look list, a
mover list — each in a class of the stack, resolved per
`r[playback.playbacks-have-priority]` and `r[playback.dimmer-htp-between-equals]`.

r[playback.keys]
A playback key MUST support the busking verbs: **flash** (on while held),
**toggle**, **swap** (this at full, every other playback of its class
suppressed while held), **kill** (this on, the others off), **black** (this
playback's intensity to zero while held). These are the two or three things an
operator hits under pressure.

r[playback.pages]
The fader bank MUST be pageable: the same eight physical faders carry another
eight assignments per page, and a fader that is up stays live when the page
changes until it is brought back to match.

r[playback.program-time]
The programmer MUST have a **program time** in beats: every value an operator
punches — a palette, a fader take, a key — arrives over that time rather than
snapping, so a busk stays smooth without every gesture being pre-timed.

r[playback.blind]
The programmer MUST be switchable to **blind**: its values are held and shown
in the visualizer's preview but do not reach output. Building the next look on
screen while the show runs is where owning a visualizer pays off.

r[playback.highlight]
The programmer MUST offer **highlight** (selected fixtures to open white at
full, above every layer) and **lowlight** (everything else dimmed), for
focusing and for finding a fixture in a running show.

r[playback.speed-scale]
A recipe MUST be able to run at a multiple of its speed master — half, double,
×3 — without a second master; a per-fader scale is how the strobe runs
double-time while the movers halve.

r[playback.remote-inputs]
Faders, keys and masters MUST be drivable from **MIDI** control change and
note messages (a nanoKONTROL, an X-Touch) and from **OSC**, mapped by a
document in the venue or profile, not by code.

r[playback.sound-in]
The engine MUST accept an **audio input** and derive a beat (the `Tap` master)
and band levels from it, so a support act with no song map still gets effects
in time and a blinder that follows the kick.

r[playback.master-modes]
Role masters MUST implement the four modes of `r[groups.master.modes]`:
positive (an upper limit), negative (an inhibit that wins where a fixture is
in two), scaling, and additive (a hand lift over a running show).

## Parking, masters and keys

r[playback.park]
An operator MUST be able to **park** a fixture, an attribute, or a DMX channel
at a value that sits above every playback and the programmer, and unpark it.
A motor screaming on one tilt is fixed at the desk in a second by parking it;
without a park the only tool is a cue that everything else can override.

r[playback.grand-master]
There MUST be a **grand master** scaling every intensity last, after every
other layer including parks of intensity, so the whole rig can be brought
down with one hand. It MUST NOT scale anything but intensity.

r[playback.playback-master]
Every playback MUST have its own intensity master, so the song list can be
pulled under a look list without touching either's content.

r[playback.selection-master]
A master MAY be keyed by a **selection** rather than a role — the fixtures in
hand — so "the four movers I just grabbed" can be ridden without naming a
role.

r[playback.speed-keys]
The `Tap` master MUST support **learn** (an averaged tap that converges rather
than jitters), **half** and **double** keys, and a reset to the learned tempo.
Riding a breakdown at half time and a drop at double is the whole point of
tapping.

r[playback.temp-and-pause]
A key MUST support **temp** — the playback on, with its own fade times, while
held — as the musical form of flash; and a playback MUST be pausable and
resumable, and steppable backwards, without losing its place.

r[playback.remote-feedback]
Remote surfaces MUST receive **feedback**: fader positions, key states and the
page over OSC (and MIDI where the device accepts it), so a motorised or
screen surface shows what the engine holds. A surface that cannot show the
state is one an operator cannot trust after a page change.

r[playback.sound-as-value]
Sound band levels MUST be usable as **values** inside a recipe — a relative
level, a generator's range — with a smoothing (sound fade) so a kick reads as a
lift and not as noise. This is the no-chart busking case: the bass drives the
blinders.

r[playback.defaults]
Every fixture attribute MUST have a **default** — from the fixture type where
it carries one, otherwise a sensible rest — and that default MUST be the floor
a released attribute falls to, and what a list's implicit **cue zero**
establishes before its first cue. A released zoom lands on the spot's default
zoom, not on zero.
