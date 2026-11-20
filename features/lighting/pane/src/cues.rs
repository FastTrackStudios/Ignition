//! The `cues` view: a session's cue list, in GO order, with the cue the
//! session's transport is in lit, and the transport's time and bar.
//!
//! What it shows is the setting [`SETTING`] (`<url>#<doc>@<lane>`, which
//! `IGNITION_SESSION_ROOM` sets): the list is read through `fts.session`
//! as this extension (`ignition_session::load`), and the transport is
//! followed as the room follows it, presence in the host's clock
//! ([`crate::session::follow_session`]). The view writes nothing.

use std::time::Duration;

use architect_extensions::hostcall::HostRequest;
use dioxus::prelude::*;
use ignition_session::{bar_one, chart_bar, load, tempo, time_of};
use session::LaneId;

/// The view's id.
pub const VIEW: &str = "cues";

/// The setting naming the session and lane the view shows.
pub const SETTING: &str = "ignition.session";

/// Where the cue list is: `<url>#<doc>@<lane>`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Place {
    pub url: String,
    pub doc: String,
    pub lane: String,
}

impl Place {
    /// `<url>#<doc>@<lane>`; `None` for anything else (an empty setting).
    #[must_use]
    pub fn parse(value: &str) -> Option<Self> {
        let (rest, lane) = value.rsplit_once('@')?;
        let (url, doc) = rest.rsplit_once('#')?;
        (!url.is_empty() && !doc.is_empty() && !lane.is_empty()).then(|| Self {
            url: url.to_owned(),
            doc: doc.to_owned(),
            lane: lane.to_owned(),
        })
    }
}

/// One cue as the view lists it.
#[derive(Debug, Clone, PartialEq)]
pub struct Row {
    pub number: f32,
    pub name: String,
    /// Its bar as the chart numbers it (from the song's bar 1, session
    /// `bar_one`); `None`: the operator's, or in the count-in.
    pub bar: Option<u32>,
    /// It falls in the count-in, before the song's bar 1.
    pub count_in: bool,
    /// Its time in the session, seconds; `None`: the operator's.
    pub at: Option<f64>,
}

/// The row the playhead is in: the last timed cue at or before `now`.
#[must_use]
pub fn current(rows: &[Row], now: f64) -> Option<usize> {
    rows.iter()
        .enumerate()
        .filter(|(_, r)| r.at.is_some_and(|at| at <= now + 1e-6))
        .max_by(|a, b| a.1.at.unwrap_or(0.0).total_cmp(&b.1.at.unwrap_or(0.0)))
        .map(|(i, _)| i)
}

/// `seconds` as `m:ss.s`.
#[must_use]
pub fn clock_text(seconds: f64) -> String {
    let seconds = seconds.max(0.0);
    #[allow(
        clippy::as_conversions,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss
    )]
    let minutes = (seconds / 60.0).floor() as u64;
    #[allow(clippy::cast_precision_loss, clippy::as_conversions)]
    let rest = seconds - (minutes as f64) * 60.0;
    format!("{minutes}:{rest:04.1}")
}

pub(crate) async fn setting() -> Option<Place> {
    let value = crate::ext::handle()
        .ok()?
        .host(HostRequest::Setting {
            key: SETTING.to_owned(),
        })
        .await
        .ok()?;
    Place::parse(value.as_str()?)
}

/// The list read through `fts.session`, timed under the session's tempo.
async fn read(place: &Place) -> Result<(String, Vec<Row>), String> {
    let ask = crate::session::host_ask()?;
    let lane = LaneId::from(place.lane.as_str());
    let list = load(&ask, &place.doc, &lane)
        .await
        .map_err(|e| e.to_string())?;
    let map = tempo(&ask, &place.doc).await.map_err(|e| e.to_string())?;
    // Bars as the chart numbers them: from the song's bar 1, where a
    // count-in ends (the session's structure says where).
    let first = bar_one(&ask, &place.doc).await.unwrap_or(1);
    let rows = list
        .cues
        .iter()
        .enumerate()
        .map(|(i, cue)| {
            let bars = cue.position();
            let bar = bars.and_then(|b| chart_bar(b.bar, first));
            Row {
                number: list.number_of(i),
                name: cue.name.clone(),
                bar,
                count_in: bars.is_some() && bar.is_none(),
                at: bars.map(|b| time_of(&map, b).seconds(&map)),
            }
        })
        .collect();
    Ok((list.name, rows))
}

#[component]
pub fn CuesView() -> Element {
    let mut place = use_signal(|| None::<Place>);
    use_future(move || async move {
        loop {
            let now = setting().await;
            if *place.peek() != now {
                place.set(now);
            }
            futures_timer::Delay::new(Duration::from_secs(1)).await;
        }
    });
    match place() {
        // In a browser the room is in the page: its own list, each cue a
        // press that takes it.
        #[cfg(all(feature = "pane", target_arch = "wasm32"))]
        None => rsx! { crate::web::RoomCues {} },
        #[cfg(not(all(feature = "pane", target_arch = "wasm32")))]
        None => rsx! {
            div { style: "flex: 1; display: flex; align-items: center; justify-content: center; padding: 20px; color: var(--muted-foreground); font-size: 13px; text-align: center;",
                "No session yet: IGNITION_SESSION_ROOM points the room and this list at one."
            }
        },
        // A list of one, so its key is diffed: another session mounts the
        // list afresh (a lone keyed child is patched in place, and went on
        // reading the session it first opened on: Ignition#25).
        Some(p) => rsx! {
            for p in std::iter::once(p) {
                Cues { key: "{p.url}#{p.doc}@{p.lane}", place: p.clone() }
            }
        },
    }
}

#[component]
fn Cues(place: Place) -> Element {
    let mut list =
        use_signal(|| Err::<(String, Vec<Row>), String>("Reading the cue list…".to_owned()));
    let mut now = use_signal(|| None::<(f64, bool)>);
    let read_place = place.clone();
    use_future(move || {
        let place = read_place.clone();
        async move {
            // Read again now and then: a cue added elsewhere shows up.
            loop {
                let got = read(&place).await;
                if *list.peek() != got {
                    list.set(got);
                }
                futures_timer::Delay::new(Duration::from_secs(5)).await;
            }
        }
    });
    let follow_place = place.clone();
    use_future(move || {
        let place = follow_place.clone();
        async move {
            let Ok(peer) = crate::session::follow_session(&place.url, &place.doc) else {
                return;
            };
            loop {
                let at = crate::session::leader_now(&peer);
                if *now.peek() != at {
                    now.set(at);
                }
                futures_timer::Delay::new(Duration::from_millis(50)).await;
            }
        }
    });

    let (name, rows) = match list() {
        Ok(got) => got,
        Err(why) => {
            return rsx! {
                div { style: "flex: 1; padding: 20px; color: var(--muted-foreground); font-size: 13px;", "{why}" }
            };
        }
    };
    let (seconds, playing) = now().unwrap_or((0.0, false));
    let lit = now().and_then(|(s, _)| current(&rows, s));
    let state = if playing { "▶" } else { "■" };
    let time = clock_text(seconds);
    let in_cue = lit
        .and_then(|i| rows.get(i))
        .map_or_else(String::new, |r| format!("cue {} · {}", r.number, r.name));
    rsx! {
        div { style: "flex: 1; display: flex; flex-direction: column; min-height: 0; min-width: 0; font-size: 13px;",
            div { style: "display: flex; flex-direction: column; gap: 2px; padding: 10px 12px; border-bottom: 1px solid var(--border);",
                div { style: "font-weight: 600; font-size: 14px;", "{name}" }
                div { style: "display: flex; gap: 10px; font-family: monospace; font-size: 12px;",
                    span { "{state} {time}" }
                    span { style: "color: var(--muted-foreground);", "{in_cue}" }
                }
            }
            div { style: "flex: 1; min-height: 0; overflow-y: scroll;",
                for (i, row) in rows.iter().enumerate() {
                    CueRow { key: "{i}", index: i, row: row.clone(), lit: lit == Some(i), past: lit.is_some_and(|l| i < l) }
                }
            }
        }
    }
}

/// A press on a cue: in a browser the room is in the page and takes it;
/// natively the room follows its clock, and a press does nothing.
#[cfg_attr(
    not(all(feature = "pane", target_arch = "wasm32")),
    expect(
        clippy::missing_const_for_fn,
        reason = "natively a press does nothing; in a browser it takes the cue"
    )
)]
fn press(index: usize) {
    #[cfg(all(feature = "pane", target_arch = "wasm32"))]
    crate::web::take(index);
    #[cfg(not(all(feature = "pane", target_arch = "wasm32")))]
    let _ = index;
}

#[component]
fn CueRow(index: usize, row: Row, lit: bool, past: bool) -> Element {
    let background = if lit { "var(--accent)" } else { "transparent" };
    let color = if lit {
        "var(--accent-foreground)"
    } else if past {
        "var(--muted-foreground)"
    } else {
        "var(--foreground)"
    };
    let weight = if lit { "600" } else { "400" };
    let bar = row
        .bar
        .map_or_else(|| "—".to_owned(), |b| format!("bar {b}"));
    let at = row.at.map_or_else(String::new, clock_text);
    let number = row.number;
    rsx! {
        div {
            style: "display: flex; gap: 10px; align-items: baseline; padding: 5px 12px; background: {background}; color: {color}; font-weight: {weight};",
            onclick: move |_| press(index),
            span { style: "width: 34px; font-family: monospace;", "{number}" }
            span { style: "flex: 1; min-width: 0; overflow: hidden;", "{row.name}" }
            span { style: "width: 56px; font-family: monospace; font-size: 12px;", "{bar}" }
            span { style: "width: 52px; font-family: monospace; font-size: 12px;", "{at}" }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_place_is_a_url_a_session_and_a_lane() {
        assert_eq!(
            Place::parse("ws://192.168.1.10:4143/vox#4d2107f2-e6a3-47bf-a511-512f9bbe5918@dtla"),
            Some(Place {
                url: "ws://192.168.1.10:4143/vox".into(),
                doc: "4d2107f2-e6a3-47bf-a511-512f9bbe5918".into(),
                lane: "dtla".into(),
            })
        );
        assert_eq!(Place::parse(""), None);
        assert_eq!(Place::parse("ws://h/vox#doc"), None);
    }

    fn row(number: f32, at: Option<f64>) -> Row {
        Row {
            number,
            name: String::new(),
            bar: None,
            count_in: false,
            at,
        }
    }

    #[test]
    fn the_lit_cue_is_the_last_timed_one_the_playhead_has_reached() {
        let rows = [
            row(1.0, Some(0.0)),
            row(2.0, None),
            row(3.0, Some(6.0)),
            row(4.0, Some(12.0)),
        ];
        assert_eq!(current(&rows, 0.0), Some(0));
        assert_eq!(current(&rows, 5.9), Some(0));
        assert_eq!(current(&rows, 6.0), Some(2));
        assert_eq!(current(&rows, 100.0), Some(3));
        assert_eq!(current(&rows[2..], 1.0), None);
    }

    #[test]
    fn a_time_reads_as_minutes_and_seconds() {
        assert_eq!(clock_text(0.0), "0:00.0");
        assert_eq!(clock_text(75.25), "1:15.2");
        assert_eq!(clock_text(-3.0), "0:00.0");
    }
}
