//! The venue-local layer: what this room is doing differently tonight.
//!
//! A rig changes between the file being written and the doors opening. A
//! fixture blows and its spare goes in on a different address; a mover
//! is out of service; somebody moves a bar and repatches four pars to
//! reach. None of that is what the room *is* — it is what the room is
//! doing this week — and the difference matters, because the base venue
//! is the thing every show is checked against and the thing the next
//! person to open the room reads.
//!
//! `r[patch.venue-layer]` is the argument in full. The short version is
//! that these changes exist whether or not there is a place to put them,
//! and without a sanctioned place they get committed into the room's own
//! description and stay there — so the room slowly becomes a record of
//! every night rather than of itself.
//!
//! This is a **different artifact** from the show's `.ig-local`
//! (`r[profile.venue-layer]`), which overrides cues. One overrides the
//! room and the other overrides the song; folding them together would
//! mean a repatch had to name a song it has nothing to do with.

use crate::venue::{FixtureRecord, Venue};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::Path;

/// The file, in the venue's directory beside its other JSON.
pub const FILE: &str = "venue.ig-local";

/// The version this build writes and the newest it will read.
pub const VERSION: u32 = 1;

/// What one fixture is doing differently.
///
/// Every field is optional and absent means "as the venue says". A
/// layer that repeats the base venue's values is a layer that will look
/// like a change in a diff without being one.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Override {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub universe: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub address: Option<u16>,
    /// `Some(false)` is a fixture out of service — still in the room,
    /// still in groups, off the wire.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub patched: Option<bool>,
    /// A swapped fixture: the spare that went in is a different model.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gel: Option<String>,
    /// Why. Not decoration: a layer is read by whoever opens the room
    /// next, and "chan 41 is at 2.100" without "because its dimmer died
    /// on the 14th" is a mystery somebody has to re-solve.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub note: String,
    #[serde(flatten)]
    pub extra: BTreeMap<String, serde_json::Value>,
}

/// One room's local changes.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct VenueLayer {
    #[serde(default)]
    pub version: u32,
    /// The venue directory this belongs to, so a layer copied into the
    /// wrong room can be spotted rather than silently applied.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub venue: String,
    /// By channel, because a channel is what survives the base venue
    /// being re-extracted and an index is not.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub fixtures: BTreeMap<u32, Override>,
    /// Fixtures in the room tonight that the venue does not know about —
    /// a hired-in bar, a followspot borrowed for one show.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub add: Vec<FixtureRecord>,
    /// Fixtures the venue has that are not in the room tonight.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub remove: Vec<u32>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, serde_json::Value>,
}

impl VenueLayer {
    /// A layer for a named venue.
    #[must_use]
    pub fn new(venue: &str) -> Self {
        Self {
            version: VERSION,
            venue: venue.to_owned(),
            ..Self::default()
        }
    }

    /// Whether this layer says anything at all.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.fixtures.is_empty() && self.add.is_empty() && self.remove.is_empty()
    }

    /// How many fixtures this layer touches, for the desk to report
    /// (`r[patch.venue-layer.visible]`).
    #[must_use]
    pub fn touched(&self) -> usize {
        self.fixtures
            .len()
            .saturating_add(self.add.len())
            .saturating_add(self.remove.len())
    }

    /// Read a venue's layer, if it has one.
    ///
    /// A missing file is `None` and not an error: the base venue must be
    /// complete and playable with no layer at all
    /// (`r[patch.venue-layer.optional]`).
    ///
    /// # Errors
    ///
    /// If the file exists but will not parse, or names a version this
    /// build does not know. Loading a layer half-understood would apply
    /// some of somebody's changes and drop the rest, which is worse than
    /// refusing.
    // r[impl patch.venue-layer] - the sanctioned home for the non-portable
    // r[impl files.versioned] - a newer layer is refused by name
    pub fn load(dir: impl AsRef<Path>) -> anyhow::Result<Option<Self>> {
        Self::source(dir)?.map_or_else(|| Ok(None), |raw| Self::from_str(&raw).map(Some))
    }

    /// The layer's text, if the room has one.
    ///
    /// A missing file is the normal case and comes back as `None`; an
    /// unreadable one does too, because "there is no layer" and "the
    /// layer cannot be opened" are the same thing to a room that is
    /// meant to work without one.
    ///
    /// # Errors
    ///
    /// Never, today. It returns a `Result` so the directory half and the
    /// parsing half read the same at every call site.
    pub fn source(dir: impl AsRef<Path>) -> anyhow::Result<Option<String>> {
        Ok(std::fs::read_to_string(dir.as_ref().join(FILE)).ok())
    }

    /// Parse a layer from its text.
    ///
    /// The pure half, so a browser can apply one — see
    /// `Venue::from_files`.
    ///
    /// # Errors
    ///
    /// If the text will not parse, or names a version this build does
    /// not know.
    #[expect(
        clippy::should_implement_trait,
        reason = "this is the fallible parse half of `load`, named to match it; \
                  `FromStr` would put a different name on the same thing"
    )]
    pub fn from_str(raw: &str) -> anyhow::Result<Self> {
        let layer: Self =
            serde_json::from_str(raw).map_err(|e| anyhow::anyhow!("reading {FILE}: {e}"))?;
        if layer.version > VERSION {
            anyhow::bail!(
                "{FILE} is version {}; this build knows {VERSION}",
                layer.version
            );
        }
        Ok(layer)
    }

    /// Write this layer beside its venue, or remove the file when the
    /// layer has nothing left to say.
    ///
    /// An empty layer is deleted rather than written: a room with a
    /// `venue.ig-local` full of nothing reads as a room with local
    /// changes, and the check reports it as one.
    ///
    /// # Errors
    ///
    /// If the directory cannot be written.
    pub fn save(&self, dir: impl AsRef<Path>) -> anyhow::Result<()> {
        let path = dir.as_ref().join(FILE);
        if self.is_empty() {
            if path.exists() {
                std::fs::remove_file(&path)?;
            }
            return Ok(());
        }
        let json = serde_json::to_string_pretty(self)?;
        crate::venue::write_atomically(&path, &format!("{json}\n"))
    }

    /// Lay this over a venue.
    ///
    /// Returns the channels it actually changed, so the desk can mark
    /// those rows (`r[patch.venue-layer.visible]`) — a room behaving
    /// differently from its own file is a fact somebody needs before the
    /// night, not during it.
    // r[impl patch.venue-layer.visible] - which fixtures are overridden
    pub fn apply(&self, venue: &mut Venue) -> Vec<u32> {
        let mut touched = Vec::new();
        for chan in &self.remove {
            if venue.fixtures.iter().any(|f| f.chan == Some(*chan)) {
                venue.fixtures.retain(|f| f.chan != Some(*chan));
                touched.push(*chan);
            }
        }
        for (chan, over) in &self.fixtures {
            let Some(fixture) = venue.by_chan_mut(*chan) else {
                // A layer naming a fixture the room no longer has is not
                // an error — the base venue was re-extracted and the
                // rig moved on. It is simply nothing to apply.
                continue;
            };
            // Each field, as an expression saying whether it changed
            // anything. A layer that repeats the venue's own values is
            // not an override and must not be marked as one.
            let model = if let Some(model) = &over.model {
                fixture.model = Some(model.clone());
                true
            } else {
                false
            };
            let label = if let Some(label) = &over.label {
                fixture.label.clone_from(label);
                true
            } else {
                false
            };
            let gel = if let Some(gel) = &over.gel {
                fixture.gel.clone_from(gel);
                true
            } else {
                false
            };
            // An address is one edit of three fields, so it goes through
            // `set_address` like every other repatch. Either half may be
            // given alone — "same universe, address 100" is the common
            // one — and the other is taken from the fixture.
            let moved = match (
                over.universe.is_some() || over.address.is_some(),
                over.universe.or(fixture.universe),
                over.address.or(fixture.address),
            ) {
                (true, Some(universe), Some(address)) => {
                    fixture.set_address(Some(ignition_proto::DmxAddress {
                        universe,
                        start_channel: address,
                    }));
                    true
                }
                _ => false,
            };
            let parked = if over.patched == Some(false) {
                fixture.set_address(None);
                true
            } else {
                false
            };
            if model || label || gel || moved || parked {
                touched.push(*chan);
            }
        }
        for fixture in &self.add {
            // A layer that adds a channel the room already has would
            // shadow it; the base venue wins, because the layer is the
            // adjustment and the venue is the room.
            if fixture.chan.is_some_and(|c| {
                venue
                    .fixtures
                    .iter()
                    .any(|existing| existing.chan == Some(c))
            }) {
                continue;
            }
            if let Some(chan) = fixture.chan {
                touched.push(chan);
            }
            venue.fixtures.push(fixture.clone());
        }
        if !touched.is_empty() {
            venue.repatch();
        }
        touched.sort_unstable();
        touched.dedup();
        touched
    }
}

#[cfg(test)]
mod tests {
    use super::{Override, VenueLayer};
    use crate::venue::Venue;

    fn norco() -> Option<Venue> {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data/venues/norco");
        Venue::load(dir).ok()
    }

    /// r[verify patch.venue-layer] - the night-of repatch, kept apart
    #[test]
    fn a_layer_repatches_without_touching_the_venue_file() {
        let Some(mut venue) = norco() else {
            return;
        };
        let chan = venue.fixtures.first().and_then(|f| f.chan).unwrap_or(1);
        let before = venue.patch().by_chan(chan).map(|p| p.address);

        let mut layer = VenueLayer::new("norco");
        layer.fixtures.insert(
            chan,
            Override {
                universe: Some(4),
                address: Some(300),
                note: "its dimmer died on the 14th".to_owned(),
                ..Override::default()
            },
        );
        let touched = layer.apply(&mut venue);
        assert_eq!(touched, vec![chan]);

        let after = venue.patch().by_chan(chan).map(|p| p.address);
        assert_ne!(before, after, "the layer did not reach the patch");
        assert_eq!(after.map(|a| (a.universe, a.start_channel)), Some((4, 300)));
    }

    /// r[verify patch.venue-layer.optional] - a room works with no layer
    #[test]
    fn a_venue_with_no_layer_loads_and_is_unchanged() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data/venues/norco");
        if !dir.join("fixtures.json").exists() {
            return;
        }
        assert!(
            VenueLayer::load(&dir)
                .expect("a missing layer is not an error")
                .is_none(),
            "the shipped venue should not carry a layer"
        );
        // And an empty one changes nothing.
        let Some(mut venue) = norco() else {
            return;
        };
        let before = venue.fixtures.clone();
        assert!(VenueLayer::new("norco").apply(&mut venue).is_empty());
        assert_eq!(before, venue.fixtures);
    }

    #[test]
    fn a_fixture_out_of_service_stays_in_the_room() {
        let Some(mut venue) = norco() else {
            return;
        };
        let chan = venue.fixtures.first().and_then(|f| f.chan).unwrap_or(1);
        let count = venue.fixtures.len();
        let mut layer = VenueLayer::new("norco");
        layer.fixtures.insert(
            chan,
            Override {
                patched: Some(false),
                note: "blown, spare on order".to_owned(),
                ..Override::default()
            },
        );
        layer.apply(&mut venue);
        assert_eq!(venue.fixtures.len(), count, "a fixture was deleted");
        assert!(venue.patch().by_chan(chan).is_none(), "still on the wire");
    }

    #[test]
    fn a_layer_naming_a_fixture_the_room_lost_is_not_an_error() {
        // The base venue gets re-extracted and the rig moves on. The
        // layer is stale, not wrong.
        let Some(mut venue) = norco() else {
            return;
        };
        let mut layer = VenueLayer::new("norco");
        layer.fixtures.insert(
            99_999,
            Override {
                address: Some(1),
                ..Override::default()
            },
        );
        assert!(layer.apply(&mut venue).is_empty());
    }

    #[test]
    fn an_empty_layer_is_deleted_rather_than_written() {
        // A `venue.ig-local` full of nothing reads as a room with local
        // changes, and the check would report it as one.
        let dir = std::env::temp_dir().join("ignition-layer-empty");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("a scratch directory");
        let mut layer = VenueLayer::new("norco");
        layer.fixtures.insert(1, Override::default());
        layer.save(&dir).expect("writes");
        assert!(dir.join(super::FILE).exists());

        layer.fixtures.clear();
        layer.save(&dir).expect("and removes");
        assert!(!dir.join(super::FILE).exists());
    }

    #[test]
    fn a_layer_round_trips_and_keeps_what_it_did_not_understand() {
        // r[verify files.additive-evolution]
        let json = r#"{"version":1,"venue":"norco",
            "fixtures":{"41":{"address":100,"note":"why","invented":true}},
            "invented_later":{"a":1}}"#;
        let layer: VenueLayer = serde_json::from_str(json).expect("parses");
        assert_eq!(layer.touched(), 1);
        let back = serde_json::to_string(&layer).expect("writes");
        assert!(back.contains("invented_later"), "{back}");
        assert!(back.contains("\"invented\""), "{back}");
    }

    #[test]
    fn a_layer_from_a_newer_build_is_refused_by_name() {
        let dir = std::env::temp_dir().join("ignition-layer-newer");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("a scratch directory");
        std::fs::write(
            dir.join(super::FILE),
            r#"{"version":99,"venue":"norco","fixtures":{}}"#,
        )
        .expect("writes");
        let error = VenueLayer::load(&dir).expect_err("a newer layer is refused");
        assert!(error.to_string().contains("version 99"), "{error}");
    }
}
