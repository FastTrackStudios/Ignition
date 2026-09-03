---
title: Running it
type: reference
order: 6
stage: Doing it
blurb: Building from source, the visualizer, and rendering a video of your own show.
---

# Running it

Ignition is Rust, and the toolchain is pinned in a Nix flake — Bevy 0.19 wants
a newer compiler than the ambient one, so build through the dev shell.

```
git clone https://github.com/FastTrackStudios/Ignition
cd Ignition
nix develop
```

## The studio

```
just studio
```

The desk: patch, groups, the programmer, cue lists, the Live view, and the
visualizer, in a multi-window Dioxus application.

## The visualizer on its own

`ignition-viz` has a binary that opens the room with no desk attached — useful
for judging a look, and the fastest way to see whether the thing is working at
all.

```
just shot                       one frame of the venue, headless, to a PNG
cargo run -p ignition-viz --bin viz -- --venue data/venues/norco
```

Add `--cuelist data/songs/bye-bye-bye.json --bar 61` to put the rig in a
particular moment of a particular [[the-four-files|show file]].

## Rendering a video

The visualizer will render a show to a file, frame by frame against the song's
clock — deterministically, so the same request always produces the same
frames. That is where the clip on the front page came from:

```
just site-video
```

which is this, with the site's framing:

```
cargo run --release -p ignition-viz --bin viz --                \
  --venue data/venues/norco --cuelist data/songs/bye-bye-bye.json \
  --export out --from-bar 61 --to-bar 73 --fps 30                 \
  --camera Wide --haze 1.2 --width 1280 --height 720
```

Without the `ffmpeg` feature this writes a numbered PNG sequence, which
`ffmpeg` will then encode; with it, it writes the H.264 file directly.

## The Live view on an iPad

```
just live-web
```

builds the browser build of the Live view, which the studio then serves at `/`
on the venue's Wi-Fi — the same components as the desk's own Live pane, so
there is one implementation, not two.

## The site

This site is `apps/ignition-web`.

```
just site        build it
just site-dev    serve it with hot reload
```

The guide you are reading is `docs/guides/*.md`, compiled into the page at
build time.

## Where to read next

The specs in `docs/spec/` are the source of truth — `cues.md` and `recipes.md`
for [[recipes-cues-effects|cues and recipes]], `groups.md` and `tricks.md` for
[[roles-selections-tricks|selections]], `song.md` for [[the-song|the song]] —
and every requirement in them carries an id the code and the tests cite back.
`docs/domain/` has the design history and the venue field notes. Start with
`docs/domain/DOMAIN.md`.

---

Previous: [[the-song|The song]] · Up: [[a-show-end-to-end|A Show, End to End]]
