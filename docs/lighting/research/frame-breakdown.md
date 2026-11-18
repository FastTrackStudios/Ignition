# Where a pane's frame goes

Measured 2026-11-18 on THEBATTLESHIP (Ryzen 9 9950X3D, RTX 4080, NixOS,
Vulkan), release builds, the JT recital rig (`data/venues/room138-cbu`:
40 emitters, 39 lit and in the haze, 4 with shadow maps), the `medium`
preset (32 fog steps, TAA, SSR), the chorus of song 8 ("Figure It Out",
from bar 31, the clock running). This is the baseline the Bevy move, the
GPU-shared frames and the beam renderer are judged against.

## Reading it

A pane's frame (`apps/ignition/src/process_view.rs`, `draw`) is a
`pane.frame` span (target `ignition::profile`) whose fields are its
phases in microseconds:

| field | what it is |
|---|---|
| `update_us` | the show followed and the visualizer stepped (`viz.render`): the main world's update and the extraction, which waits for the render thread to have finished encoding the frame before |
| `drain_us` | `IGNITION_FRAME_DRAIN=1` only: a wait for the GPU before the copy |
| `submit_us` | the copy out of the target encoded and submitted |
| `wait_us` | the wait for the GPU (`poll(Wait)`): the rendering still running, the copy, and in shared memory the readback's mapping |
| `memcpy_us` | shared memory: the readback copied into the slot |
| `send_us` | the frame's message sent |
| `total_us` | the whole frame (the wire's `render_us`) |
| `interval_us` | from the frame before's start to this one's: `total_us` plus the wait for the app |
| `transport`, `fallback` | how the frame reached the app, and why not on the GPU when it did not |

Every 300 frames (`IGNITION_FRAME_STATS=<n>`, 0 for never) one
`ignition::frames` event says each phase's p50 and p99 and the load
average. `ignition --bench <frames> --size <w>x<h>` draws the same show
through the same code with no pane and no transport.
`features/lighting/tools/frame_breakdown.sh <label>` runs the whole
matrix (interleaved, so a change in load lands on every mode alike) and
writes one line per run.

The devices are headless: nothing here opens a window or touches a
desktop session, the owner's or a test one.

## What the numbers mean (read this before the table)

Three things found while measuring change how the phases read.

1. **The frame is bound by the GPU, and almost all of that is the fog.**
   With no transport at all a frame is 20.6 ms at 2560x1440 and 45.9 ms
   at 5120x1440. The fog's step count alone moves it, linearly (same
   cue, `viz --bench`, 2560x1440, load 12 to 13):

   | `IGNITION_FOG_STEPS` | 8 | 16 | 32 (medium) | 64 |
   |---|---|---|---|---|
   | frame, ms (avg) | 8.4 | 13.0 | 22.6 | 41.2 |

   About 0.59 ms a step: at 32 steps the march is about 19 ms of the
   22.6, and everything else (scene, 4 shadow maps, TAA, SSR, bloom,
   tonemapping, and the CPU) is about 3.6 ms.
2. **The CPU already overlaps the GPU.** The embedded visualizer keeps
   Bevy's pipelined rendering, so `update` returns once the frame is
   extracted and the render thread encodes and submits it afterwards.
   In steady state the interval between frames is the GPU's time for one
   frame, and `wait` is that time less `update`: the two do not add to
   more than the GPU's frame. The report's guess that one frame in
   flight serialises CPU and GPU, and that this is the 10 ms floor, is
   not what happens.
3. **The frame sent is the one before the update just run.** The copy
   is submitted before the render thread has submitted the new frame, so
   it takes the target as the frame before left it. A pane is one frame
   further behind its input than `update` suggests. (Seen in the
   `shm-upload` runs: when the app is busy for a frame's length, the
   next `wait` is 1 to 2 ms, which it could not be if the copy waited
   for the frame just extracted.) Not changed here; copying from the
   render world, after the camera has drawn, would remove it.

So `IGNITION_FRAME_DRAIN` does not split the wait cleanly (it makes the
frame bimodal); the transport's cost is read from the interval, with and
without it.

## Before: Bevy 0.19.1, wgpu 29, frames through shared memory

`origin/main` at 6659bb2 plus the instrumentation. p50 over 300 frames
after 60 not counted; three runs each, interleaved; milliseconds. "load"
is the one-minute load average when each run began: the machine was
building for other threads throughout (16 cores, 32 threads).

### 2560x1440

| | load | update | wait | memcpy | frame (interval) p50 | p99 |
|---|---|---|---|---|---|---|
| no transport (`ignition --bench`) | 33 / 22 / 23 | 6.3 / 3.6 / 3.0 | 14.5 / 17.3 / 17.4 | 0 | **20.6 / 20.9 / 20.4** | 30.7 / 24.5 / 23.5 |
| `viz --bench`, same venue and cue | 36 / 23 / 21 | 8.2 / 3.4 / 2.9 (avg) | 13.3 / 17.2 / 17.8 (avg) | 0 | 20.9 / 20.5 / 20.6 | 32.7 / 23.5 / 23.8 |
| pane, shared memory | 36 / 26 / 18 | 3.3 / 7.0 / 2.9 | 16.9 / 11.6 / 18.1 | 1.3 / 3.7 / 1.0 | **21.6 / 23.3 / 22.0** | 26.9 / 38.9 / 25.2 |
| pane, as the pane asks (shared memory on Linux) | 39 / 23 / 20 | 3.2 / 5.6 / 2.8 | 17.1 / 13.8 / 17.7 | 1.3 / 2.7 / 1.2 | 21.7 / 22.0 / 21.7 | 25.1 / 35.7 / 25.1 |

Frames a second through the pane: 45.4 / 41.2 / 44.8.

### 5120x1440

| | load | update | wait | memcpy | frame (interval) p50 | p99 |
|---|---|---|---|---|---|---|
| no transport (`ignition --bench`) | 35 / 41 / 13 | 4.0 / 5.5 / 3.1 | 41.6 / 40.1 / 41.8 | 0 | **46.3 / 46.0 / 45.1** | 56.9 / 50.9 / 49.9 |
| `viz --bench`, same venue and cue | 38 / 39 / 11 | 3.5 / 4.0 / 3.0 (avg) | 38.9 / 41.4 / 41.8 (avg) | 0 | 41.4 / 45.3 / 44.7 | 53.5 / 50.9 / 51.0 |
| pane, shared memory | 28 / 31 / 11 | 3.5 / 3.5 / 3.2 | 41.0 / 41.3 / 42.1 | 2.6 / 2.8 / 2.3 | **47.7 / 48.6 / 47.9** | 81.1 / 62.1 / 57.3 |
| pane, as the pane asks (shared memory on Linux) | 32 / 36 / 11 | 3.3 / 3.8 / 3.4 | 38.4 / 41.0 / 42.0 | 2.4 / 3.1 / 2.3 | 44.3 / 48.4 / 48.2 | 50.5 / 58.7 / 56.9 |

Frames a second through the pane: 20.9 / 20.3 / 20.6.

### What the transport costs, before

- **In the renderer:** the pane's frame is longer than the same frame
  with no transport by **about 1.0 to 1.5 ms at 2560x1440** (21.6 to 22.0
  against 20.4 to 20.9; the middle run, under a rising load, 2.4) and
  **about 1.7 to 2.8 ms at 5120x1440** (47.7 to 48.6 against 45.1 to
  46.3). That is the copy into the readback buffer and its mapping on
  the GPU's side, and the memcpy (1.0 to 1.3 ms for 14.7 MB, 2.3 to 2.8
  ms for 29.5 MB when the machine is not saturated).
- **In the app:** each shared-memory frame is uploaded
  (`queue.write_texture`). Timed on a device of the test's own, the call
  takes **1.4 to 1.6 ms at 2560x1440 and 2.7 to 2.8 ms at 5120x1440**
  (p50), on the app's thread. (`shm-upload` rows of the script.)
- Together, about **2.5 to 3 ms at 2560x1440 and 4.5 to 5.5 ms at
  5120x1440**, of which the renderer's part is in the frame time and the
  app's part is on its UI thread. The report estimated 3 to 6 ms at
  2560x1440; the renderer's side is smaller than that because the frame
  is GPU-bound and the memcpy is faster than assumed.

## What could not be measured

- **The GPU alone.** The card was shared for every number here: with
  nothing of this running, `nvidia-smi` read 23 to 44 % utilisation (the
  owner's desktop, a browser, a running Studio with a GPU context of
  1 GB). The absolute frame times are therefore upper bounds for an idle
  card; the differences between interleaved modes are what to trust.
- **Which pass the GPU time is in, directly.** Bevy's render
  diagnostics put GPU timestamps on five passes only (bloom 0.17 ms,
  clustering 0.11, auto exposure 0.08, tonemapping 0.04, upscaling
  0.02); the fog, the main pass and the shadow passes have none. The
  fog's share above is from its step count, not from a timestamp.
- **The app's real compositing.** The upload was timed in a test
  process, not in Architect; the time until the upload was on the GPU
  there (11 to 15 ms at 2560x1440) is the renderer's next frame
  occupying the card, not the upload's cost, and is not quoted as one.
  What a pane costs Architect's own frame was not measured.
- **The copy and the mapping apart from the rendering.** See point 2
  above: `IGNITION_FRAME_DRAIN=1` makes the frame bimodal instead.
- **An unloaded machine.** The load average was 11 to 41 throughout;
  `update` doubles under the higher loads (3.0 against 6.3 ms), and the
  p99s move with it.
- **The owner's 10 to 15 ms.** At these sizes this rig and preset are
  20 ms and 46 ms. The 10 ms floor was not reproduced; a smaller pane or
  the Norco rig may be where it was seen.
