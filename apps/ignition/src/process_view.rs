//! `ignition --process-view`: the visualizer as an Architect process view.
//!
//! Architect shows another program's frames in a pane (architect#641,
//! `architect-process-view`): it starts the program with a `SOCK_SEQPACKET`
//! end in `ARCHITECT_VIEW_FD`, says hello with the pane's size, and from
//! then on forwards the pane's input and size and shows the frames the
//! program sends. The program's side there (`guest::serve`) draws with
//! Vello on wgpu 30; Bevy is on wgpu 29, so this speaks the wire itself
//! and shares its frames the portable way, **shared memory**
//! (`Transport::Shm`): each frame rendered by the visualizer
//! (`EmbeddedViz`, on a device of its own), read back, and written into a
//! slot of one memfd the app uploads from.
//!
//! The pane drives the camera: drag to orbit, right-drag (or Shift-drag)
//! to move, the wheel to come closer; the number keys cut to the room's
//! camera presets (`1` is Wide), `0` back to the first.
//!
//! [`Orbit`] is the pane's camera arithmetic, pure and tested.

use std::os::fd::{AsFd as _, OwnedFd};
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{Context as _, Result};
use architect_process_view::sock::Socket;
use architect_process_view::wire::{self, ToGuest, ToHost, Transport};
use architect_workspace::{Button, ViewInput};
use bevy::math::Vec3;
use ignition_viz::Follow;
use ignition_viz::camera::{ActiveCamera, CameraState, CameraTarget};
use ignition_viz::embedded::EmbeddedViz;
use ignition_viz::playback::Playback;
use ignition_viz::{DmxUniverses, VizConfig};

/// Frames in flight: the one shown, the one before it, one to draw into.
const SLOTS: u8 = 3;

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
    let (width, height, scale) = loop {
        let Some((bytes, _)) = socket.recv()? else {
            anyhow::bail!("the app closed the view before saying hello");
        };
        if let ToGuest::Hello {
            version,
            width,
            height,
            scale,
            ..
        } = ToGuest::decode(&bytes)?
        {
            anyhow::ensure!(
                version == wire::VERSION,
                "the app speaks process-view protocol {version}, this program {}",
                wire::VERSION
            );
            break (width.max(1), height.max(1), scale);
        }
    };
    tracing::info!(width, height, scale, "process view: hello");
    let gpu = ignition_viz::bench::headless_gpu()?;
    let (device, queue) = (gpu.device.clone(), gpu.queue.clone());
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
    let mut guest = Guest {
        frames: Frames::new(&device, (width, height), 1)?,
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
    };
    guest.announce()?;
    guest.run(&mut viz, follow.as_ref())
}

struct Guest {
    socket: Socket,
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
}

#[derive(Debug, Clone)]
enum CameraAsk {
    Free(CameraState),
    Cut(CameraTarget),
}

impl Guest {
    fn announce(&self) -> Result<()> {
        let bytes = self.frames.message().encode()?;
        self.socket.send(&bytes, &[self.frames.file.as_fd()])?;
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
                    self.frames = Frames::new(&self.device, size, id)?;
                    self.announce()?;
                }
                self.credit = true;
            }
            ToGuest::Expose | ToGuest::Tick => self.credit = true,
            ToGuest::Hello { .. } => {
                self.credit = true;
                self.announce()?;
            }
            ToGuest::Theme(_) => {}
        }
        Ok(())
    }

    /// The pane's input, in its logical pixels.
    fn input_event(&mut self, viz: &mut EmbeddedViz, input: &ViewInput) {
        let mut current = || {
            viz.app_mut()
                .world_mut()
                .get_resource::<ActiveCamera>()
                .map(|a| a.state_at(f32::MAX))
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
        {
            let world = viz.app_mut().world_mut();
            if let Some(mut playback) = world.get_resource_mut::<Playback>() {
                follow.follow(&mut playback);
            }
            if let Some(ask) = self.pending_camera.take()
                && let Some(mut active) = world.get_resource_mut::<ActiveCamera>()
            {
                match ask {
                    CameraAsk::Free(state) => active.set_free(state),
                    CameraAsk::Cut(target) => {
                        let _ = active.cut_to(&target, 0.0, f32::MAX, 120.0);
                    }
                }
            }
        }
        let (width, height) = (self.frames.width, self.frames.height);
        let Some(texture) = viz.render(width, height) else {
            return Ok(());
        };
        // A resize takes the render world a frame or two: until the new
        // target is there, the old one is the wrong size to send.
        if texture.width() != width || texture.height() != height {
            return Ok(());
        }
        self.frames
            .copy_in(&self.device, &self.queue, &texture, slot)?;
        if let Some(held) = self.frames.held.get_mut(usize::from(slot)) {
            *held = true;
        }
        self.seq = self.seq.saturating_add(1);
        let message = ToHost::Frame {
            buffers: self.frames.id,
            slot,
            seq: self.seq,
            input: self.input,
            render_us: u32::try_from(began.elapsed().as_micros()).unwrap_or(u32::MAX),
        };
        self.socket.send(&message.encode()?, &[])?;
        self.credit = false;
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Shared memory
// ---------------------------------------------------------------------------

/// The frames' memory: one memfd holding every slot, rows packed
/// (`width * 4` bytes, RGBA8, sRGB-encoded as the target is), and a
/// readback buffer with rows padded the way wgpu copies them.
struct Frames {
    id: u64,
    width: u32,
    height: u32,
    file: OwnedFd,
    map: Map,
    readback: wgpu::Buffer,
    padded: u32,
    held: Vec<bool>,
}

struct Map {
    ptr: *mut std::ffi::c_void,
    len: usize,
}

impl Drop for Map {
    fn drop(&mut self) {
        // SAFETY: mapped by `Frames::new` with this length; nothing else
        // uses it once dropped.
        let _ = unsafe { rustix::mm::munmap(self.ptr, self.len) };
    }
}

impl Frames {
    fn new(device: &wgpu::Device, (width, height): (u32, u32), id: u64) -> Result<Self> {
        let row = width.checked_mul(4).context("a frame too wide")?;
        let padded = row
            .checked_next_multiple_of(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT)
            .context("a frame too wide")?;
        let slot_len = usize::try_from(u64::from(row) * u64::from(height))?;
        let len = slot_len
            .checked_mul(usize::from(SLOTS))
            .context("frames too large")?;
        let file =
            rustix::fs::memfd_create("ignition-process-view", rustix::fs::MemfdFlags::CLOEXEC)?;
        rustix::fs::ftruncate(&file, u64::try_from(len)?)?;
        // SAFETY: a fresh shared mapping of a file we own, `len` long.
        let ptr = unsafe {
            rustix::mm::mmap(
                std::ptr::null_mut(),
                len,
                rustix::mm::ProtFlags::READ | rustix::mm::ProtFlags::WRITE,
                rustix::mm::MapFlags::SHARED,
                &file,
                0,
            )
        }?;
        let readback = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("ignition process view readback"),
            size: u64::from(padded) * u64::from(height),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        Ok(Self {
            id,
            width,
            height,
            file,
            map: Map { ptr, len },
            readback,
            padded,
            held: vec![false; usize::from(SLOTS)],
        })
    }

    const fn message(&self) -> ToHost {
        ToHost::Buffers {
            id: self.id,
            transport: Transport::Shm,
            width: self.width,
            height: self.height,
            slots: SLOTS,
            stride: self.width.saturating_mul(4),
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

    /// Copies `texture` (this size, RGBA8) into `slot`.
    fn copy_in(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        texture: &wgpu::Texture,
        slot: u8,
    ) -> Result<()> {
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("ignition process view readback"),
        });
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &self.readback,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(self.padded),
                    rows_per_image: Some(self.height),
                },
            },
            wgpu::Extent3d {
                width: self.width,
                height: self.height,
                depth_or_array_layers: 1,
            },
        );
        queue.submit([encoder.finish()]);
        let slice = self.readback.slice(..);
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
        {
            let bytes = slice.get_mapped_range();
            let row = usize::try_from(self.width)? * 4;
            let padded = usize::try_from(self.padded)?;
            let height = usize::try_from(self.height)?;
            let base = usize::from(slot) * row * height;
            anyhow::ensure!(
                base + row * height <= self.map.len,
                "a slot past the mapping"
            );
            // SAFETY: `base..base + row * height` is inside the mapping
            // (checked above), which lives as long as `self`; the app
            // reads a slot only after this frame's message names it.
            let dest = unsafe {
                std::slice::from_raw_parts_mut(self.map.ptr.cast::<u8>().add(base), row * height)
            };
            for (y, out) in dest.chunks_exact_mut(row).enumerate() {
                let from = y * padded;
                if let Some(src) = bytes.get(from..from + row) {
                    out.copy_from_slice(src);
                }
            }
        }
        self.readback.unmap();
        Ok(())
    }
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
