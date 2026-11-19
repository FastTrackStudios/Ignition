//! One wgpu for the renderer and the app.
//!
//! The room's frames reach the Architect app on the GPU because the
//! renderer (Bevy) and the app are on one wgpu: `apps/ignition` exports
//! each frame from Bevy's own device (`src/frames.rs`). Cargo builds two
//! majors of wgpu side by side without a word, and with two on the frame
//! path the frames would go back through the CPU. So `Cargo.lock` is
//! read here, and this fails when:
//!
//! - Bevy's renderer and Architect's process views are locked to
//!   different wgpus (Architect's is the reference: the root manifest's
//!   `wgpu` entry and `docs/lighting/research/bevy-pin.md` say how the
//!   two move together), or
//! - a second major of wgpu, wgpu-core, wgpu-hal, wgpu-types or naga is
//!   in the lockfile for any crate but the ones in [`OFF_THE_FRAME_PATH`].
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::collections::{BTreeMap, BTreeSet};

/// The crates that are one wgpu: a second major of any is a second wgpu.
const WGPU: [&str; 5] = ["wgpu", "wgpu-core", "wgpu-hal", "wgpu-types", "naga"];

/// Crates that may bring a wgpu of their own, because no frame of the
/// room passes through them. Each needs its reason; the list is meant to
/// become empty.
///
/// - `pixels`: Architect's `architect-workspace-hack` takes
///   `dioxus-native`'s CPU renderer (`anyrender_vello_cpu` →
///   `pixels_window_renderer` → `pixels` 0.17), which is on wgpu 29. It
///   is in Architect's own lockfile for the same reason, and goes when
///   Architect drops or bumps it.
const OFF_THE_FRAME_PATH: [&str; 1] = ["pixels"];

/// A lockfile's package: its name, version and dependencies as written
/// (`name`, or `name version` where the name alone is ambiguous).
struct Package<'a> {
    name: &'a str,
    version: &'a str,
    dependencies: Vec<&'a str>,
}

fn packages(lock: &str) -> Vec<Package<'_>> {
    lock.split("[[package]]")
        .skip(1)
        .filter_map(|entry| {
            let field = |key: &str| {
                entry
                    .lines()
                    .find_map(|line| line.strip_prefix(key))
                    .map(|value| value.trim_matches('"'))
            };
            let dependencies = entry
                .lines()
                .filter_map(|line| line.strip_prefix(" \""))
                .filter_map(|line| line.strip_suffix("\","))
                .collect();
            Some(Package {
                name: field("name = ")?,
                version: field("version = ")?,
                dependencies,
            })
        })
        .collect()
}

fn major(version: &str) -> &str {
    version.split('.').next().unwrap_or(version)
}

/// The versions of `dependency` that `package` is locked to. A
/// dependency is written with its version only when the lockfile holds
/// more than one of that name.
fn locked<'a>(all: &'a [Package<'a>], package: &str, dependency: &str) -> BTreeSet<&'a str> {
    let versions: Vec<&str> = all
        .iter()
        .filter(|p| p.name == dependency)
        .map(|p| p.version)
        .collect();
    all.iter()
        .filter(|p| p.name == package)
        .flat_map(|p| p.dependencies.iter())
        .filter_map(|written| {
            let mut words = written.split(' ');
            (words.next() == Some(dependency)).then(|| words.next())
        })
        .filter_map(|version| version.or_else(|| versions.first().copied()))
        .collect()
}

/// For each of `WGPU`'s crates locked at more than one major: the
/// majors, and who uses each.
fn split_majors<'a>(all: &'a [Package<'a>]) -> BTreeMap<&'a str, BTreeMap<&'a str, Vec<&'a str>>> {
    let mut found = BTreeMap::new();
    for name in WGPU {
        let mut majors: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
        for version in all.iter().filter(|p| p.name == name).map(|p| p.version) {
            let users = all
                .iter()
                .filter(|p| locked(all, p.name, name).contains(version))
                .map(|p| p.name)
                .collect();
            majors.insert(major(version), users);
        }
        if majors.len() > 1 {
            found.insert(name, majors);
        }
    }
    found
}

fn lockfile() -> String {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../Cargo.lock");
    std::fs::read_to_string(path).expect("the workspace's Cargo.lock")
}

#[test]
fn the_renderer_and_the_app_are_locked_to_one_wgpu() {
    let lock = lockfile();
    let all = packages(&lock);
    let renderer = locked(&all, "bevy_render", "wgpu");
    let app = locked(&all, "architect-process-view", "wgpu");
    assert_eq!(renderer.len(), 1, "bevy_render's wgpu: {renderer:?}");
    assert_eq!(
        renderer, app,
        "Bevy's renderer and Architect's process views are on different wgpus, so the room's \
         frames cannot be shared on the GPU. Architect's is the reference: move the Bevy pin \
         (root Cargo.toml) only to a revision on its major."
    );
}

#[test]
fn no_second_wgpu_major_is_on_the_frame_path() {
    let lock = lockfile();
    let all = packages(&lock);
    let reference = locked(&all, "architect-process-view", "wgpu");
    let reference = major(reference.first().expect("architect-process-view's wgpu"));
    let mut strays = Vec::new();
    for (name, majors) in split_majors(&all) {
        for (found, users) in majors {
            // The wgpu crates of a major use each other: only what brings
            // the major in from outside is a stray.
            let outside: Vec<_> = users
                .into_iter()
                .filter(|user| !WGPU.contains(user) && !user.starts_with("wgpu-"))
                .filter(|user| !OFF_THE_FRAME_PATH.contains(user))
                .collect();
            if found != reference && !outside.is_empty() {
                strays.push(format!("{name} {found} (used by {outside:?})"));
            }
        }
    }
    assert!(
        strays.is_empty(),
        "Cargo.lock holds a wgpu major other than Architect's ({reference}) outside the crates \
         known to be off the frame path {OFF_THE_FRAME_PATH:?}: {strays:?}"
    );
}

#[test]
fn a_second_major_and_who_brought_it_are_found() {
    let lock = r#"
[[package]]
name = "bevy_render"
version = "0.19.1"
dependencies = [
 "wgpu 29.0.4",
]

[[package]]
name = "architect-process-view"
version = "0.14.0"
dependencies = [
 "wgpu 30.0.1",
]

[[package]]
name = "wgpu"
version = "29.0.4"

[[package]]
name = "wgpu"
version = "30.0.1"

[[package]]
name = "wgpu-hal"
version = "30.0.1"
"#;
    let all = packages(lock);
    assert_eq!(
        locked(&all, "bevy_render", "wgpu"),
        BTreeSet::from(["29.0.4"])
    );
    assert_eq!(
        locked(&all, "architect-process-view", "wgpu"),
        BTreeSet::from(["30.0.1"])
    );
    let split = split_majors(&all);
    assert_eq!(split["wgpu"]["29"], ["bevy_render"]);
    assert_eq!(split["wgpu"]["30"], ["architect-process-view"]);
    assert!(!split.contains_key("wgpu-hal"), "one major of the hal");
}

#[test]
fn a_dependency_written_without_its_version_is_the_only_one() {
    let lock = "[[package]]\nname = \"bevy_render\"\nversion = \"0.20.0\"\ndependencies = [\n \
                \"wgpu\",\n]\n\n[[package]]\nname = \"wgpu\"\nversion = \"30.0.1\"\n";
    let all = packages(lock);
    assert_eq!(
        locked(&all, "bevy_render", "wgpu"),
        BTreeSet::from(["30.0.1"])
    );
}
