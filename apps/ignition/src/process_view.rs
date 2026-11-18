//! `ignition --process-view`: the visualizer as an Architect process view.
//!
//! Architect shows another program's frames in a pane (architect#641,
//! `architect-process-view`): it starts the program with its end of a
//! socket pair in `ARCHITECT_VIEW_FD`, says hello with the pane's size, and
//! from then on forwards the pane's input and size and shows the frames
//! the program sends. The program's side there (`guest::serve`) draws with
//! Vello on wgpu 30; Bevy is on wgpu 29, so this speaks the wire itself,
//! with the transport's own wgpu-free parts (`sock`, `shm`, `surface`):
//!
//! - **macOS, zero-copy** (`Transport::IoSurface`), when the app can take
//!   it: each slot an `IOSurface` with a texture of this device over it;
//!   a frame is Bevy's target copied into the slot's texture on the GPU,
//!   and the app's device reads the same memory. The surfaces go to the
//!   app's Mach port (`surface::Sender`) before the buffers' message.
//! - **Shared memory** (`Transport::Shm`) everywhere else (Linux, an app
//!   that can't take surfaces, `ARCHITECT_PROCESS_VIEW_TRANSPORT=shm`):
//!   each frame read back and written into a slot of one shared file the
//!   app uploads from.
//!
//! The pane drives the camera: drag to orbit, right-drag (or Shift-drag)
//! to move, the wheel to come closer; the number keys cut to the room's
//! camera presets (`1` is Wide), `0` back to the first.
//!
//! [`Orbit`] is the pane's camera arithmetic, pure and tested.
//!
//! **Where a frame goes.** Each frame is a `pane.frame` span (target
//! `ignition::profile`) whose fields are its phases in microseconds
//! (`crate::frame_stats::FrameTimes`), and every 300 frames
//! (`IGNITION_FRAME_STATS`) one `ignition::frames` event says their p50
//! and p99. [`bench`] draws the same show with no pane and no transport,
//! for the difference (`docs/lighting/research/frame-breakdown.md`).

use std::os::fd::AsFd as _;
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{Context as _, Result};
use architect_process_view::shm;
use architect_process_view::sock::Socket;
use architect_process_view::wire::{self, ToGuest, ToHost, Transport};
use architect_workspace::{Button, ViewInput};
use bevy::math::Vec3;
use ignition_viz::Follow;
use ignition_viz::camera::{ActiveCamera, CameraState, CameraTarget};
use ignition_viz::embedded::{EmbeddedViz, HostGpu};
use ignition_viz::playback::Playback;
use ignition_viz::{DmxUniverses, VizConfig};

use crate::frame_stats::{FrameStats, FrameTimes, micros_since};

/// Frames in flight: the one shown, the one before it, one to draw into.
const SLOTS: u8 = 3;

/// Forces shared memory (`shm`), as for Architect's own guests.
const TRANSPORT_ENV: &str = "ARCHITECT_PROCESS_VIEW_TRANSPORT";

/// `IGNITION_FRAME_DRAIN=1`: wait for the GPU to finish the rendering
/// before the frame is copied out, so the copy's own cost can be read
/// (`FrameTimes::drain`). A measurement's setting: it adds a wait.
const DRAIN_ENV: &str = "IGNITION_FRAME_DRAIN";

/// The frame's span: its phases are recorded on it as they end.
fn frame_span(seq: u64, (width, height): (u32, u32), transport: &str) -> tracing::Span {
    use tracing::field::Empty;
    tracing::info_span!(
        target: "ignition::profile",
        "pane.frame",
        seq,
        width,
        height,
        transport,
        slot = Empty,
        update_us = Empty,
        drain_us = Empty,
        submit_us = Empty,
        wait_us = Empty,
        memcpy_us = Empty,
        send_us = Empty,
        total_us = Empty,
        interval_us = Empty,
    )
}

/// A frame's phases onto its span.
fn record(span: &tracing::Span, times: &FrameTimes) {
    span.record("update_us", times.update);
    span.record("drain_us", times.drain);
    span.record("submit_us", times.submit);
    span.record("wait_us", times.wait);
    span.record("memcpy_us", times.memcpy);
    span.record("send_us", times.send);
    span.record("total_us", times.total);
    span.record("interval_us", times.interval);
}

/// The wire's name of a transport, as the spans and summaries say it.
const fn transport_name(transport: Transport) -> &'static str {
    match transport {
        Transport::Shm => "shm",
        Transport::Dmabuf => "dmabuf",
        Transport::IoSurface => "iosurface",
    }
}

/// The visualizer for a pane of `width` × `height`, on `gpu`.
fn start_viz(
    show: Show,
    (width, height): (u32, u32),
    gpu: HostGpu,
) -> (EmbeddedViz, Arc<dyn Follow>) {
    let Show {
        mut config,
        playback,
        gdtf,
        follow,
    } = show;
    config.width = width;
    config.height = height;
    config.overlay = false;
    let mut viz = EmbeddedViz::new(config, DmxUniverses::new(), playback, Some(gdtf), gpu);
    // The pane owns the keyboard: the number keys are ours to map.
    if let Some(mut active) = viz.app_mut().world_mut().get_resource_mut::<ActiveCamera>() {
        active.keys = false;
    }
    (viz, follow)
}

/// The show where the clock is, and the visualizer stepped: the frame's
/// target, once there is one.
fn update(
    viz: &mut EmbeddedViz,
    follow: &dyn Follow,
    ask: Option<CameraAsk>,
    (width, height): (u32, u32),
) -> Option<wgpu::Texture> {
    let world = viz.app_mut().world_mut();
    if let Some(mut playback) = world.get_resource_mut::<Playback>() {
        follow.follow(&mut playback);
    }
    if let Some(ask) = ask
        && let Some(mut active) = world.get_resource_mut::<ActiveCamera>()
    {
        match ask {
            CameraAsk::Free(state) => active.set_wide_free(state),
            CameraAsk::Cut(target) => {
                let _ = active.set_wide(&target);
            }
        }
    }
    viz.render(width, height)
}

/// Waits for the GPU to finish everything submitted so far.
fn wait_for_gpu(device: &wgpu::Device) -> Result<()> {
    device
        .poll(wgpu::PollType::Wait {
            submission_index: None,
            timeout: Some(Duration::from_secs(10)),
        })
        .map_err(|e| anyhow::anyhow!("waiting for the GPU: {e}"))?;
    Ok(())
}

/// The show drawn `frames` times at `size` with no pane: the renderer a
/// pane runs, stepped and waited for as a pane's frame is, and nothing
/// copied out or sent. What a pane's frame costs beyond this is its
/// transport. The first `warmup` frames (pipelines compiling) are not
/// counted.
///
/// # Errors
///
/// There is no GPU, or it is lost.
pub fn bench(show: Show, size: (u32, u32), frames: u32, warmup: u32) -> Result<()> {
    let gpu = ignition_viz::bench::headless_gpu()?;
    let device = gpu.device.clone();
    let (mut viz, follow) = start_viz(show, size, gpu);
    let mut stats = FrameStats::new(usize::try_from(frames)?);
    for frame in 0..warmup.saturating_add(frames) {
        let began = Instant::now();
        let interval = stats.begin(began);
        let span = frame_span(u64::from(frame), size, "none");
        let _entered = span.enter();
        let _ = update(&mut viz, follow.as_ref(), None, size);
        let update = micros_since(began);
        let waiting = Instant::now();
        wait_for_gpu(&device)?;
        let times = FrameTimes {
            update,
            wait: micros_since(waiting),
            total: micros_since(began),
            interval,
            ..FrameTimes::default()
        };
        record(&span, &times);
        if frame >= warmup
            && let Some(summary) = stats.push(times)
        {
            summary.emit("none", size);
        }
    }
    Ok(())
}

/// The pieces the runner made for a window, made for a pane instead.
pub struct Show {
    pub config: VizConfig,
    pub playback: Playback,
    pub gdtf: ignition_viz::gdtf_geometry::GdtfLibrary,
    pub follow: Arc<dyn Follow>,
}

/// Serves the show into the pane whose socket is in the environment,
/// until the app closes it.
///
/// # Errors
///
/// The socket is not there or breaks, the app speaks another protocol,
/// or there is no GPU.
pub fn serve(show: Show) -> Result<()> {
    let socket = Socket::from_env().context("ARCHITECT_VIEW_FD: not started as a process view")?;
    let (width, height, scale, zero_copy) = loop {
        let Some((bytes, _)) = socket.recv()? else {
            anyhow::bail!("the app closed the view before saying hello");
        };
        if let ToGuest::Hello {
            version,
            width,
            height,
            scale,
            zero_copy,
            ..
        } = ToGuest::decode(&bytes)?
        {
            anyhow::ensure!(
                version == wire::VERSION,
                "the app speaks process-view protocol {version}, this program {}",
                wire::VERSION
            );
            break (width.max(1), height.max(1), scale, zero_copy);
        }
    };
    tracing::info!(width, height, scale, zero_copy, "process view: hello");
    let gpu = ignition_viz::bench::headless_gpu()?;
    let (device, queue) = (gpu.device.clone(), gpu.queue.clone());
    let (mut viz, follow) = start_viz(show, (width, height), gpu);
    let link = Link::new(&device);
    let zero_copy = zero_copy && link.can_share();
    let mut guest = Guest {
        frames: Frames::new(&device, (width, height), 1, zero_copy)?,
        link,
        socket,
        device,
        queue,
        scale: scale.max(0.1),
        seq: 0,
        input: 0,
        credit: true,
        orbit: Orbit::default(),
        drag: None,
        pending_camera: None,
        stats: FrameStats::from_env(),
        drain: std::env::var(DRAIN_ENV).is_ok_and(|v| v == "1"),
    };
    tracing::info!(transport = ?guest.frames.transport(), "process view: frames");
    let served = guest
        .announce()
        .and_then(|()| guest.run(&mut viz, follow.as_ref()));
    // The app closed the view while a message was on its way to it: the
    // pane is gone, which is how this ends, not a failure.
    match served {
        Err(e)
            if e.downcast_ref::<std::io::Error>().is_some_and(|e| {
                matches!(
                    e.kind(),
                    std::io::ErrorKind::BrokenPipe | std::io::ErrorKind::ConnectionReset
                )
            }) =>
        {
            tracing::info!("process view: the app closed the view");
            Ok(())
        }
        served => served,
    }
}

/// Where the frames' GPU-shared slots go: on macOS the app's Mach port
/// (`surface::Sender`), if it named one and this device is a Metal one;
/// nowhere elsewhere (shared memory only).
struct Link {
    #[cfg(target_os = "macos")]
    sender: Option<architect_process_view::surface::Sender>,
}

impl Link {
    fn new(device: &wgpu::Device) -> Self {
        let forced_shm = std::env::var(TRANSPORT_ENV).is_ok_and(|v| v == "shm");
        #[cfg(target_os = "macos")]
        {
            // SAFETY: only asked whether the device is a Metal one.
            let metal = unsafe { device.as_hal::<wgpu::hal::api::Metal>() }.is_some();
            let sender = if forced_shm || !metal {
                None
            } else {
                architect_process_view::surface::Sender::from_env()
                    .inspect_err(|e| tracing::warn!("process view: no IOSurface rendezvous: {e}"))
                    .ok()
                    .flatten()
            };
            Self { sender }
        }
        #[cfg(not(target_os = "macos"))]
        {
            let _ = (device, forced_shm);
            Self {}
        }
    }

    /// Whether frames can go GPU-shared.
    const fn can_share(&self) -> bool {
        #[cfg(target_os = "macos")]
        {
            self.sender.is_some()
        }
        #[cfg(not(target_os = "macos"))]
        {
            false
        }
    }
}

struct Guest {
    socket: Socket,
    link: Link,
    device: wgpu::Device,
    queue: wgpu::Queue,
    frames: Frames,
    scale: f32,
    seq: u64,
    input: u64,
    /// The app showed the last frame: one may be sent.
    credit: bool,
    orbit: Orbit,
    /// A drag in progress: where it was, and whether it moves (pans)
    /// rather than orbits.
    drag: Option<(f32, f32, bool)>,
    /// What the pane asked of the camera since the last frame.
    pending_camera: Option<CameraAsk>,
    /// The frames' phases, summarised a window at a time.
    stats: FrameStats,
    /// Wait for the rendering before copying a frame out (`DRAIN_ENV`).
    drain: bool,
}

#[derive(Debug, Clone)]
enum CameraAsk {
    Free(CameraState),
    Cut(CameraTarget),
}

impl Guest {
    fn announce(&self) -> Result<()> {
        let bytes = self.frames.message().encode()?;
        match &self.frames.slots {
            Slots::Shm { map, .. } => self.socket.send(&bytes, &[map.as_fd()])?,
            // The surfaces first, on the Mach port; then the message that
            // names them.
            #[cfg(target_os = "macos")]
            Slots::Surfaces(slots) => {
                let surfaces: Vec<_> = slots.iter().map(|(s, _)| s.clone()).collect();
                if let Some(sender) = &self.link.sender {
                    sender.send(self.frames.id, &surfaces)?;
                }
                self.socket.send(&bytes, &[])?;
            }
        }
        Ok(())
    }

    fn run(&mut self, viz: &mut EmbeddedViz, follow: &dyn Follow) -> Result<()> {
        // The show moves with the music: draw whenever the app can take a
        // frame, and otherwise wait for it to say so (or for input).
        loop {
            let free = self.frames.free_slot().filter(|_| self.credit);
            let timeout = if free.is_some() {
                Duration::ZERO
            } else {
                Duration::from_millis(50)
            };
            if self.socket.wait(Some(timeout))? {
                let Some((bytes, _)) = self.socket.recv()? else {
                    tracing::info!("process view: the app closed the view");
                    return Ok(());
                };
                self.handle(viz, ToGuest::decode(&bytes)?)?;
                continue;
            }
            if let Some(slot) = free {
                self.draw(viz, follow, slot)?;
            }
        }
    }

    fn handle(&mut self, viz: &mut EmbeddedViz, message: ToGuest) -> Result<()> {
        match message {
            ToGuest::Input { seq, input } => {
                self.input = seq;
                self.input_event(viz, &input);
            }
            ToGuest::Release { buffers, slot } => {
                if buffers == self.frames.id
                    && let Some(held) = self.frames.held.get_mut(usize::from(slot))
                {
                    *held = false;
                }
            }
            ToGuest::Resize {
                width,
                height,
                scale,
            } => {
                self.scale = scale.max(0.1);
                let size = (width.max(1), height.max(1));
                if size != (self.frames.width, self.frames.height) {
                    let id = self.frames.id.saturating_add(1);
                    let shared = self.frames.shared();
                    self.frames = Frames::new(&self.device, size, id, shared)?;
                    self.announce()?;
                }
                self.credit = true;
            }
            ToGuest::Expose | ToGuest::Tick => self.credit = true,
            // Again: a new pane for this program, or the app could not
            // take the GPU-shared frames (it asks for shared memory).
            ToGuest::Hello { zero_copy, .. } => {
                if !zero_copy && self.frames.shared() {
                    let size = (self.frames.width, self.frames.height);
                    let id = self.frames.id.saturating_add(1);
                    self.frames = Frames::new(&self.device, size, id, false)?;
                }
                self.credit = true;
                self.announce()?;
            }
            ToGuest::Theme(_) => {}
        }
        Ok(())
    }

    /// The pane's input, in its logical pixels.
    fn input_event(&mut self, viz: &mut EmbeddedViz, input: &ViewInput) {
        // The pane shows the main view (the wide one; cues cut the
        // programme camera): a drag and the number keys move that.
        let mut current = || {
            viz.app_mut()
                .world_mut()
                .get_resource::<ActiveCamera>()
                .and_then(ActiveCamera::wide_state)
        };
        match input {
            ViewInput::PointerDown { x, y, button, mods } => {
                // Right-drag, or Shift-drag (the keys held came with
                // architect#831), moves; a plain drag turns.
                let pan = !matches!(button, Button::Primary) || mods.contains(&"Shift");
                self.drag = Some((*x, *y, pan));
                if let Some(state) = current() {
                    self.orbit = Orbit::from_state(&state);
                }
            }
            ViewInput::PointerMove { x, y, .. } => {
                if let Some((px, py, pan)) = self.drag {
                    let (dx, dy) = (x - px, y - py);
                    if pan {
                        self.orbit.pan(dx, dy);
                    } else {
                        self.orbit.turn(dx, dy);
                    }
                    self.drag = Some((*x, *y, pan));
                    self.pending_camera = Some(CameraAsk::Free(self.orbit.state()));
                }
            }
            ViewInput::PointerUp { .. } => self.drag = None,
            ViewInput::Wheel { dy, mods, .. } => {
                if let Some(state) = current() {
                    self.orbit = Orbit::from_state(&state);
                }
                if mods.contains(&"Shift") {
                    self.orbit.pan(0.0, *dy);
                } else {
                    self.orbit.zoom(*dy);
                }
                self.pending_camera = Some(CameraAsk::Free(self.orbit.state()));
            }
            ViewInput::Key {
                key, down: true, ..
            } => {
                if key.len() == 1 && key.chars().all(|c| c.is_ascii_digit()) {
                    if let Some(target) = CameraTarget::parse(key) {
                        self.pending_camera = Some(CameraAsk::Cut(target));
                    }
                } else if key == "Shift" {
                    // A Shift-drag moves rather than turns.
                    if let Some(drag) = self.drag.as_mut() {
                        drag.2 = true;
                    }
                }
            }
            _ => {}
        }
    }

    fn draw(&mut self, viz: &mut EmbeddedViz, follow: &dyn Follow, slot: u8) -> Result<()> {
        let began = Instant::now();
        let interval = self.stats.begin(began);
        let size = (self.frames.width, self.frames.height);
        let transport = transport_name(self.frames.transport());
        let span = frame_span(self.seq.saturating_add(1), size, transport);
        let _entered = span.enter();
        span.record("slot", slot);
        let texture = update(viz, follow, self.pending_camera.take(), size);
        let update = micros_since(began);
        let Some(texture) = texture else {
            return Ok(());
        };
        // A resize takes the render world a frame or two: until the new
        // target is there, the old one is the wrong size to send.
        if (texture.width(), texture.height()) != size {
            return Ok(());
        }
        let draining = Instant::now();
        if self.drain {
            wait_for_gpu(&self.device)?;
        }
        let drain = micros_since(draining);
        let copy = self
            .frames
            .copy_in(&self.device, &self.queue, &texture, slot)?;
        if let Some(held) = self.frames.held.get_mut(usize::from(slot)) {
            *held = true;
        }
        self.seq = self.seq.saturating_add(1);
        let sending = Instant::now();
        let message = ToHost::Frame {
            buffers: self.frames.id,
            slot,
            seq: self.seq,
            input: self.input,
            render_us: micros_since(began),
        };
        self.socket.send(&message.encode()?, &[])?;
        self.credit = false;
        let times = FrameTimes {
            update,
            drain,
            submit: copy.submit,
            wait: copy.wait,
            memcpy: copy.memcpy,
            send: micros_since(sending),
            total: micros_since(began),
            interval,
        };
        record(&span, &times);
        if let Some(summary) = self.stats.push(times) {
            summary.emit(transport, size);
        }
        Ok(())
    }
}

/// What copying a frame into its slot took, in microseconds.
#[derive(Debug, Clone, Copy, Default)]
struct CopyTimes {
    /// The copy encoded and submitted.
    submit: u32,
    /// The wait for the GPU to have done it (and all before it).
    wait: u32,
    /// Shared memory: the readback into the slot.
    memcpy: u32,
}

// ---------------------------------------------------------------------------
// The frames' slots
// ---------------------------------------------------------------------------

/// The frames' memory: the slots, which the app holds, and how they are
/// shared.
struct Frames {
    id: u64,
    width: u32,
    height: u32,
    slots: Slots,
    held: Vec<bool>,
}

enum Slots {
    /// One shared file holding every slot, rows packed (`width * 4`
    /// bytes, RGBA8, sRGB-encoded as the target is), and a readback
    /// buffer with rows padded the way wgpu copies them.
    Shm {
        map: shm::Shared,
        readback: wgpu::Buffer,
        padded: u32,
    },
    /// One `IOSurface` per slot and this device's texture over it.
    #[cfg(target_os = "macos")]
    Surfaces(Vec<(architect_process_view::surface::IoSurface, wgpu::Texture)>),
}

impl Frames {
    /// `shared`: GPU-shared slots (macOS), else shared memory.
    fn new(
        device: &wgpu::Device,
        (width, height): (u32, u32),
        id: u64,
        shared: bool,
    ) -> Result<Self> {
        #[cfg(target_os = "macos")]
        let slots = if shared {
            Slots::Surfaces(
                (0..SLOTS)
                    .map(|_| surface_slot(device, width, height))
                    .collect::<Result<_>>()?,
            )
        } else {
            shm_slots(device, (width, height))?
        };
        #[cfg(not(target_os = "macos"))]
        let slots = {
            let _ = shared;
            shm_slots(device, (width, height))?
        };
        Ok(Self {
            id,
            width,
            height,
            slots,
            held: vec![false; usize::from(SLOTS)],
        })
    }

    /// Whether the slots are GPU-shared.
    const fn shared(&self) -> bool {
        !matches!(self.slots, Slots::Shm { .. })
    }

    const fn transport(&self) -> Transport {
        match self.slots {
            Slots::Shm { .. } => Transport::Shm,
            #[cfg(target_os = "macos")]
            Slots::Surfaces(_) => Transport::IoSurface,
        }
    }

    fn message(&self) -> ToHost {
        let stride = match &self.slots {
            Slots::Shm { .. } => self.width.saturating_mul(4),
            #[cfg(target_os = "macos")]
            Slots::Surfaces(slots) => slots.first().map_or(0, |(s, _)| s.stride()),
        };
        ToHost::Buffers {
            id: self.id,
            transport: self.transport(),
            width: self.width,
            height: self.height,
            slots: SLOTS,
            stride,
            offset: 0,
            modifier: 0,
        }
    }

    fn free_slot(&self) -> Option<u8> {
        self.held
            .iter()
            .position(|held| !held)
            .and_then(|i| u8::try_from(i).ok())
    }

    /// Copies `texture` (this size, RGBA8) into `slot`, and waits for it
    /// to be there.
    fn copy_in(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        texture: &wgpu::Texture,
        slot: u8,
    ) -> Result<CopyTimes> {
        let submitting = Instant::now();
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("ignition process view frame"),
        });
        let size = wgpu::Extent3d {
            width: self.width,
            height: self.height,
            depth_or_array_layers: 1,
        };
        match &self.slots {
            Slots::Shm {
                readback, padded, ..
            } => encoder.copy_texture_to_buffer(
                texture.as_image_copy(),
                wgpu::TexelCopyBufferInfo {
                    buffer: readback,
                    layout: wgpu::TexelCopyBufferLayout {
                        offset: 0,
                        bytes_per_row: Some(*padded),
                        rows_per_image: Some(self.height),
                    },
                },
                size,
            ),
            // On the GPU, into the memory the app's device reads.
            #[cfg(target_os = "macos")]
            Slots::Surfaces(slots) => {
                let (_, to) = slots
                    .get(usize::from(slot))
                    .context("a slot past the surfaces")?;
                encoder.copy_texture_to_texture(texture.as_image_copy(), to.as_image_copy(), size);
            }
        }
        let submitted = queue.submit([encoder.finish()]);
        let submit = micros_since(submitting);
        match &self.slots {
            Slots::Shm {
                map,
                readback,
                padded,
            } => {
                // Its readback's mapping waits for the copy.
                let _ = submitted;
                let (wait, memcpy) = self.read_back(device, (map, readback, *padded), slot)?;
                Ok(CopyTimes {
                    submit,
                    wait,
                    memcpy,
                })
            }
            // The frame's pixels are in the surface before the app is
            // told: the CPU-side fence the wire asks for.
            #[cfg(target_os = "macos")]
            Slots::Surfaces(_) => {
                let waiting = Instant::now();
                device
                    .poll(wgpu::PollType::Wait {
                        submission_index: Some(submitted),
                        timeout: Some(Duration::from_secs(2)),
                    })
                    .map_err(|e| anyhow::anyhow!("waiting for the frame: {e}"))?;
                Ok(CopyTimes {
                    submit,
                    wait: micros_since(waiting),
                    memcpy: 0,
                })
            }
        }
    }

    /// The readback buffer's frame into `slot` of the shared memory:
    /// the microseconds waited for the GPU (the rendering, the copy and
    /// the mapping), and those the copy into the slot took.
    fn read_back(
        &self,
        device: &wgpu::Device,
        (map, readback, padded): (&shm::Shared, &wgpu::Buffer, u32),
        slot: u8,
    ) -> Result<(u32, u32)> {
        let waiting = Instant::now();
        let slice = readback.slice(..);
        let (tx, rx) = std::sync::mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |result| {
            let _ = tx.send(result);
        });
        device
            .poll(wgpu::PollType::Wait {
                submission_index: None,
                timeout: Some(Duration::from_secs(2)),
            })
            .map_err(|e| anyhow::anyhow!("waiting for the frame: {e}"))?;
        rx.recv()
            .map_err(|_| anyhow::anyhow!("the readback never mapped"))?
            .map_err(|e| anyhow::anyhow!("mapping the readback: {e}"))?;
        let wait = micros_since(waiting);
        let copying = Instant::now();
        {
            let bytes = slice.get_mapped_range();
            let row = usize::try_from(self.width)? * 4;
            let padded = usize::try_from(padded)?;
            let height = usize::try_from(self.height)?;
            let base = usize::from(slot) * row * height;
            // SAFETY: the app reads a slot only after this frame's message
            // names it, and holds none it was not sent; nothing else in
            // this process touches the mapping.
            let dest =
                unsafe { map.bytes_mut(base, row * height) }.context("a slot past the mapping")?;
            for (y, out) in dest.chunks_exact_mut(row).enumerate() {
                let from = y * padded;
                if let Some(src) = bytes.get(from..from + row) {
                    out.copy_from_slice(src);
                }
            }
        }
        readback.unmap();
        Ok((wait, micros_since(copying)))
    }
}

/// Shared-memory slots for frames of `width` × `height`.
fn shm_slots(device: &wgpu::Device, (width, height): (u32, u32)) -> Result<Slots> {
    let row = width.checked_mul(4).context("a frame too wide")?;
    let padded = row
        .checked_next_multiple_of(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT)
        .context("a frame too wide")?;
    let slot_len = usize::try_from(u64::from(row) * u64::from(height))?;
    let len = slot_len
        .checked_mul(usize::from(SLOTS))
        .context("frames too large")?;
    let map = shm::Shared::create(len)?;
    let readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("ignition process view readback"),
        size: u64::from(padded) * u64::from(height),
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    Ok(Slots::Shm {
        map,
        readback,
        padded,
    })
}

/// A new `IOSurface` and this (Metal) device's texture over it, which a
/// frame is copied into.
#[cfg(target_os = "macos")]
fn surface_slot(
    device: &wgpu::Device,
    width: u32,
    height: u32,
) -> Result<(architect_process_view::surface::IoSurface, wgpu::Texture)> {
    use wgpu::hal;
    let surface = architect_process_view::surface::IoSurface::new(width, height)?;
    // SAFETY: the raw device is used only while `hal_device` lives, to make
    // one texture that is handed to wgpu.
    let hal_device = unsafe { device.as_hal::<hal::api::Metal>() }.context("not a Metal device")?;
    let raw = surface
        .metal_texture(hal_device.raw_device())
        .context("Metal made no texture over the IOSurface")?;
    // SAFETY: a 2D, one-level, one-layer RGBA8Unorm texture of this device,
    // `width` × `height`, as the descriptor below says; Metal keeps the
    // surface alive as long as the texture.
    let hal_texture = unsafe {
        hal::metal::Device::texture_from_raw(
            raw,
            wgpu::TextureFormat::Rgba8Unorm,
            objc2_metal::MTLTextureType::Type2D,
            1,
            1,
            hal::CopyExtent {
                width,
                height,
                depth: 1,
            },
        )
    };
    drop(hal_device);
    // SAFETY: made on this device, respecting the descriptor.
    let texture = unsafe {
        device.create_texture_from_hal::<hal::api::Metal>(
            hal_texture,
            &wgpu::TextureDescriptor {
                label: Some("ignition process view slot"),
                size: wgpu::Extent3d {
                    width,
                    height,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8Unorm,
                usage: wgpu::TextureUsages::COPY_DST
                    | wgpu::TextureUsages::COPY_SRC
                    | wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            },
        )
    };
    Ok((surface, texture))
}

// ---------------------------------------------------------------------------
// The pane's camera
// ---------------------------------------------------------------------------

/// A camera orbiting a point: what a drag in the pane turns, pans and
/// zooms. Z is up, as in the venue.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Orbit {
    pub look: Vec3,
    /// Around Z, radians; 0 looks along +Y (from the house toward the stage).
    pub yaw: f32,
    /// Above the horizon, radians.
    pub pitch: f32,
    pub distance: f32,
    pub fov_deg: f32,
}

impl Default for Orbit {
    fn default() -> Self {
        Self {
            look: Vec3::new(0.0, 0.0, 1.0),
            yaw: 0.0,
            pitch: 0.25,
            distance: 10.0,
            fov_deg: 50.0,
        }
    }
}

impl Orbit {
    /// Radians per logical pixel of drag.
    const TURN: f32 = 0.006;

    /// The orbit a camera state is on.
    #[must_use]
    pub fn from_state(state: &CameraState) -> Self {
        let offset = state.eye - state.look;
        let distance = offset.length().max(0.5);
        let pitch = (offset.z / distance).clamp(-1.0, 1.0).asin();
        // The eye sits *behind* the look point along the view direction.
        let yaw = (-offset.x).atan2(-offset.y);
        Self {
            look: state.look,
            yaw,
            pitch,
            distance,
            fov_deg: state.fov_deg,
        }
    }

    /// The camera state of this orbit.
    #[must_use]
    pub fn state(&self) -> CameraState {
        let (sy, cy) = self.yaw.sin_cos();
        let (sp, cp) = self.pitch.sin_cos();
        let forward = Vec3::new(sy * cp, cy * cp, -sp);
        CameraState::new(self.look - forward * self.distance, self.look, self.fov_deg)
    }

    /// A drag that turns the camera around the look point.
    pub fn turn(&mut self, dx: f32, dy: f32) {
        self.yaw -= dx * Self::TURN;
        self.pitch = (self.pitch + dy * Self::TURN).clamp(-1.4, 1.5);
    }

    /// A drag that moves the look point in the view's plane.
    pub fn pan(&mut self, dx: f32, dy: f32) {
        let per_pixel = self.distance * 0.0015;
        let (sy, cy) = self.yaw.sin_cos();
        let right = Vec3::new(cy, -sy, 0.0);
        self.look += right * (-dx * per_pixel) + Vec3::Z * (dy * per_pixel);
    }

    /// The wheel: closer for a scroll up (negative `dy`).
    pub fn zoom(&mut self, dy: f32) {
        self.distance = (self.distance * (1.0 + dy * 0.002)).clamp(0.5, 200.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: Vec3, b: Vec3) -> bool {
        (a - b).length() < 1e-3
    }

    #[test]
    fn an_orbit_reads_back_the_state_it_was_made_from() {
        let state = CameraState::new(Vec3::new(2.0, -8.0, 3.0), Vec3::new(0.0, 0.0, 1.0), 45.0);
        let orbit = Orbit::from_state(&state);
        let back = orbit.state();
        assert!(
            close(back.eye, state.eye),
            "{:?} vs {:?}",
            back.eye,
            state.eye
        );
        assert!(close(back.look, state.look));
        assert!((back.fov_deg - 45.0).abs() < 1e-6);
    }

    #[test]
    fn turning_keeps_the_distance_and_the_look_point() {
        let mut orbit = Orbit::from_state(&CameraState::new(
            Vec3::new(0.0, -10.0, 2.0),
            Vec3::new(0.0, 0.0, 1.0),
            50.0,
        ));
        let before = orbit.distance;
        orbit.turn(120.0, -40.0);
        let state = orbit.state();
        assert!((state.eye.distance(state.look) - before).abs() < 1e-3);
        assert!(close(state.look, Vec3::new(0.0, 0.0, 1.0)));
        assert!(state.eye.x.abs() > 0.1, "it moved round");
    }

    #[test]
    fn the_pitch_never_flips_over_the_top() {
        let mut orbit = Orbit::default();
        orbit.turn(0.0, 10_000.0);
        assert!(orbit.pitch <= 1.5);
        orbit.turn(0.0, -10_000.0);
        assert!(orbit.pitch >= -1.4);
    }

    #[test]
    fn the_wheel_comes_closer_and_stops() {
        let mut orbit = Orbit::default();
        orbit.zoom(-100.0);
        assert!(orbit.distance < 10.0);
        for _ in 0..1000 {
            orbit.zoom(-500.0);
        }
        assert!(orbit.distance >= 0.5);
    }

    #[test]
    fn a_pan_moves_the_look_point_not_the_angle() {
        let mut orbit = Orbit::default();
        let (yaw, pitch) = (orbit.yaw, orbit.pitch);
        orbit.pan(100.0, 0.0);
        assert!(orbit.look.x.abs() > 0.1);
        assert_eq!((orbit.yaw, orbit.pitch), (yaw, pitch));
    }
}
