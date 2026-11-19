//! A pane's frames: the slots the app reads them from, and how a frame
//! gets into one.
//!
//! **On the GPU** (`Transport::Dmabuf` on Linux, `Transport::IoSurface`
//! on macOS): each slot is a texture of Bevy's own device whose memory
//! the app's device reads in place (`architect_process_view::gpu`,
//! `export_texture` / `export_surface`). A frame is Bevy's target copied
//! into the slot's texture on the GPU; nothing of it passes through the
//! CPU. That only works because the renderer and the app are on one wgpu
//! (the root manifest): `gpu`'s functions take this crate's
//! `wgpu::Device`, so a second wgpu in the graph does not build.
//!
//! **Shared memory** (`Transport::Shm`) is what is left when the frames
//! cannot be shared: each frame read back (`copy_texture_to_buffer`, a
//! mapping) and copied into a slot of one shared file, which the app
//! then uploads. It costs a few milliseconds a frame at a large pane
//! (`docs/lighting/research/frame-breakdown.md`), so taking it is said
//! out loud, with why ([`Fallback`]): a warning when the slots are made,
//! and the `transport` and `fallback` fields of every frame's span.

use std::os::fd::AsFd as _;
use std::time::{Duration, Instant};

use anyhow::{Context as _, Result};
use architect_process_view::sock::Socket;
use architect_process_view::wire::{ToHost, Transport};
use architect_process_view::{gpu, shm};

use crate::frame_stats::micros_since;

/// Frames in flight: the one shown, the one before it, one to draw into.
pub const SLOTS: u8 = 3;

/// Why a pane's frames go through shared memory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Fallback {
    /// `ARCHITECT_PROCESS_VIEW_TRANSPORT=shm` asked for it.
    Forced,
    /// The app said it cannot import GPU-shared frames (its hello's
    /// `zero_copy`), or asked again after failing to.
    Host,
    /// This device cannot export them: not Vulkan with
    /// `VULKAN_EXTERNAL_MEMORY_DMA_BUF` (Linux), not Metal (macOS).
    Device,
    /// macOS: the app named no Mach port for the surfaces.
    NoRendezvous,
    /// The device refused to make an exportable texture.
    Export(String),
}

impl Fallback {
    /// The reason as a span's field says it.
    #[must_use]
    pub const fn name(&self) -> &'static str {
        match self {
            Self::Forced => "forced",
            Self::Host => "host-cannot-import",
            Self::Device => "device-cannot-export",
            Self::NoRendezvous => "no-rendezvous",
            Self::Export(_) => "export-refused",
        }
    }
}

impl std::fmt::Display for Fallback {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Forced => f.write_str("ARCHITECT_PROCESS_VIEW_TRANSPORT=shm asked for it"),
            Self::Host => f.write_str("the app cannot import GPU-shared frames"),
            Self::Device => f.write_str(
                "this GPU device cannot export frames (Linux: Vulkan with \
                 VULKAN_EXTERNAL_MEMORY_DMA_BUF; macOS: Metal)",
            ),
            Self::NoRendezvous => f.write_str("the app named no Mach port for the surfaces"),
            Self::Export(why) => write!(f, "the device refused an exportable texture: {why}"),
        }
    }
}

/// What copying a frame into its slot took, in microseconds.
#[derive(Debug, Clone, Copy, Default)]
pub struct CopyTimes {
    /// The copy encoded and submitted.
    pub submit: u32,
    /// The wait for the GPU to have done it (and all before it).
    pub wait: u32,
    /// Shared memory: the readback into the slot.
    pub memcpy: u32,
}

/// The frames' memory: the slots, which the app holds, and how they are
/// shared.
pub struct Frames {
    pub id: u64,
    pub width: u32,
    pub height: u32,
    slots: Slots,
    /// Whether the app holds each slot.
    pub held: Vec<bool>,
    /// Why the slots are shared memory, when they are.
    fallback: Option<Fallback>,
}

enum Slots {
    /// One texture per slot the app's GPU reads in place.
    Gpu(Vec<gpu::Exported>),
    /// One shared file holding every slot, rows packed (`width * 4`
    /// bytes, RGBA8, sRGB-encoded as the target is), and a readback
    /// buffer with rows padded the way wgpu copies them.
    Shm {
        map: shm::Shared,
        readback: wgpu::Buffer,
        padded: u32,
    },
}

/// One slot's texture, exported.
fn export(device: &wgpu::Device, width: u32, height: u32) -> Result<gpu::Exported, gpu::GpuError> {
    #[cfg(target_os = "linux")]
    let exported = gpu::export_texture(device, width, height);
    #[cfg(target_os = "macos")]
    let exported = gpu::export_surface(device, width, height);
    exported
}

impl Frames {
    /// Slots for frames of `size`. `share` is whether they can be
    /// GPU-shared, or why not; a device that then refuses to export is
    /// one more reason. Shared memory is said as a warning.
    ///
    /// # Errors
    ///
    /// The shared memory could not be made.
    pub fn new(
        device: &wgpu::Device,
        (width, height): (u32, u32),
        id: u64,
        share: Result<(), Fallback>,
    ) -> Result<Self> {
        let exported = share.and_then(|()| {
            (0..SLOTS)
                .map(|_| export(device, width, height))
                .collect::<Result<Vec<_>, _>>()
                .map_err(|e| Fallback::Export(e.to_string()))
        });
        let (slots, fallback) = match exported {
            Ok(slots) => (Slots::Gpu(slots), None),
            Err(why) => (shm_slots(device, (width, height))?, Some(why)),
        };
        if let Some(why) = &fallback {
            tracing::warn!(
                transport = "shm",
                fallback = why.name(),
                width,
                height,
                "process view: frames are NOT shared on the GPU; each is read back and copied \
                 through shared memory: {why}"
            );
        }
        Ok(Self {
            id,
            width,
            height,
            slots,
            held: vec![false; usize::from(SLOTS)],
            fallback,
        })
    }

    /// Whether the slots are GPU-shared.
    pub const fn shared(&self) -> bool {
        matches!(self.slots, Slots::Gpu(_))
    }

    /// Why the slots are shared memory, when they are.
    pub const fn fallback(&self) -> Option<&Fallback> {
        self.fallback.as_ref()
    }

    pub const fn transport(&self) -> Transport {
        match self.slots {
            Slots::Gpu(_) => GPU_TRANSPORT,
            Slots::Shm { .. } => Transport::Shm,
        }
    }

    /// The message announcing these buffers.
    fn message(&self) -> ToHost {
        let (stride, offset, modifier) = match &self.slots {
            Slots::Gpu(slots) => layout(slots.first()),
            Slots::Shm { .. } => (self.width.saturating_mul(4), 0, gpu::LINEAR),
        };
        ToHost::Buffers {
            id: self.id,
            transport: self.transport(),
            width: self.width,
            height: self.height,
            slots: SLOTS,
            stride,
            offset,
            modifier,
        }
    }

    /// Tells the app of these buffers: the message, with the dma-bufs or
    /// the shared file beside it (macOS: the surfaces first, on
    /// `surfaces`' Mach port).
    ///
    /// # Errors
    ///
    /// The socket, or the Mach port, is broken.
    pub fn announce(&self, socket: &Socket, surfaces: &Surfaces) -> Result<()> {
        let bytes = self.message().encode()?;
        match &self.slots {
            #[cfg(target_os = "linux")]
            Slots::Gpu(slots) => {
                let _ = surfaces;
                let fds: Vec<_> = slots.iter().map(|e| e.fd.as_fd()).collect();
                socket.send(&bytes, &fds)?;
            }
            // The surfaces first, on the Mach port; then the message that
            // names them.
            #[cfg(target_os = "macos")]
            Slots::Gpu(slots) => {
                let sent: Vec<_> = slots.iter().map(|e| e.surface.clone()).collect();
                if let Some(sender) = &surfaces.sender {
                    sender.send(self.id, &sent)?;
                }
                socket.send(&bytes, &[])?;
            }
            Slots::Shm { map, .. } => socket.send(&bytes, &[map.as_fd()])?,
        }
        Ok(())
    }

    pub fn free_slot(&self) -> Option<u8> {
        self.held
            .iter()
            .position(|held| !held)
            .and_then(|i| u8::try_from(i).ok())
    }

    /// Copies `texture` (this size, RGBA8) into `slot`, and waits for it
    /// to be there: the fence the wire asks for is on this side.
    ///
    /// # Errors
    ///
    /// The GPU is lost, or `slot` is not one of these.
    pub fn copy_in(
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
            // On the GPU, into the memory the app's device reads.
            Slots::Gpu(slots) => {
                let to = slots
                    .get(usize::from(slot))
                    .context("a slot past the frames")?;
                encoder.copy_texture_to_texture(
                    texture.as_image_copy(),
                    to.texture.as_image_copy(),
                    size,
                );
            }
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
        }
        let submitted = queue.submit([encoder.finish()]);
        let submit = micros_since(submitting);
        match &self.slots {
            // The frame's pixels are in the slot before the app is told.
            Slots::Gpu(_) => {
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
            let bytes = slice
                .get_mapped_range()
                .map_err(|e| anyhow::anyhow!("reading the readback: {e}"))?;
            let row = usize::try_from(self.width)?.saturating_mul(4);
            let padded = usize::try_from(padded)?;
            let height = usize::try_from(self.height)?;
            let slot_len = row.saturating_mul(height);
            let base = usize::from(slot).saturating_mul(slot_len);
            // SAFETY: the app reads a slot only after this frame's message
            // names it, and holds none it was not sent; nothing else in
            // this process touches the mapping.
            let dest =
                unsafe { map.bytes_mut(base, slot_len) }.context("a slot past the mapping")?;
            for (y, out) in dest.chunks_exact_mut(row).enumerate() {
                let from = y.saturating_mul(padded);
                if let Some(src) = bytes.get(from..from.saturating_add(row)) {
                    out.copy_from_slice(src);
                }
            }
        }
        readback.unmap();
        Ok((wait, micros_since(copying)))
    }
}

/// The transport GPU-shared slots travel as.
#[cfg(target_os = "linux")]
const GPU_TRANSPORT: Transport = Transport::Dmabuf;
/// The transport GPU-shared slots travel as.
#[cfg(target_os = "macos")]
const GPU_TRANSPORT: Transport = Transport::IoSurface;

/// A GPU-shared slot's `(stride, offset, modifier)`, as the buffers'
/// message says them.
#[cfg(target_os = "linux")]
fn layout(slot: Option<&gpu::Exported>) -> (u32, u32, u64) {
    slot.map_or((0, 0, gpu::LINEAR), |e| (e.stride, e.offset, e.modifier))
}

/// A GPU-shared slot's `(stride, offset, modifier)`, as the buffers'
/// message says them.
#[cfg(target_os = "macos")]
fn layout(slot: Option<&gpu::Exported>) -> (u32, u32, u64) {
    (slot.map_or(0, |e| e.surface.stride()), 0, gpu::LINEAR)
}

/// Where GPU-shared slots go beside the socket: on macOS the app's Mach
/// port (`surface::Sender`), if it named one; on Linux nowhere (the
/// dma-bufs travel on the socket).
pub struct Surfaces {
    #[cfg(target_os = "macos")]
    sender: Option<architect_process_view::surface::Sender>,
}

impl Surfaces {
    /// The app's Mach port from the environment (macOS).
    pub fn from_env() -> Self {
        #[cfg(target_os = "macos")]
        {
            let sender = architect_process_view::surface::Sender::from_env()
                .inspect_err(|e| tracing::warn!("process view: no IOSurface rendezvous: {e}"))
                .ok()
                .flatten();
            Self { sender }
        }
        #[cfg(not(target_os = "macos"))]
        Self {}
    }

    /// Nowhere: shared memory was asked for.
    pub const fn none() -> Self {
        #[cfg(target_os = "macos")]
        {
            Self { sender: None }
        }
        #[cfg(not(target_os = "macos"))]
        Self {}
    }

    /// Whether GPU-shared slots can reach the app.
    pub const fn reachable(&self) -> bool {
        #[cfg(target_os = "macos")]
        {
            self.sender.is_some()
        }
        #[cfg(not(target_os = "macos"))]
        true
    }
}

/// Shared-memory slots for frames of `width` × `height`.
fn shm_slots(device: &wgpu::Device, (width, height): (u32, u32)) -> Result<Slots> {
    let row = width.checked_mul(4).context("a frame too wide")?;
    let padded = row
        .checked_next_multiple_of(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT)
        .context("a frame too wide")?;
    let slot_len = usize::try_from(u64::from(row).saturating_mul(u64::from(height)))?;
    let len = slot_len
        .checked_mul(usize::from(SLOTS))
        .context("frames too large")?;
    let map = shm::Shared::create(len)?;
    let readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("ignition process view readback"),
        size: u64::from(padded).saturating_mul(u64::from(height)),
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    Ok(Slots::Shm {
        map,
        readback,
        padded,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_reason_has_its_own_name() {
        let reasons = [
            Fallback::Forced,
            Fallback::Host,
            Fallback::Device,
            Fallback::NoRendezvous,
            Fallback::Export("no memory type fits the image".into()),
        ];
        let names: std::collections::BTreeSet<_> = reasons.iter().map(Fallback::name).collect();
        assert_eq!(names.len(), reasons.len());
        assert!(reasons.iter().all(|reason| !reason.to_string().is_empty()));
    }
}
