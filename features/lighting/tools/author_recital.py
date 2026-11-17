#!/usr/bin/env python3
"""JT's Senior Recital — the lighting show, generated from the Live set.

The recital is one Ableton Live set with every song laid end to end,
marked the AbleSet way: a locator per song, `SONG END` where it stops,
section cards (named MIDI clips) on the `SECTIONS +CC` track, FoH notes
on `FoH Notes +LYRICS`. This reads the set and writes the show in
Ignition's own formats, beside the other shows:

    data/songs/jt-recital/<slug>.json      one cue list per lit song
    data/shows/jt-recital.ig-show          the night: sets, running order

Each cue sits at a section of its song (`at`: "4 bars into Chorus 2")
and carries the bar it resolves to in this arrangement (`resolved`), so
the show plays as written and still follows the set if a section moves.
Run it again after the set changes; edit the DESIGN table, not the JSON.

    python3 -I features/lighting/tools/author_recital.py <path/to/set.als>

The set is read with the standard library only (gzip + ElementTree).
"""

from __future__ import annotations

import gzip
import json
import re
import sys
import xml.etree.ElementTree as ET
from pathlib import Path

HERE = Path(__file__).resolve().parent
DATA = HERE.parent / "data"

# ---------------------------------------------------------------------------
# The night: sets in running order. A song is named by its locator in the
# Live set. `lit: False` keeps a song in the setlist but writes no cues.
# ---------------------------------------------------------------------------

SETS = [
    ("Act I: The Originals", [
        ("Cinematic Intro", "DTLA Cinematic Intro"),
        ("Drive To LA", "Drive To LA"),
        ("For You", "For You"),
        ("Heart isn't Ready", "Heart Isn't Ready"),
        ("No One Knows", "No One Knows"),
        ("Hey!", "Hey!"),
    ], True),
    ("Act II: Royal Blood", [
        ("Sleeptalker", "Sleeptalker"),
        ("Figure It Out", "Figure It Out"),
        ("7 Nation Army", "Seven Nation Army"),
        ("Sad But True", "Sad But True"),
        ("Out Of The Black", "Out Of The Black"),
    ], True),
    ("Act III: The Fun Part", [
        ("She's American", "She's American"),
        ("WORST PART OF YOU", "WORST PART OF YOU"),
        ("Fire", "Fire"),
        ("Not Too Much Longer", "Not Too Much Longer (Live)"),
        ("Know Me", "Know Me (Extended)"),
    ], True),
    ("Archive", [
        ("Better Strangers", "Better Strangers"),
        ("Berenstein", "Berenstein"),
    ], False),
]

# ---------------------------------------------------------------------------
# The design. Per song: the colours its looks are built from, and its feel.
#   base    the wash in verses          accent  the back wall / second colour
#   hot     the wash in a chorus        key     the faces (a white)
#   chorus  the bundle a chorus runs    energy  0..1, how hard the rig works
# Colour names are the profile's (data/profiles/ignition.ig-profile).
# ---------------------------------------------------------------------------

DESIGN = {
    # Pads, "from our movie", band hits, a cut, the band in — the trailer
    # before Drive To LA. Cinematic: deep and slow, then the reveal.
    "Cinematic Intro": dict(base="Deep Blue", accent="Indigo", hot="Amber", key="Cool White",
                            chorus="chorus lift", energy=0.6),
    # LA at dusk: sunset amber and magenta, a driving chorus.
    "Drive To LA": dict(base="Amber", accent="Magenta", hot="Hot", key="Warm White",
                        chorus="chorus drive", energy=0.8),
    # A love song: warm pinks, lavender behind, a chorus that lifts.
    "For You": dict(base="Lavender", accent="Pink", hot="Pink", key="Warm White",
                    chorus="chorus lift", energy=0.55),
    # Heartbreak: deep red and congo, the solo in white.
    "Heart isn't Ready": dict(base="Deep Red", accent="Congo", hot="Red", key="Warm White",
                              chorus="chorus lift", energy=0.65),
    # Bright and bouncy: gold and cyan, a rainbow on the double chorus.
    "Hey!": dict(base="Gold", accent="Cyan", hot="Straw", key="Open White",
                 chorus="chorus drive", energy=0.85),
    # Red, white and blue.
    "She's American": dict(base="Blue", accent="Red", hot="Open White", key="Open White",
                           chorus="chorus drive", energy=0.8),
    # Angsty: purple and deep red, cold faces.
    "WORST PART OF YOU": dict(base="Purple", accent="Deep Red", hot="Magenta", key="Cool White",
                              chorus="chorus drive", energy=0.8),
    # Fire.
    "Fire": dict(base="Amber", accent="Deep Red", hot="Red", key="Warm White",
                 chorus="chorus drive", energy=0.9, flicker=True),
    # Acoustic guitar and voice to start, a big third chorus.
    "Not Too Much Longer": dict(base="Straw", accent="Deep Amber", hot="Gold", key="Warm White",
                                chorus="chorus lift", energy=0.6),
    # Cool blues, a keytar solo, JT talking over the outro.
    "Know Me": dict(base="Turquoise", accent="Blue", hot="Cyan", key="Open White",
                    chorus="chorus drive", energy=0.8),
    # QOTSA: stark red and white, hard stops.
    "No One Knows": dict(base="Deep Red", accent="Red", hot="Open White", key="Cool White",
                         chorus="chorus drive", energy=0.95),
    # Royal Blood: two-piece riff rock — red, white, a lot of black.
    "Figure It Out": dict(base="Red", accent="Deep Red", hot="Open White", key="Cool White",
                          chorus="chorus drive", energy=0.95),
    # The White Stripes' colours.
    "7 Nation Army": dict(base="Red", accent="Deep Red", hot="Open White", key="Open White",
                          chorus="chorus lift", energy=0.9),
    "Better Strangers": dict(base="Deep Amber", accent="Red", hot="Amber", key="Warm White",
                             chorus="chorus drive", energy=0.9),
    # Metallica: heavy, cold, slow.
    "Sad But True": dict(base="Deep Blue", accent="Congo", hot="Cool White", key="Cool White",
                         chorus="chorus lift", energy=0.85),
    "Out Of The Black": dict(base="Deep Red", accent="Magenta", hot="Red", key="Cool White",
                             chorus="chorus drive", energy=1.0),
    "Sleeptalker": dict(base="Purple", accent="Indigo", hot="Lavender", key="Cool White",
                        chorus="chorus lift", energy=0.75),
}

# ---------------------------------------------------------------------------
# Reading the set
# ---------------------------------------------------------------------------


def text(el, path, default=""):
    node = el.find(path)
    return node.get("Value") if node is not None else default


def clips_of(track):
    out = []
    for c in track.iter("MidiClip"):
        if text(c, "Disabled") == "true":
            continue
        start, end = float(text(c, "CurrentStart", "0")), float(text(c, "CurrentEnd", "0"))
        name = text(c, "Name").strip()
        if name and end > start:
            out.append((start, end, name))
    return sorted(out)


def read_set(path: Path):
    root = ET.fromstring(gzip.open(path).read())
    live = root.find("LiveSet")
    locators = sorted(
        (float(text(l, "Time")), text(l, "Name").strip()) for l in live.iter("Locator")
    )
    tracks = {}
    for tr in live.find("Tracks"):
        tracks[text(tr, "Name/EffectiveName").strip()] = tr
    cards = []
    notes = []
    for name, tr in tracks.items():
        upper = name.upper()
        if upper.startswith("SECTION"):
            cards += clips_of(tr)
        elif upper.startswith("FOH NOTES"):
            notes += clips_of(tr)
    # The main track's time-signature automation: Live writes a signature
    # as one number, (numerator - 1) + 99 * log2(denominator).
    main = live.find("MainTrack")
    if main is None:
        main = live.find("MasterTrack")
    ts_target = None
    for t in main.iter("TimeSignature"):
        at = t.find("AutomationTarget")
        if at is not None:
            ts_target = at.get("Id")
            break
    meters = []
    for env in main.iter("AutomationEnvelope"):
        if text(env, "EnvelopeTarget/PointeeId") == ts_target:
            for ev in env.iter("EnumEvent"):
                v = int(float(ev.get("Value")))
                meters.append((max(float(ev.get("Time")), 0.0), v % 99 + 1, 2 ** (v // 99)))
    meters.sort()
    if not meters or meters[0][0] > 0:
        meters.insert(0, (0.0, 4, 4))
    return locators, sorted(cards), sorted(notes), meters


def song_ranges(locators):
    """Song name -> (start, end) in beats: locator to `SONG END`."""
    out = {}
    open_song = None
    for time, name in locators:
        upper = name.upper()
        if upper in ("SONG END", "END", "STOP", "SONG STOP"):
            if open_song:
                out[open_song[0]] = (open_song[1], time)
                open_song = None
        elif name.startswith(">"):
            continue
        else:
            if open_song:
                out[open_song[0]] = (open_song[1], time)
            open_song = (name, time)
    return out


class Meter:
    """Beats from a song's start -> bars and beats, through the set's
    signatures. A song starts on bar 1; a signature change starts a bar."""

    def __init__(self, meters, start):
        self.meters = meters
        self.start = start

    def beats_per_bar(self, at):
        bpb = 4.0
        for t, num, den in self.meters:
            if t <= at + 1e-9:
                bpb = num * 4.0 / den
        return bpb

    def bars(self, beat):
        """(bar, beat) of absolute set beat `beat`, 1-based."""
        changes = [t for t, _, _ in self.meters if self.start < t < beat + 1e-9]
        cursor = self.start
        bar = 1
        for change in changes + [beat]:
            bpb = self.beats_per_bar(cursor)
            span = change - cursor
            whole = int((span + 1e-9) // bpb)
            if change == beat:
                return bar + whole, round(1.0 + (span - whole * bpb), 4)
            # a partial bar before a change still counts as a bar
            bar += whole + (1 if span - whole * bpb > 1e-9 else 0)
            cursor = change
        return bar, 1.0


def slug(name):
    out = ""
    for c in name.replace("'", "").replace("’", ""):
        if c.isalnum():
            out += c.lower()
        elif not out.endswith("-"):
            out += "-"
    return out.strip("-")


# ---------------------------------------------------------------------------
# Looks
# ---------------------------------------------------------------------------


def set_(role, dimmer, colour=None, focus=None):
    apply = [{"Dimmer": round(dimmer, 3)}]
    if colour:
        apply.append({"Color": colour})
    if focus:
        apply.append({"FocusPoint": focus})
    return {"target": {"Role": role},
            "steps": [{"apply": apply, "width": 1.0, "transition": 0.0, "ease": "Linear"}]}


def off(*roles):
    return [{"target": {"Role": r}, "apply": {"Dimmer": 0.0}} for r in roles]


def fx(effect, role=None, **params):
    r = {"effect": effect}
    if role:
        r["target"] = {"Role": role}
    if params:
        r["params"] = {k: float(v) for k, v in params.items()}
    return r


def bundle(name):
    return {"bundle": name}


ALL = ["Key", "Wash", "Back", "Bars", "Floor", "Movers", "Beams", "Drums", "Audience"]


def look(kind, d, name):
    """The recipes of one section look. Every look sets every role, so a
    cue is the same picture however the playhead got to it."""
    e = d["energy"]
    base, accent, hot, key = d["base"], d["accent"], d["hot"], d["key"]
    upper = name.upper()
    if kind == "dark":
        return off(*ALL)
    if kind == "walkin":
        return [set_("Key", 0.25, key), set_("Wash", 0.12, base), set_("Back", 0.25, accent),
                *off("Bars", "Floor", "Movers", "Beams")]
    if kind == "pads":
        return [set_("Key", 0.0), set_("Wash", 0.18, base), set_("Back", 0.45, accent),
                fx("breathe", "Back", depth=0.4), *off("Bars", "Floor", "Movers", "Beams")]
    if kind == "intro":
        return [set_("Key", 0.55, key), set_("Wash", 0.45, base), set_("Back", 0.7, accent),
                set_("Bars", 0.35, accent), set_("Floor", 0.0),
                set_("Movers", 0.5 * e, base, "Band"), fx("circle breathe", "Movers")]
    if kind == "verse":
        return [set_("Key", 0.75, key), set_("Wash", 0.4, base), set_("Back", 0.55, accent),
                set_("Bars", 0.25, base), set_("Floor", 0.5, key),
                set_("Movers", 0.35 * e, accent, "Stage"), bundle("verse drift" if "2" in name or "3" in name else "verse bed")]
    if kind == "intimate":
        return [set_("Key", 0.7, key), set_("Wash", 0.15, "Deep Blue"), set_("Back", 0.3, accent),
                *off("Bars", "Movers", "Beams"), set_("Floor", 0.55, key)]
    if kind == "pre":
        return [set_("Key", 0.85, key), set_("Wash", 0.6, base), set_("Back", 0.7, accent),
                set_("Bars", 0.5, accent), set_("Floor", 0.6, key),
                set_("Movers", 0.6 * e + 0.2, accent, "Audience"), bundle("pre chorus rise")]
    if kind in ("chorus", "big"):
        recipes = [set_("Key", 1.0, key), set_("Wash", 0.95, hot), set_("Back", 1.0, accent),
                   set_("Bars", 0.9, base), set_("Floor", 0.8, key),
                   set_("Movers", 1.0, hot, "Audience"), bundle(d["chorus"])]
        if kind == "big":
            recipes.append(bundle("drop and hold") if e >= 0.85 else fx("beat pulse", "Wash"))
        if d.get("flicker"):
            recipes.append(fx("fire flicker", "Bars"))
        return recipes
    if kind == "down":
        return [set_("Key", 0.8, key), set_("Wash", 0.35, accent), set_("Back", 0.6, base),
                set_("Bars", 0.2, accent), set_("Floor", 0.5, key),
                set_("Movers", 0.3, accent, "Band"), bundle("verse bed")]
    if kind == "inst":
        return [set_("Key", 0.5, key), set_("Wash", 0.7, base), set_("Back", 0.9, accent),
                set_("Bars", 0.7, hot), set_("Floor", 0.3, key),
                set_("Movers", 0.9, base, "Audience"), fx("ballyhoo" if e >= 0.85 else "pan sweep", "Movers")]
    if kind == "solo":
        return [set_("Key", 0.6, key), set_("Wash", 0.3, accent), set_("Back", 0.8, base),
                set_("Bars", 0.4, accent), set_("Floor", 0.3, key),
                set_("Movers", 1.0, "Open White", "Band"), fx("circle tight", "Movers")]
    if kind == "bridge":
        return [set_("Key", 0.7, key), set_("Wash", 0.35, accent), set_("Back", 0.5, base),
                set_("Bars", 0.2, base), set_("Floor", 0.45, key),
                set_("Movers", 0.5, accent, "Stage"), bundle("bridge hush")]
    if kind == "build":
        return [set_("Key", 0.85, key), set_("Wash", 0.5, base), set_("Back", 0.8, accent),
                set_("Bars", 0.6, hot), set_("Floor", 0.6, key),
                set_("Movers", 0.8, hot, "Audience"), bundle("build tension")]
    if kind == "breakdown":
        return [set_("Key", 0.5, key), set_("Wash", 0.4, accent), set_("Back", 0.6, base),
                set_("Bars", 0.4, base), set_("Floor", 0.3, key),
                set_("Movers", 0.6, accent, "Band"), bundle("breakdown embers")]
    if kind == "hits":
        # The band hitting together: the stage near dark, the towers
        # bumping on the beat, the blinders on the first hit.
        return [set_("Key", 0.5, key), set_("Wash", 0.15, hot), set_("Back", 0.2, accent),
                set_("Bars", 0.0), set_("Floor", 0.3, key),
                set_("Movers", 0.0, "Open White", "Audience"),
                fx("bump", "Wash"), fx("blinder hit", "Audience")]
    if kind == "talk":
        return [set_("Key", 0.8, "Warm White"), set_("Wash", 0.15, base), set_("Back", 0.3, accent),
                *off("Bars", "Movers", "Beams"), set_("Floor", 0.6, "Warm White")]
    if kind == "outro":
        return [set_("Key", 0.7, key), set_("Wash", 0.5, base), set_("Back", 0.6, accent),
                set_("Bars", 0.3, base), set_("Floor", 0.4, key),
                set_("Movers", 0.5, accent, "Band"), bundle("outro fade")]
    if kind == "end":
        # The last hit, then the room goes to the button.
        return [set_("Key", 1.0, key), set_("Wash", 1.0, hot), set_("Back", 1.0, accent),
                set_("Bars", 1.0, hot), set_("Floor", 0.8, key),
                set_("Movers", 1.0, "Open White", "Audience"), fx("blinder hit", "Wash")]
    raise ValueError(kind)


def kind_of(name, song):
    """A section card's name -> which look it gets."""
    n = name.upper()
    if n in ("COUNT IN", "COUNT-IN"):
        return "walkin"
    if n == "END":
        return "end"
    if "TALK" in n:
        return "talk"
    if "KEYS ONLY" in n or "DOWN" in n or "DROPPED" in n:
        return "down"
    if "BIG" in n or "DOUBLE" in n or "EPIC" in n or "FULL" in n or n == "SWELL":
        return "big"
    if "PRE" in n:
        return "pre"
    if "CHORUS" in n or "REFRAIN" in n or n == "HOOK" or n.startswith("BAND IN"):
        return "chorus"
    if "VERSE" in n or n == "TAG" or n == "TAGS":
        return "verse"
    if "SOLO" in n:
        return "solo"
    if "BREAKDOWN" in n:
        return "breakdown"
    if "BRIDGE" in n:
        return "bridge"
    if "BUILD" in n:
        return "build"
    if n in ("HITS", "BREAK"):
        return "hits"
    if "OUTRO" in n:
        return "outro"
    if "INTRO" in n or "CHORDS" in n:
        return "intro"
    if any(w in n for w in ("INSTRUMENTAL", "INTERLUDE", "TURNAROUND", "TURN AROUND",
                            "BASS", "GUITAR", "GTR", "DRUMS IN", "VOX FX")):
        return "inst"
    # A card named for the song itself is its intro (Royal Blood's sets).
    if slug(name) == slug(song) or n in (song.upper(),):
        return "intro"
    return "verse"


# How long a look takes to arrive, in beats, by kind: the big moments
# snap, the quiet ones breathe in.
FADE_BEATS = {"walkin": 4, "pads": 8, "intro": 2, "verse": 2, "intimate": 4, "pre": 1,
              "chorus": 0, "big": 0, "down": 2, "inst": 0.5, "solo": 1, "bridge": 4,
              "build": 2, "breakdown": 2, "hits": 0, "talk": 4, "outro": 4, "end": 0,
              "dark": 0}

# ---------------------------------------------------------------------------
# Writing
# ---------------------------------------------------------------------------


def bpm_at(tempo, beat):
    value = tempo[0][1]
    for t, v in tempo:
        if t <= beat + 1e-9:
            value = v
    return value


def read_tempo(path: Path):
    root = ET.fromstring(gzip.open(path).read())
    live = root.find("LiveSet")
    main = live.find("MainTrack")
    if main is None:
        main = live.find("MasterTrack")
    target = None
    manual = 120.0
    for t in main.iter("Tempo"):
        manual = float(text(t, "Manual", "120"))
        target = t.find("AutomationTarget").get("Id")
        break
    points = []
    for env in main.iter("AutomationEnvelope"):
        if text(env, "EnvelopeTarget/PointeeId") == target:
            for ev in env.iter("FloatEvent"):
                points.append((max(float(ev.get("Time")), 0.0), float(ev.get("Value"))))
    return sorted(points) or [(0.0, manual)]


def song_cues(song, display, start, end, cards, notes, meters, tempo):
    d = DESIGN[song]
    meter = Meter(meters, start)
    inside = [c for c in cards if start - 1e-9 <= c[0] < end]
    ordinals = {}
    cues = []

    def pos(beat):
        bar, b = meter.bars(beat)
        return {"bar": bar, "beat": b}

    def add(name, kind, at_beat, section, ordinal, section_start, note=""):
        bpm = bpm_at(tempo, at_beat)
        fade = round(FADE_BEATS[kind] * 60.0 / bpm, 3)
        sbar, sbeat = meter.bars(section_start)
        bar, beat = meter.bars(at_beat)
        cue = {
            "name": name,
            "fade_secs": fade,
            "values": [],
            "recipes": look(kind, d, section),
            "block": kind in ("walkin", "dark"),
            "at": {"section": section, "ordinal": ordinal,
                   "bars": bar - sbar, "beat": beat},
            "resolved": {"bar": bar, "beat": beat},
        }
        if note:
            cue["note"] = note
        cues.append((at_beat, cue))

    for c_start, c_end, name in inside:
        ordinal = ordinals.get(name, 0)
        ordinals[name] = ordinal + 1
        kind = kind_of(name, song)
        if song == "Cinematic Intro" and name == "Intro":
            kind = "pads"
        label = name if ordinal == 0 else f"{name} ({ordinal + 1})"
        add(label, kind, c_start, name, ordinal, c_start)
        if kind == "end":
            # The button: dark two beats into the END card, or at its end.
            dark_at = min(c_start + 2.0, c_end)
            add("· blackout", "dark", dark_at, name, ordinal, c_start)
        elif kind in ("chorus", "big") and (c_end - c_start) >= 32:
            # Halfway through a long chorus the movers change step.
            mid = c_start + ((c_end - c_start) // 8) * 4
            bar_cue = (mid - c_start)
            if bar_cue >= 16:
                add(f"· {label} lift", "big" if kind == "chorus" and d["energy"] >= 0.85 else kind,
                    mid, name, ordinal, c_start)

    # FoH notes become the cue's note — what the band is about to do, on
    # the operator's screen at the moment it matters.
    for n_start, _n_end, text_ in notes:
        if not (start <= n_start < end) or re.fullmatch(r"[0-9. ]*|[0-9]\.\.\.", text_):
            continue
        best = None
        for at_beat, cue in cues:
            if at_beat <= n_start + 1e-9:
                best = cue
        if best is not None:
            joined = text_.replace("\\", " / ")
            best["note"] = (best.get("note", "") + " · " if best.get("note") else "") + f"FoH: {joined}"

    cues.sort(key=lambda c: c[0])
    # Two cues on one beat: the later-written wins, the earlier is dropped.
    out = []
    for at_beat, cue in cues:
        if out and abs(out[-1][0] - at_beat) < 1e-6:
            out[-1] = (at_beat, cue)
        else:
            out.append((at_beat, cue))
    return [c for _, c in out]


def main(argv):
    if len(argv) != 2:
        print(__doc__)
        return 2
    als = Path(argv[1])
    locators, cards, notes, meters = read_set(als)
    tempo = read_tempo(als)
    ranges = song_ranges(locators)
    out_dir = DATA / "songs" / "jt-recital"
    out_dir.mkdir(parents=True, exist_ok=True)
    night = {
        "version": 1,
        "name": "JT's Senior Recital",
        "profile": "../profiles/ignition.ig-profile",
        "venue": "../venues/room138-cbu",
        "live_set": {"name": als.stem, "file": als.name},
        "sets": [],
        "songs": [],
    }
    for set_name, songs, lit in SETS:
        night["sets"].append({"name": set_name, "lit": lit,
                              "songs": [display for _, display in songs]})
        for song, display in songs:
            start, end = ranges[song]
            entry = {"set": set_name, "song": display, "locator": song,
                     "start_beats": start, "end_beats": end, "lit": lit}
            if lit:
                cues = song_cues(song, display, start, end, cards, notes, meters, tempo)
                doc = {
                    "version": 1,
                    "profile": "Ignition",
                    "song": {"live_set": als.stem, "locator": song, "name": display,
                             "start_beats": start, "end_beats": end},
                    "name": display,
                    "cues": cues,
                }
                path = out_dir / f"{slug(display)}.json"
                path.write_text(json.dumps(doc, indent=1) + "\n")
                entry["file"] = f"../songs/jt-recital/{slug(display)}.json"
                night["songs"].append(entry)
                print(f"{set_name:17} {display:40} {start:7.0f}..{end:<7.0f} {len(cues):3} cues")
            else:
                # In the setlist (`sets`), not in the running order: no cues.
                print(f"{set_name:17} {display:40} {start:7.0f}..{end:<7.0f} (not lit)")
    (DATA / "shows" / "jt-recital.ig-show").write_text(json.dumps(night, indent=1) + "\n")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
