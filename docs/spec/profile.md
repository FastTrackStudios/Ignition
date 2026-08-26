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

## Extensions

r[profile.extensions]
The file extensions are `.ig-profile`, `.ig-venue`, `.ignition`, `.ig-show`, and
`.ig-local` for a venue layer.
Written out rather than abbreviated: these are files a person picks from a
folder at a desk, in a hurry, and `.igv` versus `.igs` is a distinction nobody
makes correctly under pressure.
