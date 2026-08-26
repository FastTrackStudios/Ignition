//! Two windows from one process, then exit.
//!
//! Proves the vendored multi-window patch end to end: the launch window
//! opens a second one through `open_window`, both render, and after two
//! seconds the first closes both. Needs a display; run it with
//!
//!     cargo run -p dioxus-native --example two_windows --features vello,prelude
//!
//! It prints the device pool at the end: with the shared-device patch
//! in `anyrender_vello` that is one device for two windows.

use dioxus_native::prelude::*;

fn main() {
    dioxus_native::launch(first);
}

fn first() -> Element {
    let proxy = dioxus_native::use_shell_proxy();
    let me = dioxus_native::use_window().id();
    let second = use_hook(|| {
        std::rc::Rc::new(dioxus_native::open_window(
            dioxus_native::WindowAttributes::default().with_title("two_windows: second"),
            second,
            Some(Box::new(|| println!("second window closed"))),
        ))
    });
    use_future(move || {
        let proxy = proxy.clone();
        let second = second.clone();
        async move {
            let start = std::time::Instant::now();
            while second.try_id().is_none() && start.elapsed().as_secs() < 5 {
                tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            }
            println!(
                "second window: {:?}; devices shared: {}",
                second.try_id(),
                anyrender_vello::shared_device_handles().len()
            );
            tokio::time::sleep(std::time::Duration::from_secs(2)).await;
            if let Some(id) = second.try_id() {
                dioxus_native::close_window_via(&proxy, id);
            }
            dioxus_native::close_window_via(&proxy, me);
        }
    });
    rsx! { div { style: "padding: 20px; font-size: 24px", "first window" } }
}

fn second() -> Element {
    rsx! { div { style: "padding: 20px; font-size: 24px", "second window" } }
}
