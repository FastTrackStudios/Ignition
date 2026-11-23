# The Bevy pin, and what a FastTrackStudios fork carries

Written 2026-11-18, with the move from Bevy 0.19.1 to upstream `main`.
The plan it belongs to is the visualizer report (steps 1 and 2 of its
order of work); the numbers are in [frame-breakdown.md](frame-breakdown.md).

## What is pinned

`bevyengine/bevy` at **`948cc8270204b46fd37353eb786144a677d0c0ce`**
(2026-11-12 21:01 UTC, "Wireframe cull mode", #26482), in the root
`Cargo.toml`'s `[workspace.dependencies]`. Its crates call themselves
`0.20.0-dev`.

There is no `FastTrackStudios/bevy` repository (checked with `gh repo
view` on 2026-11-18), and this work was told not to create one. So the
pin is upstream's own revision, and the two commits the room needs are
carried as a vendored `bevy_pbr` (`vendor/bevy_pbr`, patched in over
the revision's own). That is a stand-in for the fork, not a substitute:
the section below is what to put in the fork so the vendored crate can
go.

## Why that commit

`main` compiles reliably (its merge queue) and its pictures change week
to week (its screenshot comparisons flag a diff on about four pushes in
ten), so the pin is a commit chosen on purpose, and it moves on purpose.
Four conditions chose it; it is the newest commit that meets all four.

1. **At or after Solari's spot lights** (#25913, merged 2026-10-05). It
   is 5 weeks past it, and has the Solari fixes that followed: 16-bit
   index meshes (#26157), indirect dispatch on Metal (#26100), visibility
   (#26259).
2. **On Architect's wgpu major.** `bevy_render` asks for `wgpu = "30"`;
   Architect is on 30.0.1 with `wgpu-hal` from `FastTrackStudios/vendor`
   `v0.7.1`. `Cargo.lock` resolves one wgpu, 30.0.1, with that hal.
3. **Builds on the fleet's Rust, 1.99.1.** This is what stopped the pin
   at 2026-11-12. `main` raised its `rust-version` to 1.100.0 the next
   day (#26518 moved CI to 1.100, and 41a1282e3, #26517, removed
   `bevy_ecs::never` "now that `!` is stable"), and every later commit
   needs it. At the pin, `rust-version` is 1.97.1. The fleet's toolchain
   moves for every repo together, not for one, so a newer pin waits for
   that.
4. **Green on everything upstream runs on a `main` push**, the
   screenshot comparisons on Linux, macOS and Windows included (70 check
   runs succeeded, 9 skipped, none failed; `gh api
   repos/bevyengine/bevy/commits/<sha>/check-runs`). The next commit,
   493cddd19 (the dock UI), the last before the Rust bump, is not: it
   failed the Windows screenshot comparison.

What is in it that Ignition wants, beside the above: clustered lights no
longer disappear close to the camera (#26185), cameras whose texture
usages differ share one main texture (#26018; the room has two or three
cameras), NaN pixels out of the PBR shaders are gone (#26335; they
flashed white through bloom), and the colour grading's shadows-to-
midtones blend is fixed (#26477).

What is **not** in it, because it landed after the Rust bump:

- #26484 (2026-11-17), "Guard the histogram accumulation in
  compute_histogram". The auto-exposure compute shader writes outside
  its 64-bin arrays (Metal reported it). Ignition uses auto exposure,
  and the bug is in 0.19.1 too, so it is not a regression of this move;
  it is the first cherry-pick for the fork.
- #26576 (atmosphere sun speckles; Ignition has no atmosphere).

## What the fork carries

Branch `fts/main` = upstream `948cc8270` plus, in this order:

| # | Commit | Files | Why | Until |
|---|---|---|---|---|
| 1 | "A point or spot light may be volumetric without a shadow map" (`vendor/bevy_pbr/patches/0001-…`) | `crates/bevy_pbr/src/render/light.rs`, 15 lines | Every fixture is in the haze; a dozen can afford a shadow map. | The fog is deleted (the analytic beam renderer). Upstream candidate. |
| 2 | "Volumetric fog: the inverse square stops at a point or spot light's radius" (`patches/0002-…`) | `crates/bevy_pbr/src/volumetric_fog/volumetric_fog.wesl`, 9 lines | No singularity where a beam leaves its fixture. | As 1. Upstream candidate. |
| 3 | Cherry-pick of upstream #26484 (083bfcbc8) | `crates/bevy_post_process/src/auto_exposure/auto_exposure.wesl`, 8 lines | Out-of-bounds writes in the auto-exposure histogram. Not carried here: it is outside `bevy_pbr`, and vendoring a second crate for it was not worth it for a bug 0.19.1 also has. | The pin passes 2026-11-17. |

Tag it `fts-948cc8270-1` (the upstream short SHA, then a counter), as
`FastTrackStudios/vendor` tags and Blitz is pinned by revision. Then, in
the root `Cargo.toml`:

```toml
bevy = { git = "https://github.com/FastTrackStudios/bevy", tag = "fts-948cc8270-1", default-features = false }
```

and delete `vendor/bevy_pbr` and the `[patch."https://github.com/bevyengine/bevy"]`
table. Nothing else names the source.

Not carried, from the 0.19.1 fork: the froxel volumetrics and the view
bindings' compute visibility that served them (`vendor/bevy_pbr/FTS-PATCH.md`).

## Moving the pin

- Only to a `main` commit whose `bevy_render` wgpu major is Architect's.
  `apps/ignition/tests/one_wgpu.rs` fails when `Cargo.lock` holds two.
- Only to a commit that built and whose checks you have read; prefer one
  whose screenshot comparisons passed.
- With a before and after of the same cue (`viz --snapshot`, and `just
  beam-matrix`), because the pictures move.
- A new wgpu major is one paired change, in this order: `FastTrackStudios/vendor`'s
  `wgpu-hal` on the new major; Architect (its Vello, anyrender and Blitz
  forks, `architect-process-view`, the hal's tag); the Bevy pin, to the
  first `main` commit on that major; Ignition, to both; then the
  transport re-checked (`a_pane_gets_lit_frames_as_dmabufs_on_linux`)
  and the frame breakdown re-measured. Until all four can move, the pin
  holds at its last commit on the shared major, and a Bevy fix needed
  meanwhile is cherry-picked into the fork.

## One wgpu: what is enforced, and what would take an Architect change

Enforced in Ignition:

- **A mismatch does not build.** `apps/ignition` takes
  `architect-process-view`'s `guest` feature and exports each slot with
  its `gpu::export_texture` / `gpu::export_surface`, on the device Bevy
  draws with. Those functions take the workspace's one `wgpu::Device`;
  were Bevy on another wgpu, `apps/ignition/src/frames.rs` would not
  type-check. There is no second, wgpu-free copy of the path to fall
  into at build time.
- **A mismatch in the lockfile fails a test** (`tests/one_wgpu.rs`, in
  CI's `cargo test --workspace`): Bevy's renderer and Architect's
  process views must be locked to the same wgpu, and no second major
  may be there for anything but Architect's own `pixels` 0.17
  (`architect-workspace-hack` → `dioxus-native` → `anyrender_vello_cpu`
  → `pixels_window_renderer`), which is on wgpu 29 and off the frame
  path. So the lockfile still holds two majors, as Architect's own does;
  it holds one on the path a frame takes. Getting to literally one is
  Architect's to do (drop or bump `pixels`), after which
  `OFF_THE_FRAME_PATH` in that test is emptied.
- **A fallback at run time is loud.** When the slots are shared memory
  the renderer warns once per set of buffers with the reason (`forced`,
  `host-cannot-import`, `device-cannot-export`, `no-rendezvous`,
  `export-refused`), and every frame's span carries `transport` and
  `fallback`.
- **The pane no longer forces shared memory**
  (`features/lighting/pane/src/ext.rs`).

Not done, because it is Architect's: **a strict mode that refuses shared
memory.** What it would need:

1. A third value of `ARCHITECT_PROCESS_VIEW_TRANSPORT`, say
   `gpu-only`, read by `guest::serve` (Architect's own guests) and by
   Ignition's `Link` (which reads the same variable): where the guest
   would fall back, it instead ends with an error naming the reason.
2. The host's half: `ProcessViewPane` today answers a failed import by
   saying hello again with `zero_copy: false` (`host.rs`, `fallback`).
   In strict mode it should show the pane's "stopped" state with the
   import error instead, and not restart the program into the same
   failure.
3. A setting or an environment switch on the app that turns strict mode
   on for every process view it starts (development builds, GPU-equipped
   test runs), passed down as that variable.
4. The negotiated transport is already in the pane's stats line
   (`status_line`: "dma-buf", "IOSurface", "shared memory"); a test of
   the app on a capable host can assert it.

A lighter `gpu` feature in `architect-process-view` (the module alone:
`wgpu`, `ash`) would also let a renderer like this one take the export
without the rest of `guest` (Vello, `anyrender_vello`, `architect`,
`pollster`), which Ignition links today and does not call.

## macOS: what was done here, and what a Mac has to verify

Nothing of this was compiled for or run on macOS: this machine is
Linux, and the workspace has no macOS check (CI's gate is Linux; the
iOS workflow builds `ignition-mobile`, which does not link the
renderer).

What changed for macOS, written blind:

- The IOSurface slots are now made by `architect_process_view::gpu::export_surface`
  on Bevy's device, and sent with the same `surface::Sender` as before.
  The hand-written Metal texture over an IOSurface that
  `process_view.rs` carried while Bevy was on another wgpu
  (`surface_slot`, `objc2-metal`) is deleted.
- `Frames`, `Slots::Gpu` and `Surfaces` in `apps/ignition/src/frames.rs`
  have `cfg(target_os = "macos")` arms for the transport
  (`Transport::IoSurface`), the layout (`surface.stride()`) and the
  announcement (the surfaces on the Mach port, then the message).

To verify on a Mac, in this order:

1. `cargo check --workspace --all-targets` (the `cfg(target_os =
   "macos")` arms in `frames.rs` have never met a compiler; `gpu::Exported`
   there is `{ texture, surface }`).
2. `IGNITION_LIVE_SET=… cargo test --release -p ignition --test
   process_view -- --ignored a_pane_gets_lit_frames_in_iosurfaces_on_macos`
   (the existing test: lit frames in IOSurfaces the host's process
   holds), and `an_app_that_cannot_import_gets_shared_memory`.
3. Bevy at the pin on Metal: `viz --snapshot` of the two cues in
   `frame-breakdown.md` against the pictures there. The pin predates
   upstream #26484, so Metal may report the auto-exposure histogram's
   out-of-bounds writes it reported upstream; that is the fork's third
   commit.
4. `features/lighting/tools/frame_breakdown.sh` on the M4, for its
   numbers (is its 30 fps the fog, as here?).
5. The pane in the Architect app: the stats line reads "IOSurface".
