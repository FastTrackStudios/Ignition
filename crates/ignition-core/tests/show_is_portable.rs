//! A show is written for a profile, not for a room.
//!
//! The claims are easy to state and easy to break by accident: a show
//! names roles and the venue answers them, positions are musical, and
//! nothing in the file knows which fixture is patched where. Each of
//! those fails silently — a show with a channel number in it works
//! perfectly until it is opened in the next room — so they are checked
//! against the real charted song rather than a fixture.

use ignition_core::CueList;

/// The repo's charted song, or `None` on a runner outside the checkout.
fn charted_song() -> Option<(String, CueList)> {
    let text = std::fs::read_to_string("../../data/songs/bye-bye-bye.json").ok()?;
    let list = serde_json::from_str(&text).expect("the charted song parses");
    Some((text, list))
}

/// The show names no fixture, channel, universe or patch address.
///
/// The moment a zone became "channels 3–7" the chart would be a Norco
/// chart. A show refers to the profile's vocabulary and the venue
/// answers it, which is what lets the same song play a different room.
///
/// r[verify song.no-room]
/// r[verify files.no-fixture-identity]
#[test]
fn the_show_names_no_fixture_and_no_patch() {
    let Some((text, _)) = charted_song() else {
        return;
    };

    // `channel` does appear, as a *canvas* channel — a screen and a
    // quantity, nothing to do with DMX. The patch words are the ones
    // that would tie this file to a room.
    for patch in ["\"universe\"", "\"address\"", "\"dmx\"", "\"chan\""] {
        assert!(
            !text.contains(patch),
            "the show carries {patch}, which ties it to one room's patch"
        );
    }
}

/// Every position in the show is musical.
///
/// Seconds are a property of one recording at one tempo; bars are a
/// property of the music. A cached second is a second copy of the
/// tempo, and it is the copy that will be wrong after a tempo edit.
///
/// Fade *durations* are a different thing and are allowed to be
/// seconds — `r[cues.fade-in-beats]` has them authored in beats and
/// converted when the list is written. What must never be in seconds is
/// where something happens.
///
/// r[verify song.position.never-seconds]
#[test]
fn every_position_in_the_show_is_bars_and_beats() {
    let Some((_, list)) = charted_song() else {
        return;
    };

    for cue in &list.cues {
        assert!(
            cue.position().is_some() || cue.at.is_none(),
            "cue `{}` has a position that is not a bar",
            cue.name
        );
    }
    for trigger in &list.triggers {
        assert!(
            trigger.bars().is_some(),
            "hit `{}` has a position that is not a bar",
            trigger.name
        );
    }

    // And the file stores no position in seconds beside the musical
    // one. `fade_secs` is a duration and is expected; a key that
    // *located* something in seconds would be the second copy of the
    // tempo this rule exists to prevent.
    let text = serde_json::to_string(&list).expect("serialises");
    for cached in [
        "\"at_secs\"",
        "\"position_secs\"",
        "\"seconds\"",
        "\"start_secs\"",
    ] {
        assert!(
            !text.contains(cached),
            "the show caches {cached} — a second copy of the tempo map"
        );
    }
}

/// The show speaks the profile's vocabulary: roles, not fixtures.
///
/// Which role plays a hit is the show's decision; the chart only says
/// that a hit happens and how big it is. A song that knew its hits went
/// on "Back Wall Pars" would be a song for one profile.
///
/// r[verify song.no-role-binding]
#[test]
fn the_show_addresses_roles_rather_than_fixtures() {
    let Some((text, list)) = charted_song() else {
        return;
    };

    assert!(
        text.contains("\"Role\""),
        "a show that names no role is not addressing the profile at all"
    );

    // Every trigger's recipe targets something a profile can answer —
    // a role, a group, or a set built from them — never a bare channel.
    for trigger in &list.triggers {
        let target = format!("{:?}", trigger.recipe.target);
        assert!(
            !target.contains("Chans") && !target.contains("Chan("),
            "hit `{}` targets channels directly: {target}",
            trigger.name
        );
    }
}
