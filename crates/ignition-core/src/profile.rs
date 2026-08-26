//! Profile — what a rig must provide, and what a show may say.
//!
//! A profile is an *interface*. A venue implements it; a show is written
//! against it; neither knows about the other. That indirection is not
//! tidiness — it is what turns portability from a question about pairs
//! into two independent checks.
//!
//! Without it: does show A work at venue B? With N shows and M venues
//! that is N×M questions, each answered by inspection and re-asked
//! whenever either side changes. With it: does this show only use what
//! the profile declares, and does this venue implement what the profile
//! requires. If both hold, every show plays at every venue — N+M checks,
//! and a new room is verified before a show is ever opened in it.
//!
//! A profile carries more than names. It ships the **Tricks** and the
//! **effects** a show is built from, so the vocabulary is not just what
//! the rig has but what can be done with it. A venue supplies the
//! fixtures; the profile supplies the programming.

use crate::preset::ColorPreset;
use crate::recipe::Recipe;
use crate::tricks::Trick;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// What kind of thing a role names.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum RoleKind {
    /// A layer of the rig — `Key`, `Wash`, `Back`.
    Group,
    /// A place a mover can point — `Vocal`, `Stage`.
    Focus,
    /// A video surface — `Main`.
    Canvas,
}

/// One name a venue is expected to bind.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Role {
    pub name: String,
    pub kind: RoleKind,
    /// A venue that leaves a required role unbound does not implement
    /// this profile. Optional roles are for what a room may or may not
    /// have — a follow spot, a floor package — and a show using one must
    /// still run where it is absent.
    #[serde(default)]
    pub required: bool,
    /// What the role is *for*, in the profile author's words. This is
    /// the only thing telling a new venue what to bind `Back` to, so it
    /// is data rather than a comment.
    #[serde(default)]
    pub about: String,
}

impl Role {
    pub fn required(name: &str, kind: RoleKind, about: &str) -> Self {
        Self {
            name: name.into(),
            kind,
            required: true,
            about: about.into(),
        }
    }

    pub fn optional(name: &str, kind: RoleKind, about: &str) -> Self {
        Self {
            name: name.into(),
            kind,
            required: false,
            about: about.into(),
        }
    }
}

/// A profile: the vocabulary, and the programming built on it.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Profile {
    pub name: String,
    #[serde(default)]
    pub version: u32,
    #[serde(default)]
    pub about: String,
    /// Every role a venue may bind, required or not.
    #[serde(default)]
    pub roles: Vec<Role>,
    /// Default colours, which a venue inherits unless it overrides.
    ///
    /// Colour is the one part of the vocabulary that ships with values
    /// rather than only names, because RGB is portable in a way a
    /// coordinate is not — see `r[default.colour-defaults-ship]`. It is
    /// also what keeps implementing a venue cheap: bind the groups,
    /// which genuinely differ per rig, and inherit the colours, which
    /// mostly do not.
    #[serde(default)]
    pub colors: Vec<ColorPreset>,
    /// Named Trick chains — `"odds"`, `"halves"`, `"fan out"`.
    #[serde(default)]
    pub tricks: BTreeMap<String, Vec<Trick>>,
    /// The effects library. Recipes written against role names, so they
    /// resolve at any venue implementing the profile.
    #[serde(default)]
    pub effects: BTreeMap<String, Recipe>,
}

/// How one venue implements a profile.
///
/// The indirection that makes a profile useful. Norco already has a key
/// light — it calls it `Front Wash`, and its lead position `Vocal
/// Centre` — so the work of implementing a profile is almost never
/// building anything, it is *saying which of the things you already have
/// plays each role*. A venue with no binding fails the check while owning
/// every fixture it needs, which is the correct answer: an interface
/// nobody implemented is not implemented.
///
/// A group role binds to a `Selection` rather than to a name, so a role
/// can be an expression — `Key` may be one group at one venue and
/// "washes downstage of the plaster line" at another. Focus and canvas
/// roles bind to names, since a venue's own palette already holds the
/// values.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct VenueProfile {
    /// Which profile this venue claims to implement.
    pub profile: String,
    #[serde(default)]
    pub groups: BTreeMap<String, crate::Selection>,
    #[serde(default)]
    pub focus: BTreeMap<String, String>,
    #[serde(default)]
    pub canvases: BTreeMap<String, String>,
    /// Colours this venue overrides. Anything absent is inherited from
    /// the profile, which is what keeps implementing a room cheap.
    #[serde(default)]
    pub colors: Vec<ColorPreset>,
}

impl VenueProfile {
    /// The selection a group role resolves to.
    pub fn group(&self, role: &str) -> Option<&crate::Selection> {
        self.groups.get(role)
    }

    /// The venue's own name for a focus role.
    pub fn focus(&self, role: &str) -> Option<&str> {
        self.focus.get(role).map(String::as_str)
    }

    pub fn canvas(&self, role: &str) -> Option<&str> {
        self.canvases.get(role).map(String::as_str)
    }
}

impl Bindings for VenueProfile {
    fn has_group(&self, name: &str) -> bool {
        self.groups.contains_key(name)
    }
    fn has_focus(&self, name: &str) -> bool {
        self.focus.contains_key(name)
    }
    fn has_canvas(&self, name: &str) -> bool {
        self.canvases.contains_key(name)
    }
}

/// Something a venue was expected to provide and did not.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Gap {
    pub role: String,
    pub kind: RoleKind,
    pub required: bool,
    pub about: String,
}

impl std::fmt::Display for Gap {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} {:?} role {:?} is unbound",
            if self.required { "required" } else { "optional" },
            self.kind,
            self.role
        )?;
        if !self.about.is_empty() {
            write!(f, " — {}", self.about)?;
        }
        Ok(())
    }
}

/// What a venue actually provides, as plain names.
///
/// A trait rather than a concrete venue type, and deliberately so: the
/// roles come from a profile and profiles are data, so an enum of known
/// roles would make adding one a code change and adding a profile a
/// fork. Anything that can answer "do you have a group called this" can
/// be checked.
pub trait Bindings {
    fn has_group(&self, name: &str) -> bool;
    fn has_focus(&self, name: &str) -> bool;
    fn has_canvas(&self, name: &str) -> bool;

    fn has(&self, role: &Role) -> bool {
        match role.kind {
            RoleKind::Group => self.has_group(&role.name),
            RoleKind::Focus => self.has_focus(&role.name),
            RoleKind::Canvas => self.has_canvas(&role.name),
        }
    }
}

impl Profile {
    /// Roles this venue leaves unbound, required ones first.
    ///
    /// Static: nothing runs, nothing is rendered. A new room is verified
    /// before a show is opened in it rather than during one.
    pub fn gaps(&self, venue: &impl Bindings) -> Vec<Gap> {
        let mut gaps: Vec<Gap> = self
            .roles
            .iter()
            .filter(|role| !venue.has(role))
            .map(|role| Gap {
                role: role.name.clone(),
                kind: role.kind,
                required: role.required,
                about: role.about.clone(),
            })
            .collect();
        // Required first, then by name — a report an operator reads top
        // down should start with what actually blocks the show.
        gaps.sort_by(|a, b| b.required.cmp(&a.required).then(a.role.cmp(&b.role)));
        gaps
    }

    /// Whether this venue implements the profile — every *required* role
    /// bound. Optional gaps are reported by [`Profile::gaps`] but do not
    /// disqualify: a show using an optional role still runs where it is
    /// absent, thinner.
    pub fn satisfied_by(&self, venue: &impl Bindings) -> bool {
        self.gaps(venue).iter().all(|gap| !gap.required)
    }

    /// Names a show may use, of one kind.
    pub fn vocabulary(&self, kind: RoleKind) -> Vec<&str> {
        self.roles
            .iter()
            .filter(|r| r.kind == kind)
            .map(|r| r.name.as_str())
            .collect()
    }

    /// Names this show uses that the profile does not declare.
    ///
    /// The other half of the check. A venue may legitimately provide
    /// more than its profile — room-specific programming is real work —
    /// and a show reaching for those extras is not wrong so much as
    /// *not portable*, which should be visible rather than discovered in
    /// another city.
    pub fn undeclared<'a>(
        &self,
        used: impl IntoIterator<Item = (&'a str, RoleKind)>,
    ) -> Vec<(String, RoleKind)> {
        let mut out: Vec<(String, RoleKind)> = used
            .into_iter()
            .filter(|(name, kind)| {
                !self
                    .roles
                    .iter()
                    .any(|r| r.kind == *kind && r.name.eq_ignore_ascii_case(name))
            })
            .map(|(name, kind)| (name.to_string(), kind))
            .collect();
        out.sort();
        out.dedup();
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Venue {
        groups: Vec<&'static str>,
        focus: Vec<&'static str>,
    }

    impl Bindings for Venue {
        fn has_group(&self, name: &str) -> bool {
            self.groups.iter().any(|g| g.eq_ignore_ascii_case(name))
        }
        fn has_focus(&self, name: &str) -> bool {
            self.focus.iter().any(|f| f.eq_ignore_ascii_case(name))
        }
        fn has_canvas(&self, _: &str) -> bool {
            false
        }
    }

    fn profile() -> Profile {
        Profile {
            name: "test".into(),
            roles: vec![
                Role::required("Key", RoleKind::Group, "faces"),
                Role::required("Wash", RoleKind::Group, "colour"),
                Role::optional("Movers", RoleKind::Group, "dynamic"),
                Role::required("Vocal", RoleKind::Focus, "the lead"),
            ],
            ..Default::default()
        }
    }

    #[test]
    fn a_venue_binding_everything_required_satisfies_the_profile() {
        let venue = Venue {
            groups: vec!["Key", "Wash"],
            focus: vec!["Vocal"],
        };
        assert!(profile().satisfied_by(&venue));
        // The optional gap is still reported — it is information, not a
        // failure.
        let gaps = profile().gaps(&venue);
        assert_eq!(gaps.len(), 1);
        assert_eq!(gaps[0].role, "Movers");
        assert!(!gaps[0].required);
    }

    #[test]
    fn a_missing_required_role_fails_the_check() {
        let venue = Venue {
            groups: vec!["Key"],
            focus: vec!["Vocal"],
        };
        assert!(!profile().satisfied_by(&venue));
    }

    /// The report is read top down by somebody who wants to know what
    /// blocks the show, so what blocks it comes first.
    #[test]
    fn required_gaps_are_reported_first() {
        let venue = Venue {
            groups: vec![],
            focus: vec![],
        };
        let gaps = profile().gaps(&venue);
        assert_eq!(gaps.len(), 4);
        assert!(gaps[0].required && gaps[1].required && gaps[2].required);
        assert!(!gaps[3].required, "the optional role sorted above a required one");
    }

    /// A show reaching past its profile is not wrong, but it is not
    /// portable — and that has to be visible here rather than discovered
    /// in another city.
    #[test]
    fn a_show_using_undeclared_names_is_reported() {
        let used = vec![
            ("Key", RoleKind::Group),
            ("Pars Qtr 3", RoleKind::Group),
            ("Vocal", RoleKind::Focus),
        ];
        let out = profile().undeclared(used);
        assert_eq!(out, vec![("Pars Qtr 3".to_string(), RoleKind::Group)]);
    }

    /// A binding is what implementing a profile actually consists of:
    /// saying which of the things a room already has plays each role.
    /// Norco owns a key light and calls it `Front Wash`.
    #[test]
    fn a_binding_satisfies_the_profile_using_the_venues_own_names() {
        let mut venue = VenueProfile {
            profile: "test".into(),
            ..Default::default()
        };
        venue.groups.insert(
            "Key".into(),
            crate::Selection::Group("Front Wash".into()),
        );
        venue
            .groups
            .insert("Wash".into(), crate::Selection::Group("Washers".into()));
        venue
            .focus
            .insert("Vocal".into(), "Vocal Centre".into());
        // Wash and Vocal are bound; Key is bound; nothing else is.
        let gaps = profile().gaps(&venue);
        assert!(gaps.iter().all(|g| g.role != "Key"));
        assert_eq!(venue.group("Key"), Some(&crate::Selection::Group("Front Wash".into())));
        assert_eq!(venue.focus("Vocal"), Some("Vocal Centre"));
    }

    /// Roles of one kind do not satisfy another. A venue with a *focus*
    /// point called Key does not have a key light.
    #[test]
    fn kinds_do_not_cross() {
        let venue = Venue {
            groups: vec!["Wash"],
            focus: vec!["Vocal", "Key"],
        };
        let gaps = profile().gaps(&venue);
        assert!(gaps.iter().any(|g| g.role == "Key" && g.kind == RoleKind::Group));
    }
}
