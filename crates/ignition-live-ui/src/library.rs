//! The Library panel: the whole profile, browsable and searchable, with
//! the operator's favourites first.
//!
//! One panel for both views (`r[studio.panels]`): Program hosts it as a
//! column, Live as a browse sheet behind the favourites. Every kind the
//! profile has is a tab — effects, tricks, bundles, looks, macros,
//! colours, splits, roles — plus the venue's focus points and groups,
//! which are palette entries the profile only names. Nothing here is a
//! reduced copy: the tab lists the profile's own map, and the search
//! box filters it by name, family and the note that says what it is for.

// Nothing is dead here; it is mounted when `main.rs` hosts `live::Views`
// (and its stylesheet, `live::LIVE_CSS`). Until the integrator wires
// that, the crate root does not reach these items. Remove once mounted.

// r[impl studio.views.whole-profile] - every profile map has a tab
// r[impl studio.operators.favourites] - favourites first, one-tap star
// r[impl studio.touch] - tiles are 44px and respond on press

use crate::command::Command;
use crate::operators::{Kind, Operator};
use crate::{Surface, send, use_playhead};
use dioxus::prelude::*;
use ignition_core::Selection;
use ignition_core::profile::LookKind;

/// The profile the panels browse: the shipped file — every role, colour,
/// split, trick, effect note — under `IGNITION_PROFILE` or
/// `data/profiles/ignition.ig-profile`, the same lookup `Playback::load`
/// makes. `faders::profile()` is only the busking programming baked from
/// code (pages, looks, macros); the file is that plus the vocabulary,
/// and the faders tests hold the two equal where they overlap. With no
/// file, the baked one, so the surface still opens.
// r[impl studio.views.whole-profile] - the file profile, whole
pub fn profile() -> &'static ignition_core::Profile {
    if let Some(sent) = SENT.get() {
        return sent;
    }
    if let Some(current) = *CURRENT.read().unwrap_or_else(|e| e.into_inner()) {
        return current;
    }
    let mut slot = CURRENT.write().unwrap_or_else(|e| e.into_inner());
    if let Some(current) = *slot {
        return current;
    }
    let loaded: &'static ignition_core::Profile = Box::leak(Box::new(load_file_profile()));
    *slot = Some(loaded);
    loaded
}

/// The profile as last loaded. `&'static` because every panel borrows
/// it for a render; a reload leaks the previous one rather than
/// invalidating those borrows — a few kilobytes per STORE → LOOK, an
/// operator's action, not a frame's.
static CURRENT: std::sync::RwLock<Option<&'static ignition_core::Profile>> =
    std::sync::RwLock::new(None);

/// The profile file the panels read: `IGNITION_PROFILE` or
/// `data/profiles/ignition.ig-profile`, from the working directory or,
/// failing that, the workspace root (tests run from the crate).
pub fn profile_path() -> std::path::PathBuf {
    let path = std::env::var("IGNITION_PROFILE")
        .unwrap_or_else(|_| "data/profiles/ignition.ig-profile".to_string());
    let candidates = [
        std::path::PathBuf::from(&path),
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join(&path),
    ];
    candidates
        .iter()
        .find(|c| c.exists())
        .cloned()
        .unwrap_or_else(|| candidates[0].clone())
}

/// Where looks authored at the desk are kept — beside the profile file.
// r[impl profile.looks.authored]
pub fn looks_path() -> std::path::PathBuf {
    ignition_core::profile::AuthoredLooks::path_for(profile_path())
}

/// The file profile with the authored looks merged over it; the baked
/// profile (likewise merged) when there is no file.
// r[impl profile.looks.authored] - merged wherever the file is loaded
fn load_file_profile() -> ignition_core::Profile {
    let path = profile_path();
    match ignition_core::Profile::load_with_authored(&path) {
        Ok(profile) => profile,
        Err(error) => {
            tracing::warn!(
                %error,
                path = %path.display(),
                "no profile file; the library shows the baked profile only"
            );
            crate::faders::profile().clone()
        }
    }
}

/// Re-reads the profile and its authored looks, so a look stored a
/// moment ago is on the bank at the next render. Also refreshes the
/// baked profile the widget resolves look keys through.
// r[impl profile.looks.authored] - a stored look shows at once
pub fn reload_authored_looks() {
    crate::faders::reload_authored_looks();
    if SENT.get().is_some() {
        return;
    }
    let fresh: &'static ignition_core::Profile = Box::leak(Box::new(load_file_profile()));
    *CURRENT.write().unwrap_or_else(|e| e.into_inner()) = Some(fresh);
}

/// The profile a host handed over instead of a file — what the browser
/// gets in its bootstrap, since it has no disk to read one from. Wins
/// over the file lookup once set; setting it twice keeps the first.
static SENT: std::sync::OnceLock<ignition_core::Profile> = std::sync::OnceLock::new();

/// Use this profile for the library. Call before the first render.
// r[impl studio.touch.ipad] - the browser lists the studio's profile, not a baked one
pub fn install_profile(profile: ignition_core::Profile) {
    let _ = SENT.set(profile);
}

/// One tile of the library.
#[derive(Debug, Clone, PartialEq)]
pub struct Entry {
    pub tab: Tab,
    pub name: String,
    /// The effect's family, a bundle's family, a look's kind, a trick's
    /// steps — whatever a second line should say.
    pub family: String,
    pub about: String,
    /// A swatch for colours and splits.
    pub css: Option<String>,
    /// The swatch is light, so a name written on it has to be dark.
    pub light: bool,
    /// Small marks for a card's corner: what this does beyond its
    /// colour, as `(glyph, what it means)`. A thumbnail says what a
    /// look *looks* like; these say what it will *do* once it is up,
    /// which a still frame cannot show.
    pub marks: Vec<(String, String)>,
    /// A favourite the profile no longer has. Shown, never acted on.
    pub missing: bool,
    pub favourite: bool,
}

/// The panel's tabs. The first eight are the favourite kinds; splits
/// and roles are profile maps with no favourites set of their own.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Tab {
    Kind(Kind),
    Splits,
    Roles,
}

impl Tab {
    pub const ALL: [Tab; 10] = [
        Tab::Kind(Kind::Effect),
        Tab::Kind(Kind::Trick),
        Tab::Kind(Kind::Bundle),
        Tab::Kind(Kind::Look),
        Tab::Kind(Kind::Macro),
        Tab::Kind(Kind::Colour),
        Tab::Splits,
        Tab::Kind(Kind::Focus),
        Tab::Kind(Kind::Group),
        Tab::Roles,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Tab::Kind(k) => k.label(),
            Tab::Splits => "Splits",
            Tab::Roles => "Roles",
        }
    }

    pub fn kind(self) -> Option<Kind> {
        match self {
            Tab::Kind(k) => Some(k),
            _ => None,
        }
    }
}

/// A colour preset as CSS — the same plain byte scale the swatches use.
pub fn css_of(c: &ignition_core::preset::ColorPreset) -> String {
    format!(
        "rgb({} {} {})",
        (c.red * 255.0) as u8,
        (c.green * 255.0) as u8,
        (c.blue * 255.0) as u8
    )
}

/// Where `viz --previews` writes each kind of loop.
pub const EFFECT_PREVIEW_DIR: &str = "data/effects/previews";
pub const MACRO_PREVIEW_DIR: &str = "data/macros/previews";
pub const LOOK_PREVIEW_DIR: &str = "data/looks/previews";

/// An effect's loop as `file:` URLs, in order.
///
/// Listed from disk at run time so a re-render shows without a rebuild,
/// the same way a look's thumbnail does. An effect nobody has rendered
/// yet returns nothing and its row simply has no picture — a missing
/// preview is not an error, it is a preview that has not been made.
///
/// URLs, not the bytes: see [`crate::panes::file_uri`] for why that is
/// worth a quarter of the studio's CPU.
pub fn effect_frames(name: &str) -> Vec<String> {
    frames_in(EFFECT_PREVIEW_DIR, name)
}

/// The same, for any of the preview directories.
pub fn frames_in(root: &str, name: &str) -> Vec<String> {
    let dir = std::path::Path::new(root).join(slug(name));
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return Vec::new();
    };
    let mut files: Vec<std::path::PathBuf> = entries
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|e| e == "png"))
        .collect();
    // `00.png`, `01.png` … so the names sort into the loop's order.
    files.sort();
    files
        .iter()
        .filter_map(|p| crate::panes::file_uri(p))
        .collect()
}

/// A name as the directory `viz --previews` wrote it to. Kept in step
/// with `ignition_viz::preview::slug` — the two are a file-name
/// contract between the renderer and the pane.
pub fn slug(name: &str) -> String {
    name.chars()
        .map(|c| match c {
            'a'..='z' | 'A'..='Z' | '0'..='9' | '-' => c,
            _ => '_',
        })
        .collect::<String>()
        .to_lowercase()
}

/// What a look does that its thumbnail cannot show.
///
/// A still frame of a chase and a still frame of a static wash are the
/// same picture. The marks are the difference: whether anything in the
/// look is going to move once it is up, and how many recipes are
/// stacked to make it.
fn look_marks(look: &ignition_core::profile::Look) -> Vec<(String, String)> {
    use ignition_core::recipe::RecipeRef;
    // A library effect or a bundle is animated by construction; an
    // inline recipe is a phaser exactly when it has more than one step
    // (`r[recipes.steps-are-the-switch]`).
    let animated = look.recipes.iter().any(|r| match r {
        RecipeRef::Named { .. } | RecipeRef::Bundle { .. } => true,
        RecipeRef::Inline(recipe) => recipe.steps.len() > 1,
        RecipeRef::Look { .. } => false,
    });
    let mut marks = Vec::new();
    if animated {
        marks.push((
            "FX".to_string(),
            "runs an effect — this look moves".to_string(),
        ));
    }
    let n = look.recipes.len();
    if n > 1 {
        marks.push((n.to_string(), format!("{n} recipes stacked")));
    }
    marks
}

/// The colour a look's kind is drawn in — bed, full, punt, safe.
pub fn look_css(kind: LookKind) -> &'static str {
    match kind {
        LookKind::Bed => "#3a5a8a",
        LookKind::Full => "#a04a3a",
        LookKind::Punt => "#c8a050",
        LookKind::Safe => "#2c2c36",
    }
}

/// The beats a macro runs for — the sum of its waits — and whether it
/// lets go at the end.
pub fn macro_shape(m: &ignition_core::profile::Macro) -> (f32, bool) {
    use ignition_core::profile::MacroStep;
    let beats = m
        .steps
        .iter()
        .map(|s| match s {
            MacroStep::Wait { beats } => *beats,
            _ => 0.0,
        })
        .sum();
    let releases = m.steps.iter().any(|s| matches!(s, MacroStep::Release));
    (beats, releases)
}

/// Every entry of one tab, in the profile's order, unstarred.
pub fn catalogue(tab: Tab, surface: &Surface) -> Vec<Entry> {
    let profile = profile();
    let entry = |name: &str, family: String, about: String, css: Option<String>| Entry {
        tab,
        name: name.to_string(),
        family,
        about,
        css,
        light: false,
        marks: Vec::new(),
        missing: false,
        favourite: false,
    };
    match tab {
        Tab::Kind(Kind::Effect) => profile
            .effects
            .keys()
            .map(|name| {
                let note = profile.effect_notes.get(name);
                entry(
                    name,
                    note.map(|n| n.family.clone()).unwrap_or_default(),
                    note.map(|n| n.about.clone()).unwrap_or_default(),
                    None,
                )
            })
            .collect(),
        Tab::Kind(Kind::Trick) => profile
            .tricks
            .iter()
            .map(|(name, steps)| {
                let shape: Vec<String> = steps.iter().map(|t| format!("{t:?}")).collect();
                entry(name, shape.join(" · "), String::new(), None)
            })
            .collect(),
        Tab::Kind(Kind::Bundle) => profile
            .bundles
            .values()
            .map(|b| entry(&b.name, b.family.clone(), b.about.clone(), None))
            .collect(),
        Tab::Kind(Kind::Look) => profile
            .looks
            .iter()
            .map(|(name, look)| Entry {
                marks: look_marks(look),
                ..entry(
                    name,
                    format!("{:?}", look.kind).to_lowercase(),
                    look.about.clone(),
                    Some(look_css(look.kind).to_string()),
                )
            })
            .collect(),
        Tab::Kind(Kind::Macro) => profile
            .macros
            .iter()
            .map(|(name, m)| {
                let (beats, releases) = macro_shape(m);
                let shape = format!("{beats} beats{}", if releases { " · releases" } else { "" });
                entry(name, shape, m.about.clone(), None)
            })
            .collect(),
        // Colours are the whole colour palette: the single gels, then
        // the multi-colour ones. They are one question — "what colour" —
        // and splitting them across two panes only ever hid the half an
        // operator reaches for when a flat wash is not enough.
        //
        // A split keeps `Tab::Splits` as its *own* tab while it sits in
        // this list, so `tap` still sends `Split` rather than `Color`.
        // The tab a tile is listed under and the thing a tile does are
        // not the same fact.
        Tab::Kind(Kind::Colour) => profile
            .colors
            .iter()
            .map(|c| Entry {
                light: crate::ColorChip::is_light(c.red, c.green, c.blue),
                ..entry(&c.name, String::new(), String::new(), Some(css_of(c)))
            })
            .chain(surface.splits.iter().map(|s| Entry {
                tab: Tab::Splits,
                name: s.name.clone(),
                family: String::new(),
                about: String::new(),
                // The tile is a disc, so it wants the wedges.
                css: Some(s.disc()),
                light: s.light,
                marks: Vec::new(),
                missing: false,
                favourite: false,
            }))
            .collect(),
        Tab::Splits => surface
            .splits
            .iter()
            .map(|s| Entry {
                light: s.light,
                ..entry(&s.name, String::new(), String::new(), Some(s.disc()))
            })
            .collect(),
        Tab::Kind(Kind::Focus) => surface
            .focus
            .iter()
            .map(|f| entry(f, String::new(), String::new(), None))
            .collect(),
        Tab::Kind(Kind::Group) => surface
            .groups
            .iter()
            .map(|g| entry(g, String::new(), String::new(), None))
            .collect(),
        Tab::Roles => profile
            .roles
            .iter()
            .map(|r| {
                let mut family = format!("{:?}", r.kind).to_lowercase();
                if r.required {
                    family.push_str(" · required");
                }
                if profile.is_protected(&r.name) {
                    family.push_str(" · protected");
                }
                entry(&r.name, family, r.about.clone(), None)
            })
            .collect(),
    }
}

/// Favourites first, in the operator's order — a stale name as a
/// missing tile — then everything else in the profile's order.
// r[impl studio.operators.favourites] - shown first; missing rather than failing
pub fn ordered(all: &[Entry], favourites: &[String]) -> Vec<Entry> {
    let mut out: Vec<Entry> = favourites
        .iter()
        .map(|name| match all.iter().find(|e| &e.name == name) {
            Some(e) => Entry {
                favourite: true,
                ..e.clone()
            },
            None => Entry {
                tab: all.first().map(|e| e.tab).unwrap_or(Tab::Roles),
                name: name.clone(),
                family: "missing".into(),
                about: "not in this profile".into(),
                css: None,
                light: false,
                marks: Vec::new(),
                missing: true,
                favourite: true,
            },
        })
        .collect();
    out.extend(
        all.iter()
            .filter(|e| !favourites.iter().any(|f| f == &e.name))
            .cloned(),
    );
    out
}

/// Whether a tile matches the search box: name, family or note,
/// case-insensitively; an empty query matches everything.
pub fn matches(entry: &Entry, query: &str) -> bool {
    let q = query.trim().to_lowercase();
    if q.is_empty() {
        return true;
    }
    entry.name.to_lowercase().contains(&q)
        || entry.family.to_lowercase().contains(&q)
        || entry.about.to_lowercase().contains(&q)
}

/// The operator at the desk, shared by every panel in the tree. A
/// panel mounted outside a provider gets its own copy rather than a
/// panic.
pub fn use_operator() -> Signal<Operator> {
    let existing = try_use_context::<Signal<Operator>>();
    let own = use_signal(Operator::current);
    existing.unwrap_or(own)
}

/// Star or unstar `name` for the operator, and keep it.
pub fn toggle_favourite(mut operator: Signal<Operator>, kind: Kind, name: &str) {
    let mut op = operator();
    op.favourites.toggle(kind, name);
    if let Err(error) = op.save() {
        tracing::warn!(%error, "operator file not saved");
    }
    operator.set(op);
}

/// What tapping a tile does — the natural thing for its kind, through
/// the commands the engine already has. Tricks have no command: a trick
/// is applied by a recipe, not fired, so the tile only shows its shape.
pub fn tap(entry: &Entry, playhead: &crate::command::Playhead) {
    if entry.missing {
        return;
    }
    let name = entry.name.clone();
    match entry.tab {
        Tab::Kind(Kind::Effect) | Tab::Kind(Kind::Bundle) => {
            if playhead.effects_playing.iter().any(|e| *e == name) {
                send(Command::Untake(name));
            } else {
                send(Command::Take { name, level: 1.0 });
            }
        }
        Tab::Kind(Kind::Look) => {
            if playhead.held_look.as_deref() == Some(&name) {
                send(Command::Look(None));
            } else {
                send(Command::Look(Some(name)));
            }
        }
        Tab::Kind(Kind::Macro) => send(Command::Macro(name)),
        Tab::Kind(Kind::Colour) => send(Command::Color(name)),
        Tab::Splits => send(Command::Split(name)),
        Tab::Kind(Kind::Focus) => send(Command::Focus(name)),
        Tab::Kind(Kind::Group) => send(Command::Select(Selection::Group(name))),
        Tab::Roles => send(Command::Select(Selection::Role(name))),
        Tab::Kind(Kind::Trick) => {}
    }
}

/// Whether the engine says this tile is active — for the lit state.
pub fn is_on(entry: &Entry, playhead: &crate::command::Playhead) -> bool {
    match entry.tab {
        Tab::Kind(Kind::Effect) | Tab::Kind(Kind::Bundle) => {
            playhead.effects_playing.iter().any(|e| *e == entry.name)
        }
        Tab::Kind(Kind::Look) => playhead.held_look.as_deref() == Some(&entry.name),
        _ => false,
    }
}

/// The Library panel.
#[component]
pub fn Library(surface: Surface, #[props(default = Tab::Kind(Kind::Effect))] open: Tab) -> Element {
    let mut tab = use_signal(|| open);
    let mut query = use_signal(String::new);
    let operator = use_operator();
    let playhead = use_playhead();

    let all = catalogue(tab(), &surface);
    let favourites = tab()
        .kind()
        .map(|k| operator().favourites.of(k).clone())
        .unwrap_or_default();
    let tiles: Vec<Entry> = ordered(&all, &favourites)
        .into_iter()
        .filter(|e| matches(e, &query()))
        .collect();

    rsx! {
        section { class: "library",
            header { class: "lib-head",
                span { class: "lib-title", "Library" }
                input {
                    class: "lib-search",
                    r#type: "text",
                    placeholder: "search name, family, note",
                    value: "{query}",
                    oninput: move |e| query.set(e.value()),
                }
            }
            div { class: "lib-tabs",
                for t in Tab::ALL {
                    button {
                        key: "{t.label()}",
                        class: if tab() == t { "lib-tab on" } else { "lib-tab" },
                        onpointerdown: move |_| tab.set(t),
                        "{t.label()}"
                    }
                }
            }
            div { class: "lib-grid",
                for entry in tiles.iter().cloned() {
                    LibraryTile { key: "{entry.tab.label()}-{entry.name}", entry, operator, playhead }
                }
                if tiles.is_empty() {
                    span { class: "lib-empty", "nothing matches" }
                }
            }
        }
    }
}

/// One touch-sized tile: the name, a second line, a swatch where there
/// is one, and the star.
#[component]
pub fn LibraryTile(
    entry: Entry,
    operator: Signal<Operator>,
    playhead: Signal<crate::command::Playhead>,
) -> Element {
    let on = is_on(&entry, &playhead());
    let class = match (entry.missing, on, entry.favourite) {
        (true, _, _) => "lib-tile missing",
        (_, true, _) => "lib-tile on",
        (_, _, true) => "lib-tile fav",
        _ => "lib-tile",
    };
    let swatch = entry.css.clone();
    let tap_entry = entry.clone();
    let star_entry = entry.clone();
    // r[impl studio.program.pick-and-gizmos] - a hovered group tile is outlined in the room
    let is_group = entry.tab == Tab::Kind(Kind::Group);
    let hover_name = entry.name.clone();
    rsx! {
        div { class: "{class}", title: "{entry.about}",
            onmouseenter: move |_| if is_group { send(Command::HighlightGroup(Some(hover_name.clone()))) },
            onmouseleave: move |_| if is_group { send(Command::HighlightGroup(None)) },
            button {
                class: "lib-body",
                onpointerdown: move |_| tap(&tap_entry, &playhead()),
                if let Some(css) = swatch {
                    span { class: "lib-swatch", style: "background: {css}" }
                }
                span { class: "lib-name", "{entry.name}" }
                if !entry.family.is_empty() {
                    span { class: "lib-family", "{entry.family}" }
                }
            }
            if let Some(kind) = entry.tab.kind() {
                button {
                    class: if entry.favourite { "lib-star on" } else { "lib-star" },
                    onpointerdown: move |_| toggle_favourite(operator, kind, &star_entry.name),
                    if entry.favourite { "★" } else { "☆" }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn e(name: &str, family: &str, about: &str) -> Entry {
        Entry {
            tab: Tab::Kind(Kind::Effect),
            name: name.into(),
            family: family.into(),
            about: about.into(),
            css: None,
            light: false,
            marks: Vec::new(),
            missing: false,
            favourite: false,
        }
    }

    /// The pane finds an effect's frames by the same name the renderer
    /// wrote them under. The two `slug`s live in different crates —
    /// nothing links the pane to the visualizer — so these cases are
    /// the contract, and they are duplicated verbatim in
    /// `ignition_viz::preview::tests`. If one side changes, one of the
    /// two tests goes red.
    #[test]
    fn a_name_finds_the_directory_the_renderer_wrote() {
        assert_eq!(slug("chase"), "chase");
        assert_eq!(slug("bar sparkle"), "bar_sparkle");
        assert_eq!(slug("Audience Sweep"), "audience_sweep");
        assert_eq!(slug("circle/tight"), "circle_tight");
    }

    /// The Colours pane is the whole palette: the flat gels and the
    /// multi-colour ones together, the way the busking tile grid has
    /// always shown them. A split listed here still *is* a split — it
    /// keeps its own tab, so tapping it sends `Split`, not `Color`.
    // r[verify studio.views.whole-profile]
    #[test]
    fn colours_carries_the_multi_colour_palettes_and_they_stay_splits() {
        let surface = Surface {
            splits: vec![crate::ColorChip {
                name: "Warm/Cool".into(),
                css: "linear-gradient(90deg, red, blue)".into(),
                colors: vec!["red".into(), "blue".into()],
                spread: false,
                light: false,
            }],
            ..Default::default()
        };
        let all = catalogue(Tab::Kind(Kind::Colour), &surface);
        let split = all
            .iter()
            .find(|e| e.name == "Warm/Cool")
            .expect("the palette is in the colours pane");
        assert_eq!(
            split.tab,
            Tab::Splits,
            "a split listed under colours still acts like a split"
        );
        assert!(
            split
                .css
                .as_deref()
                .is_some_and(|c| c.starts_with("conic-gradient(")),
            "and draws as wedges, not a flat bar: {:?}",
            split.css
        );
        // The flat gels are still there, ahead of it.
        assert!(
            all.iter().any(|e| e.tab == Tab::Kind(Kind::Colour)),
            "the single colours did not survive the merge"
        );
    }

    /// r[verify studio.operators.favourites]
    #[test]
    fn favourites_come_first_in_their_own_order_and_missing_ones_show() {
        let all = vec![e("a", "", ""), e("b", "", ""), e("c", "", "")];
        let out = ordered(&all, &["c".into(), "zzz".into(), "a".into()]);
        let names: Vec<&str> = out.iter().map(|x| x.name.as_str()).collect();
        assert_eq!(names, vec!["c", "zzz", "a", "b"]);
        assert!(out[1].missing);
        assert!(out[0].favourite && out[2].favourite && !out[3].favourite);
    }

    #[test]
    fn search_reads_name_family_and_note() {
        let x = e("pan wave", "movement", "the beams grazing slowly");
        assert!(matches(&x, ""));
        assert!(matches(&x, "PAN"));
        assert!(matches(&x, "move"));
        assert!(matches(&x, "grazing"));
        assert!(!matches(&x, "strobe"));
    }

    /// Every profile map reaches a tab, at the profile's own size.
    /// r[verify studio.views.whole-profile]
    #[test]
    fn every_tab_lists_the_whole_profile() {
        let profile = profile();
        let surface = Surface {
            groups: vec!["All".into()],
            colors: Vec::new(),
            splits: Vec::new(),
            focus: vec!["Centre".into()],
            cues: Vec::new(),
        };
        assert_eq!(
            catalogue(Tab::Kind(Kind::Effect), &surface).len(),
            profile.effects.len()
        );
        assert_eq!(
            catalogue(Tab::Kind(Kind::Trick), &surface).len(),
            profile.tricks.len()
        );
        assert_eq!(
            catalogue(Tab::Kind(Kind::Bundle), &surface).len(),
            profile.bundles.len()
        );
        assert_eq!(
            catalogue(Tab::Kind(Kind::Look), &surface).len(),
            profile.looks.len()
        );
        assert_eq!(
            catalogue(Tab::Kind(Kind::Macro), &surface).len(),
            profile.macros.len()
        );
        assert_eq!(
            catalogue(Tab::Kind(Kind::Colour), &surface).len(),
            profile.colors.len()
        );
        assert_eq!(catalogue(Tab::Roles, &surface).len(), profile.roles.len());
        assert_eq!(catalogue(Tab::Kind(Kind::Focus), &surface).len(), 1);
        assert_eq!(catalogue(Tab::Kind(Kind::Group), &surface).len(), 1);
        // Effects carry their family and note for the search.
        let effects = catalogue(Tab::Kind(Kind::Effect), &surface);
        assert!(
            effects.iter().all(|x| !x.family.is_empty()),
            "an effect has no family"
        );
        // The protected role says so on its tile.
        let roles = catalogue(Tab::Roles, &surface);
        let house = roles
            .iter()
            .find(|r| r.name == "House Lights")
            .expect("house");
        assert!(house.family.contains("protected"));
    }

    #[test]
    fn a_macro_shows_its_beats_and_release() {
        let profile = profile();
        let (beats, releases) = macro_shape(&profile.macros["build 8"]);
        assert_eq!(beats, 32.0);
        assert!(releases);
    }
}
