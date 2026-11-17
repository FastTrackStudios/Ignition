# Profile, Venue, Ignition, Show

Four concepts, and the relationship between the first two is the load-bearing
one.

```
Rockstars.ig-profile     the interface — what a rig must provide to run a show
Norco.ig-venue           an implementation of it, at one address
Bye Bye Bye.ignition     a show for one song, written against the profile
Sunday 14th.ig-show      the night: a profile, a venue, and the songs
```

A **Profile** is a declaration: *if a room provides these things, a busking show
can be run in it.* A **Venue** is one room's implementation of a profile. An
**Ignition** file is a song's light show, written in the profile's vocabulary
and against no particular room. A **Show** is what an operator opens for the
night, binding the three together.

## Why the Profile is in the middle

Without it, portability is a question about pairs: *does this show work at this
venue?* With N shows and M venues that is N×M questions, each answered by
inspection, and each re-asked whenever either side changes.

With a profile it becomes two independent questions, asked once each:

- Does this **show** only use vocabulary the profile declares?
- Does this **venue** implement everything the profile requires?

If both hold, every show plays at every venue, and it is *provable* rather than
tested. That is N+M checks instead of N×M, and — more usefully — a new venue is
verified before a single show is opened in it.

This is why the profile is not merely a naming convention. A convention is
something a show can quietly violate; a declared interface is something a check
can hold both sides to.

## The Profile

r[profile.declares-vocabulary]
A profile MUST declare the vocabulary a show may use: named groups, colour
roles, focus roles, and canvas names. A show MUST NOT use a name the profile
does not declare, and that MUST be checkable.

r[profile.roles-are-intent]
A profile's names MUST describe **intent**, not equipment. `Key`, `Back Wash`,
`Floor` and `House` are roles a rig plays; "eight Uking pars on the front truss"
is one venue's answer to one of them. A profile naming equipment is a profile
only one venue can ever implement.

r[profile.required-and-optional]
Each role MUST be marked **required** or **optional**. Required roles are the
contract — a venue that lacks one cannot claim to implement the profile.
Optional roles are for what a room may or may not have: a follow spot, a haze
machine, a floor package. A show using an optional role MUST still run where it
is absent.

r[profile.minimal]
A profile SHOULD declare the fewest roles that make a busking show possible.
Every required role is a barrier to a room implementing the profile, and a
profile with forty required roles describes one venue with extra steps.

r[profile.several-may-exist]
Several profiles MUST be able to coexist. A profile is a shared standard among
whoever agrees to it, not a property of the software — a church circuit, a
touring rig and a festival stage want different vocabularies, and hard-coding
one would make the others second-class.

## The Venue implements it

r[profile.venue-binds]
A venue MUST bind each of a profile's roles to something concrete: a group role
to a selection expression, a colour role to a colour, a focus role to a point or
orientation, a canvas role to a set of panels. The binding is the venue's
business and MUST NOT leak into any show.

r[profile.venue-declares-what-it-implements]
A venue MUST name the profile it implements. A venue implementing no profile is
still valid — it can be programmed directly — but no show can be *checked*
against it, which is the guarantee being given up.

r[profile.venue-may-exceed]
A venue MAY provide groups, colours and focus points beyond its profile's
vocabulary, for programming that is specific to that room. A show using them
MUST fail the check against the profile, which is the point: room-specific
programming is legitimate, and it should be visibly non-portable rather than
silently so.

r[profile.check-is-static]
Checking a venue against a profile MUST NOT require running anything, and MUST
report every required role the venue leaves unbound. A new room is verified
before a show is opened in it, not during one.

## The consumer side

r[profile.resolution-by-role]
Code and shows MUST reach the rig **through** the profile's roles. Asking for
`Key` MUST return whatever this venue binds to `Key`. Nothing outside the venue
may reach for a channel, a fixture id, or a group the venue happens to define.

r[profile.trait-not-hardcode]
The consuming interface MUST be a trait implemented over a venue's bindings, not
an enum of known roles. Roles come from a profile, and profiles are data; an
enum would make adding a role a code change and adding a profile a fork.

r[profile.unbound-is-visible]
Resolving an unbound role MUST return a clearly empty result and be reportable —
never silently nothing. A recipe covering no fixtures is legitimate and common;
a recipe covering no fixtures *because the venue forgot to bind Back Wash* is a
defect, and the two look identical at output unless the difference is carried.

## The Ignition file

r[profile.ignition-is-per-song]
An `.ignition` file MUST hold one song's light show: its cues, its recipes, its
triggers, and which song it is written against. It MUST name the profile whose
vocabulary it uses, so it can be checked without a venue present.

r[profile.ignition-has-no-venue]
An `.ignition` file MUST contain nothing venue-specific — no DMX address, no
universe, no fixture id, no coordinate in metres. See `r[files.no-fixture-identity]`.

## The Show

r[profile.show-binds-the-night]
A `.ig-show` MUST bind a profile, a venue, and an ordered list of `.ignition`
files. It is what an operator opens for the night, and what an audit runs
against to answer "is everything in tonight's set playable in this room".

r[profile.show-check-before-doors]
Opening a show MUST check every song against the profile and the profile against
the venue, and MUST report every gap in one place before anything runs. A gap
found at the desk during the first song is a gap found too late.
`igcheck <show.ig-show>` prints that report; `ignition_core::show_file::
check_ig_show` is the check behind it, and `igcheck --venue <dir> --profile
<file>` runs the venue half alone.

r[profile.show-many-per-song]
The same `.ignition` file MUST be usable in any number of shows, and a show MUST
be able to include a song more than once. Neither may assume exclusivity — a
song played twice in a night is one file referenced twice, not two copies to
keep in step.

## Areas — the blocking grid

r[profile.areas]
**Areas** — named regions of the stage where people stand — MUST be owned by the
**venue**, not enumerated by the profile.

How many a stage has is a property of the stage. A club has three; a stadium
with a thrust and a B-stage has fifteen, with names no central list could have
anticipated. Declaring them centrally either excludes the large room or burdens
the small one, and a profile that declares nine areas has broken its own
minimality rule — every declared role is a barrier to implementing it.

r[profile.areas.portable-question-is-focus]
The portable question — *where is the talent* — MUST be answerable through focus
roles. `Vocal` is downstage centre at every venue that has one, so a generic
show never needs the blocking grid. Areas are for a room's own programming, for
busking, and for venue layers.

r[profile.areas.profile-may-require-one]
A profile MAY still declare an area as a role, for a standard that genuinely
depends on one. The default profile declares none.

r[profile.areas.performer-orientation]
Area names MUST use the **performer's** left and right, not the audience's.
This is the convention a band, a stage manager and a lighting designer already
share, and adopting the other one would make every conversation at the desk a
translation.

r[profile.areas.not-a-focus-point]
An area MUST be a distinct kind from a focus point, even where a venue binds one
to the other. A focus point answers "where do I aim"; an area answers "where is
the talent". They diverge as soon as a show wants the fixtures that *cover* a
region rather than the aim that reaches its centre — and naming the distinction
now is what lets that arrive later without re-authoring every show that used it.

r[profile.areas.venue-decides-granularity]
A venue MUST be free to declare as many areas as its stage warrants, at whatever
granularity it finds useful. Six for a band on a club stage; more where there is
more stage. Nothing may cap or require a particular number.

## Venue-specific decisions

An `.ignition` file is generic by construction — it cannot name a room. But some
decisions genuinely *are* room-specific: this venue's upstage right is behind a
pillar, that one's floor package is worth pushing harder, this stage is shallow
enough that the mover fan wants narrowing.

The point of naming this as its own layer is not to permit exceptions. It is that
**non-portable decisions exist whether or not there is a place to put them**, and
without a sanctioned one they leak into the portable show — a hard-coded level
here, a venue-shaped selection there — until the generic file is quietly generic
in name only.

r[profile.venue-layer]
A show MAY have a **venue layer**: a separate artifact, bound to one
`.ignition` file and one venue, that overrides or adds to it. It MUST be a
separate file. Merged into the show it would destroy the property the show
exists to have; merged into the venue it would apply to every song.
On disk it is an `.ig-local`: `{ "version", "song", "venue", "override_cues":
{ name: cue }, "add_cues": [cue] }`, named from the `.ig-show`'s song entry as
`"layer"`.

r[profile.venue-layer.optional]
The generic show MUST be complete and playable without any venue layer. A layer
adjusts a show that already works — it is never where part of the show lives, or
the "generic" show is a fiction and the first venue to lack a layer plays a
broken one.

r[profile.venue-layer.explicitly-local]
A venue layer MUST be free to use anything the venue provides, including names
its profile does not declare. That is the entire purpose: it is the sanctioned
home for the non-portable, and requiring it to stay portable would leave the
non-portable with nowhere to go but the show.

r[profile.venue-layer.visible]
The check MUST report which songs in a set have a venue layer and which do not.
A show that behaves differently in one room is a fact an operator needs before
the night, not a surprise during it.

## Notes from prior art

r[profile.spatial-grid-is-derived]
Group ordering MUST be derived from real fixture positions rather than assembled
by hand. This is a deliberate divergence from how console template shows do it:
a widely used grandMA3 template requires the operator to build each group "using
the 2D selection grid" according to the fixtures' positions on stage, and warns
that "otherwise the effect forms might look different".

That instruction is a workaround for the console not knowing where anything is.
Ignition's venue already carries every fixture's surveyed position, so the grid
is a *consequence* of the patch rather than a parallel structure to maintain —
and the failure mode where a correct patch and a mis-built group disagree cannot
occur.

r[profile.setup-cost-is-the-metric]
The measure of a profile is how long it takes a new room to implement it. The
same template advertises patch, groups and presets in about thirty minutes, and
that number is the product. A profile requiring more of a room than it can do in
an afternoon will not be adopted, however good the effects behind it are.

## Extensions

r[profile.extensions]
The file extensions are `.ig-profile`, `.ig-venue`, `.ignition`, `.ig-show`, and
`.ig-local` for a venue layer.
Written out rather than abbreviated: these are files a person picks from a
folder at a desk, in a hurry, and `.igv` versus `.igs` is a distinction nobody
makes correctly under pressure.

## Busking programming the profile ships

A profile carries more than names and effects. What an operator reaches for
in the dark — the safe scenes, the two-key moves, the layout of the fader
bank — is programming too, and it is programming written against roles, so
it is as portable as an effect. Shipping it in the profile is what makes a
new room buskable the moment its venue binds the roles.

r[profile.looks]
A profile MAY ship **looks**: named static scenes, each a list of recipe
references written against roles, with a **kind** — `Bed` (what sits under a
verse), `Full` (a chorus), `Punt` (faces lit, nothing moving, the state a
stage can always be dropped into) and `Safe` (a blackout or near it, which
protected roles survive). A look is not a cue: it has no fade times and no
place in a list. It is what a key holds, a macro takes, or a fader carries —
and what a cue MAY open on, by name (`{"look": "verse bed"}` among its
recipes), stating its own recipes on top; a look the profile lacks is
reported like an unknown effect.

r[profile.looks.static]
A look's recipes SHOULD be static — one step — or looping beds. A look that
fires a one-shot is a macro wearing the wrong hat.

r[profile.looks.authored]
The shipped profile file is baked from code, so a look written into it is
lost on the next bake. Looks authored at the desk — the Program view's
STORE → LOOK — MUST therefore live beside the profile in an overlay,
`data/profiles/<name>.looks.json` (a `looks` map in the profile's own
shape, `<name>` being the profile file's stem), which every loader of the
profile MUST merge over the baked looks at load. Where the overlay and the
bake carry the same name, the authored look wins. A missing overlay is an
empty one, never an error.

r[profile.macros]
A profile MAY ship **macros**: a named list of steps the programmer executes
with timing. The steps are: take a look, set a fader's level, fire a library
effect by name at a level, flash a role with a bump kind, wait a number of
beats of the `Song` master, release everything the macro took, blackout, and
switch DMX output. A macro is the two-key move written down — *drop*, *build
eight*, *breakdown*, *end* — so it lands the same every night.

r[profile.macros.beats]
A macro's waits MUST be in beats, resolved against the `Song` speed master at
the moment the wait starts, so the same macro fits a ballad and a banger.
Where the master is unset the wait MUST resolve at the fallback tempo rather
than never.

r[profile.macros.release]
A macro's **release** step MUST let go of everything the macro itself took —
the look it held, the effects it fired, a blackout it set — and MUST NOT
touch what the operator's hand or faders were doing before the macro started.

r[profile.pages]
A profile MAY declare the fader bank's **pages**: each a name and eight
fader specs — a label, a source (a library effect, a bundle, a look, a role
master, or an inline recipe), an attribute filter, an optional speed, and the
effect parameters the fader exposes. The studio's bank MUST be built from
these pages rather than from code, so a profile can lay its own desk out.

r[profile.pages.label-fits]
A page fader's label MUST fit under a hardware track: eight characters or
fewer.

r[profile.attribute-filter]
A fader MAY carry an **attribute filter** — any subset of intensity, colour,
position, beam — and the programmer MUST drop every emit of that fader
outside the filter. A colour chase on a fader filtered to colour cannot move
a mover, however the library effect was written. A cue's reference to a
library effect MAY carry the same filter, with the same meaning, so a rainbow
on the bars may own their colour while a strip chase beside it owns their
intensity.

r[profile.protected-roles]
A profile MAY name **protected roles**: roles a blackout, a rig drop, a black
key, a held `Safe` look and the grand master MUST never touch. House lights
are the case: a rig drop that took the house to black would empty a room
under fire regulations. The operator's direct hand still reaches a protected
role — `r[playback.hand-wins]` has no exception — and a protected role is
declared `optional`, since a room without house lights on the desk is still
a room.

r[profile.speed-routing]
A profile MAY declare **speed routing** defaults per effect family: what
speed a fader carrying an effect of that family runs at when the fader does
not say. The default profile routes movement to the `Tap` master at half,
beam to `Tap` at double, and intensity, colour and strip to the `Song`
master, so the movers stay slow while the strobes run double against one
tapped tempo. A fader's own speed, where declared, wins.

r[profile.effect-parameters]
A page fader MAY expose **effect parameters** as a second control beside its
level: `depth` (how far the recipe's relative values and swings go, scaled
like size but for this fader alone), `bars` (the loop length in bars) and
`duty` (the first step's share of the cycle — a strobe's on-time). Each is
declared with a name, a range and a default, and the programmer MUST apply
the values at fold, leaving the library recipe unchanged. A cue's reference
to a library effect MAY carry the same parameters by name (`"params":
{"depth": 0.5}`), applied at resolution through the same rule, so a strobe at
a quarter duty on a fader and in a cue are one thing. A cue has no family
table to route speed by — it is synced to the song — so the show-side form of
speed routing is an explicit `speed` on the reference, a scale against the
`Song` master.
