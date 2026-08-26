//! Charts a song's hits onto its own grid, and writes them out.
//!
//! ```text
//! cargo run -p ignition-song --bin hits -- <project.RPP> <audio> <out.json>
//! ```
//!
//! The project supplies the tempo map, so the grid the hits land on is
//! the same grid the cues are written against — not a tempo guessed
//! from the audio, which would put the whole chart a few milliseconds
//! out and drifting.

use anyhow::{Result, bail};

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (project, audio, out) = match args.as_slice() {
        [p, a, o] => (p, a, o),
        _ => bail!("usage: hits <project.RPP> <audio> <out.json>"),
    };

    let song = ignition_song::load(project)?;
    let hits = ignition_song::hits::detect(audio, &song, 2)?;

    let total = hits.hits.len();
    let count = |band| hits.hits.iter().filter(|h| h.band == band).count();
    use ignition_song::hits::HitBand::{High, Low, Mid};
    println!(
        "{total} hits on 1/8 · low {} · mid {} · high {}",
        count(Low),
        count(Mid),
        count(High)
    );

    // How well the detector agreed with the grid. A large average snap
    // means the tempo map and the recording disagree, and every hit
    // below is then a guess dressed up as a position — worth seeing
    // before trusting any of it.
    let drift: f32 = hits
        .hits
        .iter()
        .map(|h| (h.secs - song.tempo.seconds_at(h.at) as f32).abs())
        .sum::<f32>()
        / total.max(1) as f32;
    println!("mean snap distance {:.0} ms\n", drift * 1000.0);

    println!("accents (strength > 0.5):");
    for hit in hits.hits.iter().filter(|h| h.strength > 0.5) {
        let section = song
            .section_at(hit.at)
            .map(|s| s.name.as_str())
            .unwrap_or("—");
        println!(
            "  bar {:>3}.{:<4.2}  {:<5} {:.2}  dyn {:.2}  {section}",
            hit.at.bar,
            hit.at.beat,
            format!("{:?}", hit.band),
            hit.strength,
            hit.dynamics
        );
    }

    std::fs::write(out, format!("{}\n", serde_json::to_string_pretty(&hits)?))?;
    println!("\nwrote {out}");
    Ok(())
}
