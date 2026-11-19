//! A room edited while the renderer runs: the patch, the routes, where
//! fixtures hang and where people stand.
//!
//! The programming actions write the venue's files. Twice a second this
//! looks at them, and when one has changed it reads the room again and
//! carries what moved onto the running scene:
//!
//! - **the patch** (universe, address, patched, mirrors): the venue's
//!   records take the new addresses and the patch table is resolved again,
//!   so the next frame is encoded to the new addresses;
//! - **the routes** (`dmx` in the manifest): the output is bound again;
//! - **a fixture moved or re-hung**: its entity is placed again by the
//!   rule that placed it at start (rigged to the nearest surface), and the
//!   show's rig takes its position, so a focus is solved from where it
//!   now hangs;
//! - **groups, role bindings and focus points** (a person moved): the
//!   show's.
//!
//! A fixture added, removed or changed to another type is not carried: the
//! scene's entities are made at start. It is said in the log, and the rest
//! of the edit still applies.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::SystemTime;

use bevy::prelude::*;

use crate::output::DmxOutput;
use crate::playback::Playback;
use crate::spawn::{Fixture, RigSurfaces, VenueRes, nearest_rig_surface, rig_to_surface};
use crate::venue::Venue;

static DIR: OnceLock<PathBuf> = OnceLock::new();

/// Names the venue directory the running renderer follows. Called once,
/// before the app runs, by a host that shows a room being edited.
pub fn watch(venue_dir: &Path) {
    let _ = DIR.set(venue_dir.to_path_buf());
}

/// The files an edit to a room lands in.
const FILES: [&str; 5] = [
    "fixtures.json",
    "venue.ig-venue",
    "groups.json",
    "palettes.json",
    "profile.json",
];

/// When each was last written, and when they were last looked at.
#[derive(Resource, Default)]
pub struct Watched {
    stamps: Option<Vec<Option<SystemTime>>>,
    /// By the wall clock: an offline render's own clock is the video's.
    looked: Option<std::time::Instant>,
}

fn stamps(dir: &Path) -> Vec<Option<SystemTime>> {
    FILES
        .iter()
        .map(|f| std::fs::metadata(dir.join(f)).and_then(|m| m.modified()).ok())
        .collect()
}

/// What an edit changed, fixture by fixture, against the room in hand.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Changed {
    /// Indices whose address, universe, patched flag or mirrors changed.
    pub repatched: Vec<usize>,
    /// Indices that moved or were re-hung.
    pub moved: Vec<usize>,
    /// The fixtures are not the same fixtures (added, removed, re-typed).
    pub different_rig: bool,
}

/// Carries the patch and the poses of `new` onto `room`, where the two have
/// the same fixtures in the same order. What changed.
pub fn carry(room: &mut Venue, new: &Venue) -> Changed {
    let mut changed = Changed::default();
    let same = room.fixtures.len() == new.fixtures.len()
        && room
            .fixtures
            .iter()
            .zip(&new.fixtures)
            .all(|(a, b)| a.chan == b.chan && a.model == b.model && a.manufacturer == b.manufacturer);
    if !same {
        changed.different_rig = true;
    }
    for (index, old) in room.fixtures.iter_mut().enumerate() {
        // By channel, never by index: an edit may have reordered the file.
        let Some(now) = new.fixtures.iter().find(|f| f.chan == old.chan && f.chan.is_some()) else {
            continue;
        };
        if (old.universe, old.address, old.patched) != (now.universe, now.address, now.patched)
            || old.mirrors != now.mirrors
        {
            old.universe = now.universe;
            old.address = now.address;
            old.global_address = now.global_address;
            old.patched = now.patched;
            old.mirrors.clone_from(&now.mirrors);
            changed.repatched.push(index);
        }
        if old.position != now.position || old.quat != now.quat {
            old.position = now.position;
            old.eulers = now.eulers;
            old.quat = now.quat;
            changed.moved.push(index);
        }
    }
    if !changed.repatched.is_empty() {
        room.repatch();
    }
    changed
}

#[expect(
    clippy::needless_pass_by_value,
    clippy::too_many_arguments,
    reason = "a Bevy system takes its parameters by value, and this one touches the room, the scene, the show and the output"
)]
pub fn follow_venue_edits(
    mut watched: Local<Watched>,
    mut venue: ResMut<VenueRes>,
    surfaces: Option<Res<RigSurfaces>>,
    mut fixtures: Query<(Entity, &mut Fixture, &mut Transform)>,
    mut commands: Commands,
    playback: Option<ResMut<Playback>>,
    output: Option<ResMut<DmxOutput>>,
) {
    let Some(dir) = DIR.get() else { return };
    let at = std::time::Instant::now();
    if watched
        .looked
        .is_some_and(|last| at.duration_since(last) < std::time::Duration::from_millis(500))
    {
        return;
    }
    watched.looked = Some(at);
    let now = stamps(dir);
    let Some(before) = watched.stamps.replace(now.clone()) else {
        return;
    };
    if before == now {
        return;
    }
    let new = match Venue::load(dir) {
        Ok(new) => new,
        Err(error) => {
            // A half-written edit: the room in hand keeps playing.
            tracing::warn!(%error, "venue: edited, and it does not read yet");
            watched.stamps = Some(before);
            return;
        }
    };
    let changed = carry(&mut venue.0, &new);
    if changed.different_rig {
        tracing::warn!(
            "venue: fixtures were added, removed or re-typed; those show when the renderer starts again"
        );
    }
    if !changed.moved.is_empty() {
        let none = RigSurfaces::default();
        let surfaces = surfaces.as_deref().unwrap_or(&none);
        for (entity, mut fixture, mut transform) in &mut fixtures {
            if !changed.moved.contains(&fixture.index) {
                continue;
            }
            let Some(record) = venue.0.fixtures.get(fixture.index) else {
                continue;
            };
            let (position, hang) = (record.position.to_vec3(), record.orientation());
            match nearest_rig_surface(position, &surfaces.0) {
                Some(surface) => {
                    let (local_pos, local_rot) = rig_to_surface(position, hang, &surface);
                    transform.translation = local_pos;
                    transform.rotation = local_rot;
                    fixture.base_rot = local_rot;
                    commands.entity(entity).insert(ChildOf(surface.entity));
                }
                None => {
                    transform.translation = position;
                    transform.rotation = hang;
                    fixture.base_rot = hang;
                    commands.entity(entity).remove::<ChildOf>();
                }
            }
        }
    }
    if venue.0.dmx != new.dmx {
        venue.0.dmx.clone_from(&new.dmx);
        if let Some(mut output) = output {
            output.rebind(&venue.0.output_config(), "Ignition");
        }
        tracing::info!("venue: output routes bound again");
    }
    venue.0.group_records.clone_from(&new.group_records);
    venue.0.palettes.focus.clone_from(&new.palettes.focus);
    venue.0.profile.clone_from(&new.profile);
    if let Some(mut playback) = playback {
        playback.rig = venue.0.rig();
        playback.groups = venue.0.groups();
        playback.profile.clone_from(&venue.0.profile);
        playback.palettes.focus.clone_from(&new.palettes.focus);
        if !changed.repatched.is_empty() {
            playback.defaults = venue.0.patch().defaults();
        }
    }
    tracing::info!(
        repatched = changed.repatched.len(),
        moved = changed.moved.len(),
        "venue: edited, carried onto the running room"
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    fn room(raw: &str) -> Venue {
        Venue::from_files(&crate::venue::VenueFiles {
            fixtures: raw.to_owned(),
            room: "[]".into(),
            screens: "[]".into(),
            props: "[]".into(),
            ..Default::default()
        })
        .unwrap_or_default()
    }

    fn fixture(chan: u32, address: u16, x: f32) -> String {
        format!(
            r#"{{"chan":{chan},"name":"F{chan}","tags":[],"model":"36 LED Par Can","position":{{"x":{x},"y":0,"z":2}},
                "eulers":{{"x":0,"y":0,"z":0}},"quat":{{"w":1,"x":0,"y":0,"z":0}},"size":{{"x":0.2,"y":0.2,"z":0.2}},
                "universe":1,"address":{address}}}"#
        )
    }

    #[test]
    fn a_repatch_and_a_move_are_carried_by_channel_and_the_table_is_resolved_again() {
        let mut live = room(&format!("[{},{}]", fixture(1, 1, 0.0), fixture(2, 9, 1.0)));
        let before = live.patch().by_chan(2).and_then(|p| p.addresses().next().copied());
        // The edit: channel 2 to address 101, channel 1 a metre along; the
        // file's order swapped.
        let edited = room(&format!("[{},{}]", fixture(1, 1, 1.0), fixture(2, 101, 1.0)));
        let changed = carry(&mut live, &edited);
        assert_eq!(changed.repatched, vec![1]);
        assert_eq!(changed.moved, vec![0]);
        assert!(!changed.different_rig);
        let after = live.patch().by_chan(2).and_then(|p| p.addresses().next().copied());
        assert_ne!(before, after, "the next frame is encoded to the new address");
        // The same edit again changes nothing.
        assert_eq!(carry(&mut live, &edited), Changed::default());
        // A fixture added is said, and the rest still applies.
        let more = room(&format!("[{},{},{}]", fixture(1, 1, 1.0), fixture(2, 201, 1.0), fixture(3, 300, 0.0)));
        let changed = carry(&mut live, &more);
        assert!(changed.different_rig);
        assert_eq!(changed.repatched, vec![1]);
    }
}
