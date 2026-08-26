# KWin window rules for the studio's docked windows

Under Wayland a client cannot place its own window: `with_position` and
`set_outer_position` are ignored, and the compositor puts a new window
wherever its placement policy says. The studio therefore does the half it
can (`r[studio.windows.wayland]`):

* a **fullscreen** window asks for `Fullscreen::Borderless(<monitor>)`
  directly, which Wayland honours — no rule needed;
* a **docked** window sets its own size to the region (full monitor
  height, `fraction` of its width), sets its title to the panels it
  hosts, and carries the app id `ignition-studio`. Where it lands is
  KWin's decision, and a window rule is how to make that decision
  permanent.

The layout file records the intent (`data/operators/<name>.ig-user`,
`windows[].placement`); the rule turns it into pixels.

## The rule for Cody's layout

Window 2 of `data/operators/cody.ig-user` is

```json
{ "monitor": "DP-4",
  "placement": { "docked": { "region": "right", "fraction": 0.5 } },
  "panels": ["visualizer", "transport"] }
```

DP-4 is the 5120×1440 ultrawide at (1440, 0); the right half is 2560 wide
starting at x = 1440 + 2560 = 4000. The window's title is
`Ignition Studio — Visualizer, Transport` and its app id is
`ignition-studio`.

System Settings → Window Management → Window Rules → Add New…

| Field | Value |
|---|---|
| Description | Ignition Studio — visualizer, right half of DP-4 |
| Window class (application) | `ignition-studio` — *Exact match* |
| Match whole window class | no |
| Window title | `Ignition Studio — Visualizer, Transport` — *Exact match* |
| Window types | Normal Window |

Then under *Size & Position*:

| Property | Setting | Value |
|---|---|---|
| Position | Apply initially (or Force) | `4000, 0` |
| Size | Apply initially (or Force) | `2560 x 1440` |
| Screen | Force | the screen KWin numbers DP-4 as (check *Display Configuration*; on this desk it is the primary) |
| Fullscreen | Force | No |
| No titlebar and frame | Force | Yes (optional — the studio draws its own strip) |

Use *Apply initially* so the window can still be moved by hand; *Force*
to pin it.

The same rule as `kwinrulesrc` (`~/.config/kwinrulesrc`), which is what
the dialog writes and what a dotfiles repo would carry:

```ini
[Ignition Studio viz]
Description=Ignition Studio — visualizer, right half of DP-4
wmclass=ignition-studio
wmclassmatch=1
title=Ignition Studio — Visualizer, Transport
titlematch=1
types=1
position=4000,0
positionrule=3
size=2560,1440
sizerule=3
fullscreen=false
fullscreenrule=2
```

(`*match=1` is *Exact*; `*rule=3` is *Apply initially*, `2` is *Force*.)
Reload with `qdbus org.kde.KWin /KWin reconfigure`.

## Every docked window

The title is deterministic in the panels — `Ignition Studio — <Panel>,
<Panel>` in layout order — so one rule per docked window, keyed on title,
is enough. A popped-out panel (the POP OUT key on any panel bar) opens a
window titled `Ignition Studio — <that panel>`, docked to the centre
half of the monitor it left; give it a rule the same way if it should
always land somewhere in particular.

## What is not done in code

* The position: KWin's. The studio still calls `set_outer_position`,
  which X11 honours, so the same layout works there without a rule.
* The screen: `Fullscreen::Borderless(monitor)` names it for fullscreen
  windows; a docked window's size is computed from the named monitor's
  mode, but which output KWin opens it on is its own placement policy
  until the rule says otherwise.
* KWin does not expose "monitor by connector name" in a rule; the rule
  uses the screen index or an absolute position in the global layout.
