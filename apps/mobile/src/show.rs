//! A small demo show, built from the real domain types.
//!
//! Ignition has no phone-side data source yet — no vox link to a running
//! console. Rather than invent placeholder structs for the UI to render,
//! this constructs genuine `PatchEntry` / `Group` / `CueList` values, so
//! the screens exercise the same vocabulary the console does and a change
//! to those types breaks this file instead of quietly diverging.
use ignition_core::{ChanId, Cue, CueList, CueValue, Group, PatchEntry, Placement, Quat, Vec3};
use ignition_proto::{Attribute, DmxAddress};

/// Identity rotation. `Quat` derives no `Default`, and "no rotation" is
/// w=1 rather than the all-zeros a derive would have given.
const UPRIGHT: Quat = Quat { w: 1.0, x: 0.0, y: 0.0, z: 0.0 };

fn at(x: f64, y: f64, z: f64) -> Placement {
    Placement {
        position: Vec3 { x, y, z },
        orientation: UPRIGHT,
    }
}

/// Six fixtures: a front-of-house wash pair, two overhead movers, and two
/// backlight pars — a plausible smallest real rig.
pub fn patch() -> Vec<PatchEntry> {
    [
        ("FOH Wash L", -4.0, 6.0, 5.0, 1u16),
        ("FOH Wash R", 4.0, 6.0, 5.0, 9),
        ("OH Mover L", -3.0, 8.0, 0.0, 17),
        ("OH Mover R", 3.0, 8.0, 0.0, 33),
        ("Back Par L", -2.5, 4.0, -4.0, 49),
        ("Back Par R", 2.5, 4.0, -4.0, 57),
    ]
    .into_iter()
    .enumerate()
    .map(|(i, (name, x, y, z, start))| PatchEntry {
        chan: i as ChanId + 1,
        fixture_type: name.to_string(),
        placement: at(x, y, z),
        dmx: Some(DmxAddress { universe: 1, start_channel: start }),
    })
    .collect()
}

pub fn groups() -> Vec<Group> {
    vec![
        Group { name: "FOH Wash".into(), chans: vec![1, 2] },
        Group { name: "OH Movers".into(), chans: vec![3, 4] },
        Group { name: "Back Pars".into(), chans: vec![5, 6] },
    ]
}

/// A dimmer level for one channel. `Attribute::Dimmer` is the GDTF-style
/// attribute identity, not a raw DMX offset.
fn dim(chan: ChanId, level: f32) -> CueValue {
    CueValue { chan, attr: Attribute::Dimmer, value: level }
}

pub fn cues() -> CueList {
    let mut list = CueList {
        name: "Main".into(),
        // Cue and CueList have grown a lot of optional structure
        // (numbers, notes, appearance, triggers, wrap/restart). Spreading
        // Default keeps this demo honest about only setting what it means
        // to set, and stops it breaking every time a field is added.
        ..Default::default()
    };
    list.cues = vec![
            Cue {
                name: "Blackout".into(),
                fade_secs: 0.0,
                values: (1..=6).map(|c| dim(c, 0.0)).collect(),
                ..Default::default()
            },
            Cue {
                name: "Preset — House".into(),
                fade_secs: 3.0,
                values: vec![dim(1, 0.45), dim(2, 0.45)],
                ..Default::default()
            },
            Cue {
                name: "Verse".into(),
                fade_secs: 2.0,
                values: vec![dim(1, 0.7), dim(2, 0.7), dim(5, 0.3), dim(6, 0.3)],
                ..Default::default()
            },
            Cue {
                name: "Chorus".into(),
                fade_secs: 0.8,
                values: (1..=6).map(|c| dim(c, 1.0)).collect(),
                ..Default::default()
            },
    ];
    list
}
