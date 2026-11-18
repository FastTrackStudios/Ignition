//! Ignition on a session, through `fts.session`: cue lists stored as lanes
//! and items, read back in GO order, followed from the transport, beside
//! another extension's data.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::float_cmp,
    clippy::arithmetic_side_effects,
    clippy::missing_panics_doc
)]

use ignition_daw_proto::{Bars, Position};
use ignition_session::{CueFollower, EXTENSION, Take, load, store, tempo};
use ignition_show::{Cue, CueList};
use session::doc::SessionDoc;
use session::extension::{answer, call_json, response_of};
use session::service::{Grants, Request, Response, Sessions, TempoChange, TempoMapData, handle};
use session::{ExtensionId, LaneId, Transport};

const DOC: &str = "song-1";

/// A session with 120 BPM in 4/4, slowing to 60 BPM at bar 5 (8 s).
fn session() -> Sessions {
    let doc = SessionDoc::new();
    handle(
        &doc,
        &ExtensionId::from("studio"),
        Grants { structure: true },
        Request::SetTempo {
            tempo: TempoMapData {
                default_bpm: 120.0,
                numerator: 4,
                denominator: 4,
                points: vec![TempoChange {
                    seconds: 8.0,
                    bpm: 60.0,
                    meter: None,
                    shape: None,
                    tension: None,
                }],
            },
        },
    )
    .unwrap();
    let sessions = Sessions::default();
    sessions.open(DOC, doc);
    sessions
}

/// Ignition asking through `fts.session`, as its host answers it.
fn ignition(
    sessions: &Sessions,
) -> impl Fn(&'static str, String) -> Result<String, String> + Sync + '_ {
    move |method, args: String| answer(sessions, method, &args, Some(EXTENSION))
}

fn cue(name: &str, number: f32, bar: Option<u32>) -> Cue {
    Cue {
        name: name.into(),
        number: Some(number),
        fade_secs: 2.0,
        at: bar.map(|b| Position::Absolute(Bars::bar(b))),
        ..Cue::default()
    }
}

/// Cues on bars 1, 3, 5 and 7 (0 s, 4 s, 8 s, 16 s under the session's
/// tempo), and one with no bar between 2 and 3: an operator's cue.
fn list() -> CueList {
    CueList {
        name: "Main".into(),
        cues: vec![
            cue("Walk-in", 1.0, Some(1)),
            cue("Verse", 2.0, Some(3)),
            cue("Speech", 2.5, None),
            cue("Chorus", 3.0, Some(5)),
            cue("Drop", 4.0, Some(7)),
        ],
        wrap: true,
        ..CueList::default()
    }
}

fn numbers(list: &CueList) -> Vec<f32> {
    list.cues.iter().filter_map(|c| c.number).collect()
}

#[tokio::test]
async fn a_cue_list_is_stored_as_a_lane_and_read_back_in_go_order() {
    let sessions = session();
    let ask = ignition(&sessions);
    let lane = LaneId::from("main");
    store(&ask, DOC, &lane, &list()).await.unwrap();

    let back = load(&ask, DOC, &lane).await.unwrap();
    assert_eq!(back.name, "Main");
    assert!(back.wrap);
    // GO order: cue number, timed and untimed together, as CuePlayer steps.
    assert_eq!(numbers(&back), [1.0, 2.0, 2.5, 3.0, 4.0]);
    let bars: Vec<Option<u32>> = back
        .cues
        .iter()
        .map(|c| c.position().map(|b| b.bar))
        .collect();
    assert_eq!(bars, [Some(1), Some(3), None, Some(5), Some(7)]);
    assert_eq!(back.cues[1].fade_secs, 2.0);

    // In the session: the lane, four timed items, the speech cue untimed.
    let doc = sessions.get(DOC).unwrap();
    let lane_held = doc.lane(&lane).unwrap();
    assert_eq!(
        (lane_held.owner.as_str(), lane_held.kind.as_str()),
        ("ignition", "ignition.cues")
    );
    assert_eq!(doc.items().len(), 4);

    // Storing the list without the drop removes it.
    let mut shorter = list();
    shorter.cues.pop();
    store(&ask, DOC, &lane, &shorter).await.unwrap();
    assert_eq!(
        numbers(&load(&ask, DOC, &lane).await.unwrap()),
        [1.0, 2.0, 2.5, 3.0]
    );
    assert_eq!(doc.items().len(), 3);
}

#[tokio::test]
async fn a_cue_moves_with_the_songs_tempo() {
    let sessions = session();
    let ask = ignition(&sessions);
    let lane = LaneId::from("main");
    store(&ask, DOC, &lane, &list()).await.unwrap();
    let map = tempo(&ask, DOC).await.unwrap();
    // Bar 7 is 16 s in under 120 then 60 BPM.
    let follower_bar7 = ignition_session::time_of(&map, Bars::bar(7)).seconds(&map);
    assert!((follower_bar7 - 16.0).abs() < 1e-9);

    // Someone makes the whole song 60 BPM: bar 7 is now 24 s in, and the
    // cue is still on bar 7.
    let doc = sessions.get(DOC).unwrap();
    handle(
        &doc,
        &ExtensionId::from("studio"),
        Grants { structure: true },
        Request::SetTempo {
            tempo: TempoMapData {
                default_bpm: 60.0,
                numerator: 4,
                denominator: 4,
                points: vec![],
            },
        },
    )
    .unwrap();
    let map = tempo(&ask, DOC).await.unwrap();
    let back = load(&ask, DOC, &lane).await.unwrap();
    assert_eq!(back.cues[4].position().unwrap().bar, 7);
    let at = ignition_session::time_of(&map, back.cues[4].position().unwrap());
    assert!((at.seconds(&map) - 24.0).abs() < 1e-9);
}

#[tokio::test]
async fn ignition_and_keyflow_keep_their_own_data_in_one_session() {
    let sessions = session();
    let ask = ignition(&sessions);
    store(&ask, DOC, &LaneId::from("main"), &list())
        .await
        .unwrap();
    let chart = call_json(
        DOC,
        Request::TextSplice {
            name: "chart".into(),
            at: 0,
            delete: 0,
            insert: "Verse: C G\n".into(),
        },
    )
    .unwrap();
    answer(&sessions, "write", &chart, Some("keyflow")).unwrap();

    // Keyflow's chart is untouched by Ignition, and Keyflow sees Ignition's
    // cues on the timeline, but not what is in them.
    let read = call_json(DOC, Request::ReadData).unwrap();
    let Response::Data(k) =
        response_of(&answer(&sessions, "read", &read, Some("keyflow")).unwrap()).unwrap()
    else {
        panic!("not data")
    };
    assert_eq!(k.text("chart"), Some("Verse: C G\n"));
    assert!(k.get("list:main").is_none());
    let timeline = call_json(DOC, Request::ReadTimeline).unwrap();
    let Response::Timeline { items, .. } =
        response_of(&answer(&sessions, "read", &timeline, Some("keyflow")).unwrap()).unwrap()
    else {
        panic!("not the timeline")
    };
    assert_eq!(items.len(), 4);
    assert!(
        items
            .values()
            .all(|i| i.kind == "ignition.cue" && i.payload == session::Value::Null)
    );
    // And Ignition's list still loads whole.
    assert_eq!(
        numbers(&load(&ask, DOC, &LaneId::from("main")).await.unwrap()).len(),
        5
    );
}

#[tokio::test]
async fn the_cues_follow_the_transport_through_play_seek_and_stop() {
    let sessions = session();
    let ask = ignition(&sessions);
    let lane = LaneId::from("main");
    store(&ask, DOC, &lane, &list()).await.unwrap();
    let loaded = load(&ask, DOC, &lane).await.unwrap();
    let mut follower = CueFollower::new(&loaded, &tempo(&ask, DOC).await.unwrap());

    // Frames every 16 ms of this clock; one clock with the leader here.
    let frames = |follower: &mut CueFollower, from: f64, to: f64| {
        let mut taken = Vec::new();
        let mut t = from;
        while t < to {
            match follower.tick(t * 1e6, 0.0) {
                Take::Hold => {}
                other => taken.push((t, other)),
            }
            t += 0.016;
        }
        taken
    };

    // Play from the top at t = 0.
    follower.observe(Transport::playing(0.0, 0.0, 1.0));
    let taken = frames(&mut follower, 0.0, 10.0);
    assert_eq!(
        taken[0].1,
        Take::Seek(Some(1.0)),
        "starting on bar 1 is cue 1's state"
    );
    let gos: Vec<(f64, &Take)> = taken[1..].iter().map(|(t, k)| (*t, k)).collect();
    assert_eq!(gos.len(), 2, "{taken:?}");
    assert_eq!(gos[0].1, &Take::Go(vec![2.0]));
    assert!(
        (gos[0].0 - 4.0).abs() < 0.017,
        "cue 2 at 4 s, took at {}",
        gos[0].0
    );
    assert_eq!(gos[1].1, &Take::Go(vec![3.0]));
    assert!(
        (gos[1].0 - 8.0).abs() < 0.017,
        "cue 3 at 8 s, took at {}",
        gos[1].0
    );

    // A seek to 12 s of song (bar 6) at t = 10: cue 3's state holds there,
    // and playing on, the drop (16 s of song) is taken at t = 14.
    follower.observe(Transport::playing(12.0, 10.0e6, 1.0));
    let taken = frames(&mut follower, 10.0, 17.0);
    assert_eq!(taken.len(), 2, "{taken:?}");
    assert_eq!(taken[0].1, Take::Seek(Some(3.0)));
    assert_eq!(taken[1].1, Take::Go(vec![4.0]));
    assert!(
        (taken[1].0 - 14.0).abs() < 0.017,
        "the drop at t = 14, took at {}",
        taken[1].0
    );

    // Stop: one seek to where it stopped, then nothing while it stands.
    follower.observe(Transport::stopped(20.0, 17.0e6));
    let taken = frames(&mut follower, 17.0, 18.0);
    assert_eq!(
        taken.iter().map(|(_, k)| k.clone()).collect::<Vec<_>>(),
        [Take::Seek(Some(4.0))]
    );
    // The speech cue (2.5) has no bar: only an operator's GO takes it.
}
