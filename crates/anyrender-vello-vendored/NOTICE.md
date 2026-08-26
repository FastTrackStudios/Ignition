# anyrender_vello — vendored copy

Upstream: crates.io `anyrender_vello` 0.12.0
(https://github.com/dioxuslabs/anyrender), licensed MIT OR Apache-2.0
(the standard texts; see the upstream repository). The workspace
`[patch.crates-io]` table points `anyrender_vello` here.

## What changed (marked `IGNITION PATCH` in `src/window_renderer.rs`)

Upstream builds a private `WGPUContext` — its own `wgpu::Instance` and
device pool — per `VelloWindowRenderer`, so a second window means a
second `wgpu::Device`. The studio's embedded Bevy visualizer renders on
Blitz's device and hands the texture back; a texture from one device
cannot be painted by another. The patch shares one `Instance` and one
device pool across every renderer in the process: a later window's
`resume` finds the first window's device (`Adapter::is_surface_supported`)
and reuses it, and `shared_device_handles()` exposes the pool.

Spec: `r[studio.windows.visualizer-anywhere]` in `docs/spec/studio.md`.
