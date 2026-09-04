# Ignition — common commands. Everything runs inside `nix develop`
# already if you use direnv; otherwise prefix with `nix develop -c`.

# Which monitor the studio goes fullscreen on. `right` is the outermost
# by position, so it survives replugging a display — an output name
# (`DP-3`) or a corner (`6560,0`) also work. THEBATTLESHIP today:
# DP-5 1440x2560 at 0,0 · DP-4 5120x1440 at 1440,0 · DP-3 2560x1440 at 6560,0.
studio_monitor := env_var_or_default("IGNITION_MONITOR", "right")

# The desktop app, hot-reloading, release. THE one to reach for.
#
# `dx serve`, so a change to the rsx appears without a restart and a
# change to Rust rebuilds and relaunches on its own. Native renderer
# only: the visualizer is composited through Blitz's own wgpu device,
# which a webview does not have — `--renderer webview` builds and runs,
# but the viewport is empty. The first serve is a cold build into dx's
# own target dir; after that it is incremental.
#
# `NO_DOWNLOADS=1` is load-bearing: dx's Tailwind step otherwise fetches
# a standalone binary into ~/.cache, which will not run on NixOS. With
# it set, dx uses `which tailwindcss` — supplied by the flake. It also
# means the stylesheets are rebuilt on every serve, so nothing here has
# to run `just tailwind` by hand.
#
# `--release` because the visualizer has to hold 120 fps. A debug Bevy
# is not a slightly slower Bevy: the transform/visibility/render-extract
# passes are all generic-heavy code that optimises away to very little
# and, unoptimised, dominate the frame. The cost is rebuild time — rsx
# hot-reload still applies without a rebuild, but a Rust change is now a
# release compile. `just desktop-dev` is the fast-iteration escape hatch.
desktop *ARGS:
    IGNITION_MONITOR={{studio_monitor}} NO_DOWNLOADS=1 \
        dx serve -p ignition-studio --platform desktop --renderer native \
        --hot-patch false --release {{ARGS}}

# The app unoptimised, for when the Rust edit loop matters more than the
# frame rate. Expect the viewport to be visibly slower.
desktop-dev *ARGS:
    IGNITION_MONITOR={{studio_monitor}} NO_DOWNLOADS=1 \
        dx serve -p ignition-studio --platform desktop --renderer native \
        --hot-patch false {{ARGS}}

# Windowed, for when fullscreen is in the way.
desktop-windowed *ARGS:
    IGNITION_FULLSCREEN=0 just desktop {{ARGS}}

# One shot, release, no watcher: a plain cargo run against the workspace
# `target/`, so it reuses whatever the tests and `just shot` already
# built and starts in seconds once warm.
#
# Not the one to develop in — nothing here notices an edit. It is for
# looking: open the show, take a cue, confirm a change landed, close it.
# `just desktop` is what you leave running.
#
# `just tailwind` first, for 150ms of insurance: the utility classes come
# from the committed `assets/tailwind.css`, and a class added to the rsx
# but not compiled into that sheet does nothing at all, silently. dx
# rebuilds it on every serve; a plain cargo run has no idea it exists.
desktop-once *ARGS:
    just tailwind
    IGNITION_MONITOR={{studio_monitor}} cargo run --release -p ignition-studio {{ARGS}}

# The old names. `just studio` is in the profiling and iPad runbooks, in
# `ignition-profile`'s own docs and in `main.rs`, so it stays working —
# as an alias rather than a second copy, because two recipes running the
# same command is how they come to run different ones.
alias studio := desktop
alias studio-dev := desktop-dev
alias studio-windowed := desktop-windowed
alias studio-once := desktop-once

# The phone app in a desktop window, at iPhone dimensions.
#
# The same `App`, the same wry webview and the same stylesheet the device
# gets, so the screens can be looked at from Linux — an iOS build needs a
# Mac, and the UI wants looking at far more often than it wants shipping.
# What it does not show is the safe-area inset, which resolves to zero
# off-device.
phone *ARGS:
    cargo run -p ignition-mobile --no-default-features --features preview \
        --bin ignition-preview {{ARGS}}

# Compile the stylesheets once, for a plain `cargo run` — which knows
# nothing about dx's Tailwind pipeline. Both apps, because both scan the
# shared `ignition-live-ui` sources: a utility used in a shared pane has
# to be emitted into each sheet that mounts it.
tailwind: _site-tw-input
    tailwindcss -i apps/ignition-studio/tailwind.css \
        -o apps/ignition-studio/assets/tailwind.css
    tailwindcss -i apps/ignition-live-web/tailwind.css \
        -o apps/ignition-live-web/assets/tailwind.css
    tailwindcss -i apps/ignition-web/.tailwind.gen.css \
        -o apps/ignition-web/assets/tailwind.css --minify

# The Live view for an iPad: the same components as the desk, built
# for the browser and copied to where the studio serves them from
# (`apps/ignition-live-web/dist`, at `/` on `IGNITION_LIVE_PORT`).
# Then `IGNITION_LIVE=1 just studio` and open the URL it prints.
# See docs/ops/ipad-live.md. r[impl studio.touch.ipad]
live-web:
    NO_DOWNLOADS=1 dx build -p ignition-live-web --platform web --release
    rm -rf apps/ignition-live-web/dist
    mkdir -p apps/ignition-live-web/dist
    cp -r target/dx/ignition-live-web/release/web/public/. apps/ignition-live-web/dist/

# ── The site's Tailwind sheet ────────────────────────────────────────
#
# `apps/ignition-web` is hand-written CSS except for one thing: the
# guide's knowledge graph is `view-knowledge-graph`'s component, and it is
# styled entirely in Tailwind utilities. Its sheet is compiled from
# `apps/ignition-web/tailwind.css`, which needs `@source` globs into that
# crate and into `architect-ui` — both GIT DEPS, so the globs cannot be
# written literally: a git dep has no stable path on disk.
#
# `cargo metadata` knows where cargo actually resolved them. This asks and
# substitutes the answers into `.tailwind.gen.css`, which is what
# tailwindcss is actually pointed at.
#
# ABSOLUTE PATHS, not the symlink tree keyflow's site uses. The studio's
# and the Live view's sheets use v4 automatic content detection rooted at
# this repo; Tailwind follows a symlink to its target, and a target in the
# nix store is outside anything `.gitignore` or `@source not` can exclude
# — so symlinking the checkouts in here silently added classes to two
# unrelated sheets. See the long comment in apps/ignition-web/tailwind.css.
#
# The failure this guards is silent in the other direction too: a
# `@source` matching nothing is not an error, it just yields fewer classes
# and an unstyled component. Hence the explicit check below.
_site-tw-input:
    #!/usr/bin/env bash
    set -euo pipefail
    resolve() {
        cargo metadata --format-version 1 2>/dev/null \
            | python3 -c "import json,sys,os;p=json.load(sys.stdin)['packages'];print(next(os.path.dirname(x['manifest_path']) for x in p if x['name']=='$1'))"
    }
    ui=$(resolve architect-ui)
    graph=$(resolve view-knowledge-graph)
    for dir in "$ui" "$graph"; do
        if [ -z "$dir" ] || [ ! -d "$dir" ]; then
            echo "cannot resolve a tailwind @source crate — is it in the dependency graph?" >&2
            exit 1
        fi
    done
    sed -e "s|@@ARCHITECT_UI@@|$ui|g" -e "s|@@VIEW_KNOWLEDGE_GRAPH@@|$graph|g" \
        apps/ignition-web/tailwind.css > apps/ignition-web/.tailwind.gen.css

# Compile the site's Tailwind sheet on its own. `just tailwind` does this
# alongside the other two; this is the fast loop when only the site moved.
site-tailwind: _site-tw-input
    tailwindcss -i apps/ignition-web/.tailwind.gen.css \
        -o apps/ignition-web/assets/tailwind.css --minify

# Fail if the sheet is missing what the graph needs. Cheap insurance
# against the silent-@source failure described above.
#
# It greps for the emitted DECLARATIONS, not for the class names that
# produce them, and that is not fussiness: the studio's and the Live
# view's sheets use automatic content detection rooted at this repo, so a
# class name written literally in this file is a class name Tailwind finds
# and emits into both of them. A check that listed the classes it was
# checking for put `cursor: grab` into two sheets that have no graph in
# them. Asserting on the output is also the stronger test.
site-tailwind-check: site-tailwind
    #!/usr/bin/env bash
    set -euo pipefail
    sheet=apps/ignition-web/assets/tailwind.css
    missing=()
    # The pan cursor, the legend's dimmed text, and the palette the node
    # colours are interpolated from — spelled as `oklch(` because naming
    # a colour stem here would be naming a class, with the effect above.
    for rule in 'cursor:grab' 'cursor:grabbing' '--muted-foreground' 'oklch('; do
        grep -q -- "$rule" "$sheet" || missing+=("$rule")
    done
    if [ ${#missing[@]} -ne 0 ]; then
        echo "the site's tailwind sheet is missing: ${missing[*]}" >&2
        echo "the @source globs resolved to nothing — check 'just _site-tw-input'" >&2
        exit 1
    fi
    echo "the site tailwind sheet covers the graph"

# The demo's data, copied from where it is AUTHORED.
#
# `/demo` boots the real visualizer on the real Norco venue, so it needs
# that venue's documents and the visualizer's own models (the people, the
# kit, the screens) served beside the site. `asset!` takes a literal path
# inside the crate, so they have to BE inside it — and 4 MB of glTF and
# JSON that already live in this repo have no business being committed to
# it twice.
#
# So they are copied, gitignored, and remade here. `just site` and
# `just site-dev` depend on this; a bare `cargo check -p ignition-web`
# after a fresh clone needs it run first, which is the same contract
# `_site-tw-input` has and for the same reason.
site-demo-assets:
    #!/usr/bin/env bash
    set -euo pipefail
    venue=apps/ignition-web/assets/venue
    viz=apps/ignition-web/assets/viz
    rm -rf "$venue" "$viz"
    mkdir -p "$venue" "$viz"
    cp data/venues/norco/*.json "$venue"/
    cp data/venues/norco/venue.ig-venue "$venue"/
    # The profile the venue names, under a fixed name: the browser has no
    # `profiles/` beside `venues/` to find it by convention, so the demo
    # passes the document in (see `Venue::from_files`).
    cp data/profiles/ignition.ig-profile "$venue"/profile.ig-profile
    # The show the demo plays.
    cp data/songs/bye-bye-bye.json "$venue"/show.json
    # Only what the room actually draws. `qlc-meshes`, `gdtf-samples` and
    # `ofl-samples` are importer fixtures, not scenery.
    cp -r crates/ignition-viz/assets/people \
          crates/ignition-viz/assets/props \
          crates/ignition-viz/assets/screens \
          crates/ignition-viz/assets/gdtf-primitives "$viz"/
    du -sh "$venue" "$viz"

# The site's tab icon, re-copied from where the mark is AUTHORED
# (`apps/ignition-mobile/ios/icon.svg`) and re-rasterised for the
# browsers that will not take an SVG favicon.
#
# A copy rather than a reference because `asset!` takes a literal path
# inside the crate — the usual build-script answer to a cross-crate read
# does not apply, since a build script can only write to OUT_DIR and
# `asset!` cannot name one. This recipe is what keeps the copy honest:
# run it after changing the mark.
site-icons:
    cp apps/ignition-mobile/ios/icon.svg apps/ignition-web/assets/icon.svg
    ffmpeg -y -loglevel error \
      -i apps/ignition-mobile/ios/Assets.xcassets/AppIcon.appiconset/icon-1024.png \
      -vf scale=32:32 apps/ignition-web/assets/icon-32.png
    ffmpeg -y -loglevel error \
      -i apps/ignition-mobile/ios/Assets.xcassets/AppIcon.appiconset/icon-1024.png \
      -vf scale=180:180 apps/ignition-web/assets/icon-180.png

# The public site — the landing page and the guide
# (apps/ignition-web). Static: no server, no backend, nothing to deploy
# but the directory `dist` ends up as.
site: site-tailwind site-demo-assets
    # `--debug-symbols false`: drops DWARF, which both shrinks the bundle
    # and sidesteps the DWARF-version mismatch that makes wasm-opt abort
    # (dx logs the SIGABRT and ships the UNOPTIMISED wasm, so the failure
    # costs megabytes rather than the build).
    # dx writes content-hashed asset names into `public/` and never
    # prunes the old ones, so a directory built over several commits
    # accumulates every wasm it has ever produced — and `dist` is a copy
    # of that directory. Start from empty.
    #
    # Starting from empty is load-bearing for a second reason now: the
    # guide is pre-rendered into this same directory, and the renderer's
    # cache is configured `clear_cache(false)` (it must be — the cache
    # directory IS the bundle), so a route already in it would be served
    # rather than re-rendered and the build would ship the old html.
    rm -rf target/dx/ignition-web/release/web/public
    # `--ssg` pre-renders the guide: dx builds the app's server as well,
    # runs it, asks it for `static_routes` (the router's static ones plus
    # every note of the guide vault) and requests each, which writes it
    # here as finished HTML. Nothing deploys that server — what ships is
    # still a directory of static files.
    #
    # `--fullstack` because dx decides whether to build a server from the
    # CLIENT's features, and this crate keeps `dioxus/fullstack` on its
    # `server` feature alone (its reqwest would be a second major in the
    # wasm binary — see apps/ignition-web/Cargo.toml). Without the flag
    # there is no server target and `--ssg` silently does nothing.
    #
    # `--force-sequential` because the pre-render borrows
    # `public/index.html` for its page shell and the CLIENT build writes
    # that file; in parallel the pages can come out in Dioxus's bare
    # fallback shell — no title, no charset, no hydration — with the
    # build still reporting success. (dioxus#3518.)
    NO_DOWNLOADS=1 dx build -p ignition-web --platform web --release --debug-symbols false \
        --ssg --fullstack --force-sequential
    # `sitemap.xml` and `rss.xml` are written by build.rs into a
    # gitignored directory. They cannot go through `asset!` — that
    # content-hashes what it touches, and `/sitemap.xml` is the fixed URL
    # every crawler looks for — so they are copied in at the top level.
    cp apps/ignition-web/generated/*.xml target/dx/ignition-web/release/web/public/
    rm -rf apps/ignition-web/dist
    mkdir -p apps/ignition-web/dist
    cp -r target/dx/ignition-web/release/web/public/. apps/ignition-web/dist/

# The site with hot reload. Editing a guide page under `docs/guides/`
# rebuilds too — see the crate's Dioxus.toml watch list.
site-dev: site-tailwind site-demo-assets
    dx serve -p ignition-web --platform web

# The landing page's hero clip: the third chorus of Bye Bye Bye on the
# Norco rig, rendered offline against the song's clock and encoded to
# H.264. Twelve bars at 120 BPM is 24 seconds, which loops without
# either end drawing attention to itself — both are warm chorus light.
#
# Through the PNG sequence and the `ffmpeg` BINARY rather than the crate
# feature, because the feature links libav* into the visualizer and this
# is the only thing in the tree that needs a video file out of it.
#
# Regenerate after a change to the rig, the show or the look of the
# render — the front page's whole claim is that it is the real thing.
SITE_VIDEO_BARS_FROM := "61"
SITE_VIDEO_BARS_TO := "73"
site-video:
    #!/usr/bin/env bash
    set -euo pipefail
    frames=$(mktemp -d -t ignition-site-video-XXXXXX)
    trap 'rm -rf "$frames"' EXIT
    cargo run -q --release -p ignition-viz --bin viz -- \
      --venue data/venues/norco --cuelist data/songs/bye-bye-bye.json \
      --export "$frames" \
      --from-bar {{SITE_VIDEO_BARS_FROM}} --to-bar {{SITE_VIDEO_BARS_TO}} \
      --fps 30 --camera {{quote(CAMERA)}} --haze 1.2 --width 1280 --height 720
    ffmpeg -y -loglevel error -framerate 30 -i "$frames/frame_%06d.png" \
      -c:v libx264 -profile:v high -pix_fmt yuv420p -crf 25 -preset slow \
      -movflags +faststart -an apps/ignition-web/assets/hero.mp4
    # The poster is the frame the page shows before the clip has loaded,
    # so it wants to be a good one rather than the first one.
    ffmpeg -y -loglevel error -i "$frames/frame_000192.png" -q:v 4 \
      apps/ignition-web/assets/hero-poster.jpg
    ls -lh apps/ignition-web/assets/hero.mp4 apps/ignition-web/assets/hero-poster.jpg

# A headless render of the venue. `--overlay` adds the cue sheet.
shot OUT="/tmp/ignition.png" *ARGS:
    cargo run -p ignition-viz --bin viz -- \
        --venue data/venues/norco --snapshot {{OUT}} {{ARGS}}

# One 320×180 still per profile look (baked + authored) for the Looks
# pane's thumbnails. GPU; every look in one process, so a few seconds
# rather than a few seconds each.
#
# Through `--previews` rather than one `--snapshot` per look, because
# that is the path that writes the file name the pane looks for: a slug.
# Naming them after the look wrote "chorus full.png", which the pane
# then referred to as a `file:` URL — and a URL percent-encodes a space,
# which Blitz reads back without decoding. The picture silently did not
# load. See `ignition_viz::preview::slug`.
#
# The wide camera: dead centre of the house, far enough out to hold the
# whole stage. A look is a thing that happens across the room, so the
# frame has to contain the room.
#
# `--auto-exposure off` is the point of the whole target. With the eye
# following each frame, every look is exposed to the same average
# brightness — so a blackout and a full chorus come back looking alike,
# which is exactly the comparison the thumbnails exist to make. Fixed at
# the stage exposure, a dark look *is* dark.
#
#   just look-previews CAMERA="Wide"   to frame them differently
CAMERA := "Wide"
look-previews:
    cargo run -q -p ignition-viz --bin viz -- --venue data/venues/norco \
      --camera {{quote(CAMERA)}} --auto-exposure off --screens-off \
      --width 320 --height 180 \
      --previews data/looks/previews --preview-looks all

# One 16-frame loop per library effect, for the Effects pane's hover
# previews. Every effect renders in a single process — the rig is built
# once and each is staged onto it in turn — so the whole library is a
# couple of minutes rather than a couple of minutes *each*.
#
# The base is a blackout when the effect drives intensity itself, so the
# picture is the effect and nothing else; an effect that only moves or
# only colours is given its own fixtures lit, or there would be nothing
# to see it happen to. The white is the emitters at full rather than the
# rig's warm "Open White", because a yellow cast is the one thing in
# frame that is not the effect. The exposure is fixed, so a dim effect
# looks dim beside a bright one — which is the comparison these exist
# to make. Screens off: a logo on the upstage TVs is a distraction in a
# thumbnail this size. And no ambient fill: the default 0.15 is there so
# a window is never pitch black, but in a preview it means the room is
# plainly visible before the effect has done anything — which reads as
# the effect doing something. At zero, unlit is black.
#
#   just effect-previews EFFECTS="chase,circle"   for just a few
EFFECTS := "all"
FRAMES := "16"
effect-previews:
    cargo run -q -p ignition-viz --bin viz -- --venue data/venues/norco \
      --camera {{quote(CAMERA)}} --auto-exposure off --exposure 2.0 --ambient 0 --haze 1.6 --screens-off \
      --width 320 --height 180 \
      --previews data/effects/previews --preview-frames {{FRAMES}} \
      --preview-effects {{quote(EFFECTS)}}

# A 16-frame loop per profile macro, for the Macros pane. Same
# treatment as the effects: a macro is a little programme that plays out
# over time, so a still of one says nothing.
macro-previews:
    cargo run -q -p ignition-viz --bin viz -- --venue data/venues/norco \
      --camera {{quote(CAMERA)}} --auto-exposure off --exposure 2.0 --ambient 0 \
      --haze 1.6 --screens-off --width 320 --height 180 \
      --previews data/macros/previews --preview-frames {{FRAMES}} --preview-macros all

# The beam test: `data/songs/beam-alias.json` rendered headlessly at
# whatever quality you name, for judging what the volumetric raymarch
# does to a *steep* beam. Every mover at full white into a hazed room,
# the two ranks crossed, nothing else lit.
#
# This is the picture that decides the step count, and it is not the
# benchmark cue: a diagonal beam runs along the camera ray and hides the
# artefact, while a near-vertical one is crossed by it and shows every
# step boundary as a bead or a bulge. A step count chosen against the
# benchmark cue will be too low.
#
#   just beam-test                          medium, into /tmp
#   just beam-test QUALITY=ultra            what it looks like when it is right
#   IGNITION_FOG_STEPS=256 just beam-test   what one dial buys
QUALITY := "medium"
BEAM_OUT := "/tmp/ignition-beam-test.png"
beam-test:
    IGNITION_QUALITY={{QUALITY}} cargo run -q --release -p ignition-viz --bin viz -- \
      --venue data/venues/norco --cuelist data/songs/beam-alias.json --cue 0 \
      --camera {{quote(CAMERA)}} --width 2560 --height 1440 --snapshot {{BEAM_OUT}}
    @echo "wrote {{BEAM_OUT}}"

# The whole beam matrix, both renderers, contact-sheeted.
#
# `beam-test` is one picture and it decides one dial. This is the set:
# a cue per case — vertical, crossed, aimed at the house, long and short
# throws, a par's mouth, and the whole rig at once — rendered through
# the froxel grid and the screen-space march alike, because the two fail
# differently and each was tuned against a case the other hides.
#
#   just beam-matrix                              both sheets into /tmp
#   IGNITION_FOG_STEPS=256 just beam-matrix       what one march dial buys
#   IGNITION_FROXEL_SAMPLES=4 just beam-matrix    what one froxel dial buys
BEAM_MATRIX_OUT := "/tmp/ignition-beam-matrix"
beam-matrix:
    tools/beam_matrix.sh {{BEAM_MATRIX_OUT}} 1280 720

# Everything the library panes draw, in one go.
previews: look-previews effect-previews macro-previews

test:
    cargo test --workspace

# Every workspace member that is ours — i.e. not one of the five
# `crates/*-vendored` forks. Those are upstream code carrying one
# deliberate hunk each; policing their style would mean editing them for
# reasons the NOTICE cannot justify, and every such edit makes the diff
# against upstream harder to read at the next version bump.
vendored := "--exclude anyrender_vello --exclude bevy_pbr --exclude blitz-dom --exclude dioxus-native --exclude gdtf"

# The hygiene gate, and what CI runs. `-D warnings` is the point: a
# clippy run that only prints is a gate nothing has to pass, and this
# tree went from 0 to 55 warnings without anyone noticing. Bevy's two
# unavoidable lints are allowed at the top of `ignition-viz/src/lib.rs`,
# with the reason, rather than by loosening this line.
lint:
    cargo fmt --all --check
    cargo clippy --workspace {{vendored}} --all-targets -- -D warnings

# --- performance ---------------------------------------------------

# The studio with the frame-stage profiler on: a table in the log every
# couple of seconds, sorted by self time, saying which stage of the
# frame spent it. Release, because that is the build whose numbers mean
# anything — see docs/ops/profiling.md.
#
#   just profile ALL=1        every span in the process (needs `just profile-build-trace`)
#   just profile TRACE=/tmp/ignition-trace.json   plus a Perfetto timeline
ALL := "0"
TRACE := ""
profile *ARGS:
    IGNITION_PROFILE={{ if ALL == "1" { "all" } else { "1" } }} \
    {{ if TRACE == "" { "" } else { "IGNITION_PROFILE_TRACE=" + TRACE } }} \
        just studio {{ARGS}}

# The profiler on the *benchmark* cue, with the rig lit and the effects
# running: `data/songs/benchmark.json` on every mover, par, bar, beam
# and hazer at once, chases and figures and colour cycles going, the
# screens playing. This is the worst case and the only number worth
# quoting — an idle transport flatters the studio by a third.
#
#   just profile-bench                          the stage table
#   just profile-bench ALL=1                    down to Bevy's schedules
#   IGNITION_FOG_STEPS=96 just profile-bench    what a quality dial buys
profile-bench *ARGS:
    IGNITION_BENCH=1 just profile {{ARGS}}

# Every quality preset in turn, on the benchmark cue, with the haze
# camera each one resolved to. The table in docs/ops/profiling.md is
# this recipe's output on an RTX 4080 — re-run it on the machine that
# has to hold the show. `r[viz.quality-presets]`
quality-ladder:
    #!/usr/bin/env bash
    set -euo pipefail
    log=$(mktemp -d)/q.log
    for q in potato low medium high ultra; do
      IGNITION_QUALITY=$q IGNITION_BENCH=1 IGNITION_MONITOR={{studio_monitor}}         IGNITION_PROFILE=1 IGNITION_PROFILE_INTERVAL=6 IGNITION_LOG_FILE=$log         timeout 28 cargo run -q --release -p ignition-studio >/dev/null 2>&1 || true
      printf "%-8s " "$q"
      grep "viz.haze: resized" $log | tail -1         | grep -oE "width=[0-9]+ height=[0-9]+ scale=[0-9]+ steps=[0-9]+" | tr "\n" " "
      grep "^profile:" $log | tail -2 | grep -oE "[0-9.]+ fps" | tr "\n" " "
      echo
    done

# `just profile ALL=1` only has Bevy's per-system spans to aggregate if
# the binary was built with them. This is that binary — a full release
# rebuild of the visualizer, so expect a few minutes.
profile-build-trace *ARGS:
    IGNITION_MONITOR={{studio_monitor}} NO_DOWNLOADS=1 \
        dx serve -p ignition-studio --platform desktop --renderer native \
        --hot-patch false --release --features trace {{ARGS}}

# Everything the spans cannot see: Vello's own encoding, wgpu, the image
# decoders behind the library thumbnails, malloc. A sampling profile of
# the whole process, written where `hotspot` or `perf report` can read
# it.
#
# Needs `kernel.perf_event_paranoid <= 1`; the recipe says so rather
# than failing with an empty file. Run the studio first, then this
# against it — sampling from launch would spend the whole profile in
# shader compilation, which is a real cost but not the one that repeats
# every frame.
PERF_SECONDS := "20"
perf-studio:
    #!/usr/bin/env bash
    set -euo pipefail
    paranoid=$(cat /proc/sys/kernel/perf_event_paranoid)
    if [ "$paranoid" -gt 1 ]; then
      echo "perf_event_paranoid is $paranoid; run:" >&2
      echo "  sudo sysctl kernel.perf_event_paranoid=1" >&2
      exit 1
    fi
    pid=$(pgrep -f 'target/.*ignition-studio' | head -1)
    if [ -z "$pid" ]; then echo "no studio running — start one with 'just studio'" >&2; exit 1; fi
    echo "sampling pid $pid for {{PERF_SECONDS}}s"
    perf record --call-graph dwarf -F 999 -p "$pid" -o /tmp/ignition-perf.data \
        -- sleep {{PERF_SECONDS}}
    echo "wrote /tmp/ignition-perf.data — 'perf report -i /tmp/ignition-perf.data' or 'hotspot /tmp/ignition-perf.data'"

# The visualizer's half of the frame, headless and repeatable, through
# the same embedded route the studio uses — `crates/ignition-viz/src/bench.rs`.
# This is what a change to the renderer is judged against; the profiler
# is what tells you which change to make.
BENCH_FRAMES := "300"
bench *ARGS:
    cargo run --release -p ignition-viz --bin viz -- \
        --venue data/venues/norco --bench {{BENCH_FRAMES}} {{ARGS}}
