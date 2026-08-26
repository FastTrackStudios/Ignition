//! `igcheck` — is tonight's set playable in this room, answered before
//! doors and without running anything.
//!
//! ```text
//! igcheck data/shows/sunday.ig-show
//! igcheck --venue data/venues/riverside --profile data/profiles/ignition.ig-profile
//! ```
//!
//! The first form checks a night: the venue against the profile, every
//! song against the profile, and which songs carry a venue layer. The
//! second is the venue half alone — a new room, verified before a show
//! is opened in it. Everything printed is a warning except a *required*
//! role the venue leaves unbound, which is the only thing that exits
//! non-zero: a show missing a follow spot plays; a show missing its key
//! light is not a show.

// r[impl profile.show-check-before-doors] - one report, before anything runs
// r[impl profile.check-is-static] - loads files and prints; nothing is rendered
// r[impl files.compatibility-check] - the two independent halves, as a command
// r[impl files.graceful-degradation] - findings are printed, not fatal; only a required gap fails

use ignition_core::Profile;
use ignition_core::show_file::{
    ShowFile, check_areas, check_ig_show, check_venue_against_profile, load_venue_binding,
};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

fn usage() -> ExitCode {
    eprintln!("usage: igcheck <show.ig-show>");
    eprintln!("       igcheck --venue <venue dir> --profile <profile.ig-profile>");
    ExitCode::from(2)
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut venue: Option<PathBuf> = None;
    let mut profile: Option<PathBuf> = None;
    let mut show: Option<PathBuf> = None;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--venue" => {
                i += 1;
                venue = args.get(i).map(PathBuf::from);
            }
            "--profile" => {
                i += 1;
                profile = args.get(i).map(PathBuf::from);
            }
            "-h" | "--help" => return usage(),
            other => show = Some(PathBuf::from(other)),
        }
        i += 1;
    }

    match (show, venue, profile) {
        (Some(show), None, None) => check_show(&show),
        (None, Some(venue), Some(profile)) => check_venue(&venue, &profile),
        _ => usage(),
    }
}

fn check_show(path: &Path) -> ExitCode {
    let show = match ShowFile::load(path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::from(2);
        }
    };
    let dir = path.parent().unwrap_or(Path::new("."));
    let report = check_ig_show(&show, dir);
    println!("{report}");
    if report.ok() {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

fn check_venue(venue_dir: &Path, profile_path: &Path) -> ExitCode {
    let profile = match Profile::load(profile_path) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::from(2);
        }
    };
    let binding = match load_venue_binding(venue_dir) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::from(2);
        }
    };
    println!("venue   {}", venue_dir.display());
    println!("profile {} ({})", profile_path.display(), profile.name);
    if !binding.profile.is_empty() && !binding.profile.eq_ignore_ascii_case(&profile.name) {
        println!(
            "  note: the venue says it implements {:?}, not {:?}",
            binding.profile, profile.name
        );
    }
    let gaps = check_venue_against_profile(&binding, &profile);
    if gaps.is_empty() {
        println!("every role bound");
    } else {
        println!("{} unbound:", gaps.len());
        for g in &gaps {
            println!("  {g}");
        }
    }
    for f in check_areas(binding.areas.keys().map(String::as_str)) {
        println!("  {f}");
    }
    let required = gaps.iter().filter(|g| g.required).count();
    if required == 0 {
        println!("ok: nothing required is missing");
        ExitCode::SUCCESS
    } else {
        println!("FAIL: {required} required role(s) unbound");
        ExitCode::FAILURE
    }
}
