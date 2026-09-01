//! Plays a project and prints where the lights are, without a window.
//!
//! ```bash
//! cargo run -p ignition-daw --features play --bin playsong -- song.RPP
//! ```
//!
//! The point is to check the *link* — that the audio plays, that its
//! playhead converts to the right bar, and that the cue list resolves to
//! the section you can hear — before any of it is behind a renderer. If
//! this prints the chorus while the chorus is playing, the rest is
//! drawing.

use ignition_daw::SongTransport;
use ignition_daw_proto::Bars;
use ignition_rig::Rig;
use ignition_show::{CuePlayer, Show, SpeedMasters};
use std::time::Duration;

fn main() -> anyhow::Result<()> {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .try_init();

    let mut args = std::env::args().skip(1);
    let project = args
        .next()
        .ok_or_else(|| anyhow::anyhow!("usage: playsong <project> [cues.json] [start-bar]"))?;
    let cues_path = args.next();
    let start_bar: Option<u32> = args.next().and_then(|v| v.parse().ok());

    // The backend's service layer spawns tasks, so a runtime has to be
    // current for the whole life of the transport — not just while it
    // loads. Held to the end of `main` for that reason.
    let runtime = tokio::runtime::Runtime::new()?;
    let _guard = runtime.enter();

    let transport = SongTransport::open(&project)?;
    let song = transport.song().clone();

    // The cue list, if one was given. Generated on the fly otherwise, so
    // there is always something to watch.
    let list = match &cues_path {
        Some(path) => {
            let raw = std::fs::read_to_string(path)?;
            serde_json::from_str::<ignition_show::CueList>(&raw)?
        }
        None => ignition_daw::generate(&song, &ignition_daw::Roles::default()),
    };
    let mut player = CuePlayer::new(list.cues);

    // No venue here, so groups and palettes resolve to nothing — the
    // cue *positions* are what this is checking, not their content.
    let rig = Rig::default();
    let speeds = SpeedMasters::new();
    let show = Show::new(&[], &rig);
    let _ = &speeds;

    if let Some(bar) = start_bar {
        transport.locate(Bars::bar(bar));
    }
    transport.play();

    let mut last = String::new();
    loop {
        let seconds = transport.seconds();
        let position = transport.position();
        player.seek(position, &show);

        let section = song
            .section_at(position)
            .map(|s| s.name.as_str())
            .unwrap_or("—");
        let cue = player.current_name().unwrap_or("—");
        let line = format!("{section} / {cue}");
        if line != last {
            println!(
                "{:>7.2}s  bar {:>3}.{:<4.2}  {section:<12} cue: {cue}",
                seconds, position.bar, position.beat
            );
            last = line;
        }

        if !transport.is_playing() {
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    println!("stopped at bar {}", transport.position().bar);
    Ok(())
}
