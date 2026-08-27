# Where the frame went

`r[studio.profiling]`

The studio draws with two renderers in one process. Blitz lays the
document out and paints it with Vello; inside one element of that paint,
Bevy steps the entire visualizer and hands back a texture. A frame-rate
readout tells you the two of them together took 44 ms and nothing about
which one spent it — which is the only question worth asking about a slow
studio, and the reason "it feels slow, let's tune something" has been
guesswork here twice.

Three tools, in the order to reach for them.

## 1. The stage table — which stage

```
IGNITION_PROFILE=1 just studio          # or: just profile
```

No rebuild, no extra binary, nothing to install. Every stage of a frame
opens a `tracing` span on one target of its own (`ignition::profile`);
`IGNITION_PROFILE=1` adds a directive for that target to the log filter
and installs a layer that aggregates the spans. Every two seconds a table
lands in the log — and so in `~/.local/state/ignition/studio.log`:

```
profile: 2.0 s · 84 frames · 23.81 ms/frame · 42.0 fps (budget 8.33 ms at 120 Hz)
  stage                          calls self/frame  all/frame      avg      p99      max
  viz.render                        84     14.20!     14.20    14.20    22.10    31.40
  blitz.render                      84      6.10       23.81    23.81    30.02    41.90
  blitz.style                       84      2.40        2.40     2.40     4.10     6.20
  viz.commands                      84      0.61        0.61     0.61     1.20     2.90
```

Read the **self/frame** column and nothing else first. It is the time a
stage spent that was *not* inside a nested stage, so it is the only
column that attributes anything: `blitz.render` is by construction 100%
of the frame every time and says nothing, while its self time — what is
left after the scene build, and so after the visualizer — is Vello
encoding and the GPU submit. A `!` marks a stage over the whole 120 Hz
budget on its own; there is rarely more than one, and it is the answer.

The stages, nested as they nest:

| stage | what it is |
| --- | --- |
| `loop.window_event` | one winit event; the `redraw` one contains the frame. |
| `loop.poll` | the Dioxus virtual DOM coming up for air: every dirty component re-rendered, diffed and written into the document. |
| `loop.redraw` / `loop.resume` / `loop.shell_other` | the other shell events. |
| `blitz.render` | one window's whole frame. Self time is Vello encode + submit. |
| `blitz.scene`  | the DOM paint walk — and, inside one element, the visualizer. |
| `viz.paint`    | the Visualizer pane's element. |
| `viz.commands` | the operator's commands, the sound fade, the transport, the playhead published back. |
| `viz.render`   | one visualizer frame: resize, main-world step, extract, encode. |
| `viz.step`     | just `App::update` — the main world. |
| `viz.programme`| the Programme pane, when one is up. |
| `blitz.resolve`| style and layout together. |
| `blitz.style`  | stylo. |
| `blitz.layout` | taffy. |

A stage that never appears either never ran (`viz.programme` with no
Programme pane up) or was compiled out.

Two windows painting means two `blitz.render` calls a frame, so `avg`
(per call) and `self/frame` (per frame) are deliberately different
columns. Do not read one for the other.

Knobs: `IGNITION_PROFILE_INTERVAL` (seconds, default 2) and
`IGNITION_PROFILE_FRAME` (the span that counts as a frame, default
`blitz.render`).

## 1a. Measure the worst case, not an idle one

```
just profile-bench
```

`IGNITION_BENCH=1` opens the studio on `data/songs/benchmark.json` — one
cue lighting every mover, par, bar, beam and hazer at once, with chases,
figures, strobes and colour cycles running — and **takes** it. Quote
numbers from this and nothing else: an idle transport flatters the
studio by a third, and the two are not the same shape of frame. Idle,
the studio is CPU-bound in Blitz; lit, it is GPU-bound in the volumetric
pass, and `blitz.render`'s self time *rises* because Vello's submit ends
up waiting on the visualizer.

Two traps, both of which cost this repo real time:

* A cue list that is loaded but never GOed outputs nothing, so the
  studio comes up on a dark rig and reports a lovely frame rate for an
  empty room. `crates/ignition-viz/tests/benchmark_cue.rs` exists
  because of it.
* `follow_song` seeks the cue player to the transport's position *every
  frame*. With a project open and its transport parked at bar one, it
  drags the player off the benchmark cue as fast as GO puts it there —
  the cue fires, the rig stays dark, and nothing in the logs is wrong.
  Benchmark mode opens no project for exactly this reason.

Prove the rig is lit and moving rather than assuming it: capture two
screenshots a second apart and diff the viewport. Ninety-odd per cent of
those pixels should differ.

```
tools/shoot.sh --out /tmp/a && sleep 1 && tools/shoot.sh --out /tmp/b
magick compare -metric AE -crop 1290x190+300+40 /tmp/a/DP-4.png /tmp/b/DP-4.png null:
```

## 2. `IGNITION_PROFILE=all` — which system

Once the table says `viz.step`, the next question is *which system*.

```
just profile-build-trace       # a release build with `--features trace`
IGNITION_PROFILE=all just studio
```

`ignition-viz/trace` turns on `bevy/trace`, which opens a span per
system, per schedule and per render pass; `all` aggregates every span in
the process rather than just the named stages. The table gets long and
the profiler itself starts costing a millisecond or two — which is why
this is a separate build and a separate mode.

## 3. A timeline, and a sampling profile — which line

An average hides shape. A stage costing 12 ms a frame might be one frame
in ten costing 120 ms; two windows might be painting in lockstep and
serialising. For that:

```
just profile TRACE=/tmp/ignition-trace.json
```

which writes a Chrome trace-event file — open it at
<https://ui.perfetto.dev>. It is plain JSON, one event per line, and the
array is deliberately never closed (the format allows a truncated file),
so killing the studio however you like still leaves a trace that opens.

And for everything the spans cannot see at all — Vello's own encoding,
wgpu, the image decoders behind the library thumbnails, malloc:

```
just studio          # then, in another terminal, against the running one:
sudo sysctl kernel.perf_event_paranoid=1
just perf-studio
hotspot /tmp/ignition-perf.data
```

Sampling from launch would spend the whole profile in shader
compilation, which is a real cost but not the one that repeats every
frame — hence attaching to a studio that is already up.

## The visualizer on its own

```
just bench                     # crates/ignition-viz/src/bench.rs
```

Headless, repeatable, through the same embedded route the studio uses,
and it splits the CPU step from the GPU tail. This is what a change to
the renderer is judged against (`r[viz.performance-budget]`); the
profiler is what tells you which change to make.

## Why not Tracy

It would be the obvious choice, and it is not usable here yet: Bevy 0.19
pins `tracing-tracy` 0.11, whose wire protocol wants a Tracy 0.11 server,
and nixpkgs ships 0.13. Matching those up is a build rabbit hole in
exchange for a nicer picture of a number the table already gives. If Bevy
bumps its pin to something nixpkgs carries, the layer to add is four
lines next to `ignition_profile::from_env()` in the studio's `main.rs`.

## What it found the first time

Worth keeping, because both answers were somewhere nobody was looking
and both were found by reading one column.

The studio was at 30 fps, 44 ms a frame, and the standing assumption was
that the visualizer was too expensive. The table said `viz.step` — the
entire Bevy world, every fixture, the volumetrics — was **2.7 ms**, and
that `loop.poll`, the Dioxus vdom poll, was **22.4 ms**. Under it, a
sampling profile (`just perf-studio`) put twenty-seven per cent of the
whole process in `url::parser::parse_cannot_be_a_base_path`: the library
panes animate their thumbnails by swapping an `img src` twelve times a
second, those `src` values were base64 `data:` URIs, and Blitz parses an
`img src` as a URL every time it is set — two hundred thousand
characters of it, before a cache lookup keyed on the parsed string.
`file:` URLs instead: 22.4 ms to 0.21, and 30 fps to 77.

Then `blitz.layout` at 3.9 ms a frame, every frame, on a document whose
shape had not changed since the window opened. The frame loop kept Blitz
repainting by bumping a signal into a `data-frame` attribute — which
re-rendered a component, diffed it, mutated the document, restyled it
and relaid the whole tree out, to change a number nothing reads.
`use_window().request_redraw()` asks winit for the same frame and
touches nothing: 3.9 ms to 1.4, and 77 fps to 105.

Neither of those is a thing to guess at, and both are obvious in the
table. That is the argument for the profiler.

## Render quality presets

`IGNITION_QUALITY=potato|low|medium|high|ultra`, default `medium` —
`r[viz.quality-presets]`. Measured on the benchmark cue at 5120x1440
with the rig lit, the effects running and the screens playing, on an RTX
4080; the haze camera's size is the one the studio logs
(`viz.haze: resized`).

| preset | haze camera | steps | fps |
| --- | --- | --- | --- |
| potato | 518x72 | 32 | 123 |
| low | 1036x144 | 64 | 125 |
| **medium** | 2071x288 | 128 | **97** |
| high | 2071x288 | 192 | 84 |
| ultra | full size, on the camera itself | 192 (256 for a narrow shaft) | 25 |

Three things in that table are worth more than the numbers.

**Potato and low are the same speed**, and that is not a mistake in the
ladder. Below `medium` the frame stops being GPU-bound: what is left is
Blitz painting the document and Bevy stepping its main world, about
8 ms of CPU, and no graphics setting touches it. **~125 fps is the
ceiling on this machine** until that CPU work comes down. Potato earns
its rung on a weaker GPU, not this one.

**High marches the same haze camera as medium**, because the scale is a
whole divisor: doubling the budget is not enough to reach the next one
at this viewport. High is a finer march over the same pixels, which is
the dial that matters anyway.

**Ultra is a quarter of the frame rate** — it marches on the camera
itself at the picture's full size, and `for_rig` is then free to raise
the count to 256 for a narrow shaft. It is for looking closely at
something, not for running a show.

## Where it stands, and the dial that moves it

On the benchmark cue at 5120x1440 with the screens playing: **101 fps,
9.86 ms a frame**, against an 8.33 ms budget. The same window was 30 fps
before the four fixes above it.

The frame is now GPU-bound and the volumetric raymarch is nearly all of
it. `IGNITION_FOG_STEPS` is the dial, and it is a quality decision
rather than a free win — 128 is the fewest that keeps a narrow shaft a
solid shaft rather than a column of rings (see `RenderQuality::live`):

| fog steps | fps |
| --- | --- |
| 128 (default) | 101 |
| 112 | 104 |
| 96 | 109 |
| 64 | 118 |

`IGNITION_SSR=0` is worth about 3 fps, `IGNITION_TAA=0` about 2,
`IGNITION_SSAO=0` none.

One earlier reading here was wrong and is worth recording as such:
`IGNITION_FOG_SCALE=2` appeared to give the haze camera four times the
pixels for free. It gave it exactly the pixels it already had — the
automatic scale had *already* chosen 2 at this viewport, which the
studio now says out loud in `viz.haze: resized`. The cost model holds:
pixels times steps times lights, with the pixels already budgeted, which
is why the step count is the dial that moves.

## The wall medium is against, and what is not the answer

The quality tiers cost exactly one thing, and it is not where you would
look for it. Comparing `low` and `medium` stage by stage:

| stage | low | medium |
| --- | --- | --- |
| `blitz.render` self | 3.39 ms | **5.50 ms** |
| `viz.step` | 3.01 ms | 3.09 ms |
| `blitz.scene` self | 1.40 ms | 1.46 ms |
| frame | 8.03 ms | 10.32 ms |

The entire 2.3 ms lands in **Vello's submit**, and the visualizer's own
CPU time does not move at all. The extra raymarch is pure GPU, and the
frame is waiting for it: Bevy steps *inside* Blitz's paint, both halves
go to one queue, and Vello's composite samples the texture Bevy is still
writing.

**Buffering is not the fix, and this has been measured.** Raising
`desired_maximum_frame_latency` from 2 to 3, switching the present mode
to Mailbox, and both together, all land within noise of 98 fps. The CPU
is not blocking on acquiring a swapchain image, so there is no point
patching `anyrender-vello-vendored` for it — that patch was written,
measured, and reverted.

What remains plausible, and is the next thing to try: let the
visualizer's volumetric pass and Vello's composite overlap instead of
serialising, by giving the visualizer **two target textures** and
alternating them — Vello samples the one the GPU finished last frame
while Bevy writes the other.

Note what this is *not*. `EmbeddedViz::last_good` looks like it already
holds the previous frame and does not: there is one target `Image`,
rendered into every frame, and `last_good` is a second handle to the
same `wgpu::Texture`. It exists so a resize shows the old picture rather
than a black flash. Reordering the calls in `VizCore::paint` therefore
buys nothing at all; the targets have to actually be two, which means
the resize path and the camera retargeting in `embedded.rs` both have to
learn to ping-pong.

The cost is one frame of latency on the viewport, which for a lighting
visualizer is not a cost. If the two halves do overlap, medium's 2.3 ms
disappears under the 8 ms of CPU and medium runs at low's frame rate
with medium's picture — which is the whole objective.
