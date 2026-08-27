# The Live view on an iPad

The studio can serve its Live view to a browser on the same network.
It is the desk's own components (`crates/ignition-live-ui`) compiled to
wasm (`apps/ignition-live-web`), talking to the running studio over one
WebSocket that carries the same `Command` and `Playhead` types the desk
uses — `r[studio.touch.ipad]`. Every connected client, desk windows
included, sees the same playhead, so a fader moved on the iPad shows
moved on the desk within a tick — `r[studio.touch.presence]`.

## Build the web app once

    just live-web

That runs `dx build -p ignition-live-web --platform web --release` and
copies the bundle to `apps/ignition-live-web/dist` (gitignored), which
is where the studio serves it from. Rebuild after any change to
`ignition-live-ui` or the web app.

`dx` insists on a `wasm-bindgen` CLI at the exact version the lock
pins (`0.2.127` today). The dev shell carries that version
(`wasmBindgenFor` in `flake.nix`); when the `wasm-bindgen` crate is
bumped in `Cargo.lock`, bump the flake's version and hashes with it,
or `dx` refuses the tree at startup.

`wasm-opt` may abort on this toolchain; `dx` still writes the bundle
(unoptimised, ~3 MB — fine on a LAN).

## Run the studio serving

Serving is opt-in — there is no authentication, and the socket has a
hand on the rig — so nothing listens unless asked:

    IGNITION_LIVE=1 just studio          # port 8420
    IGNITION_LIVE_PORT=9000 just studio  # another port (also opts in)

At startup the studio logs and prints every LAN address it can find:

    Live view for an iPad: http://192.168.1.20:8420

The same URL is shown on the Live view's mode strip, beside the
operator's name, so it can be read off the desk and typed into Safari.
The server binds `0.0.0.0`; keep the venue Wi-Fi private.

`IGNITION_LIVE_DIST` points the server at a bundle somewhere other than
`apps/ignition-live-web/dist`. Without a bundle, `/` serves a page
saying to run `just live-web`.

## Connect

On the iPad, open the URL in Safari. The page connects to `/ws` on its
own host, receives the bootstrap (surface, desk banks, operator
favourites, the studio's profile), then draws the Live view. A red
strip at the top means the socket is down; it reconnects on its own
every 1.5 s, so a studio restart needs no reload.

## Add to the home screen

Share → *Add to Home Screen*. The page declares
`apple-mobile-web-app-capable` and a fullscreen, landscape manifest, so
launched from the icon it fills the screen with no Safari chrome and no
pinch-zoom (`user-scalable=no`). The Live stylesheet's
`@media (pointer: coarse)` block enlarges every target and disables
page scrolling on the fader tracks — `r[studio.touch]`.

## What travels

- iPad → studio: `Command` as JSON text frames, one per gesture. They
  land on the studio's one command channel, indistinguishable from a
  click on the desk or a MIDI fader.
- studio → iPad: a `{"t":"hello","v":{…}}` bootstrap on connect, then
  `{"t":"playhead","v":{…}}` whenever the engine's playhead changes, at
  most every 50 ms per client.

Nothing else: no show file, no venue, no DMX. The browser caches no
lighting state of its own — `r[studio.one-truth]`.
