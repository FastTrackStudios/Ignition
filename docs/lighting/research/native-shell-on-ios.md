# Embedding the visualizer in a non-Blitz app (iOS)

How to get a GPU view into the phone app when the rest of the UI is a
webview, and what stops us doing it today.

## The desk's answer does not transfer

On the desk the visualizer is a widget *inside* the document: a Blitz
`<object>` whose rectangle a `Widget` paints, sharing Blitz's own wgpu
device (`apps/ignition-studio/src/viz_widget`). It works because Blitz
renders with wgpu and can be handed a texture.

A webview cannot. Dioxus on iOS is wry, which is a `WKWebView`, and
there is no way to composite a Metal surface into a DOM node inside one.
This is the same wall the Justfile already records for the desk:

> `--renderer webview` builds and runs, but the viewport is empty.

So there is no "widget" to write. The DOM is not a place a GPU surface
can go.

## Invert the hosting

The webview cannot host a GPU surface, so the GPU surface hosts the
webview:

```text
  window            (UIWindow on iOS, winit on a desktop)
    ├── wgpu surface     the visualizer, full-bleed, behind
    └── webview, on top  the Dioxus UI, transparent background
```

Every pixel the document does not paint is the room showing through. The
UI keeps its layout, its scrolling and its event handling; the
visualizer keeps its own render loop. Neither has to know about the
other, which is what makes this tractable — the two halves are siblings
in a view hierarchy, not one inside the other.

This is the ordinary hybrid-app technique for native maps, video and AR
under a web UI. Nothing exotic.

## The machinery exists, in-house, on a fork

`dioxus-desktop` has an `embedded` module that does exactly the hard
half — it attaches a wry webview to a window the **host** owns rather
than owning one:

```rust
pub fn new<W: HasWindowHandle>(
    parent: &W,
    dom: VirtualDom,
    config: EmbeddedConfig,
    wake: impl Fn() + Send + Sync + 'static,
) -> Result<EmbeddedDesktopView, EmbeddedError>
```

with the three things this needs:

- `with_background_color((u8, u8, u8, u8))` — the alpha is the hole.
- `with_bounds` / `set_bounds` — place it in the host's window.
- `poll()` — drive it from the host's frame loop.

Its own doc says what it is for: *"hosts like plugin editors and DAW
extension panels where another framework owns the native parent window
and event loop."* A game window is the same shape of problem.

**But it is not in the revision Ignition pins.** Two dioxus checkouts
are on this machine:

| rev | head commit | `embedded`? |
|---|---|---|
| `f717a8e` | Sync Blitz/Native (Blitz v0.3.0-beta.1) (#5673) | no |
| `32d9f51` | feat(desktop): plumb `linux_offscreen` through `EmbeddedDesktopView` | **yes** |

`f717a8e` is what the root `Cargo.toml` names, from upstream
`DioxusLabs/dioxus`. `32d9f51` carries an FTS-authored commit on top of
`EmbeddedDesktopView`, so the capability is already ours and already in
use by a sibling repo — Ignition is simply on an older upstream rev.

## What it would take

1. **Move Ignition's dioxus rev** to the fork carrying `embedded`. The
   root `Cargo.toml` already warns this rev is not free to move: the
   `dx` CLI in the devshell is pinned to match it (`nix/modules/dx.nix`),
   the studio's Blitz renderer comes from the same tree, and
   `ignition-live-web` builds wasm against it. That is the whole cost of
   this feature and it is a fleet decision, not a file edit.
2. **A shell binary** that owns the window: create the wgpu device, hand
   it to `EmbeddedViz` as a `HostGpu` (which already takes exactly
   `{instance, adapter, device, queue}` — it was written to borrow
   someone else's device), blit its texture to the surface, and attach
   the UI over it with zero alpha. On a desktop this is winit; on iOS,
   UIKit supplies the window and the rest is unchanged.
3. **Nothing in `ignition-viz` has to change.** `EmbeddedViz::render(w,
   h) -> wgpu::Texture` is already the contract that lets the visualizer
   live inside somebody else's frame.

## Why this is worth doing on the desktop first

None of the above is iOS-specific. The same arrangement runs on Linux,
where it can be built, looked at, and debugged without a Mac in the
loop — and the iOS app is then the same code with a different window
provider. A shell that has been run is worth more than a shell that has
been described, and only one of the two can be written from here.

## The alternative, if the rev cannot move

Serve it. The studio already streams its Live view to a tablet over one
WebSocket (`docs/ops/ipad-live.md`, `r[studio.touch.ipad]`); adding the
visualizer's texture as an encoded video track on that socket puts a
picture on any phone with a browser, needs no App Store build, and
leaves the GPU work on the desk. It is not a local 3D view — no picking,
no free camera — but it is the cheapest way to see the room from a
phone, and it does not touch the dioxus pin.
