# Ignition specification

Normative requirements, one file per topic. Each requirement is a paragraph
headed `r[<topic>.<id>]`; code cites them as `// r[impl <id>]` and tests as
`/// r[verify <id>]` (Tracey, `.config/tracey/config.styx`).
`python3 tools/spec_coverage.py` reports coverage without the daemon.

Read in this order — each file assumes the ones above it.

| File | What it decides |
|---|---|
| [profile.md](profile.md) | The four files and the contract between them: **Profile** (interface), **Venue** (implementation), **Ignition** (a song's show), **Show** (the night). Venue layers. Areas. |
| [files.md](files.md) | What each file may and may not contain. No fixture identity in a show. Graceful degradation. |
| [default-profile.md](default-profile.md) | The profile Ignition ships: `Key`/`Wash`/`Back` required; `Movers`/`Bars`/`Floor`/… optional; focus roles `Vocal`/`Stage`/`Audience`; colour roles `Open`/`Warm`/`Cool`/`Deep`/`Hot`; canvases. |
| [groups.md](groups.md) | Selections are ordered and resolve live. Group masters. One ordering authority. |
| [tricks.md](tricks.md) | Addressing part of a selection — `Block`, `Group`, `Wings`, `Shuffle`, `Shift`, `Mirror` — and spreading a value across one. |
| [color.md](color.md) | Colour as intent, not emitter levels. Preset scope: universal / global / selective. Multi-colour distribution. Recall by reference. |
| [focus.md](focus.md) | Point vs orientation. Stage space. Patterns: fan, splay, per-fixture. Resolved at output against the live hang. |
| [recipes.md](recipes.md) | A recipe is a template cooked against the live rig. The cascade. Absolute vs relative layers. One-shots. Size and rate. |
| [effects.md](effects.md) | A recipe with two or more steps. Step width/transition/ease; speed, measure, phase, play mode; speed masters; waveforms as sugar; the shipped library; bumps. |
| [cues.md](cues.md) | Cue shape, tracking, block, fades, musical position, seek, replay. |
| [playback.md](playback.md) | **The priority stack.** Hand › masters › flashes › faders › triggers › cue player. Transient over sustained. HTP/LTP. Nothing merges at DMX. |
| [triggers.md](triggers.md) | Hits the song fires. Crossing fires; stopped fires nothing; seek locates. Sum, retire, bound. |
| [canvas.md](canvas.md) | Canvases as fixture grids: procedural content, bitmap channels driving any attribute, clips as one source among many. |
| [dmx.md](dmx.md) | The wire: sACN and Art-Net transmit, rate and keep-alive, priority, sequence, venue-owned network config, loopback into the visualizer. |
| [visualizer.md](visualizer.md) | Rendering from DMX bytes, video export, gobo raster; GDTF: real meshes (`viz.gdtf-meshes`), generated profiles (`viz.gdtf-generated`), library loading and name/alias matching (`viz.gdtf-aliases`). |
| [song.md](song.md) | The song level: bars not seconds, the song map from the DAW, the hit chart (pulses, hits, figures, zones), generation, transport. |

Design history and prior art live in [`../domain/`](../domain/) and
[`../research/`](../research/); the specs cite them rather than repeat them.
