---
name: ignition-program
description: Edit Ignition's lighting through its programming actions (IGNITION_*) from the CLI, MCP or Luau scripts — the library (colours, palettes, effects, looks, focus points, trick presets), a song's cues and a whole set's, environments (rooms), people, fixtures, the patch, tricks (grandMA3's MAtricks). Use when asked to add, change, move, retime, re-skin, re-patch, hang, focus or script anything in a lighting show, or to write a Luau script that does.
---

# Program lighting with the actions

Do not edit the JSON by hand: every edit has an action, the action checks
what it writes, and what it wrote is one undo step. Load `ignition-domain`
for the vocabulary and `ignition-program-song` for how a song should look.

## Read these two pages first

- `docs/lighting/guides/programming.md`: the model on one page (what each
  thing is, the file it is in, the action that edits it, the data flow from
  a cue file to a lit fixture, the grandMA3 mappings for Phasers and
  MAtricks).
- `docs/lighting/guides/actions.md`: every action with its arguments
  (generated; `IGNITION_ACTIONS` answers the same as data).

## How to run an action

| From | How |
|---|---|
| A shell, no app running | `ignition-program IGNITION_SONG_GET '{"song":"Fire"}'` (`cargo build -p ignition-pane --bin ignition-program`) |
| The app's CLI | `studio actions run IGNITION_SONG_GET --json '{"song":"Fire"}'` (`--headless` for no window) |
| Luau (`SCRIPT_RUN`) | `actions.run("IGNITION_CUE_ADD", {…})`; `state.query("IGNITION_SONG_GET", {…})` for read-only ones |
| MCP | the tool of the same name from `studio mcp` |

`IGNITION_ROOT` is the lighting data root (the folder holding `data/`; in
this repo `features/lighting`). Work on a copy unless asked to change the
repo's data: `cp -r features/lighting/data /tmp/x/` and
`IGNITION_ROOT=/tmp/x`.

## Rules

1. **Query, decide, write.** `IGNITION_SONG_LIST`, `IGNITION_SONG_GET`,
   `IGNITION_LIBRARY_LIST`, `IGNITION_ENV_GET`, `IGNITION_PATCH_GET`,
   `IGNITION_TRICKS_UNITS` return what is there. Read before writing.
2. **One undo step per job.** A script starts with
   `actions.run("IGNITION_EDIT_BEGIN", {label = "…"})` and ends with
   `actions.run("IGNITION_EDIT_COMMIT", {})`. `IGNITION_SET_EACH` and
   `IGNITION_BATCH` are one step by themselves. `IGNITION_UNDO` takes a step
   back.
3. **The app refuses arguments an action does not list.** Do not pass
   `song` or `env` to an action that has none (`IGNITION_EDIT_BEGIN`,
   `IGNITION_ENV_CREATE`).
4. **Name slots, not colours, in cues** (`Primary`, `Secondary`, `Accent`,
   `White`), so a palette re-skins the song. `IGNITION_SONG_SLOTS` converts
   a song that names colours.
5. **Fills are reviewable.** Run `IGNITION_FILL_SECTIONS` and
   `IGNITION_FILL_HITS` with `dry_run = true` first and read the plan.
6. **After any change to a set**, run `IGNITION_SET_CHECK` and fix what it
   lists.
7. **A fixture with no `aim` points straight down.** Give
   `IGNITION_FIXTURE_ADD` an `aim` point for anything that is not a mover.
8. **To see it**, bake and shoot: `IGNITION_SONG_BAKE {song, to}`, then
   `viz --venue data/venues/<env> --cuelist <to> --cue <n> --camera Wide
   --snapshot out.png` (inside the dev shell: it needs the GPU libraries),
   and look at the picture.

## Worked examples

`features/lighting/examples/scripts/` (run with `run.sh <app> <root>`):
`01-library.luau`, `02-program-song.luau`, `03-reskin-set.luau`,
`04-environment.luau`, `05-tricks.luau`. Copy the nearest one.

## Adding an action

Add its entry to the catalogue, `features/lighting/pane/actions.json`
(id, name, about, effect, args, required), handle the id in the `run` of
the module it belongs to (`pane/src/program/{library,song,env,tricks}.rs`),
then `python3 -I features/lighting/tools/program_actions.py` (writes the
manifest's `[[actions]]` and `actions.md`). `cargo test -p ignition-pane
program` fails when the three disagree.
