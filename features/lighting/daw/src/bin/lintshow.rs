//! A song's show held to the cue design guide, whoever wrote it.
//!
//! ```text
//! lintshow <set.als> <song> <song.json> [--profile <ignition.ig-profile>] [--grid <out.json>]
//! ```
//!
//! `authorshow --lint` judges the list its own generator makes. A show
//! written through the programming actions (`IGNITION_CUE_*`,
//! `IGNITION_FILL_*`) is a file with no generator, and this is the same
//! judgement for it: the song's map comes from the Live set the night is
//! played from, so its sections and tempo are the ones the cues resolve
//! against. Prints the energy curve and every finding by rule number, and
//! exits non-zero when there are any.
//!
//! `--grid` writes where every bar of the song starts, in seconds from the
//! song's first bar, through every tempo change: what a script that reads
//! the song's audio needs to put a hit on the bar it is in.

use anyhow::{Context as _, Result, bail};
use ignition_daw::Bars;
use ignition_daw::ableton::LiveSet;
use ignition_show::CueList;

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut plain = Vec::new();
    let (mut profile, mut grid) = (None, None);
    let mut it = args.into_iter();
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--profile" => profile = it.next(),
            "--grid" => grid = it.next(),
            _ => plain.push(arg),
        }
    }
    let [set, name, file] = plain.as_slice() else {
        bail!("usage: lintshow <set.als> <song> <song.json> [--profile <p>] [--grid <out.json>]")
    };
    let live = LiveSet::read(set)?;
    let song = live
        .song(name)
        .with_context(|| format!("no song {name:?} in the set"))?;
    let map = &song.song;

    if let Some(out) = grid {
        let bars: u32 = map
            .sections
            .iter()
            .map(|s| s.end(&map.tempo).bar)
            .max()
            .unwrap_or(1);
        let starts: Vec<f64> = (1..=bars.saturating_add(1))
            .map(|bar| map.tempo.seconds_at(Bars::new(bar, 1.0)))
            .collect();
        let sections: Vec<serde_json::Value> = map
            .sections
            .iter()
            .map(|s| serde_json::json!({ "name": s.name, "bar": s.start.bar, "end": s.end(&map.tempo).bar }))
            .collect();
        std::fs::write(
            &out,
            serde_json::to_string(&serde_json::json!({ "song": map.name, "bar_starts": starts, "sections": sections }))?,
        )?;
        eprintln!("grid: {} bars to {out}", starts.len().saturating_sub(1));
    }

    let raw = std::fs::read_to_string(file).with_context(|| format!("reading {file}"))?;
    let list: CueList = serde_json::from_str(&raw).with_context(|| format!("{file} as a cue list"))?;
    let profile = profile.unwrap_or_else(|| {
        concat!(env!("CARGO_MANIFEST_DIR"), "/../data/profiles/ignition.ig-profile").to_owned()
    });
    let splits: Vec<ignition_colour::preset::ColorSplit> = std::fs::read_to_string(&profile)
        .ok()
        .and_then(|raw| serde_json::from_str::<serde_json::Value>(&raw).ok())
        .and_then(|v| v.get("splits").cloned())
        .and_then(|v| serde_json::from_value(v).ok())
        .unwrap_or_default();

    for (cue, energy, level, layers) in ignition_daw::lint::energy_curve(&list, map, &splits) {
        println!("{cue:<18} energy {energy:.2}  level {level:.2}  layers {layers}");
    }
    let findings = ignition_daw::lint::lint(&list, map, &splits);
    for f in &findings {
        println!("{f}");
    }
    println!("{} finding(s) against the design guide", findings.len());
    if !findings.is_empty() {
        std::process::exit(1);
    }
    Ok(())
}
