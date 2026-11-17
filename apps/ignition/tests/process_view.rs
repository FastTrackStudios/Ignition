//! `ignition --process-view` against a host of this test's own: hello,
//! the shared-memory buffers, frames with a picture in them, a resize.
//!
//! Needs a GPU and the Live set: `IGNITION_LIVE_SET=<the .als> cargo
//! nextest run -p ignition --run-ignored all -E 'test(a_pane)'`.
#![cfg(target_os = "linux")]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use std::os::fd::{AsRawFd as _, OwnedFd};
use std::time::{Duration, Instant};

use architect_process_view::sock::{FD_ENV, Socket};
use architect_process_view::wire::{Adapter, ToGuest, ToHost, Transport, VERSION};

fn hello(width: u32, height: u32) -> Vec<u8> {
    ToGuest::Hello {
        version: VERSION,
        width,
        height,
        scale: 1.0,
        adapter: Adapter::default(),
        dmabuf: false,
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

/// Frames until one has a lit pixel, releasing each; its buffers' size.
fn a_lit_frame(host: &Socket, map: &[u8], width: u32, height: u32, id: u64) -> bool {
    let slot_len = (width * 4 * height) as usize;
    let deadline = Instant::now() + Duration::from_secs(90);
    while Instant::now() < deadline {
        host.send(&ToGuest::Tick.encode().unwrap(), &[]).unwrap();
        let (message, _) = next(host, Duration::from_secs(60));
        if let ToHost::Frame { buffers, slot, .. } = message {
            assert_eq!(buffers, id);
            let pixels = &map[slot as usize * slot_len..(slot as usize + 1) * slot_len];
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

fn map(fd: &OwnedFd, len: usize) -> &'static [u8] {
    // SAFETY: the guest's memfd, `len` long as its Buffers message says;
    // mapped read-only and leaked for the test's life.
    unsafe {
        let ptr = rustix::mm::mmap(
            std::ptr::null_mut(),
            len,
            rustix::mm::ProtFlags::READ,
            rustix::mm::MapFlags::SHARED,
            fd,
            0,
        )
        .unwrap();
        std::slice::from_raw_parts(ptr.cast::<u8>(), len)
    }
}

#[test]
#[ignore = "needs a GPU and IGNITION_LIVE_SET"]
fn a_pane_gets_lit_frames_in_shared_memory_and_follows_a_resize() {
    let set = std::env::var("IGNITION_LIVE_SET").expect("IGNITION_LIVE_SET");
    let (host, guest) = Socket::pair().unwrap();
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut child = std::process::Command::new(env!("CARGO_BIN_EXE_ignition"))
        .args([
            "--set",
            &set,
            "--clock",
            "internal",
            "--start",
            "8",
            "--process-view",
        ])
        .env(FD_ENV, guest.as_raw_fd().to_string())
        .current_dir(&root)
        .stdin(std::process::Stdio::null())
        .spawn()
        .unwrap();
    drop(guest);

    host.send(&hello(640, 360), &[]).unwrap();
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
    let shm = map(&fds[0], (stride * height) as usize * slots as usize);
    assert!(
        a_lit_frame(&host, shm, width, height, id),
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
    let shm = map(&fds[0], (stride * height) as usize * slots as usize);
    assert!(
        a_lit_frame(&host, shm, width, height, id),
        "no frame after the resize"
    );

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
