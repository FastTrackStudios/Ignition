//! Prints an `.lrc` against a song's own timeline.
//!
//! The check this exists for is alignment against the *project*, not
//! against the recording: a lyric that lands on bar 25 beat 1 is a lyric
//! the lights can be cued off. One that lands on bar 24 beat 4.9 means
//! the LRC and the project disagree about where the song starts, and
//! that is worth seeing before building a lyric screen on top of it.
//!
//! ```text
//! cargo run -p ignition-daw --bin lyrics -- \
//!     "/home/cody/Downloads/Bye Bye Bye/Bye Bye Bye.RPP" data/songs/bye-bye-bye.lrc
//! ```

use anyhow::{Result, bail};

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [project, lrc] = args.as_slice() else {
        bail!("usage: lyrics <project.RPP> <lyrics.lrc>");
    };

    let song = ignition_daw::load(project)?;
    let lyrics = ignition_daw::lyrics::load(lrc, &song)?;

    if let (Some(title), Some(artist)) = (&lyrics.title, &lyrics.artist) {
        println!("{artist} — {title}");
    }
    println!("{} lines against {}\n", lyrics.lines.len(), song.name);

    for line in &lyrics.lines {
        let section = song.section_at(line.at).map_or("—", |s| s.name.as_str());
        let text = if line.text.is_empty() {
            "·".to_string()
        } else {
            line.text.clone()
        };
        println!(
            "{:>7.2}s  bar {:>3}.{:<5.2}  {:<12}  {}",
            line.secs, line.at.bar, line.at.beat, section, text
        );
    }
    Ok(())
}
