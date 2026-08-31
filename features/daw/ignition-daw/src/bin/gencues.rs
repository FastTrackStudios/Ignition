//! Generates a starting cue list from a project's song map.
//!
//! ```bash
//! cargo run -p ignition-daw --bin gencues -- song.RPP > show.json
//! ```
//!
//! The output is a draft to edit and busk over, not a finished show —
//! it knows the shape of the song and nothing about the song.

fn main() -> anyhow::Result<()> {
    let path = std::env::args()
        .nth(1)
        .ok_or_else(|| anyhow::anyhow!("usage: gencues <project file>"))?;
    let song = ignition_daw::load(&path)?;
    let list = ignition_daw::generate(&song, &ignition_daw::Roles::default());
    println!("{}", serde_json::to_string_pretty(&list)?);
    Ok(())
}
