# The programming actions

Generated from `features/lighting/pane/actions.json` by
`features/lighting/tools/program_actions.py`; edit the catalogue, not this page.
How they fit together: [programming.md](programming.md).

Every action also takes `root` (the lighting data root, default `IGNITION_ROOT`).
**bold** arguments are required.

## History and batching

| Action | Effect | What it does | Arguments |
|---|---|---|---|
| `IGNITION_ACTIONS` | read only | Every programming action with its arguments, as data (this catalogue). |  |
| `IGNITION_HISTORY` | read only | The steps of the edit history (data/.history) and which is current. | limit |
| `IGNITION_UNDO` | mutating | Puts every file the last step wrote back as it was. |  |
| `IGNITION_REDO` | mutating | Applies the step that was undone. |  |
| `IGNITION_EDIT_BEGIN` | mutating | Opens an edit: every programming action until IGNITION_EDIT_COMMIT is one history step (one undo). A Luau script opens one first. | label |
| `IGNITION_EDIT_COMMIT` | mutating | Closes the open edit as one history step. |  |
| `IGNITION_EDIT_ABORT` | mutating | Closes the open edit and puts every file it wrote back. |  |
| `IGNITION_BATCH` | mutating | Runs `steps` ([{action, args}]) in order as one history step; if one fails, nothing of the batch is kept. | **steps**, label |

## Library

| Action | Effect | What it does | Arguments |
|---|---|---|---|
| `IGNITION_LIBRARY_LIST` | read only | What is in the library, by kind, each entry with the layer it comes from (song, set or profile), its tags and folder, and a one-line summary; and the palette in use with its slots as RGB. | show, song, kind, tag, folder, family, search, layer |
| `IGNITION_LIBRARY_GET` | read only | One entry in full, the layer it comes from and the layers it overrides. | show, song, **kind**, **name** |
| `IGNITION_LIBRARY_CREATE` | mutating | Adds an entry. Shapes: colors {red,green,blue}; palettes {slots:{Primary: colour name or {red,green,blue}}}; splits {colors:[names], distribute: cycle\|spread\|block}; effects {recipe, family}; looks {recipes:[parts], kind}; focus {target:{x,y,z}}; beams {values:[[attribute, value]]}; tricks (see IGNITION_TRICKS_UNITS). Every entry takes about, tags, folder. | show, song, **kind**, **name**, **entry**, scope, replace |
| `IGNITION_LIBRARY_UPDATE` | mutating | Merges `set` into an entry (a null removes a key). An entry that is only in a lower layer is copied up into the layer written: that is how a song overrides the set, and the set the profile. | show, song, **kind**, **name**, **set**, scope |
| `IGNITION_LIBRARY_DUPLICATE` | mutating | Copies an entry (from any layer, the profile's included) under another name, with `set` merged in. | show, song, **kind**, **name**, **as**, set, scope |
| `IGNITION_LIBRARY_DELETE` | destructive | Removes an entry from the set's (or the song's) layer; says which lower layer's entry of that name now shows through. The profile's entries are read-only. | show, song, **kind**, **name**, scope |
| `IGNITION_LIBRARY_USES` | read only | The songs and cues of the show that name an entry. | show, **name** |
| `IGNITION_LIBRARY_TAGS` | read only | Every tag and folder in use. | show, song |
| `IGNITION_PALETTE_USE` | mutating | Names the palette in use: the set's, or with `song` that song's (an empty `palette` gives a song back the set's). Its slots become the colours cues name (Primary, Secondary…): this is the re-skin. | show, song, **palette** |

## Effects and tricks

| Action | Effect | What it does | Arguments |
|---|---|---|---|
| `IGNITION_EFFECT_MAKE` | mutating | Writes an effect from parameters and saves it to the library: `what` moves, in a `shape`, once every `beats`, `depth` either side of where it is, spread across the target by `tricks` or `spread_deg`. | show, song, **name**, **target**, what, shape, beats, depth, base, colors, spread_deg, direction, tricks, speed, once, family, about, tags, folder, scope, replace, dry_run |
| `IGNITION_EFFECT_PREVIEW` | read only | Runs an effect (`effect` by name, a `recipe`, or IGNITION_EFFECT_MAKE's parameters) on a target of an environment and answers what every fixture does at each sample of the loop. Saves nothing. | show, song, env, effect, recipe, target, tricks, bpm, beats_shown, samples, what, shape, beats, depth, spread_deg |
| `IGNITION_TRICKS_UNITS` | read only | Cuts a target into units with tricks and answers each unit's fixtures, its place on the selection grid (x along the selection, y across, z up, read from where the fixtures hang, or a preset's hand-made grid), and its phase, rate, delay and fade under the preset's spreads. With no `tricks`: the grid itself. `step` names one unit (wrapping): next and previous. | show, song, env, **target**, tricks, step |
| `IGNITION_TRICKS_APPLY` | mutating | Puts a trick preset on a cue's effect parts (those of `target`, or the one named `effect`): its tricks, phase, speed and direction go on the part, its delay and fade become the cue's fan. A part that names a library effect becomes that effect written in place. | show, **song**, **cue**, target, effect, **tricks**, statics |

## A song

| Action | Effect | What it does | Arguments |
|---|---|---|---|
| `IGNITION_SONG_LIST` | read only | Every song of the show: title, set, file, counts of cues, triggers, sections and markers, and its palette. | show |
| `IGNITION_SONG_GET` | read only | A song: its sections (with the cue that opens each), its markers by kind, every cue with its position and parts, its palette's slots and what it overrides. | show, **song**, markers |
| `IGNITION_SONG_CREATE` | mutating | Adds a song to the show with an empty cue list (and the show file, when there is none yet). | show, env, **title**, locator, set, sections, file |
| `IGNITION_SONG_MAP_SET` | mutating | Sets the song's section map ([{name, bars, bar?}]) and adds markers ([{name?, kind, bar, beat, strength?}]: kick, snare, a stop), then resolves every cue's bar again. | show, **song**, sections, markers, replace_markers |
| `IGNITION_SONG_MAP_IMPORT` | mutating | Reads every song's sections from the Live set (its locator's range) into the song files and resolves the cues again. | show, live_set, song |
| `IGNITION_SONG_SLOTS` | mutating | Makes the colours a song's cues name into slots (White, Primary, Secondary, Tertiary, Accent…, by use), adds a palette of the song's name holding them to the set's library, and points the cues at the slots. The look is unchanged; another palette now re-skins it. | show, **song** |
| `IGNITION_SONG_BAKE` | mutating | Writes a copy of a song's cue list with nothing left to look up in the library: slot and library colours as values, the library's effects and looks in place, its focus points as points. For a player that does not read the library (the viz tool, a session lane, a copy sent elsewhere). The song's file is not changed. | show, **song**, **to** |
| `IGNITION_CUE_GET` | read only | One cue as the file has it. | show, **song**, **cue** |
| `IGNITION_CUE_ADD` | mutating | Adds a cue at a position, with parts. | show, **song**, **name**, **at**, parts, fade_secs, block, note, set |
| `IGNITION_CUE_UPDATE` | mutating | Changes a cue: `at` moves it, `set` merges keys (name, fade_secs, block, note, timing, fan…), `parts` puts parts on it, `remove` ({target?, effect?, look?}) takes parts off. | show, **song**, **cue**, at, set, parts, remove |
| `IGNITION_CUE_DUPLICATE` | mutating | Copies a cue to another position. | show, **song**, **cue**, **at**, name |
| `IGNITION_CUE_DELETE` | destructive | Removes a cue. | show, **song**, **cue** |
| `IGNITION_SECTION_APPLY` | mutating | Puts parts on the cue that opens each named section (made if the section has none): a palette slot or colour, a level, an effect, a look. | show, **song**, **section**, **parts** |
| `IGNITION_SECTION_COPY` | mutating | Copies the look of a section's cue (its parts, fade and timing) onto other sections, of the same song or of `to_song`. | show, **song**, **from**, **to**, to_song |
| `IGNITION_FILL_SECTIONS` | mutating | Fills from the section map: `looks` maps a section's kind (intro, verse, pre, chorus, bridge, solo, break, outro, other, or *) or its name to the parts it gets. Answers the plan section by section; `dry_run` writes nothing. | show, **song**, **looks**, replace, block, dry_run |
| `IGNITION_FILL_HITS` | mutating | Adds a trigger (a one-shot bump on `target`) at every marker of a `kind`, or every hit of a `hits` file (a band, above a strength). Answers what it would add; `dry_run` writes nothing. | show, **song**, **target**, kind, hits, band, min_strength, section, bump, depth, name, replace, dry_run |
| `IGNITION_TRIGGER_LIST` | read only | The song's triggers: name, position and target. | show, **song**, name |
| `IGNITION_TRIGGER_CLEAR` | destructive | Removes the song's triggers (those whose name starts with `name`). | show, **song**, name |

## A set

| Action | Effect | What it does | Arguments |
|---|---|---|---|
| `IGNITION_SET_EACH` | mutating | Runs one action for every lit song of the show (or `songs`), `args.song` set to each, as one history step. | show, **action**, args, songs, unlit |
| `IGNITION_SET_CHECK` | read only | What the set's songs refer to that is not there: colours, slots, palettes, effects, looks, bundles, splits, tricks, focus points, roles (and roles the room binds nothing to), groups, sections. | show |

## Environments

| Action | Effect | What it does | Arguments |
|---|---|---|---|
| `IGNITION_ENV_LIST` | read only | Every environment under data/venues: fixtures, universes, people, and which the show uses. | show |
| `IGNITION_ENV_GET` | read only | One environment: its room (floor, walls, audience), props (truss, risers, people), screens, people, groups, role bindings, focus points and where its universes go; and, for a measured room (environment.json), its dimensions, every object of its model (kind, position, turn, shown, movable) and its own light groups. | show, env |
| `IGNITION_ENV_CREATE` | mutating | A room from its dimensions (metres): a floor and four walls, and when asked a stage riser, an audience area and trusses. Empty of fixtures; seven focus points and the Wide and Top cameras come with it. | **name**, title, **width**, **depth**, height, upstage, stage, audience, truss |
| `IGNITION_ENV_DUPLICATE` | mutating | Copies an environment's files under another name, as a starting point. | show, env, **as**, title |
| `IGNITION_ENV_USE` | mutating | Points the show at another environment. | show, **env** |
| `IGNITION_ENV_PIECE_SET` | mutating | Adds or changes a piece by name: a wall, the floor, the audience area (`in: room`), a truss, a riser or any prop (`in: props`), a screen (`in: screens`). Position is its centre on the floor, size {x across, y deep, z tall}, eulers degrees. `in: environment` moves, turns (eulers.z), shows or hides an object of a measured room's model (`visible`, `move`); the architecture moves only with `force`. | show, env, **name**, in, visible, move, force, position, size, eulers, kind, set |
| `IGNITION_ENV_PIECE_REMOVE` | destructive | Removes a piece by name. | show, env, **name**, in |
| `IGNITION_ENV_LIGHTS` | mutating | A measured room's own lights (the house lights, not the rig): its groups, how many lights each has and whether each starts on. With `group` and `on`, switches one. | show, env, group, on |
| `IGNITION_ENV_LOOK` | mutating | The air and the camera of an environment: `haze` (0 a clear room, 1 normally hazed; a room with no hazer is near 0, and then beams do not show in the air) `exposure` (stops on the stage exposure; 0 as calibrated) and `output` (what the rig delivers of its nominal flux, 1 as labelled: budget LED fixtures deliver less, and a lux reading sets it). The renderer and the stills read them. With neither, answers what is set. | show, env, haze, exposure, output |

## People and focus

| Action | Effect | What it does | Arguments |
|---|---|---|---|
| `IGNITION_PERSON_SET` | mutating | Places a performer or stand-in (or several, `people`): position on the floor, facing, height, a label such as the instrument; `move` shifts by {x, y, z}. Each is a prop the visualizer draws and a focus point at head height under their name. | show, env, name, label, position, facing_deg, height, stand_in, move, people |
| `IGNITION_PERSON_REMOVE` | destructive | Removes a person, their prop and their focus point. | show, env, **name** |
| `IGNITION_PEOPLE_LIST` | read only | The people, each with their head's point; with `song` (and `section`) as that song's blocking has them. | show, env, song, section |
| `IGNITION_BLOCKING_SET` | mutating | Saves where people stand for a song (`section` *, the default) or one section: {name: {x, y, z?, facing_deg?}}. `show: true` also moves the room's people there. | show, env, **song**, section, positions |
| `IGNITION_FOCUS_ON` | mutating | Aims fixtures at a person (as the song or section has them standing), a focus point or a point: answers the pan and tilt each needs from where it hangs. With `song` and a `section` or `cue`, puts that focus on the cue's part for the target. | show, env, **target**, person, focus, point, song, section, cue, dimmer |
| `IGNITION_FOCUS_SET` | mutating | Adds or moves a named point of the room (a position palette entry every show there shares); `remove: true` deletes it. | show, env, **name**, target, remove |

## Fixtures, groups and roles

| Action | Effect | What it does | Arguments |
|---|---|---|---|
| `IGNITION_FIXTURE_TYPES` | read only | The fixture types (data/fixtures): each type's modes with their channel counts, beam angle and size. | search |
| `IGNITION_FIXTURE_LIST` | read only | The environment's fixtures (or a target's, in its order): channel, name, type, mode, position, rotation, tags, groups and address. | show, env, target |
| `IGNITION_FIXTURE_ADD` | mutating | Hangs `count` fixtures of a type from `position`, `step` apart, on the next free channels and addresses of `universe`, into `groups`. | show, env, **type**, mode, name, count, **position**, step, aim, eulers, quat, tags, groups, universe, address, chan |
| `IGNITION_FIXTURE_SET` | mutating | Changes fixtures (`chan`, `chans` or a `target`): `position` places, `move` shifts, `aim` (a point the beam points at, at rest), `eulers` or `quat` re-hangs, `mode` and `type` change what they are on the wire, `name`, `label`, `tags`. Answers the patch problems the change leaves. | show, env, chan, chans, target, position, move, aim, eulers, quat, mode, type, name, label, tags |
| `IGNITION_FIXTURE_REMOVE` | destructive | Takes fixtures out of the environment and out of its groups. | show, env, chan, chans, target |
| `IGNITION_GROUP_LIST` | read only | The environment's groups with their channels, and which selection each role of the profile is bound to. | show, env |
| `IGNITION_GROUP_SET` | mutating | Sets a group's fixtures (`chans` or a `target`); `add: true` adds to it, `delete: true` removes the group. | show, env, **name**, chans, target, add, delete |
| `IGNITION_ROLE_BIND` | mutating | Says which fixtures of this room play a role of the profile: `to` a group or a selection (null unbinds); `focus` binds a focus role to a focus point. | show, env, **role**, to, focus |

## The patch

| Action | Effect | What it does | Arguments |
|---|---|---|---|
| `IGNITION_PATCH_GET` | read only | The whole patch: every fixture's universe, address, footprint and last address; each universe's use, free space and output; and every conflict (overlap, out of range, unaddressed, a universe with no output). | show, env |
| `IGNITION_PATCH_SET` | mutating | Moves fixtures on the wire: one (`chan`, `universe`, `address`) or several (`moves`). Refused whole, with the overlaps, when it would conflict, unless `force`. `patched: false` parks a fixture. | show, env, chan, universe, address, patched, moves, force |
| `IGNITION_PATCH_PACK` | mutating | Addresses fixtures one after another from `address` in `universe`, in the target's order. | show, env, target, chans, universe, address, force |
| `IGNITION_PATCH_ROUTE` | mutating | Where a universe goes: `sacn` ({priority, universe?}), `artnet` ({net, subnet, universe}), `enabled`; a null drops a protocol, `remove: true` the route. | show, env, **universe**, sacn, artnet, enabled, max_hz, remove |

