//! One pointer for the whole window, so a slider grabbed on its track
//! keeps following the hand after the pointer leaves the track.
//!
//! Neither Blitz nor a browser without pointer capture delivers moves
//! to the element the press began on once the pointer is elsewhere; the
//! window's root sees every move, though, because everything under the
//! pointer bubbles to it. So the host mounts [`PointerRoot`] around its
//! content, that root writes the pointer into a signal, and a latched
//! control reads the signal instead of its own `onpointermove`. A
//! control that is not latched never reads it, so a moving mouse
//! re-renders nothing.
//!
//! The maths of a drag is relative — the level moves by how far the
//! hand moved from where it pressed — so no control has to know its own
//! rectangle, which Blitz reports only relative to whichever child
//! element happens to be under the pointer (the cause of the faders
//! "freaking out": a move over the handle came back in the handle's
//! coordinates, not the track's).

use dioxus::prelude::*;

/// Where the pointer is, in window (client) pixels, and whether a
/// button is down. `ups` counts releases, so a control that latched
/// can tell a release that came after its press from one before it.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Pointer {
    pub x: f32,
    pub y: f32,
    pub down: bool,
    pub ups: u64,
}

#[derive(Clone, Copy)]
pub struct PointerFeed(pub Signal<Pointer>);

/// The window's pointer. Outside a [`PointerRoot`] a control gets a
/// private, never-moving one — it still works from its own events, it
/// just stops at its edge.
pub fn use_pointer_feed() -> Signal<Pointer> {
    let own = use_signal(Pointer::default);
    try_use_context::<PointerFeed>().map(|f| f.0).unwrap_or(own)
}

/// Mount once around everything a window shows.
#[component]
pub fn PointerRoot(children: Element) -> Element {
    let mut feed = use_signal(Pointer::default);
    use_context_provider(|| PointerFeed(feed));
    rsx! {
        div {
            class: "pointer-root",
            onpointermove: move |e| {
                let p = e.data.client_coordinates();
                let mut cur = feed.peek().clone();
                cur.x = p.x as f32;
                cur.y = p.y as f32;
                if cur != *feed.peek() {
                    feed.set(cur);
                }
            },
            onpointerdown: move |e| {
                let p = e.data.client_coordinates();
                let mut cur = feed.peek().clone();
                cur.x = p.x as f32;
                cur.y = p.y as f32;
                cur.down = true;
                feed.set(cur);
            },
            onpointerup: move |e| {
                let p = e.data.client_coordinates();
                let mut cur = feed.peek().clone();
                cur.x = p.x as f32;
                cur.y = p.y as f32;
                cur.down = false;
                cur.ups += 1;
                feed.set(cur);
            },
            onpointercancel: move |_| {
                let mut cur = feed.peek().clone();
                cur.down = false;
                cur.ups += 1;
                feed.set(cur);
            },
            {children}
        }
    }
}

/// A press on a control: where the pointer was and what the level was,
/// and how many releases had happened, so the next release is the one
/// that lets go.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Latch {
    pub at: (f32, f32),
    pub level: f32,
    pub ups: u64,
}

/// A vertical fader's level after the hand moved from `latch.at.1` to
/// `y` on a track `track` pixels tall: up is more.
pub fn drag_up(latch: &Latch, y: f32, track: f32) -> f32 {
    let track = track.max(1.0);
    (latch.level + (latch.at.1 - y) / track).clamp(0.0, 1.0)
}

/// A horizontal slider's level after the hand moved from `latch.at.0`
/// to `x` on a track `width` pixels wide: right is more.
pub fn drag_right(latch: &Latch, x: f32, width: f32) -> f32 {
    let width = width.max(1.0);
    (latch.level + (x - latch.at.0) / width).clamp(0.0, 1.0)
}

/// Whether the release the feed reports came after the press.
pub fn released(latch: &Latch, pointer: &Pointer) -> bool {
    pointer.ups > latch.ups
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The drag is relative to the press, so the value never jumps when
    /// the pointer crosses the handle or leaves the track.
    #[test]
    fn a_drag_moves_the_level_by_how_far_the_hand_went() {
        let latch = Latch {
            at: (40.0, 300.0),
            level: 0.5,
            ups: 3,
        };
        assert_eq!(drag_up(&latch, 300.0, 200.0), 0.5);
        assert_eq!(drag_up(&latch, 200.0, 200.0), 1.0);
        assert!((drag_up(&latch, 350.0, 200.0) - 0.25).abs() < 1e-6);
        // Far past the ends, it stops at the ends.
        assert_eq!(drag_up(&latch, -1000.0, 200.0), 1.0);
        assert_eq!(drag_up(&latch, 1000.0, 200.0), 0.0);
        assert!((drag_right(&latch, 85.0, 90.0) - 1.0).abs() < 1e-6);
        assert!((drag_right(&latch, 40.0 - 45.0, 90.0) - 0.0).abs() < 1e-6);
        // A degenerate track does not divide by zero.
        assert!(drag_up(&latch, 250.0, 0.0) <= 1.0);
    }

    /// A release that happened before the press does not let go; the
    /// next one does.
    #[test]
    fn only_a_release_after_the_press_lets_go() {
        let latch = Latch {
            at: (0.0, 0.0),
            level: 0.0,
            ups: 3,
        };
        let before = Pointer {
            ups: 3,
            down: false,
            ..Default::default()
        };
        let after = Pointer {
            ups: 4,
            down: false,
            ..Default::default()
        };
        assert!(!released(&latch, &before));
        assert!(released(&latch, &after));
    }
}
