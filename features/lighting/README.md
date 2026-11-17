# Lighting (Ignition)

Ignition, the lighting engine — DMX / sACN / Art-Net out, a Bevy 3D
visualizer of the room, cues and effects written against roles. Its design
docs are in [`docs/lighting/`](../../docs/lighting/) (start with
`domain/DOMAIN.md` and `guides/`).

This folder is the port that was made inside Studio (studio 8bfa77ea,
from this repo at 196e159) brought back as it was, so the paths and the
crate names are the same in both. What follows was written there: where it
says Studio links the extension in or starts the renderer, that is Studio
Architect, built in Studio's repo, with this repo's `ignition-pane` and
`ignition`. The recipes are in the root `Justfile` (`just <recipe>`), and
the app they start is named by `IGNITION_ARCHITECT` (`studio-architect` by
default), since it is not built here.

## Crates

| Dir | Crate | What |
|---|---|---|
| `proto` | `ignition-proto` | the types every lighting crate shares (attributes, chans, vectors) |
| `daw-proto` | `ignition-daw-proto` | musical time: bars, tempo map, sections, song map. Its own crate only because `show` sits between it and `daw` |
| `colour` · `rig` · `show` · `effects` · `playback` | `ignition-*` | the domain: colour presets, selections and tricks, cues and recipes, the effect library, the priority stack |
| `core` | `ignition-core` | the domain as one namespace |
| `dmx` | `ignition-dmx` | sockets, rate limiting, and the `sacn` / `artnet` codecs (was four crates) |
| `daw` | `ignition-daw` | a song's shape from its project — REAPER (`reaper`) or a song of an Ableton Live set (`ableton`, on studio's `ableton` adapter) — the cue generator, and the playhead: `transport` / `timecode` (MTC, LTC, Art-Net), `beat_clock` (MIDI beat clock + SPP, the `midi-clock` feature) and `play` on studio's standalone engine (was four crates) |
| `fixture` · `profile` · `viz` | `ignition-*` | fixture library, the frame profiler, the visualizer (`viz` binary) |
| `pane` | `ignition-pane` | **Lighting, the Studio extension** (id `ignition`): the product — see below |
| `../../apps/ignition` | `ignition` | the renderer Studio starts for the extension (`--process-view`, a pop-out), and tooling: `render`, `--check`, `--at`, a debug runner |

`data/` is Ignition's data (venues, profiles, GDTF, songs, shows);
`tools/` its generators. The forks of `bevy_pbr` and `gdtf` are in
`vendor/` (FTS-PATCH.md each).

## Lighting in Studio — the extension

Lighting is a Studio extension, not an app of its own: `pane/`
(`ignition-pane`, id `ignition`), linked into Studio Architect and on by
default (its `lighting` feature). Studio starts and stops its renderer
(the `ignition` program) itself; nobody runs it by hand.

- **Lighting: show the room** (⌘K, `IGNITION_SHOW_OPEN`) — the pane: the
  room in 3D, following Studio's playback. It is an Architect process
  view (architect#641): Studio runs `ignition --process-view`, which
  renders on its own GPU device and sends frames over shared memory (Bevy
  is on wgpu 29, the app on 30). Drag to orbit, right- or Shift-drag to
  move, the wheel to come closer, `1`–`9` for the room's camera presets.
- **Lighting: pop out to a display** (`IGNITION_POP_OUT {display?}`) — the
  room borderless fullscreen on a display (a projector, the stage
  monitor; `display` an index, none: Studio's), same show, camera and
  clock; Studio owns that process. **Lighting: pop back in**
  (`IGNITION_POP_IN`) closes it.
- **Settings → Lighting** (or its actions): `ignition.show`
  (`IGNITION_SHOW_PICK`; empty: the recital), `ignition.live_set`,
  `ignition.camera` (`IGNITION_CAMERA_PICK`), `ignition.clock`
  (`IGNITION_CLOCK_PICK`; **`studio`** by default — Studio's own playback,
  the running engine's transport; `internal`, `midi:<port>`), and
  `ignition.renderer` (the program — swappable live). A change restarts
  the renderer with it. `IGNITION_STATE` says which copy runs and what.

**How it is configured and launched** (for `devenv up` / a desktop
build): build Studio Architect (`lighting` is a default feature) and the
`ignition` program into the same target dir; start Studio Architect with

| env | what |
|---|---|
| `IGNITION_LIVE_SET` | the recital's `.als` (the songs' timing); required unless `ignition.live_set` is set |
| `IGNITION_ROOT` | this folder (`features/lighting`): the data — venue, show, GDTF |
| `IGNITION_BIN` | the renderer, when it is not `ignition` beside the app |

The show defaults to the recital (`data/shows/jt-recital.ig-show`) and the
clock to Studio's engine (the newest `$XDG_RUNTIME_DIR/fts/engine-<pid>.sock`),
so with Studio playing, opening the pane is all there is to do.
`just architect` does exactly this for development.

**Developing it — a copy that replaces the built-in live** (Architect's
`EXT_CLONE`, rule 4 of its extensions skill): `just pane-dev`
writes the extension's crate from this checkout into the app's development
folder (`studio-architect dev-source ignition`, Architect's
`source::write_crate`), then `EXT_CLONE {id: "ignition"}` builds its
process copy there (`cargo build --bin ignition-pane`) and runs it **over
the built-in under the same id** ("built in (0.2.0), overridden by 0.2.0
(in development)"); each save rebuilds and swaps it in.
`just pane-undev` gives the built-in back; `just
pane-check` runs `extensions check` on the manifest.

Two gaps in Architect it works around or names:

- **No vendored source for a distribution.** Architect's own app maps its
  repository to its checkout (`Extensions::with_vendored`), but a
  `Distribution` cannot register one, so `EXT_CLONE` of a Studio built-in
  would clone the manifest's `[source]` (this repo's `main`). `dev-source`
  writes the crate first; `EXT_CLONE` builds what is there.
- **No GPU process view in a process extension's pane.** A process copy's
  view is a remote Dioxus document (`ExtensionView` draws web views,
  pages, linked components or remote documents), so while a development
  copy runs its pane shows what it would run and pops the room out to a
  display. A `process` view kind (`kind = "process"`, the program in the
  package, drawn by the app's `ProcessViewPane` whichever copy runs) would
  close it.

**Known bug:** resizing Studio Architect's window while the Lighting pane
is open panics in Blitz's style engine (stylo `ElementStyles::viewport_unit_usage`,
an unwrap on an element with no style, during viewport-unit invalidation);
without the pane it does not. It is the process-view widget in a Blitz
document, Architect's to fix.

**A web view later** (`[[views]] kind = "web"`): the visualizer already
builds for the browser (`ignition_viz::run_web`, Bevy on WebGPU), so a
`web` view holding that build would draw in every copy, a process copy
and a share link included. What it needs: Architect's web views mounted
in the web app (#495; desktop needs `--features webview`, whose
WebKitGTK has weak WebGPU on Linux), the clock through the host bridge
(a web view has `connect-src 'none'`, so it cannot reach Studio's engine
itself — Studio would publish the playhead to it), and the show and venue
data in the package. Feasible; not built.

The window's Wayland app id (X11 WM_CLASS) is `dev.fts.ignition`, its
title "Ignition — visualizer" — for a window rule on a popped-out room.

## JT's Senior Recital — the show

The room is **Room 138 (the LAB)**, `data/venues/room138-cbu`: the
Rockstars mobile rig — 4 towers × 6 blinder/RGB lights, 2 trees × 4
36-LED pars, 4 150 W beam movers on the band wall, 4 Solena bars — 40
fixtures, one universe (sACN universe 1 + Art-Net 0:0:0).

The show is generated from the Live set by `tools/author_recital.py`
(`just author-recital`): one cue list per song in
`data/songs/jt-recital/`, the night in `data/shows/jt-recital.ig-show`.
The acts, in order: **Act I: The Originals** (DTLA Cinematic Intro, Drive
To LA, For You, Heart Isn't Ready, No One Knows, Hey!), **Act II: Royal
Blood** (Sleeptalker, Figure It Out, Seven Nation Army, Sad But True, Out
Of The Black), **Act III: The Fun Part** (She's American, WORST PART OF
YOU, Fire, Not Too Much Longer (Live), Know Me (Extended)), and
**Archive** (Better Strangers, Berenstein — in the setlist, not lit).
Each song's look per section comes from its section cards (`SECTIONS
+CC`), its palette from the `DESIGN` table in the script; the FoH notes
ride on the cues as notes. Edit the script and regenerate rather than
editing the JSON.

**Driving the rig**: the renderer sends sACN (multicast, priority 100) and
Art-Net (broadcast) for universe 1 at the venue's rate when output is on
(`--output`). Fixture addresses are in `data/venues/room138-cbu/patch.json`
(packed from 1: towers, trees, movers, wall bars, key bars — **assumed**,
see that venue's README).

## Tooling: the `ignition` program

Studio runs it; these are for checking, rendering and debugging:

```sh
just check-recital                  # the running order, every song found in the set
just check-recital --at 3122        # song / bar / section / cue at a beat of the set
just recital --clock studio --watch # what the show would do, following Studio, no window
just recital                        # a debug window on the operator's clock (a stopgap, not the product)
```

`--clock midi:<port>` follows Ableton's MIDI clock + Song Position Pointer
(Live: Preferences → Link, Tempo & MIDI → Sync on for a port that reaches
this machine; `ignition --midi-ports` lists the inputs).

## Rendering the show to video

`ignition --set <als> render --song <title|locator|n> --out <file.mp4> [--audio <file>]
[--camera Wide] [--fps 30] [--size 1920x1080] [--from <s>] [--to <s>]` renders one
song offline (`apps/ignition/src/render.rs`, on `ignition_viz::video::driven`): frame
`i` is the show exactly `i/fps` s after the song's locator, through the set's tempo
map and the live runner's own `Follower`, so a video and a night agree on every cue;
frames are piped to ffmpeg (H.264, BT.709) and the audio is muxed from its first
sample. Beside the video: `<out>.cues.tsv` (the frame each cue lands on), and the
cues as the MP4's chapters. Checked: a snap cue at bar 9 of Drive To LA (12.000 s)
is first seen on frame 360 exactly, and the muxed audio is 0 samples off the source.

`just render-recital [slug …] [--draft] [--jobs 8]` renders the recital's lit
songs side by side (`tools/render_recital.py`): `<recital>/videos/<slug>.mp4` (1080p,
medium quality), or with `--draft` `<recital>/videos/draft/<slug>.mp4` (720p,
`IGNITION_QUALITY=low`), each with `recordings/<slug>.ogg` (else `refs/`) and at least
as long as it (`--min-length`). `just remux-recital` swaps in newer audio,
keeping the picture. Run in the dev shell (the GPU's Vulkan loader).

How it is fast: one Bevy update per frame, the GPU polled without waiting (it waits
only past 4 captures in flight), frames handed to ffmpeg by ownership on a writer
thread, NVENC taking the RGBA as it is (no CPU colour pass; it converts with the
BT.601 matrix, so the stream is tagged `bt470bg`), several songs at once with
`IGNITION_THREADS` workers each (8 jobs: GeForce's NVENC session limit). Measured on
the RTX 4080 sharing the GPU with the live visualizer, at nice 19: the night (16
songs, ~111 000 frames) in 11.6 min as the draft (160 fps overall, 21–31 fps a
song) and 29.7 min at 1080p (62 fps overall, 8–12 fps a song). The GPU is the limit
(each process waits ~30 ms (draft) / ~100 ms (1080p) an update on it), mostly the
volumetric haze; `potato` is faster but breaks the beams into discs.

## Lighting tab view id

**`lighting`** — a view of the `studio.transport` extension (Studio's island,
`apps/architect/island/extension.toml`, `src/lighting.rs` + `src/lighting_view.rs`),
so the setlist type's favourites, declared in that same manifest, may open it:

```toml
[[favourites.tabs]]
id = "lighting"
label = "Lighting"
icon = "lightbulb"
target = { view = "lighting" }
```

It plays the song playing's `<Title> - Lights.mp4` (beside its recording, in the
song's `Media/`), muted, kept on the playhead (seeks past 0.25 s of drift; paused,
stands on the playhead's frame), and a Review button that hands the song note and
the video to Review: `services.call("architect.review", "open", {path, file})`, sent
as the `EXT_SERVICE_CALL` ask. It is an ordinary view, the same in the app and for a
share link's guest. It waits on Architect for these, so until then it names the
video it would play:

- a view handed a video's URL: `ViewFileKind` has only images and lyrics, as bytes
  (`architect: features/spaces/src/view.rs`; built in `apps/web/src/ext_host.rs`);
  the view reads `{path, kind: "video", url}` from `note.files`, or `item.video`;
- media allowed in the view's frame (`media-src 'none'` in
  `architect: features/sites/src/island.rs`);
- the favourite resolving: the web app opens only the island's slot and embed views
  (`Island::views()`, `architect: apps/web/src/ext_host.rs`), not its manifest
  `[[views]]`; and a guest's shell resolves no favourites at all (its vault is
  empty: `architect: apps/web/src/app/vault.rs`, `app/favourites.rs`). On desktop
  and iOS a WASI view draws in Blitz, which has no `<video>` element; and no host
  call gives an extension a playable URL for a vault file (the web app resolves media
  URLs only for its own player, architect#628);
- a web view's asks reaching `services.call`: the frame host forwards only the
  playhead's (`architect: apps/web/src/app/frames.rs`, `architect_playhead::ask`),
  so Review's `EXT_SERVICE_CALL` is refused there.
