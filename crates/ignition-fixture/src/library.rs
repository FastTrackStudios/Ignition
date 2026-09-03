//! The fixture-type library: every document in `data/fixtures/`, and
//! the one place a venue's model string is resolved to one.
//!
//! Resolution used to happen in three places with three slightly
//! different rules — `channel_map.rs`'s substring match,
//! `fixture_profile.rs`'s shape match, and `gdtf_geometry.rs`'s
//! alias-aware match — over one `(manufacturer, model)` pair. Three
//! answers to one question is how a fixture ends up drawn as one model
//! and addressed as another. There is one answer here, and it is the
//! same order `r[viz.gdtf-aliases]` fixes for the geometry side:
//!
//! 1. the model string equals a type's console name,
//! 2. it equals one of that type's declared aliases,
//! 3. `data/gdtf/aliases.json` maps it to a console name,
//! 4. a normalised substring match, in either direction, as a last
//!    resort.
//!
//! Comparison is case- and punctuation-insensitive throughout, because
//! a venue file says `U'King` where a document says `UKing` and neither
//! is wrong.

// r[impl patch.type-resolution] - one rule, shared with the geometry side

use crate::document::FixtureType;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Where the library lives, relative to the workspace root.
pub const DEFAULT_DIR: &str = "data/fixtures";

/// The alias table shared with the visualizer's GDTF matching, so a
/// venue that needed an alias to draw does not need a second one to
/// address.
pub const ALIASES_FILE: &str = "data/gdtf/aliases.json";

/// A document that would not load, kept rather than thrown: a library
/// with one broken file should still patch the other seventeen
/// fixtures, and the desk should be able to say which file is broken.
#[derive(Debug, Clone)]
pub struct Rejected {
    pub path: PathBuf,
    pub message: String,
}

/// Every fixture type Ignition knows about.
#[derive(Debug, Clone, Default)]
pub struct Library {
    types: Vec<FixtureType>,
    /// Normalised name → index into `types`. Both console names and
    /// declared aliases land here; a later file never displaces an
    /// earlier one, so loading is deterministic.
    by_name: BTreeMap<String, usize>,
    /// Normalised venue model string → normalised console name, from
    /// `aliases.json`.
    aliases: BTreeMap<String, String>,
    rejected: Vec<Rejected>,
}

impl Library {
    /// Load a directory of documents. A missing directory is an empty
    /// library, not an error — the desk still has to open.
    #[must_use]
    pub fn load_dir(dir: impl AsRef<Path>) -> Self {
        let mut library = Self::default();
        let Ok(entries) = std::fs::read_dir(dir.as_ref()) else {
            return library;
        };
        // Sorted, so a name collision resolves the same way twice.
        let mut paths: Vec<PathBuf> = entries
            .flatten()
            .map(|e| e.path())
            .filter(|p| {
                p.extension()
                    .is_some_and(|e| e.eq_ignore_ascii_case("json"))
            })
            .collect();
        paths.sort();
        for path in paths {
            library.add_file(&path);
        }
        library
    }

    /// Load `data/fixtures` and `data/gdtf/aliases.json`, looking from
    /// the working directory and then from the workspace root — the
    /// same two-place search the rest of the tree does, so a test that
    /// runs from its crate directory finds the data.
    #[must_use]
    pub fn load_default() -> Self {
        let mut library = Self::load_dir(resolve_path(DEFAULT_DIR));
        library.load_aliases(resolve_path(ALIASES_FILE));
        library
    }

    fn add_file(&mut self, path: &Path) {
        let text = match std::fs::read_to_string(path) {
            Ok(text) => text,
            Err(error) => {
                self.rejected.push(Rejected {
                    path: path.to_owned(),
                    message: error.to_string(),
                });
                return;
            }
        };
        match serde_json::from_str::<FixtureType>(&text) {
            Ok(doc) => self.insert(doc),
            Err(error) => self.rejected.push(Rejected {
                path: path.to_owned(),
                message: error.to_string(),
            }),
        }
    }

    /// Add a document, indexing its console name and every alias it
    /// declares. An empty console name is refused: the name is the
    /// identity, and a nameless type can never be patched to.
    pub fn insert(&mut self, doc: FixtureType) {
        if doc.console_name.trim().is_empty() {
            self.rejected.push(Rejected {
                path: PathBuf::new(),
                message: "a fixture type with no console_name cannot be patched to".to_owned(),
            });
            return;
        }
        let index = self.types.len();
        let names: Vec<String> = doc.names().map(normalize).collect();
        self.types.push(doc);
        for name in names {
            self.by_name.entry(name).or_insert(index);
        }
    }

    /// Read `aliases.json` — venue model string to console name.
    ///
    /// The file is shared with the visualizer, which keeps its own
    /// non-alias keys in it (`_comment`, `_display_scale`), so only the
    /// string-to-string entries are aliases and anything else is
    /// somebody else's business. A missing or unreadable file leaves the
    /// table empty: an alias is a convenience, not a requirement.
    pub fn load_aliases(&mut self, path: impl AsRef<Path>) {
        let Ok(text) = std::fs::read_to_string(path.as_ref()) else {
            return;
        };
        let Ok(map) = serde_json::from_str::<BTreeMap<String, serde_json::Value>>(&text) else {
            self.rejected.push(Rejected {
                path: path.as_ref().to_owned(),
                message: "not a JSON object".to_owned(),
            });
            return;
        };
        for (from, to) in map {
            if from.starts_with('_') {
                continue;
            }
            let Some(to) = to.as_str() else {
                continue;
            };
            self.aliases.insert(normalize(&from), normalize(to));
        }
    }

    /// Every type, in load order.
    #[must_use]
    pub fn types(&self) -> &[FixtureType] {
        &self.types
    }

    /// Documents that would not load, for the desk to show.
    #[must_use]
    pub fn rejected(&self) -> &[Rejected] {
        &self.rejected
    }

    /// Resolve a venue's `(manufacturer, model)` to a fixture type.
    ///
    /// The manufacturer is a hint, not a key: venues spell it four ways
    /// for one OEM, and the model string is what carries the identity.
    /// It is used only to break a tie between two substring candidates.
    #[must_use]
    pub fn find(&self, manufacturer: &str, model: &str) -> Option<&FixtureType> {
        let wanted = normalize(model);
        if wanted.is_empty() {
            return None;
        }
        // 1 and 2 — an exact console name or a declared alias.
        if let Some(found) = self.by_name.get(&wanted).and_then(|i| self.types.get(*i)) {
            return Some(found);
        }
        // 3 — the shared alias table.
        if let Some(found) = self
            .aliases
            .get(&wanted)
            .and_then(|name| self.by_name.get(name))
            .and_then(|i| self.types.get(*i))
        {
            return Some(found);
        }
        // 4 — a normalised substring, either direction. Longest match
        // first, so `mini gobo moving head light 11ch` prefers the type
        // named for it over the shorter `mini gobo`.
        let maker = normalize(manufacturer);
        let mut best: Option<(usize, &FixtureType)> = None;
        for doc in &self.types {
            for name in doc.names() {
                let candidate = normalize(name);
                if candidate.is_empty() {
                    continue;
                }
                if !(wanted.contains(&candidate) || candidate.contains(&wanted)) {
                    continue;
                }
                let mut score = candidate.len();
                // A shared manufacturer breaks a tie towards the right
                // OEM when two types share a generic model word.
                if !maker.is_empty() && normalize(&doc.manufacturer).contains(&maker) {
                    score = score.saturating_add(1000);
                }
                if best.is_none_or(|(existing, _)| score > existing) {
                    best = Some((score, doc));
                }
            }
        }
        best.map(|(_, doc)| doc)
    }
}

/// Letters and digits, lower-cased; everything else dropped.
///
/// Spaces go too, not just punctuation. A venue writes `U'King
/// ZQ-02341`, a document writes `UKing ZQ02341` and a reseller writes
/// `uking zq 02341`; all three are the same fixture, and the only rule
/// that makes them equal is to keep nothing but the alphanumerics. This
/// is the same comparison the visualizer's GDTF matching uses
/// (`r[viz.gdtf-aliases]`, "case- and punctuation-insensitively"), and
/// the two must agree or a fixture draws as one model and addresses as
/// another.
fn normalize(text: &str) -> String {
    text.chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

/// A repo-relative path, found from the working directory or from the
/// workspace root two levels above this crate.
fn resolve_path(relative: &str) -> PathBuf {
    let here = PathBuf::from(relative);
    if here.exists() {
        return here;
    }
    let from_root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(relative);
    if from_root.exists() {
        return from_root;
    }
    here
}

#[cfg(test)]
mod tests {
    use super::{Library, normalize};
    use crate::document::FixtureType;

    fn doc(console_name: &str, manufacturer: &str) -> FixtureType {
        FixtureType {
            console_name: console_name.to_owned(),
            manufacturer: manufacturer.to_owned(),
            ..FixtureType::default()
        }
    }

    #[test]
    fn spelling_does_not_decide_identity() {
        // A venue file says `U'King`; a document says `UKing`. Neither
        // is wrong and both have to patch.
        assert_eq!(normalize("U'King ZQ-02341"), normalize("uking zq 02341"));
        assert_eq!(normalize("  Mini   Gobo  "), "minigobo");
    }

    /// r[verify patch.type-resolution] - exact before fuzzy
    #[test]
    fn an_exact_name_wins_over_a_substring() {
        let mut library = Library::default();
        library.insert(doc("Mini Gobo", "Lixada"));
        library.insert(doc("Mini Gobo Moving Head Light 11ch", "Riukoe"));
        let found = library.find("", "Mini Gobo").unwrap();
        assert_eq!(found.console_name, "Mini Gobo");
    }

    #[test]
    fn the_longest_substring_wins_when_nothing_is_exact() {
        let mut library = Library::default();
        library.insert(doc("Mini Gobo", "Lixada"));
        library.insert(doc("Mini Gobo Moving Head Light", "Riukoe"));
        let found = library
            .find("", "Riukoe Mini Gobo Moving Head Light 11ch")
            .unwrap();
        assert_eq!(found.console_name, "Mini Gobo Moving Head Light");
    }

    /// r[verify patch.type-aliases] - one head, several names
    #[test]
    fn a_declared_alias_resolves() {
        // The same OEM head is sold as a Lixada, a Riukoe and a U'King.
        // One document, three names.
        let mut doc = doc("ZKYMZL TY-30", "ZKYMZL");
        doc.console_aliases = vec!["Mini Gobo Moving Head Light".to_owned()];
        let mut library = Library::default();
        library.insert(doc);
        let found = library
            .find("Riukoe", "Mini Gobo Moving Head Light")
            .unwrap();
        assert_eq!(found.console_name, "ZKYMZL TY-30");
    }

    /// r[verify patch.type-identity] - the name is the identity
    #[test]
    fn a_nameless_type_is_refused_not_indexed() {
        // The console name is the identity; a blank one would silently
        // claim every empty model string in the venue.
        let mut library = Library::default();
        library.insert(doc("", "Nobody"));
        assert!(library.types().is_empty());
        assert_eq!(library.rejected().len(), 1);
        assert!(library.find("", "").is_none());
    }
}
