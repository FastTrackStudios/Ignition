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
