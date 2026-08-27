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
