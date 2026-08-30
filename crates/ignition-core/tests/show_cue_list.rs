//! The shape of a *generated* cue list.
//!
//! Two of the rules about how a show is laid out only exist at the
//! scale of a whole song: that its accents never block, and that its
//! order matches its clock. Both are properties of the list `authorshow`
//! writes, and both fail quietly — a blocking accent looks like an
//! accent until the section under it goes out, and a list out of order
//! plays correctly under GO and wrongly under the transport. So they
//! are checked against the shipped charted song rather than a fixture.

use ignition_core::CueList;

/// The repo's charted song, or `None` on a runner outside the checkout.
fn charted_song() -> Option<CueList> {
    let text = std::fs::read_to_string("../../data/songs/bye-bye-bye.json").ok()?;
    Some(serde_json::from_str(&text).expect("the charted song parses"))
}

/// Accents never block.
///
/// A lift, a hit or a widening adds to the section it lands on. If one
/// blocked, everything the section was doing would go out underneath it
/// — a section with a very short name — and the fault would show up
/// only in the two bars the accent covers, which is nobody's idea of a
/// reproducible bug.
///
/// r[verify cues.accents-do-not-block]
#[test]
fn no_accent_in_the_charted_song_blocks() {
    let Some(list) = charted_song() else {
        return;
    };

    let accents: Vec<&str> = list
        .cues
        .iter()
        .filter(|c| c.name.trim_start().starts_with('·'))
        .map(|c| c.name.as_str())
        .collect();
    assert!(
        accents.len() > 5,
        "the charted song has no accents to check: {accents:?}"
    );

    for cue in &list.cues {
        if cue.name.trim_start().starts_with('·') {
            assert!(
                !cue.block,
                "accent `{}` blocks, so it puts out the section it lands on",
                cue.name
            );
        }
    }
}

/// The list is in playing order, and a section is under the hit on its
/// downbeat.
///
/// Order is what an operator pressing GO walks; position is what the
/// transport seeks. They have to agree, or a show run by hand and the
/// same show run to a track are two different shows. The tie-break
/// matters as much as the sort: on a shared bar the section has to be
/// taken first, because the accent adds to what is running and there is
/// nothing running yet if it goes first.
///
/// r[verify cues.sorted-by-position]
#[test]
fn the_generated_list_is_in_playing_order_with_sections_under_accents() {
    let Some(list) = charted_song() else {
        return;
    };

    // Positioned cues only: an unpositioned cue — the safety state the
    // list opens with — belongs where the author put it.
    let positioned: Vec<_> = list
        .cues
        .iter()
        .filter_map(|c| c.position().map(|at| (at.bar, at.beat, c)))
        .collect();
    assert!(positioned.len() > 10, "too few positioned cues to check");

    for pair in positioned.windows(2) {
        let (before, after) = (&pair[0], &pair[1]);
        let key = |bar: u32, beat: f64| (bar, (beat * 1000.0).round() as i64);
        assert!(
            key(before.0, before.1) <= key(after.0, after.1),
            "`{}` at {}.{} comes before `{}` at {}.{} in the list but after it in the song",
            before.2.name,
            before.0,
            before.1,
            after.2.name,
            after.0,
            after.1
        );

        // Same bar and beat: the blocking section first.
        if key(before.0, before.1) == key(after.0, after.1) {
            assert!(
                before.2.block || !after.2.block,
                "`{}` (an accent) is taken before `{}` (a section) on the same beat, so the \
                 section lands on top of the accent it was meant to be under",
                before.2.name,
                after.2.name
            );
        }
    }
}
