//! A named, reusable set of fixture channels — the same concept as an Eos
//! "Group" or grandMA3 "Group Pool" entry. `ignition_viz::venue` loads
//! Norco's own real 112 groups straight from the live rig's exported
//! `groups.json`/`group-names.txt` (Eos's own group data — see
//! `docs/domain/norco-patch-and-groups.md`) and converts them into these;
//! this type itself has no venue-file or JSON-shape knowledge, just the
//! resolved `(name, chans)` pairs `recipe.rs` targets.

use ignition_proto::ChanId;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
// r[impl groups.order-is-data] - membership is an ordered Vec, not a set
// r[impl groups.order-is-stable]
pub struct Group {
    pub name: String,
    pub chans: Vec<ChanId>,
}

/// Finds a group by exact name — case-sensitive, matching Eos's own group
/// labels verbatim (unlike `fixture_profile.rs`'s manufacturer/model
/// matching, group names in the live show file are operator-authored and
/// exact by convention, e.g. "OH Movers", not something worth fuzzy-matching).
pub fn find<'a>(groups: &'a [Group], name: &str) -> Option<&'a Group> {
    groups.iter().find(|g| g.name == name)
}
