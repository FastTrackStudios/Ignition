//! `ignition --process-view` against a host of this test's own: hello,
//! the shared-memory buffers, frames with a picture in them, a resize;
//! and on macOS the same over IOSurfaces.
//!
//! Needs a GPU and the Live set: `IGNITION_LIVE_SET=<the .als> cargo
//! nextest run -p ignition --run-ignored all -E 'test(a_pane)'`.
#![cfg(any(target_os = "linux", target_os = "macos"))]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
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
