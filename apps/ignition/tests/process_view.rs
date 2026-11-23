//! `ignition --process-view` against a host of this test's own: hello,
//! the buffers, frames with a picture in them, a resize. On Linux the
//! frames are dma-bufs this test imports on a device of its own, as the
//! app does; on macOS they are IOSurfaces; and shared memory when it is
//! forced or the app cannot import, which the renderer says.
//!
//! Needs a GPU and the Live set: `IGNITION_LIVE_SET=<the .als> cargo
//! nextest run -p ignition --run-ignored all -E 'test(a_pane)'`.
#![cfg(any(target_os = "linux", target_os = "macos"))]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::as_conversions,
    clippy::cast_possible_truncation,
    clippy::arithmetic_side_effects
)]

use std::os::fd::OwnedFd;
use std::time::{Duration, Instant};

use architect_process_view::shm;
use architect_process_view::sock::Socket;
use architect_process_view::wire::{Adapter, ToGuest, ToHost, Transport, VERSION};

fn hello(width: u32, height: u32, zero_copy: bool) -> Vec<u8> {
    ToGuest::Hello {
        version: VERSION,
        width,
        height,
        scale: 1.0,
        adapter: Adapter::default(),
        zero_copy,
        theme: architect_workspace::ThemeTokens::default(),
    }
    .encode()
    .unwrap()
}

/// The next message from the guest, within `within`.
fn next(host: &Socket, within: Duration) -> (ToHost, Vec<OwnedFd>) {
    assert!(host.wait(Some(within)).unwrap(), "the guest said nothing");
    let (bytes, fds) = host.recv().unwrap().expect("the guest went away");
    (ToHost::decode(&bytes).unwrap(), fds)
}

/// Frames until one has a lit pixel, releasing each. `pixels` reads a
/// slot's (RGBA8, rows packed).
fn a_lit_frame(host: &Socket, id: u64, pixels: impl Fn(u8) -> Vec<u8>) -> bool {
    let deadline = Instant::now() + Duration::from_secs(90);
    while Instant::now() < deadline {
        host.send(&ToGuest::Tick.encode().unwrap(), &[]).unwrap();
        let (message, _) = next(host, Duration::from_secs(60));
        if let ToHost::Frame { buffers, slot, .. } = message {
            assert_eq!(buffers, id);
            let pixels = pixels(slot);
            let lit = pixels
                .as_chunks::<4>()
                .0
                .iter()
                .any(|p| p[0] > 8 || p[1] > 8 || p[2] > 8);
            host.send(&ToGuest::Release { buffers, slot }.encode().unwrap(), &[])
                .unwrap();
            if lit {
                return true;
            }
        }
    }
    false
}

/// A slot reader over the guest's shared memory (`slots` of `stride` ×
/// `height`).
fn shm_slots(fd: OwnedFd, (stride, height, slots): (u32, u32, u8)) -> impl Fn(u8) -> Vec<u8> {
    let slot_len = (stride * height) as usize;
    let map = shm::Shared::map(fd, slot_len * usize::from(slots), false).unwrap();
    move |slot| {
        // SAFETY: the guest writes a slot only once it is released, after
        // this read.
        unsafe { map.bytes(usize::from(slot) * slot_len, slot_len) }
            .unwrap()
            .to_vec()
    }
}

/// The renderer, started as a pane's program is.
fn start(set: &str, transport: Option<&str>) -> (std::process::Child, Socket) {
    start_with(set, transport, &[])
}

/// [`start`], with more arguments for the renderer.
fn start_with(
    set: &str,
    transport: Option<&str>,
    more: &[String],
) -> (std::process::Child, Socket) {
    let (host, guest) = Socket::pair().unwrap();
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut command = std::process::Command::new(env!("CARGO_BIN_EXE_ignition"));
    command
        .args([
            "--set",
            set,
            "--clock",
            "internal",
            "--start",
            "8",
            "--process-view",
        ])
        .args(more)
        .current_dir(&root)
        .stdin(std::process::Stdio::null());
    if let Some(t) = transport {
        command.env("ARCHITECT_PROCESS_VIEW_TRANSPORT", t);
    }
    architect_process_view::sock::hand_to(&mut command, &guest);
    let child = command.spawn().unwrap();
    drop(guest);
    #[cfg(target_os = "macos")]
    if let Some(r) = architect_process_view::surface::Rendezvous::get() {
        r.expect(child.id());
    }
    (child, host)
}

/// The guest ends when its pane closes (the socket's other end).
fn ends_when_closed(host: Socket, mut child: std::process::Child) {
    drop(host);
    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline {
        if child.try_wait().unwrap().is_some() {
            return;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    let _ = child.kill();
    panic!("the guest did not end when its pane closed");
}

#[test]
#[ignore = "needs a GPU and IGNITION_LIVE_SET"]
fn a_pane_gets_lit_frames_in_shared_memory_and_follows_a_resize() {
    let set = std::env::var("IGNITION_LIVE_SET").expect("IGNITION_LIVE_SET");
    let (child, host) = start(&set, Some("shm"));

    host.send(&hello(640, 360, true), &[]).unwrap();
    let (message, fds) = next(&host, Duration::from_secs(120));
    let ToHost::Buffers {
        id,
        transport,
        width,
        height,
        slots,
        stride,
        ..
    } = message
    else {
        panic!("expected buffers, got {message:?}");
    };
    assert_eq!(transport, Transport::Shm);
    assert_eq!((width, height, stride), (640, 360, 640 * 4));
    let fd = fds.into_iter().next().unwrap();
    assert!(
        a_lit_frame(&host, id, shm_slots(fd, (stride, height, slots))),
        "no frame with a picture"
    );

    host.send(
        &ToGuest::Resize {
            width: 800,
            height: 450,
            scale: 1.0,
        }
        .encode()
        .unwrap(),
        &[],
    )
    .unwrap();
    let (message, fds) = loop {
        let got = next(&host, Duration::from_secs(60));
        if matches!(got.0, ToHost::Buffers { .. }) {
            break got;
        }
    };
    let ToHost::Buffers {
        id,
        width,
        height,
        slots,
        stride,
        ..
    } = message
    else {
        unreachable!()
    };
    assert_eq!((width, height), (800, 450));
    let fd = fds.into_iter().next().unwrap();
    assert!(
        a_lit_frame(&host, id, shm_slots(fd, (stride, height, slots))),
        "no frame after the resize"
    );
    ends_when_closed(host, child);
}

/// A dma-buf frame as the app has it: imported as a texture of a device
/// of this test's (`gpu::import_dmabuf`, what Architect's pane calls),
/// then read back here to look at.
#[cfg(target_os = "linux")]
struct Imported {
    device: wgpu::Device,
    queue: wgpu::Queue,
    textures: Vec<wgpu::Texture>,
    size: (u32, u32),
}

#[cfg(target_os = "linux")]
impl Imported {
    fn new(fds: Vec<OwnedFd>, size: (u32, u32), layout: (u32, u32, u64)) -> Self {
        use architect_process_view::gpu;
        let host = ignition_viz::bench::headless_gpu_on(None, gpu::sharing_features).unwrap();
        assert!(
            gpu::can_share(&host.device),
            "this machine's GPU cannot import dma-bufs"
        );
        let textures = fds
            .into_iter()
            .map(|fd| gpu::import_dmabuf(&host.device, fd, size, layout).unwrap())
            .collect();
        Self {
            device: host.device,
            queue: host.queue,
            textures,
            size,
        }
    }

    /// A slot's pixels (RGBA8, rows packed).
    fn pixels(&self, slot: u8) -> Vec<u8> {
        let (width, height) = self.size;
        let padded = (width * 4).next_multiple_of(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT);
        let buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("the test's look at a frame"),
            size: u64::from(padded) * u64::from(height),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
        encoder.copy_texture_to_buffer(
            self.textures[usize::from(slot)].as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(padded),
                    rows_per_image: Some(height),
                },
            },
            wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );
        self.queue.submit([encoder.finish()]);
        let slice = buffer.slice(..);
        slice.map_async(wgpu::MapMode::Read, |result| result.unwrap());
        self.device
            .poll(wgpu::PollType::Wait {
                submission_index: None,
                timeout: Some(Duration::from_secs(5)),
            })
            .unwrap();
        let mapped = slice.get_mapped_range().unwrap();
        mapped
            .chunks_exact(padded as usize)
            .flat_map(|row| row[..(width * 4) as usize].iter().copied())
            .collect()
    }
}

/// Linux: the frames are dma-bufs, one per slot, which another device
/// imports and finds the room lit in; a resize brings new ones. Nothing
/// of a frame went through the CPU on its way.
#[cfg(target_os = "linux")]
#[test]
#[ignore = "needs a GPU and IGNITION_LIVE_SET"]
fn a_pane_gets_lit_frames_as_dmabufs_on_linux() {
    let set = std::env::var("IGNITION_LIVE_SET").expect("IGNITION_LIVE_SET");
    let (child, host) = start(&set, None);
    host.send(&hello(640, 360, true), &[]).unwrap();
    let mut size = (640, 360);
    for round in 0..2 {
        let (message, fds) = loop {
            let got = next(&host, Duration::from_secs(120));
            if matches!(got.0, ToHost::Buffers { .. }) {
                break got;
            }
        };
        let ToHost::Buffers {
            id,
            transport,
            width,
            height,
            slots,
            stride,
            offset,
            modifier,
        } = message
        else {
            unreachable!()
        };
        assert_eq!(transport, Transport::Dmabuf, "round {round}");
        assert_eq!((width, height), size);
        assert_eq!(fds.len(), usize::from(slots), "a dma-buf per slot");
        let imported = Imported::new(fds, size, (stride, offset, modifier));
        assert!(
            a_lit_frame(&host, id, |slot| imported.pixels(slot)),
            "no lit frame in a dma-buf (round {round})"
        );
        size = (800, 450);
        host.send(
            &ToGuest::Resize {
                width: size.0,
                height: size.1,
                scale: 1.0,
            }
            .encode()
            .unwrap(),
            &[],
        )
        .unwrap();
    }
    ends_when_closed(host, child);
}

/// An app that cannot import the frames (its hello says so) gets them in
/// shared memory, and one that finds out later (a second hello) is
/// answered with shared-memory buffers.
#[test]
#[ignore = "needs a GPU and IGNITION_LIVE_SET"]
fn an_app_that_cannot_import_gets_shared_memory() {
    let set = std::env::var("IGNITION_LIVE_SET").expect("IGNITION_LIVE_SET");
    let (child, host) = start(&set, None);
    // It can, it says.
    host.send(&hello(640, 360, true), &[]).unwrap();
    let (first, _) = next(&host, Duration::from_secs(120));
    let ToHost::Buffers { transport, .. } = first else {
        panic!("expected buffers, got {first:?}");
    };
    // Then it could not after all.
    host.send(&hello(640, 360, false), &[]).unwrap();
    let (message, fds) = loop {
        let got = next(&host, Duration::from_secs(60));
        if matches!(got.0, ToHost::Buffers { .. }) {
            break got;
        }
    };
    let ToHost::Buffers {
        id,
        transport: again,
        height,
        slots,
        stride,
        ..
    } = message
    else {
        unreachable!()
    };
    assert_eq!(again, Transport::Shm, "after {transport:?}");
    let fd = fds.into_iter().next().unwrap();
    assert!(
        a_lit_frame(&host, id, shm_slots(fd, (stride, height, slots))),
        "no frame in shared memory"
    );
    ends_when_closed(host, child);
}

/// macOS: the frames in IOSurfaces the host's process holds, the room
/// lit in them (read on the CPU here; a pane makes a texture over them).
#[cfg(target_os = "macos")]
#[test]
#[ignore = "needs a GPU and IGNITION_LIVE_SET"]
fn a_pane_gets_lit_frames_in_iosurfaces_on_macos() {
    let set = std::env::var("IGNITION_LIVE_SET").expect("IGNITION_LIVE_SET");
    let (child, host) = start(&set, None);
    host.send(&hello(640, 360, true), &[]).unwrap();
    let (message, fds) = next(&host, Duration::from_secs(120));
    let ToHost::Buffers {
        id,
        transport,
        width,
        height,
        slots,
        ..
    } = message
    else {
        panic!("expected buffers, got {message:?}");
    };
    assert_eq!(transport, Transport::IoSurface);
    assert_eq!((width, height), (640, 360));
    assert!(fds.is_empty(), "surfaces travel on the Mach port");
    let surfaces = architect_process_view::surface::Rendezvous::get()
        .unwrap()
        .take(child.id(), id, Duration::from_secs(5))
        .expect("the renderer's surfaces");
    assert_eq!(surfaces.len(), usize::from(slots));
    assert!(
        a_lit_frame(&host, id, |slot| surfaces[usize::from(slot)]
            .pixels()
            .unwrap()),
        "no lit frame in a surface"
    );
    ends_when_closed(host, child);
}

/// The measurement's settings, from the environment.
struct Measured {
    /// `IGNITION_PANE_SIZE` (`<w>x<h>`, 1280x720).
    size: (u32, u32),
    /// `IGNITION_PANE_FRAMES` (300), after 60 that are not counted.
    frames: usize,
    /// `IGNITION_PANE_ARGS`: more arguments for the renderer
    /// (`--start 8 --bar 17 --go`).
    args: Vec<String>,
    /// `IGNITION_HOST_UPLOAD=1`: shared-memory frames are uploaded to a
    /// texture of a device of this test's, as the app does.
    upload: bool,
}

impl Measured {
    fn from_env() -> Self {
        let var = |name: &str| std::env::var(name).ok();
        let size = var("IGNITION_PANE_SIZE").map_or((1280, 720), |v| {
            let (w, h) = v.split_once('x').expect("IGNITION_PANE_SIZE: <w>x<h>");
            (w.parse().unwrap(), h.parse().unwrap())
        });
        Self {
            size,
            frames: var("IGNITION_PANE_FRAMES").map_or(300, |v| v.parse().unwrap()),
            args: var("IGNITION_PANE_ARGS")
                .map(|v| v.split_whitespace().map(str::to_owned).collect())
                .unwrap_or_default(),
            upload: var("IGNITION_HOST_UPLOAD").is_some_and(|v| v == "1"),
        }
    }
}

/// The app's side of a shared-memory frame: the slot's bytes written
/// into a texture of its device (`queue.write_texture`, as Architect's
/// pane does), on a device of this test's.
struct Upload {
    device: wgpu::Device,
    queue: wgpu::Queue,
    texture: wgpu::Texture,
    size: (u32, u32),
}

impl Upload {
    fn new((width, height): (u32, u32)) -> Self {
        let gpu = ignition_viz::bench::headless_gpu().unwrap();
        let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("the app's upload"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::COPY_DST | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        Self {
            device: gpu.device,
            queue: gpu.queue,
            texture,
            size: (width, height),
        }
    }

    /// Uploads a frame: the microseconds the call took (the app's
    /// thread), and those until the GPU had it.
    fn upload(&self, pixels: &[u8]) -> (u32, u32) {
        let began = Instant::now();
        self.queue.write_texture(
            self.texture.as_image_copy(),
            pixels,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(self.size.0 * 4),
                rows_per_image: Some(self.size.1),
            },
            wgpu::Extent3d {
                width: self.size.0,
                height: self.size.1,
                depth_or_array_layers: 1,
            },
        );
        self.queue.submit([]);
        let call = began.elapsed().as_micros() as u32;
        self.device
            .poll(wgpu::PollType::Wait {
                submission_index: None,
                timeout: Some(Duration::from_secs(5)),
            })
            .unwrap();
        (call, began.elapsed().as_micros() as u32)
    }
}

fn p50_p99(times: &mut [u32]) -> (f64, f64) {
    if times.is_empty() {
        return (0.0, 0.0);
    }
    times.sort_unstable();
    (
        f64::from(times[times.len() / 2]) / 1e3,
        f64::from(times[times.len() * 99 / 100]) / 1e3,
    )
}

/// What the renderer costs as a pane's program, with a host that releases
/// and ticks every frame at once (no display to wait for): its own frame
/// time (`render_us`, p50 and p99) and the frames a second it reaches.
/// The renderer says each phase of its frames itself, on stderr (its
/// `ignition::frames` events). Printed, not asserted: `--nocapture` to
/// read it. `IGNITION_TRANSPORT` (`shm`, or unset for the platform's
/// GPU-shared frames), and [`Measured`]'s settings.
#[test]
#[ignore = "a measurement: needs a GPU and IGNITION_LIVE_SET"]
fn the_renderers_frame_time_as_a_pane() {
    let set = std::env::var("IGNITION_LIVE_SET").expect("IGNITION_LIVE_SET");
    let transport = std::env::var("IGNITION_TRANSPORT").ok();
    let measured = Measured::from_env();
    let (child, host) = start_with(&set, transport.as_deref(), &measured.args);
    let (width, height) = measured.size;
    host.send(&hello(width, height, true), &[]).unwrap();
    let (message, fds) = next(&host, Duration::from_secs(120));
    let ToHost::Buffers {
        transport,
        slots,
        stride,
        ..
    } = message
    else {
        panic!("expected buffers, got {message:?}");
    };
    let upload = (measured.upload && transport == Transport::Shm).then(|| {
        let fd = fds.into_iter().next().unwrap();
        (
            Upload::new(measured.size),
            shm_slots(fd, (stride, height, slots)),
        )
    });
    // The first frames compile pipelines: let them pass.
    let mut warm = 0;
    let mut times = Vec::new();
    let (mut upload_call, mut upload_done) = (Vec::new(), Vec::new());
    let mut began = None;
    while times.len() < measured.frames {
        let (message, _) = next(&host, Duration::from_secs(60));
        if let ToHost::Frame {
            buffers,
            slot,
            render_us,
            ..
        } = message
        {
            if let Some((upload, pixels)) = &upload {
                let (call, done) = upload.upload(&pixels(slot));
                if warm >= 60 {
                    upload_call.push(call);
                    upload_done.push(done);
                }
            }
            host.send(&ToGuest::Release { buffers, slot }.encode().unwrap(), &[])
                .unwrap();
            host.send(&ToGuest::Tick.encode().unwrap(), &[]).unwrap();
            if warm < 60 {
                warm += 1;
                continue;
            }
            began.get_or_insert_with(Instant::now);
            times.push(render_us);
        }
    }
    let wall = began.unwrap().elapsed();
    let frames = times.len();
    let (p50, p99) = p50_p99(&mut times);
    eprintln!(
        "renderer as a pane ({transport:?}, {width}x{height}): render p50 {p50:.2} ms, p99 {p99:.2} ms; {:.1} frames/s over {frames} frames",
        frames as f64 / wall.as_secs_f64(),
    );
    if upload.is_some() {
        let (call50, call99) = p50_p99(&mut upload_call);
        let (done50, done99) = p50_p99(&mut upload_done);
        eprintln!(
            "the app's upload of each frame (write_texture, a device of this test's; the copy out of the shared memory is in it): call p50 {call50:.2} ms, p99 {call99:.2} ms; on the GPU after p50 {done50:.2} ms, p99 {done99:.2} ms"
        );
    }
    ends_when_closed(host, child);
}
