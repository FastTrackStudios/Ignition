//! Charts a song's hits onto its own grid, and writes them out.
//!
//! ```text
//! cargo run -p ignition-daw --bin hits -- <project.RPP> <audio> <out.json>
//! ```
//!
//! The project supplies the tempo map, so the grid the hits land on is
//! the same grid the cues are written against — not a tempo guessed
//! from the audio, which would put the whole chart a few milliseconds
//! out and drifting.

use anyhow::{Result, bail};

/// A tempo-derived second count, narrowed to match a detected hit's
/// own `f32` seconds. A song is minutes long, far below where an
/// `f32`'s mantissa would start losing precision that matters here.
#[expect(
    clippy::as_conversions,
    clippy::cast_possible_truncation,
    reason = "a song's duration in seconds is far below f32's precision limit"
)]
const fn narrow_secs(value: f64) -> f32 {
    value as f32
}

fn main() -> Result<()> {
    use ignition_daw::hits::HitBand::{High, Low, Mid};

    let args: Vec<String> = std::env::args().skip(1).collect();
    let [project, audio, out] = args.as_slice() else {
        bail!("usage: hits <project.RPP> <audio> <out.json>")
    };

    let song = ignition_daw::load(project)?;
    let hits = ignition_daw::hits::detect(audio, &song, 2)?;

    let total = hits.hits.len();
    let count = |band| hits.hits.iter().filter(|h| h.band == band).count();
    println!(
        "{total} hits on 1/8 · low {} · mid {} · high {}",
        count(Low),
        count(Mid),
        count(High)
    );

    // How well the detector agreed with the grid. A large average snap
    // means the tempo map and the recording disagree, and every hit
    // below is then a guess dressed up as a position — worth seeing
    // before trusting any of it. `total` is the number of hits in this
    // run, always far below where a `usize`-to-`f32` count loses
    // precision that matters.
    #[expect(
        clippy::as_conversions,
        clippy::cast_precision_loss,
        reason = "a hit count for one song is far below f32's precision limit"
    )]
    let total_f32 = total.max(1) as f32;
    let drift: f32 = hits
        .hits
        .iter()
        .map(|h| (h.secs - narrow_secs(song.tempo.seconds_at(h.at))).abs())
        .sum::<f32>()
        / total_f32;
    println!("mean snap distance {:.0} ms\n", drift * 1000.0);

    println!("accents (strength > 0.5):");
    for hit in hits.hits.iter().filter(|h| h.strength > 0.5) {
        let section = song.section_at(hit.at).map_or("—", |s| s.name.as_str());
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
