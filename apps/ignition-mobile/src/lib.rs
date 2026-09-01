//! The Ignition iPhone app.
//!
//! A prototype only in that it has no link to a running console. The
//! data is real: the shipped NSYNC cue list and Room 138's patch, groups,
//! role bindings and palettes are compiled into the bundle (`show.rs`),
//! so GO drives the real `CuePlayer` over the real forty-fixture rig —
//! tracking, fades, role resolution and the effect library included. The
//! levels below the cue list are what the rig would take.
//!
//! When the vox link lands, `show::` is the only module that changes.
use dioxus::prelude::*;
use ignition_core::{Attribute, ChanId, CuePlayer};
use std::collections::HashMap;
use std::rc::Rc;

pub mod show;

/// Inlined rather than linked.
///
/// `asset!` + `document::Link` only resolves under the `dx` asset
/// pipeline, so the app renders unstyled the moment it is launched any
/// other way — which is exactly how the phone preview runs. The sheet is
/// three kilobytes; carrying it in the binary costs nothing and removes
/// a way for the UI to silently arrive with no CSS at all.
const CSS: &str = include_str!("../assets/mobile.css");

#[derive(Routable, Clone, PartialEq)]
#[rustfmt::skip]
pub enum Route {
    #[layout(Shell)]
    #[route("/")]
    Cues {},
    #[route("/patch")]
    Patch {},
    #[route("/groups")]
    Groups {},
}

// PascalCase is the Dioxus component convention; the launch entrypoint is
// a plain fn rather than a `#[component]`, so the lint needs silencing here.
#[allow(non_snake_case)]
pub fn App() -> Element {
    rsx! {
        document::Style { {CSS} }
        Router::<Route> {}
    }
}

#[component]
fn Shell() -> Element {
    rsx! {
        div { class: "app",
            main { class: "body", Outlet::<Route> {} }
            nav { class: "tabs",
                Link { to: Route::Cues {}, class: "tab", "Cues" }
                Link { to: Route::Patch {}, class: "tab", "Patch" }
                Link { to: Route::Groups {}, class: "tab", "Groups" }
            }
        }
    }
}

/// One fixture's state, as the strip draws it: its resolved colour and
/// how far up it is.
///
/// The colour is the *emitters'*, read straight off the cooked output —
/// a fixture the cue put in Turquoise reports turquoise here. A fixture
/// whose personality has no colour channels reports white, which is the
/// honest answer for a dimmer.
struct Lit {
    chan: ChanId,
    level: f32,
    rgb: (f32, f32, f32),
}

fn lit(chans: &[ChanId], out: &HashMap<(ChanId, Attribute), f32>) -> Vec<Lit> {
    use ignition_core::ColorChannel::{Blue, Green, Red};
    let get = |c: ChanId, ch| {
        out.get(&(c, Attribute::ColorAdd { channel: ch }))
            .copied()
            .unwrap_or(0.0)
    };
    chans
        .iter()
        .map(|&chan| {
            let level = out
                .get(&(chan, Attribute::Dimmer))
                .copied()
                .unwrap_or(0.0)
                .clamp(0.0, 1.0);
            let (r, g, b) = (get(chan, Red), get(chan, Green), get(chan, Blue));
            // No colour channels at all is a dimmer, not a black fixture.
            let rgb = if r + g + b <= f32::EPSILON {
                (1.0, 1.0, 1.0)
            } else {
                (r, g, b)
            };
            Lit { chan, level, rgb }
        })
        .collect()
}

impl Lit {
    /// `background` for the cell: the hue at the fixture's own level, so
    /// a dark fixture is dark rather than a dark *colour*.
    fn css(&self) -> String {
        let s = |v: f32| (v * self.level * 255.0).clamp(0.0, 255.0) as u8;
        format!(
            "background: rgb({}, {}, {})",
            s(self.rgb.0),
            s(self.rgb.1),
            s(self.rgb.2)
        )
    }
}

/// Section cues carry a plain name; the show marks its lifts and accents
/// with a leading `·`. Worth drawing differently — an operator scanning
/// forty-three rows is looking for the next *section*, and the accents
/// between them are texture.
fn is_accent(name: &str) -> bool {
    name.trim_start().starts_with('\u{b7}')
}

#[component]
fn Cues() -> Element {
    let list = use_signal(show::cues);
    // The player is real state: GO advances it and the strip below is its
    // actual interpolated output, not a lookup of the cue's target values.
    let mut player = use_signal(|| CuePlayer::new(list.peek().cues.clone()));
    // `go`/`output` resolve against a `Show` — the groups, rig, palettes,
    // role bindings and effect library a recipe needs to know what "the
    // key light" means here. The shipped show names roles and never a
    // group, so without the real bindings and a real rig every cue would
    // resolve to nothing and the strip would sit black. Loaded once: it
    // parses five files and the whole effect library.
    //
    // `Rc` because a hook hands back a clone each render and the cooked
    // venue is a rig, a palette table and that library.
    let cooked = use_hook(|| Rc::new(show::Cooked::load()));
    // Fades are wall-clock in the console; stepping to the end of the move
    // on GO keeps this honest without an animation loop the prototype has
    // no use for yet.
    let for_go = cooked.clone();
    let mut go = move || {
        player.write().go(&for_go.show());
        let fade = player
            .read()
            .current_index()
            .map(|i| list.read().cues[i].fade_secs);
        if let Some(f) = fade {
            player.write().tick(f);
        }
    };

    // Keep the live cue on screen. Forty-three rows is four screens
    // deep, and after a few GOs the row that matters has scrolled off
    // the top — an operator should never have to find it by hand. The
    // row carries `id="current-cue"`; only one ever does.
    //
    // Through `eval` rather than a mounted-node handle: `onmounted`
    // fires once, and these rows are keyed, so the row that *becomes*
    // current is never remounted and would never fire again.
    use_effect(move || {
        // Read it so the effect re-runs on every take.
        let _ = player.read().current_index();
        document::eval(
            "document.getElementById('current-cue')\
                ?.scrollIntoView({ block: 'center', behavior: 'smooth' });",
        );
    });

    let current = player.read().current_index();
    let out: HashMap<(ChanId, Attribute), f32> = player.read().output(&cooked.show());
    let rig = lit(&cooked.channels, &out);
    let total = list.read().cues.len();
    // What GO will take. A desk shows the *next* cue on the button, not
    // the one already on stage — that is the question the operator's
    // thumb is asking.
    let next = match current {
        Some(i) if i + 1 < total => Some(list.read().cues[i + 1].name.clone()),
        Some(_) => None,
        None => list.read().cues.first().map(|c| c.name.clone()),
    };

    rsx! {
        div { class: "screen",
            header { class: "head",
                h1 { "{list.read().name}" }
                span { class: "sub",
                    {current.map_or_else(
                        || format!("standby — {total} cues"),
                        |i| format!("cue {} of {total}", i + 1))}
                }
            }
            // The list scrolls; the strip and GO do not. On a phone the
            // primary control cannot be at the bottom of forty-three rows.
            ol { class: "cuelist",
                for (i, cue) in list.read().cues.iter().enumerate() {
                    li {
                        key: "{i}",
                        id: if Some(i) == current { "current-cue" } else { "" },
                        class: {
                            let mut c = String::from("cue");
                            if Some(i) == current { c.push_str(" active"); }
                            if is_accent(&cue.name) { c.push_str(" accent"); }
                            c
                        },
                        span { class: "cue-num", "{i + 1}" }
                        span { class: "cue-name", "{cue.name}" }
                        span { class: "cue-fade",
                            {if cue.fade_secs == 0.0 { "snap".to_string() } else { format!("{:.1}s", cue.fade_secs) }}
                        }
                    }
                }
            }
            footer { class: "deck",
                // Forty fixtures as a grid rather than forty slivers: at
                // this width a bar per fixture is seven pixels, which
                // shows nothing. A cell carries both numbers — its
                // colour is the fixture's, its brightness is its level.
                div { class: "rig",
                    for f in rig.iter() {
                        div {
                            key: "{f.chan}",
                            class: if f.level > 0.02 { "cell on" } else { "cell" },
                            style: "{f.css()}",
                            title: "{f.chan}",
                        }
                    }
                }
                button { class: "go", onclick: move |_| go(),
                    span { class: "go-label", "GO" }
                    span { class: "go-next",
                        {next.clone().unwrap_or_else(|| "end of list".to_string())}
                    }
                }
            }
        }
    }
}

#[component]
fn Patch() -> Element {
    let patch = use_signal(show::patch);
    rsx! {
        div { class: "screen",
        header { class: "head", h1 { "Patch" } span { class: "sub", "{patch.read().len()} fixtures" } }
        ul { class: "rows",
            for f in patch.read().iter() {
                li { key: "{f.chan}", class: "row",
                    span { class: "chan", "{f.chan}" }
                    span { class: "name", "{f.fixture_type}" }
                    span { class: "addr",
                        {f.dmx.map_or("—".to_string(), |d| format!("{}/{}", d.universe, d.start_channel))}
                    }
                }
            }
        }
        }
    }
}

#[component]
fn Groups() -> Element {
    let groups = use_signal(show::groups);
    rsx! {
        div { class: "screen",
        header { class: "head", h1 { "Groups" } span { class: "sub", "{groups.read().len()} groups" } }
        ul { class: "rows",
            for g in groups.read().iter() {
                li { key: "{g.name}", class: "row",
                    span { class: "name", "{g.name}" }
                    span { class: "addr", "{g.chans.len()} ch" }
                }
            }
        }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ignition_core::ColorChannel::Red;

    /// The strip is not decoration — it is the cue's output. Playing the
    /// shipped show into it has to light Room 138's rig, in colour.
    ///
    /// Worth a test rather than a look: the cells are three pixels wide
    /// on the device, and a colour that silently resolved to white would
    /// read as "the fixture is a dimmer" instead of as a bug. That is
    /// exactly the failure this had before the profile's palette was
    /// inherited.
    #[test]
    fn the_rig_strip_shows_the_cue_in_colour() {
        let cooked = show::Cooked::load();
        let list = show::cues();
        let mut player = CuePlayer::new(list.cues.clone());

        // Far enough in to be past the count-in's dark cues and into a
        // section that has the wash up in a hue.
        let mut coloured = None;
        for _ in 0..list.cues.len() {
            player.go(&cooked.show());
            player.tick(4.0);
            let cells = lit(&cooked.channels, &player.output(&cooked.show()));
            let up: Vec<_> = cells.iter().filter(|c| c.level > 0.05).collect();
            if up.len() >= 4 && up.iter().any(|c| c.rgb != (1.0, 1.0, 1.0)) {
                coloured = Some(up.len());
                break;
            }
        }
        assert!(
            coloured.is_some(),
            "no cue in the shipped show ever lit four fixtures in a colour"
        );
    }

    /// A dark fixture draws as black, whatever hue the cue left in it —
    /// the cell multiplies by the level, so the grid reads as levels and
    /// not as a fixed pattern of colours.
    #[test]
    fn a_fixture_at_zero_draws_black() {
        let out = HashMap::from([
            ((1, Attribute::Dimmer), 0.0),
            ((1, Attribute::ColorAdd { channel: Red }), 1.0),
        ]);
        let cells = lit(&[1], &out);
        assert_eq!(cells[0].css(), "background: rgb(0, 0, 0)");
    }

    /// A personality with no colour channels is a dimmer, not a fixture
    /// that is off — Room 138's tower blinders would otherwise be a
    /// black hole in the middle of the wall.
    #[test]
    fn a_fixture_with_no_colour_channels_draws_white() {
        let out = HashMap::from([((1, Attribute::Dimmer), 1.0)]);
        assert_eq!(lit(&[1], &out)[0].css(), "background: rgb(255, 255, 255)");
    }

    /// The show's own convention: sections are named plainly and the
    /// lifts between them lead with `·`. The list indents on that.
    #[test]
    fn the_shows_accents_are_the_dotted_cues() {
        let cues = show::cues().cues;
        assert!(cues.iter().any(|c| is_accent(&c.name)), "no accent cues");
        assert!(cues.iter().any(|c| !is_accent(&c.name)), "no section cues");
        assert!(!is_accent("CH 1"));
        assert!(is_accent("· lift"));
    }
}
