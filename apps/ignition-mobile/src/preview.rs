//! The phone app in a desktop window, at phone size.
//!
//! Same `App`, same stylesheet, same wry — so what appears here is what
//! appears on the device, give or take the safe-area insets. It exists
//! because the alternative is designing a screen you cannot see: an iOS
//! build needs a Mac, and the UI needs looking at far more often than it
//! needs shipping.
//!
//!     cargo run -p ignition-mobile --features preview --bin ignition-preview
fn main() {
    // iPhone 15 logical points, so the layout is judged at the width it
    // has to survive rather than at whatever the window happens to be.
    dioxus::LaunchBuilder::desktop()
        .with_cfg(
            dioxus::desktop::Config::new().with_window(
                dioxus::desktop::WindowBuilder::new()
                    .with_title("Ignition — phone preview")
                    .with_inner_size(dioxus::desktop::tao::dpi::LogicalSize::new(393.0, 852.0))
                    .with_resizable(true),
            ).with_menu(None),
        )
        .launch(ignition_mobile::App);
}
