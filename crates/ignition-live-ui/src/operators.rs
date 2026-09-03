//! Operators: the person at the desk, and what they keep to hand.
//!
//! An operator file (`data/operators/<name>.ig-user`) holds preferences
//! only — favourites, default mode and view, a remote mapping — and
//! never lighting data. A favourite is a *name* into the profile, so a
//! stale one renders as missing rather than failing, and two operators
//! on one profile see one library and two sets of shortcuts.
//!
//! The same file carries the operator's window layout under `windows`,
//! which another module owns. This one reads the file as loose JSON,
//! takes its own keys, and writes back read-modify-write so the layout
//! survives a favourites change.

// Nothing is dead here; it is mounted when `main.rs` hosts `live::Views`
// (and its stylesheet, `live::LIVE_CSS`). Until the integrator wires
// that, the crate root does not reach these items. Remove once mounted.

// r[impl studio.operators] - preferences only, by name into the profile
// r[impl studio.operators.favourites] - per-kind shortcut sets, orderable

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// The kinds a favourite can be — one set per kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Kind {
    Effect,
    Look,
    Macro,
    Trick,
    Bundle,
    Colour,
    Focus,
    Group,
}

impl Kind {
    /// Every kind, in library-tab order (the Library panel iterates its
    /// own tab list; tests use this one).
    #[cfg_attr(not(test), allow(dead_code))]
    pub const ALL: [Self; 8] = [
        Self::Effect,
        Self::Look,
        Self::Macro,
        Self::Trick,
        Self::Bundle,
        Self::Colour,
        Self::Focus,
        Self::Group,
    ];

    /// The tab label — short, because it is a touch tab.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Effect => "Effects",
            Self::Look => "Looks",
            Self::Macro => "Macros",
            Self::Trick => "Tricks",
            Self::Bundle => "Bundles",
            Self::Colour => "Colours",
            Self::Focus => "Focus",
            Self::Group => "Groups",
        }
    }
}

/// Per-kind shortcut sets over the profile, in the operator's order.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Favourites {
    #[serde(default)]
    pub effects: Vec<String>,
    #[serde(default)]
    pub looks: Vec<String>,
    #[serde(default)]
    pub macros: Vec<String>,
    #[serde(default)]
    pub tricks: Vec<String>,
    #[serde(default)]
    pub bundles: Vec<String>,
    #[serde(default)]
    pub colours: Vec<String>,
    #[serde(default)]
    pub focus: Vec<String>,
    #[serde(default)]
    pub groups: Vec<String>,
}

impl Favourites {
    #[must_use]
    pub const fn of(&self, kind: Kind) -> &Vec<String> {
        match kind {
            Kind::Effect => &self.effects,
            Kind::Look => &self.looks,
            Kind::Macro => &self.macros,
            Kind::Trick => &self.tricks,
            Kind::Bundle => &self.bundles,
            Kind::Colour => &self.colours,
            Kind::Focus => &self.focus,
            Kind::Group => &self.groups,
        }
    }

    pub const fn of_mut(&mut self, kind: Kind) -> &mut Vec<String> {
        match kind {
            Kind::Effect => &mut self.effects,
            Kind::Look => &mut self.looks,
            Kind::Macro => &mut self.macros,
            Kind::Trick => &mut self.tricks,
            Kind::Bundle => &mut self.bundles,
            Kind::Colour => &mut self.colours,
            Kind::Focus => &mut self.focus,
            Kind::Group => &mut self.groups,
        }
    }

    #[cfg_attr(not(test), allow(dead_code))]
    #[must_use]
    pub fn is_favourite(&self, kind: Kind, name: &str) -> bool {
        self.of(kind).iter().any(|n| n == name)
    }

    /// One tap: in if it was out, out if it was in. Returns whether it
    /// is a favourite afterwards.
    pub fn toggle(&mut self, kind: Kind, name: &str) -> bool {
        let list = self.of_mut(kind);
        if let Some(i) = list.iter().position(|n| n == name) {
            list.remove(i);
            false
        } else {
            list.push(name.to_string());
            true
        }
    }

    /// Move a favourite `by` places (negative is earlier). Clamped at
    /// the ends; a name that is not a favourite is a no-op.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn reorder(&mut self, kind: Kind, name: &str, by: isize) {
        let list = self.of_mut(kind);
        let Some(from) = list.iter().position(|n| n == name) else {
            return;
        };
        let end = list.len().saturating_sub(1);
        let to = if by.is_negative() {
            from.saturating_sub(by.unsigned_abs())
        } else {
            from.saturating_add(by.unsigned_abs()).min(end)
        };
        let item = list.remove(from);
        list.insert(to, item);
    }
}

/// The keys this module owns in an operator file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Operator {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub favourites: Favourites,
    /// `lights`, `graphics` or `video`.
    #[serde(default = "default_mode")]
    pub default_mode: String,
    /// `live` or `program`.
    #[serde(default = "default_view")]
    pub default_view: String,
    /// Which `data/profiles/remote.json` mapping the operator prefers.
    #[serde(default)]
    pub remote: Option<String>,
}

fn default_mode() -> String {
    "lights".into()
}
fn default_view() -> String {
    "live".into()
}

/// Which operator is at the desk: `IGNITION_OPERATOR`, else `cody`.
#[must_use]
pub fn current_name() -> String {
    std::env::var("IGNITION_OPERATOR").unwrap_or_else(|_| "cody".to_string())
}

/// Where operator files live, relative to the working directory like
/// every other data path the studio opens.
pub const DIR: &str = "data/operators";

/// Where an operator's file lives.
#[cfg_attr(not(test), allow(dead_code))]
#[must_use]
pub fn path_of(name: &str) -> PathBuf {
    path_in(std::path::Path::new(DIR), name)
}

fn path_in(dir: &std::path::Path, name: &str) -> PathBuf {
    dir.join(format!("{name}.ig-user"))
}

impl Operator {
    /// The shipped starter: a spread of effects across the families, every
    /// look and macro, the tricks and colours a busk reaches for first.
    #[must_use]
    pub fn starter(name: &str) -> Self {
        Self {
            name: name.to_string(),
            favourites: Favourites {
                effects: [
                    "chase",
                    "sparkle",
                    "back breathe",
                    "strobe",
                    "ballyhoo",
                    "windmill",
                    "pan wave",
                    "fly out",
                    "rainbow",
                    "colour wipe",
                    "colour fire",
                    "two colour chase",
                    "zoom pulse",
                    "iris chase",
                    "strip chase",
                    "blinder chase",
                    "white pop",
                    "colour bump",
                    "rig build",
                    "strobe riser",
                ]
                .iter()
                .map(std::string::ToString::to_string)
                .collect(),
                looks: ["verse bed", "chorus full", "punt", "blackout"]
                    .iter()
                    .map(std::string::ToString::to_string)
                    .collect(),
                macros: ["drop", "build 8", "breakdown", "end"]
                    .iter()
                    .map(std::string::ToString::to_string)
                    .collect(),
                tricks: [
                    "odds",
                    "evens",
                    "pairs",
                    "halves",
                    "mirror",
                    "centre out",
                    "ends in",
                    "four wings",
                ]
                .iter()
                .map(std::string::ToString::to_string)
                .collect(),
                bundles: Vec::new(),
                colours: [
                    "Warm White",
                    "Open White",
                    "Amber",
                    "Red",
                    "Deep Blue",
                    "Cyan",
                    "Magenta",
                    "Congo",
                ]
                .iter()
                .map(std::string::ToString::to_string)
                .collect(),
                focus: Vec::new(),
                groups: Vec::new(),
            },
            default_mode: default_mode(),
            default_view: default_view(),
            remote: None,
        }
    }

    /// The operator from a file's loose JSON — only this module's keys.
    /// A key that will not parse falls back to its default rather than
    /// taking the operator down with it.
    #[must_use]
    pub fn from_value(name: &str, value: &serde_json::Value) -> Self {
        let mut op: Self = serde_json::from_value(value.clone()).unwrap_or_else(|_| {
            let mut op = Self::starter(name);
            op.favourites = value
                .get("favourites")
                .and_then(|f| serde_json::from_value(f.clone()).ok())
                .unwrap_or_default();
            op
        });
        if op.name.is_empty() {
            op.name = name.to_string();
        }
        op
    }

    /// Load `name`'s file, or the starter when there is none.
    #[must_use]
    pub fn load(name: &str) -> Self {
        Self::load_from(std::path::Path::new(DIR), name)
    }

    /// `load`, from a directory other than the shipped one.
    #[must_use]
    pub fn load_from(dir: &std::path::Path, name: &str) -> Self {
        std::fs::read_to_string(path_in(dir, name)).map_or_else(
            |_| Self::starter(name),
            |raw| match serde_json::from_str::<serde_json::Value>(&raw) {
                Ok(value) => Self::from_value(name, &value),
                Err(error) => {
                    tracing::warn!(name, %error, "operator file does not parse; using the starter");
                    Self::starter(name)
                }
            },
        )
    }

    /// The operator at the desk.
    #[must_use]
    pub fn current() -> Self {
        Self::load(&current_name())
    }

    /// Write this module's keys into the file, keeping every other key
    /// (`windows`, anything a later module adds) as it was.
    ///
    /// # Errors
    ///
    /// Whatever `save_to` returns.
    pub fn save(&self) -> std::io::Result<()> {
        self.save_to(std::path::Path::new(DIR))
    }

    /// `save`, into a directory other than the shipped one.
    ///
    /// # Panics
    ///
    /// Never: a serialisation failure (which an `Operator` cannot
    /// actually produce — every field is a plain string, bool or list)
    /// is logged and leaves the file with its previous contents rather
    /// than panicking.
    ///
    /// # Errors
    ///
    /// The directory cannot be created, or the file cannot be written.
    pub fn save_to(&self, dir: &std::path::Path) -> std::io::Result<()> {
        let path = path_in(dir, &self.name);
        let mut value = std::fs::read_to_string(&path)
            .ok()
            .and_then(|raw| serde_json::from_str::<serde_json::Value>(&raw).ok())
            .unwrap_or_else(|| serde_json::Value::Object(serde_json::Map::default()));
        match serde_json::to_value(self) {
            Ok(mine) => {
                if let (Some(into), Some(from)) = (value.as_object_mut(), mine.as_object()) {
                    for (k, v) in from {
                        into.insert(k.clone(), v.clone());
                    }
                } else {
                    value = mine;
                }
            }
            Err(error) => {
                tracing::error!(
                    %error,
                    "operator failed to serialise; the file keeps its previous contents"
                );
            }
        }
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let text = serde_json::to_string_pretty(&value)
            .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))?;
        std::fs::write(&path, text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// r[verify studio.operators]
    #[test]
    fn a_missing_file_is_the_starter_and_never_a_panic() {
        let op = Operator::load("nobody-has-this-name");
        assert_eq!(op.name, "nobody-has-this-name");
        assert_eq!(op.favourites.effects.len(), 20);
        assert_eq!(op.favourites.looks.len(), 4);
        assert_eq!(op.favourites.macros.len(), 4);
        assert_eq!(op.favourites.tricks.len(), 8);
        assert_eq!(op.favourites.colours.len(), 8);
    }

    /// The starter names only things the shipped profile has, so the
    /// first operator never sees "missing".
    /// r[verify studio.operators.favourites]
    #[test]
    fn the_starter_favourites_exist_in_the_profile() {
        let profile = crate::library::profile();
        let f = &Operator::starter("cody").favourites;
        for e in &f.effects {
            assert!(profile.effects.contains_key(e), "effect {e:?}");
        }
        for l in &f.looks {
            assert!(profile.looks.contains_key(l), "look {l:?}");
        }
        for m in &f.macros {
            assert!(profile.macros.contains_key(m), "macro {m:?}");
        }
        for t in &f.tricks {
            assert!(profile.tricks.contains_key(t), "trick {t:?}");
        }
        for c in &f.colours {
            assert!(profile.colors.iter().any(|p| &p.name == c), "colour {c:?}");
        }
        // A spread, not one family: at least four families among the effects.
        let families: std::collections::BTreeSet<&str> = f
            .effects
            .iter()
            .filter_map(|e| profile.family_of(e))
            .collect();
        assert!(families.len() >= 4, "{families:?}");
    }

    /// r[verify studio.operators.favourites]
    #[test]
    fn toggle_and_reorder() {
        let mut f = Favourites::default();
        assert!(f.toggle(Kind::Effect, "chase"));
        assert!(f.toggle(Kind::Effect, "sparkle"));
        assert!(f.toggle(Kind::Effect, "strobe"));
        assert!(f.is_favourite(Kind::Effect, "sparkle"));
        assert!(!f.toggle(Kind::Effect, "sparkle"));
        assert!(!f.is_favourite(Kind::Effect, "sparkle"));
        assert_eq!(f.effects, vec!["chase", "strobe"]);
        f.reorder(Kind::Effect, "strobe", -1);
        assert_eq!(f.effects, vec!["strobe", "chase"]);
        // Clamped at the ends, and a stranger is a no-op.
        f.reorder(Kind::Effect, "strobe", -5);
        f.reorder(Kind::Effect, "nobody", 1);
        assert_eq!(f.effects, vec!["strobe", "chase"]);
        f.reorder(Kind::Effect, "strobe", 9);
        assert_eq!(f.effects, vec!["chase", "strobe"]);
    }

    /// Only this module's keys are read, and a stale favourite is kept
    /// as a name — the library renders it as missing.
    /// r[verify studio.operators]
    #[test]
    fn foreign_keys_are_ignored_on_read_and_kept_on_write() {
        let raw = serde_json::json!({
            "name": "x",
            "windows": [{"monitor": "DP-1"}],
            "favourites": {"effects": ["chase", "no such effect"]},
        });
        let op = Operator::from_value("x", &raw);
        assert_eq!(op.favourites.effects, vec!["chase", "no such effect"]);
        assert_eq!(op.default_view, "live");

        // Round trip through a temp directory, preserving `windows`.
        let dir = std::env::temp_dir().join(format!("ig-op-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(path_in(&dir, "x"), raw.to_string()).unwrap();
        let mut op = Operator::load_from(&dir, "x");
        op.favourites.toggle(Kind::Look, "punt");
        op.save_to(&dir).unwrap();
        let back: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(path_in(&dir, "x")).unwrap()).unwrap();
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(back["windows"][0]["monitor"], "DP-1");
        assert_eq!(back["favourites"]["looks"][0], "punt");
        assert_eq!(back["favourites"]["effects"][0], "chase");
    }
}
