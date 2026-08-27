# Studio

The studio is the operator's surface over the engine: the place a show is
programmed, the place a night is busked, and the place the room's screens
and cameras are run from. This spec fixes its shape — modes, views,
windows, operators — so the surfaces can be rebuilt without re-deciding
what they are. Everything the studio shows is a view over the profile,
the venue, the show and the playback state; the studio owns no lighting
data of its own.

## Modes and views

r[studio.modes]
The studio has exactly three modes, selected from one persistent mode
strip: **Lights**, **Graphics** and **Video**. A mode is a domain, not a
window: Lights is fixtures, cues, effects and busking; Graphics is the
canvases — lyrics, CG, clips, procedural content — mapped onto the room's
screens; Video is live camera feeds and their switching. The three share
one transport, one song clock and one show file, so a cue can carry
lighting, a canvas source and a camera cut together.

r[studio.modes.lights-first]
Lights mode MUST be complete before Graphics and Video are more than
their transport and a placeholder; the other two are shaped here so the
Lights surfaces are built with them in mind, not so they ship together.

r[studio.views]
Every mode has two views, **Program** and **Live**, switchable at any
time without losing state. Program is for building — the programmer,
selection, recipes, cue editing, the visualizer as the truth of what the
cue does. Live is for running a night — big touch targets, no
destructive editing, everything reachable in one tap. Switching views
never changes what the rig is outputting.

r[studio.views.whole-profile]
Both views expose the *entire* profile: every role, colour and split
palette, focus point, group, effect, trick, bundle, look, macro, page and
fader spec, the protected roles, the speed routing and the effect
parameters. Nothing in the profile is Program-only; Live reaches it
through favourites first (`r[studio.operators.favourites]`) and a
browse/search sheet second, never through a reduced copy.

r[studio.views.seven-busking-features]
The seven busking features of the profile are first-class controls in
Live, not settings:

- looks: a scene bank, coloured by kind (bed / full / punt / safe), with
  the held look shown as held;
- macros: buttons that show their beat length and whether they release;
- pages: the fader bank's page selector, built from `Profile.pages`
  (`r[profile.pages]`), with pickup state per fader;
- attribute filters: a fader whose spec carries a filter shows it as a
  badge, and the filter can be changed from the fader's detail sheet;
- protected roles: shown as a state on the roles they protect, with the
  protection toggle on the Live surface (it is an operator decision, not
  a preference);
- speed routing: every fader shows which clock it follows (Song, Tap,
  Tap ½, Tap ×2) and the tap masters are on the Live surface;
- effect parameters: a fader with parameters shows its secondary sliders
  (`r[profile.effect-parameters]`) inline, touch-sized.

r[studio.live.desk-scenes]
A venue that came with a console show — Riverside's myDMX 5 scenes in
`data/shows/riverside-desk.json`, grouped by their desk banks — surfaces
those scenes in Live as a **Desk** bank alongside the profile's looks,
bank by bank, so a night at that room can be run the way its old desk ran
it while the profile's busking is layered on top. Desk scenes are cues in
a playback, subject to the same stack as everything else
(`r[playback.busking-over-show]`).

r[studio.program.cue-editing]
Program view edits cues in place: select fixtures, set attributes through
the programmer, store to a cue or a look, and see the edit on the
visualizer and in the cue list without leaving the view. The Program
view's cue list and the Live view's cue list are the same panel with
different chrome.

r[studio.program.pick-and-gizmos]
The Program view's visualizer is part of the programmer. Clicking a
fixture in it selects that fixture (shift adds to the selection, ctrl
toggles it), hovering one tints it and names it, and the selection made
there is the same selection every other surface sees — it travels as the
one `Select` command and comes back on the playhead. Over the room the
view draws the venue's focus points, the selected fixtures' beam axes,
the outline of whichever group the Library is hovering, and — when asked
— the DMX address above every fixture; each overlay switches on its own
from a FOCUS / BEAMS / GROUPS / LABELS row, and all of them are off in
Live. Picking and drawing use the engine's own facilities (`bevy_picking`
with the mesh backend, `Gizmos`), not a raycast or a line renderer of the
studio's own.

## Panels and windows

r[studio.panels]
The studio is composed of panels, each of which can live in any window:
Cue List, Visualizer, Transport, Busking (faders + keys + pages), Palettes
(colours, splits, focus, groups), Library (effects, tricks, bundles,
looks, macros — browse and search), Programmer, Command Line, Output
(DMX status), Canvases (Graphics), Cameras (Video), and Lyrics preview
(Graphics). A panel has one implementation regardless of which window or
view hosts it.

r[studio.windows.multiple]
The studio MUST be able to run more than one OS window in one process,
each hosting any set of panels, on any monitor. A panel can be popped out
of its window into a new window and docked back. All windows share the
same engine state — the command channel and the playhead watch are
process-wide — so a fader moved in one window moves in all of them.

r[studio.windows.implementation]
Multi-window is delivered by patching `dioxus-native` rather than forking
Blitz: `blitz-shell` already keeps a `HashMap<WindowId, View>` and
exposes `add_window`; what is missing is a runtime event that reaches it.
The patch adds a `NewWindow { attributes, root }` embedder event and a
`use_open_window()` hook, one `VirtualDom` per window, in a workspace
`[patch]` on the pinned dioxus revision. If the patch cannot land cleanly
on that revision, the fallback is one borderless window per monitor
launched from the same process, which still satisfies
`r[studio.windows.multiple]`.

r[studio.windows.visualizer-anywhere]
The visualizer panel can be hosted by any window. The embedded Bevy
renderer shares the process's one wgpu device with Blitz, so moving the
panel re-registers its target texture with the new window's renderer
rather than creating a second device.

r[studio.windows.wayland]
On Wayland the studio cannot place its own windows. Fullscreen-on-monitor
is honoured directly (`set_fullscreen(Borderless(monitor))`); a
non-fullscreen window's position is expressed as a monitor plus a docked
region (left / right / centre, with a fraction of the monitor's width),
and the studio sets its size to that region and asks the compositor to
place it via the window's app-id and title so a KWin window rule can pin
it. The layout file records the intent; the compositor decides the
pixels.

## Operators

r[studio.operators]
An **operator** is a person running the studio. Operator files live in
`data/operators/<name>.ig-user` and hold only preferences: window layout,
favourites, default mode and view, and remote mapping choice. They never
hold lighting data — a favourite is a reference into the profile by name,
and an operator file with a stale name shows the entry as missing rather
than failing.

r[studio.operators.favourites]
An operator's favourites are per-kind shortcut sets over the profile —
effects, looks, macros, tricks, bundles, colours, focus points, groups —
shown first in Live and in Program's library panel. Two operators on the
same profile see the same library and different shortcuts. Adding to and
removing from favourites is a one-tap action on any library entry, and
favourites can be ordered.

r[studio.operators.layout]
An operator's layout names, for each window: the monitor (by output name,
with position and `left`/`centre`/`right` fallbacks), whether it is
fullscreen or docked to a region, and the panels it hosts with their
sizes. The studio restores the layout on launch for the selected operator
(`IGNITION_OPERATOR` or the last used) and offers "save layout" from the
mode strip. The first shipped operator, `cody`, is: Cue List fullscreen on
the left portrait monitor; Visualizer docked to the right half of the
centre ultrawide, not fullscreen; Live view fullscreen on the right
monitor.

## Touch and remote

r[studio.touch]
Live view is designed for touch first: targets no smaller than 44 px at
the iPad's logical size, no hover-only affordances, sliders with a wide
grab area, and momentary keys that respond on press rather than release.
The desktop Live view is the same component at a larger size.

r[studio.touch.ipad]
The Live view is served to an iPad as a web page from an HTTP server the
studio runs, using the same Dioxus components compiled for the web,
connected to the engine over a WebSocket that carries the same `Command`
and `Playhead` types the desktop uses. No separate touch UI is authored.
TouchOSC over OSC (`r[playback.remote-feedback]`) remains supported for
operators who prefer it.

r[studio.touch.presence]
More than one Live client (desktop windows, iPads) can be connected at
once. Every client shows the same state; a control moved on one shows
moved on the others within one playhead tick.

## Graphics and Video

r[studio.graphics]
Graphics mode shows the room's canvases as the visualizer's screens see
them plus a source panel per canvas: a clip, a procedural source, a
lyrics layer over either (`r[canvas.clip-is-a-source]`), and a still. The
canvas clock is the transport's
(`CanvasClock::at(transport.position())`), so scrubbing the song scrubs
the screens.

r[studio.graphics.lyrics]
The lyrics layer renders `.lrc` lines from the song at musical time, with
the current line emphasised and the next line previewed, on any canvas
that has the layer enabled. The Program view's Lyrics panel shows the
line list with timing and lets an operator nudge a line.

r[studio.video]
Video mode lists live camera sources (a `VideoSource` that ignores the
supplied clock), previews them, and cuts or dissolves the programme
output between them; the programme can be routed to a canvas so a screen
shows a camera. Camera input is greenfield and is scoped here only so
the mode strip, transport and canvas routing are built to receive it.

## Constraints carried over

r[studio.labels]
Fader and key labels follow `r[profile.pages.label-fits]`: eight
characters or fewer, so a label reads on a physical strip, a desktop
fader and a touch key alike.

r[studio.one-truth]
The studio never caches lighting state of its own. What a control shows
comes from the playhead watch; what a control does goes down the command
channel. A panel that needs a value the playhead does not carry adds it to
`Playhead` rather than computing it locally.
