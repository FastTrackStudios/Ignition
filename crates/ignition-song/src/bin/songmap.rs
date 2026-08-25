//! Prints a project's song map — tempo, time signature, sections.
//!
//! ```bash
//! cargo run -p ignition-song --bin songmap -- "path/to/song.RPP"
//! ```
//!
//! A tool rather than a test: the point is to look at a project and see
//! whether its sections land where you think they do, which is the first
//! question when a show does not line up.

use ignition_core::Bars;

fn main() -> anyhow::Result<()> {
    let path = std::env::args()
        .nth(1)
        .ok_or_else(|| anyhow::anyhow!("usage: songmap <project file>"))?;
    let song = ignition_song::load(&path)?;
    let point = song.tempo.at(Bars::START);

    println!(
        "{}  —  {} BPM, {}/{}",
        song.name, point.bpm, point.time_signature.numerator, point.time_signature.denominator
    );
    println!(
        "{:<12} {:>6} {:>7} {:>10}",
        "section", "bar", "bars", "start"
    );

    let mut total = 0.0;
    for section in &song.sections {
        println!(
            "{:<12} {:>6} {:>7} {:>9.2}s",
            section.name,
            section.start.bar,
            section.bars,
            song.tempo.seconds_at(section.start),
        );
        total += section.bars;
    }
    println!("{:<12} {:>6} {:>7}", "total", "", total);
    Ok(())
}
