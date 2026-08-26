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
// r[impl profile.declares-vocabulary] - the four kinds of name a show may use
pub enum RoleKind {
    /// A layer of the rig — `Key`, `Wash`, `Back`.
    Group,
// r[impl profile.areas.portable-question-is-focus] - "where is the talent" is a focus role, not an area
    /// A place a mover can point — `Vocal`, `Stage`.
    Focus,
    /// A region of the stage where people stand — `Downstage Left`.
    ///
    /// A profile *may* declare one, and the default profile declares
    /// none. How many areas a stage has is a property of the stage: a
    /// club has three, a stadium with a thrust and a B-stage has fifteen
    /// with names no central list could have anticipated. Enumerating
    /// them centrally would either exclude the large room or burden the
    /// small one, and the portable question — where is the talent — is
    /// already answered by the `Vocal` and `Stage` focus roles.
    ///
    /// So areas are venue-owned, and this kind exists for the profile
    /// that genuinely wants to require one.
    ///
    /// An area is still not a focus point wearing a hat, even where a
    /// venue binds it to one. A focus point answers "where do I aim"; an
    /// area answers "where is the talent", and those diverge the moment
    /// something wants the fixtures that *cover* a region rather than
    /// the aim that reaches its centre.
    // r[impl profile.areas.profile-may-require-one]
    // r[impl profile.areas.not-a-focus-point] - a distinct kind from Focus, even though a venue binds it by focus-point name
    Area,
    /// A video surface — `Main`.
    Canvas,
    /// A colour role — `Warm`, `Deep`. Declared with a value in
    /// `Profile::colors` rather than as a `Role`, so a venue inherits it
    /// rather than binding it; the kind exists so the show-side check
    /// can say what kind of name it did not find.
    // r[impl default.colour-roles-are-semantic] - a colour is a name a show uses, checked like one
    Colour,
}

/// One name a venue is expected to bind.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
// r[impl profile.roles-are-intent] - a role is a named datum with an `about`, not an enum of equipment
pub struct Role {
    pub name: String,
    pub kind: RoleKind,
    /// A venue that leaves a required role unbound does not implement
    /// this profile. Optional roles are for what a room may or may not
    /// have — a follow spot, a floor package — and a show using one must
    /// still run where it is absent.
    // r[impl profile.required-and-optional]
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
// r[impl profile.declares-vocabulary]
// r[impl profile.several-may-exist] - a profile is a data value, any number may be loaded
// r[impl files.required-roles] - the required vocabulary is declared here, not fixed in software
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
    // r[impl default.colour-defaults-ship]
    #[serde(default)]
    pub colors: Vec<ColorPreset>,
    /// Named Trick chains — `"odds"`, `"halves"`, `"fan out"`.
    // r[impl tricks.shared-or-inline] - the shared named pool; inline chains live on the recipe
    #[serde(default)]
    pub tricks: BTreeMap<String, Vec<Trick>>,
    /// The effects library. Recipes written against role names, so they
    /// resolve at any venue implementing the profile.
    // r[impl effects.library.profile-ships-it]
    #[serde(default)]
    pub effects: BTreeMap<String, Recipe>,
    /// What each effect is, for the person choosing one: its family
    /// and a one-line note. Keyed like `effects`, and a separate map
    /// rather than a field on `Recipe` because a recipe is a recipe
    /// wherever it lives — in a cue, a bump, a show — and the note is
    /// about the *library entry*, not the steps.
    // r[impl effects.library.profile-ships-it] - the notes ship beside the recipes
    #[serde(default)]
    pub effect_notes: BTreeMap<String, EffectNote>,
    /// Named bundles — several library effects taken as one — keyed by
    /// the name a programmer types. Members are `effects` keys, never
    /// copied recipes, so a bundle is one line in a cue.
    // r[impl effects.bundle] - the profile ships the bundles beside the library
    #[serde(default)]
    pub bundles: BTreeMap<String, Bundle>,
}

/// Several library effects taken as one: "pan sweeps every two bars,
/// dimmer pulses every beat, colour steps every bar" under one name.
///
/// Timing stays uniform *per recipe* — each member keeps its own clock —
/// which is exactly what a bundle is for: one attribute set, several
/// clocks. `recipes` are names in `Profile::effects`, resolved the way a
/// cue resolves any library effect (`r[effects.library.by-name]`).
// r[impl effects.bundle]
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Bundle {
    pub name: String,
    /// The song moment it is for — see `effects::BUNDLE_FAMILIES`.
    pub family: String,
    /// One sentence for a picker: what it looks like — and where it belongs.
    pub about: String,
    /// Library effect names, in the order they were thought of.
    pub recipes: Vec<String>,
}

/// What a library effect is, in the words a picker shows.
///
/// `family` is one of the six the library is organised by — intensity,
/// movement, colour, beam, strip, one-shot — and `about` is one sentence
/// an operator reads on a button: what it looks like and where it
/// belongs.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct EffectNote {
    pub family: String,
    pub about: String,
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
// r[impl profile.venue-binds]
// r[impl files.vocabulary] - the venue publishes its bindings by name
pub struct VenueProfile {
    // r[impl profile.venue-declares-what-it-implements]
    /// Which profile this venue claims to implement.
    pub profile: String,
    #[serde(default)]
    pub groups: BTreeMap<String, crate::Selection>,
    #[serde(default)]
    pub focus: BTreeMap<String, String>,
    #[serde(default)]
    pub canvases: BTreeMap<String, String>,
    /// This venue's blocking grid, as its own names for its own areas.
    ///
    /// Venue-owned rather than a profile binding, because the number of
    /// areas is a property of the stage — see [`RoleKind::Area`]. A
    /// profile that does require an area is still checked against this
    /// map, so the two models do not fight.
    ///
    /// Bound to focus-point names rather than regions, for now: every
    /// venue already has focus points, which makes areas usable today.
    /// An area with real extent — and therefore derivable fixture
    /// coverage — is the follow-on, and binding by name does not stand
    /// in its way.
    // r[impl profile.areas]
    // r[impl profile.areas.venue-decides-granularity] - an open map, nothing caps or requires a count
    #[serde(default)]
    pub areas: BTreeMap<String, String>,
    /// Colours this venue overrides. Anything absent is inherited from
    /// the profile, which is what keeps implementing a room cheap.
    // r[impl default.colour-defaults-ship] - venue overrides; anything absent is inherited
    #[serde(default)]
    pub colors: Vec<ColorPreset>,
}

impl VenueProfile {
    /// The selection a group role resolves to.
    // r[impl profile.unbound-is-visible] - `None` for an unbound role, never an empty selection
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

    /// The venue's own focus point for a stage area.
    pub fn area(&self, role: &str) -> Option<&str> {
        self.areas.get(role).map(String::as_str)
    }
}

/// Resolving a role to fixtures, which is the binding's whole job at
/// run time. `Bindings` below answers "is this bound" for the static
/// check; this answers "bound to what" for the rig.
// r[impl profile.resolution-by-role]
impl crate::selection::Roles for VenueProfile {
    fn role(&self, name: &str) -> Option<&crate::Selection> {
        self.groups.get(name)
    }
}

// r[impl profile.trait-not-hardcode]
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
    fn has_area(&self, name: &str) -> bool {
        self.areas.contains_key(name)
    }
    fn has_colour(&self, name: &str) -> bool {
        self.colors
            .iter()
            .any(|c| c.name.eq_ignore_ascii_case(name))
    }
}

/// Something a venue was expected to provide and did not.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
// r[impl profile.unbound-is-visible] - the reportable form of an unbound role
// r[impl files.required-roles] - the gap a half-patched venue reports
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
            if self.required {
                "required"
            } else {
                "optional"
            },
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
// r[impl profile.trait-not-hardcode]
pub trait Bindings {
    fn has_group(&self, name: &str) -> bool;
    fn has_focus(&self, name: &str) -> bool;
    fn has_canvas(&self, name: &str) -> bool;
    fn has_area(&self, name: &str) -> bool;
    /// Colours a venue defines itself. Off by default: a colour role is
    /// inherited from the profile's defaults, so its absence at the
    /// venue is never a gap.
    fn has_colour(&self, _name: &str) -> bool {
        false
    }

    fn has(&self, role: &Role) -> bool {
        match role.kind {
            RoleKind::Group => self.has_group(&role.name),
            RoleKind::Focus => self.has_focus(&role.name),
            RoleKind::Canvas => self.has_canvas(&role.name),
            RoleKind::Area => self.has_area(&role.name),
            RoleKind::Colour => self.has_colour(&role.name),
        }
    }
}

impl Profile {
    /// Reads a profile from JSON. The default profile is a data file,
    /// `data/profiles/ignition.ig-profile`: every `r[default.*]`
    /// requirement — which layers are required, which colours ship with
    /// values, that `Main` is the canvas — is a fact about that file,
    /// verified by the tests below, never a constant in this code.
    // r[impl default.layers-not-positions] - the roles are whatever the file declares; nothing spatial is coded
    // r[impl default.small]
    // r[impl default.key]
    // r[impl default.wash]
    // r[impl default.back]
    // r[impl default.optional-layers]
    // r[impl default.focus-required]
    // r[impl default.focus-optional]
    // r[impl default.colour-set]
    // r[impl default.canvas-main]
    // r[impl default.canvas-optional]
    // r[impl default.implementation-cost] - the required count is a property of the data, held by test
    // r[impl profile.minimal] - minimality is the file's; the loader imposes no floor
    // r[impl profile.several-may-exist] - any file that parses is a profile
    pub fn parse(raw: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(raw)
    }

    /// Reads a profile file.
    // r[impl profile.extensions] - `.ig-profile`, written out
    pub fn load(path: impl AsRef<std::path::Path>) -> Result<Self, crate::show_file::Error> {
        let path = path.as_ref();
        let raw = std::fs::read_to_string(path).map_err(|source| crate::show_file::Error::Io {
            path: path.to_path_buf(),
            source,
        })?;
        Self::parse(&raw).map_err(|source| crate::show_file::Error::Json {
            path: path.to_path_buf(),
            source,
        })
    }

    /// Whether the roles a rig must bind stay few: the number that
    /// decides whether a second venue happens.
    // r[impl profile.setup-cost-is-the-metric] - required roles are the cost, counted
    pub fn required(&self, kind: RoleKind) -> Vec<&str> {
        self.roles
            .iter()
            .filter(|r| r.kind == kind && r.required)
            .map(|r| r.name.as_str())
            .collect()
    }

    /// Roles this venue leaves unbound, required ones first.
    ///
    /// Static: nothing runs, nothing is rendered. A new room is verified
    /// before a show is opened in it rather than during one.
    // r[impl profile.check-is-static]
    // r[impl files.compatibility-check] - venue against profile, every gap by name
    // r[impl files.required-roles] - a missing role is reported, not fatal
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
    // r[impl profile.required-and-optional]
    // r[impl profile.check-is-static]
    // r[impl default.optional-is-not-second-class] - an optional gap never disqualifies; the look is thinner
    pub fn satisfied_by(&self, venue: &impl Bindings) -> bool {
        self.gaps(venue).iter().all(|gap| !gap.required)
    }

    /// Names a show may use, of one kind.
    // r[impl profile.declares-vocabulary]
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
    // r[impl profile.venue-may-exceed]
    // r[impl profile.declares-vocabulary] - the show-side check
    // r[impl files.compatibility-check] - show against profile
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
        fn has_area(&self, _: &str) -> bool {
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

    /// r[verify profile.required-and-optional]
    /// r[verify profile.check-is-static]
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

    /// r[verify profile.required-and-optional]
    /// r[verify files.required-roles]
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
    /// r[verify profile.check-is-static]
    /// r[verify files.compatibility-check]
    #[test]
    fn required_gaps_are_reported_first() {
        let venue = Venue {
            groups: vec![],
            focus: vec![],
        };
        let gaps = profile().gaps(&venue);
        assert_eq!(gaps.len(), 4);
        assert!(gaps[0].required && gaps[1].required && gaps[2].required);
        assert!(
            !gaps[3].required,
            "the optional role sorted above a required one"
        );
    }

    /// A show reaching past its profile is not wrong, but it is not
    /// portable — and that has to be visible here rather than discovered
    /// in another city.
    /// r[verify profile.venue-may-exceed]
    /// r[verify profile.declares-vocabulary]
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
    /// r[verify profile.venue-binds]
    /// r[verify profile.resolution-by-role]
    #[test]
    fn a_binding_satisfies_the_profile_using_the_venues_own_names() {
        let mut venue = VenueProfile {
            profile: "test".into(),
            ..Default::default()
        };
        venue
            .groups
            .insert("Key".into(), crate::Selection::Group("Front Wash".into()));
        venue
            .groups
            .insert("Wash".into(), crate::Selection::Group("Washers".into()));
        venue.focus.insert("Vocal".into(), "Vocal Centre".into());
        // Wash and Vocal are bound; Key is bound; nothing else is.
        let gaps = profile().gaps(&venue);
        assert!(gaps.iter().all(|g| g.role != "Key"));
        assert_eq!(
            venue.group("Key"),
            Some(&crate::Selection::Group("Front Wash".into()))
        );
        assert_eq!(venue.focus("Vocal"), Some("Vocal Centre"));
    }

    /// Roles of one kind do not satisfy another. A venue with a *focus*
    /// point called Key does not have a key light.
    /// r[verify files.compatibility-check]
    #[test]
    fn kinds_do_not_cross() {
        let venue = Venue {
            groups: vec!["Wash"],
            focus: vec!["Vocal", "Key"],
        };
        let gaps = profile().gaps(&venue);
        assert!(
            gaps.iter()
                .any(|g| g.role == "Key" && g.kind == RoleKind::Group)
        );
    }

    // ----- the default profile, as shipped -----

    fn default_profile() -> Profile {
        Profile::load(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../data/profiles/ignition.ig-profile"
        ))
        .expect("the shipped default profile loads")
    }

    fn role<'a>(p: &'a Profile, name: &str, kind: RoleKind) -> Option<&'a Role> {
        p.roles.iter().find(|r| r.name == name && r.kind == kind)
    }

    /// r[verify default.key]
    /// r[verify default.wash]
    /// r[verify default.back]
    /// r[verify default.small]
    /// r[verify default.layers-not-positions]
    #[test]
    fn the_default_profile_requires_key_wash_and_back_and_nothing_spatial() {
        let p = default_profile();
        for name in ["Key", "Wash", "Back"] {
            let r = role(&p, name, RoleKind::Group).expect(name);
            assert!(r.required, "{name} is required");
        }
        assert_eq!(p.required(RoleKind::Group), vec!["Key", "Wash", "Back"]);
        for r in p.roles.iter().filter(|r| r.kind == RoleKind::Group) {
            let lower = r.name.to_lowercase();
            for word in ["left", "right", "centre", "center", "upstage", "downstage"] {
                assert!(
                    !lower.contains(word),
                    "{:?} is a position, not a layer",
                    r.name
                );
            }
        }
    }

    /// r[verify default.optional-layers]
    /// r[verify default.optional-is-not-second-class]
    #[test]
    fn the_default_profile_offers_the_optional_layers() {
        let p = default_profile();
        for name in [
            "Movers", "Beams", "Bars", "Floor", "Audience", "Drums", "Spot", "Haze",
        ] {
            let r = role(&p, name, RoleKind::Group).expect(name);
            assert!(!r.required, "{name} is optional");
        }
        // Optional is not second-class: an optional role is a full role
        // with the same `about` a required one gets.
        assert!(p.roles.iter().all(|r| !r.about.is_empty()));
    }

    /// r[verify default.focus-required]
    /// r[verify default.focus-optional]
    /// r[verify profile.areas.portable-question-is-focus]
    #[test]
    fn the_default_profile_requires_vocal_stage_and_audience() {
        let p = default_profile();
        assert_eq!(
            p.required(RoleKind::Focus),
            vec!["Vocal", "Stage", "Audience"]
        );
        for name in ["Band", "Drums", "Back Wall", "House"] {
            let r = role(&p, name, RoleKind::Focus).expect(name);
            assert!(!r.required);
        }
    }

    /// r[verify default.canvas-main]
    /// r[verify default.canvas-optional]
    #[test]
    fn the_default_profile_declares_main_and_the_sides() {
        let p = default_profile();
        assert!(role(&p, "Main", RoleKind::Canvas).is_some());
        for name in ["Side Left", "Side Right"] {
            assert!(!role(&p, name, RoleKind::Canvas).unwrap().required);
        }
        assert!(p.required(RoleKind::Canvas).len() <= 1);
    }

    /// r[verify default.colour-set]
    /// r[verify default.colour-defaults-ship]
    /// r[verify default.colour-roles-are-semantic]
    #[test]
    fn the_default_profile_ships_the_five_colour_roles_with_values() {
        let p = default_profile();
        for name in ["Open", "Warm", "Cool", "Deep", "Hot"] {
            let c = p.colors.iter().find(|c| c.name == name).expect(name);
            let rgb = [c.red, c.green, c.blue];
            assert!(
                rgb.iter().all(|v| (0.0..=1.0).contains(v)),
                "{name} ships a value"
            );
        }
        // Semantic: the job, not the hue — the values are what a venue
        // inherits, and a venue overriding one changes every cue.
        let venue = VenueProfile::default();
        assert!(
            !venue.has_colour("Warm"),
            "a venue inherits rather than binds"
        );
    }

    /// r[verify default.implementation-cost]
    /// r[verify profile.minimal]
    /// r[verify profile.setup-cost-is-the-metric]
    #[test]
    fn implementing_the_default_profile_is_three_groups_three_points_one_canvas() {
        let p = default_profile();
        assert!(p.required(RoleKind::Group).len() <= 3);
        assert!(p.required(RoleKind::Focus).len() <= 3);
        assert!(p.required(RoleKind::Canvas).len() <= 1);
        assert!(p.required(RoleKind::Area).is_empty());
        assert!(p.required(RoleKind::Colour).is_empty());
    }

    /// r[verify profile.roles-are-intent]
    #[test]
    fn the_default_profile_names_jobs_not_equipment() {
        let p = default_profile();
        for r in &p.roles {
            let lower = r.name.to_lowercase();
            for word in [
                "par",
                "led",
                "uking",
                "betopper",
                "truss",
                "chauvet",
                "moving head",
            ] {
                assert!(!lower.contains(word), "{:?} names equipment", r.name);
            }
            assert!(!r.about.is_empty(), "{:?} says what it is for", r.name);
        }
    }

    /// r[verify profile.areas.profile-may-require-one]
    /// r[verify profile.areas]
    #[test]
    fn the_default_profile_declares_no_area() {
        assert!(default_profile().vocabulary(RoleKind::Area).is_empty());
    }

    /// r[verify profile.several-may-exist]
    /// r[verify profile.extensions]
    #[test]
    fn a_second_profile_loads_beside_the_default() {
        let a = default_profile();
        let b = Profile::parse(
            r#"{"name":"church","roles":[{"name":"Pulpit","kind":"Group","required":true}]}"#,
        )
        .unwrap();
        assert_ne!(a.name, b.name);
        assert!(b.required(RoleKind::Group) == vec!["Pulpit"]);
    }
}
