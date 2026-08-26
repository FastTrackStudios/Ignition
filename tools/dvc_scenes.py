#!/usr/bin/env python3
"""Mirrors scenes out of a Daslight 5 / myDMX 5 showfile into an Ignition cue list.

The desk stores a scene as a set of raw DMX values per fixture. This turns
that back into intent — a dimmer level and a colour per fixture — by
running each fixture's bytes through its own channel map, then groups
fixtures that agree so the cue reads as a look rather than as 400 numbers.

Encoding, confirmed against the real file:

    SCENE > STEPS > STEP > FIXTUREDATAS > FIXTUREDATA
      FIXTURE = the fixture's DASUID (matches PATCHS)
      DATA    = base64( raw zlib )        <- note: NO length prefix, unlike
                                             the PATCHS container
      plaintext = uint16be channel_count, then one uint16be per channel,
                  where 0xFFFF means "this scene does not touch it"

Values are plain DMX 0..255 held in a uint16. The tail after the values is
per-channel fade/curve data and is not read.

Usage:
    tools/dvc_scenes.py SHOW.dvc --list
    tools/dvc_scenes.py SHOW.dvc --bank "Main Scenes" --out data/shows/x.json
"""

import argparse
import base64
import json
import pathlib
import struct
import sys
import xml.etree.ElementTree as ET
import zlib

UNSET = 0xFFFF

# A desk effect scene stores its own machinery — a colour palette or a
# pan/tilt path — not something Ignition can execute directly. Ignition
# runs effects from its own library, so each is mapped to the closest
# entry there. This is the one part of a mirror that is an
# *approximation* rather than a transcription, so it is a visible table
# rather than buried logic.
#
# Keyed on the desk's own scene name, lowercased. `None` means "leave it
# to the type default below".
EFFECT_BY_NAME = {
    "7 main | rainbow": "rainbow",
    "8 wall knight rider": "wall sweep",
    "4 all fades": "colour cycle",
    "7 sparkle": "sparkle",
    "8 perlin": "shimmer",
    "movers slow new": "square path",
    "movers fast new": "square path",
    "5 wall move": "wall sweep",
    "6 wall move 2": "wall sweep",
    "2 strobe movers": "strobe",
    "3 strobe siezure": "random strobe",
    "1 movers flash": "strobe burst",
    "7 white flash": "white pop",
    "8 blue flash": "white pop",
}

# Fallback by the desk's EFFECT TYPE: 2 is a colour effect over a palette,
# 4 is a position effect over a point path.
EFFECT_BY_TYPE = {"2": "colour cycle", "4": "ballyhoo"}


def effect_for(scene_name, effect_el):
    """The library effect this desk effect is mirrored onto, or None."""
    named = EFFECT_BY_NAME.get(scene_name.strip().lower())
    if named:
        return named
    return EFFECT_BY_TYPE.get(effect_el.get("TYPE"))


def palette(effect_el):
    """The effect's colour list as (r, g, b) floats, 0..1.

    `COLOR VAL` is slash-separated floats; the first three are RGB and the
    rest are the other colour channels and flags, which nothing here uses.
    """
    out = []
    for c in effect_el.iter("COLOR"):
        parts = (c.get("VAL") or "").split("/")
        if len(parts) >= 3:
            try:
                out.append(tuple(round(float(v), 3) for v in parts[:3]))
            except ValueError:
                pass
    return out


def scene_steps(el):
    """One `{fixture_uid: [value|None, ...]}` per STEP of a scene.

    Most scenes have a single step and are a static look. A scene with
    several is a chase, and every step is real content — mirroring only
    the first would silently drop most of what the operator programmed.
    """
    return [step_values(st) for st in el.iter("STEP")]


def step_values(step):
    """{fixture_uid: [value|None, ...]} for one STEP element."""
    out = {}
    for fd in step.iter("FIXTUREDATA"):
        blob = fd.get("DATA")
        if not blob:
            continue
        # The stream has no proper terminator, so a plain decompress()
        # raises; the object form keeps what it decoded.
        raw = zlib.decompressobj().decompress(base64.b64decode(blob))
        if len(raw) < 2:
            continue
        (count,) = struct.unpack_from(">H", raw, 0)
        if len(raw) < 2 + count * 2:
            continue
        vals = struct.unpack_from(f">{count}H", raw, 2)
        out[fd.get("FIXTURE")] = [None if v == UNSET else min(v, 255) for v in vals]
    return out


# `ignition_viz::dmx` reads a mover's angles as
#     pan  = (byte/255 - 0.5) * 540
#     tilt = (byte/255 - 0.5) * 270
# so mirroring a desk position means running that same formula forward.
PAN_RANGE_DEG = 540.0
TILT_RANGE_DEG = 270.0


def channel_roles(profiles):
    """{profile_name: {role: offset}} from the extract.

    Roles are matched by channel name first and by the desk's own semantic
    type code second — one profile (MINI DERBY) types every channel 7, so
    names are the only truth there.
    """
    NAME = {"dimmer": "dimmer", "total dimmer": "dimmer",
            "red": "r", "green": "g", "blue": "b",
            "x": "pan", "y": "tilt"}
    TYPE = {7: "dimmer", 25: "r", 26: "g", 27: "b", 1: "pan", 2: "tilt"}
    roles = {}
    for p in profiles:
        slot = {}
        for c in p["channels"]:
            key = NAME.get(c["name"].strip().lower()) or TYPE.get(c["type"])
            if key and key not in slot:
                slot[key] = c["offset"]
        roles[p["profile"]] = slot
    return roles


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__,
                                 formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("showfile", type=pathlib.Path)
    ap.add_argument("--venue", type=pathlib.Path,
                    default=pathlib.Path("data/venues/riverside"))
    ap.add_argument("--bank", action="append", default=[],
                    help="bank name to mirror; repeatable. Default: all.")
    ap.add_argument("--list", action="store_true", help="list banks and scenes")
    ap.add_argument("--out", type=pathlib.Path)
    ap.add_argument("--name", default="Riverside Desk")
    args = ap.parse_args(argv)

    root = ET.parse(args.showfile).getroot()
    if args.list:
        for bank in root.iter("BANK"):
            print(f'[{bank.get("NAME")}]')
            for sc in bank.iter("SCENE"):
                print("   ", sc.get("NAME"))
        return 0

    show = json.loads((args.venue / "console-show.json").read_text())
    patch = json.loads((args.venue / "patch.json").read_text())
    roles = channel_roles(show["profiles"])
    # console fixture uid -> (venue chan, profile)
    addr_to_chan = {e["address"]: e["chan"] for e in patch if e.get("source")}
    uid_to = {}
    for f in show["fixtures"]:
        chan = addr_to_chan.get(f["address"])
        if chan is not None:
            uid_to[f["uid"]] = (chan, f["profile"])

    cues = [{"name": "Blackout", "fade_secs": 0, "block": True, "recipes": []}]
    skipped = 0
    stats = {"look": 0, "chase": 0, "effect": 0, "empty": 0}

    def look_recipes(vals_by_uid):
        """Recipes for one step's worth of raw values."""
        nonlocal skipped
        looks, positions = {}, {}
        for uid, vals in vals_by_uid.items():
            if uid not in uid_to:
                skipped += 1
                continue
            chan, profile = uid_to[uid]
            slot = roles.get(profile, {})

            def at(key):
                i = slot.get(key)
                return vals[i] if i is not None and i < len(vals) else None

            dim, r, g, b = at("dimmer"), at("r"), at("g"), at("b")
            pan, tilt = at("pan"), at("tilt")
            # A mover position is content in its own right: several Movers
            # scenes set pan/tilt and nothing else, and reading only
            # dimmer and colour would mirror them as empty.
            if pan is not None or tilt is not None:
                angles = []
                if pan is not None:
                    angles.append(["Pan", round((pan / 255.0 - 0.5) * PAN_RANGE_DEG, 2)])
                if tilt is not None:
                    angles.append(["Tilt", round((tilt / 255.0 - 0.5) * TILT_RANGE_DEG, 2)])
                positions.setdefault(tuple(map(tuple, angles)), []).append(chan)
            if dim is None and r is None and g is None and b is None:
                continue
            # A fixture with colour but no dimmer channel (the bars) is
            # driven by its colour alone, so it reads as full.
            level = 1.0 if dim is None else round(dim / 255.0, 3)
            colour = None
            if None not in (r, g, b) and max(r, g, b) > 0:
                peak = max(r, g, b)
                colour = (round(r / peak, 3), round(g / peak, 3), round(b / peak, 3))
                if dim is None:
                    level = round(peak / 255.0, 3)
            looks.setdefault((level, colour), []).append(chan)

        out = []
        for (level, colour), chans in sorted(
                looks.items(), key=lambda kv: (kv[0][0], kv[0][1] or ())):
            if level <= 0.0:
                continue
            chans = sorted(chans)
            out.append({"target": {"Chans": chans}, "apply": {"Dimmer": level}})
            if colour:
                out.append({"target": {"Chans": chans},
                            "apply": {"Color": {"name": "", "red": colour[0],
                                                "green": colour[1],
                                                "blue": colour[2]}}})
        for angles, chans in sorted(positions.items()):
            out.append({"target": {"Chans": sorted(chans)},
                        "apply": {"Raw": [list(a) for a in angles]}})
        return out

    def add(name, fade, recipes, kind):
        stats[kind if recipes else "empty"] += 1
        cues.append({"name": name, "fade_secs": fade, "block": True,
                     "recipes": recipes})

    for bank in root.iter("BANK"):
        if args.bank and bank.get("NAME") not in args.bank:
            continue
        for sc in bank.iter("SCENE"):
            label = f'{bank.get("NAME")} · {sc.get("NAME").strip()}'
            raw = sc.get("FADE_IN")
            fade = round(int(raw) / 10.0, 2) if raw and raw.isdigit() else 0

            eff = sc.find(".//EFFECT")
            if eff is not None:
                # An effect scene: the desk stores a palette or a point
                # path plus the fixtures it runs on. Ignition runs its own
                # library effects, so this maps to the nearest one and
                # targets the same fixtures.
                chans = sorted({uid_to[b.get("FIXTURE")][0]
                                for b in sc.iter("BEAM")
                                if b.get("FIXTURE") in uid_to})
                name = effect_for(sc.get("NAME"), eff)
                recipes = []
                if chans and name:
                    # The palette's mean is the base the effect moves
                    # around, so the look sits where the desk's did even
                    # though the motion is Ignition's.
                    cols = palette(eff)
                    if cols:
                        mean = [round(sum(c[i] for c in cols) / len(cols), 3)
                                for i in range(3)]
                        if max(mean) > 0:
                            recipes.append({"target": {"Chans": chans},
                                            "apply": {"Dimmer": 1.0}})
                            recipes.append({"target": {"Chans": chans},
                                            "apply": {"Color": {
                                                "name": "", "red": mean[0],
                                                "green": mean[1], "blue": mean[2]}}})
                    recipes.append({"effect": name, "target": {"Chans": chans}})
                add(f"{label}  [fx: {name or '?'}]", fade, recipes, "effect")
                continue

            steps = scene_steps(sc)
            if len(steps) <= 1:
                add(label, fade, look_recipes(steps[0]) if steps else [], "look")
            else:
                # A chase. Every step is content, so each becomes its own
                # cue — GO walks the chase rather than losing it.
                for i, vals in enumerate(steps, start=1):
                    add(f"{label} ▸ {i}/{len(steps)}", fade,
                        look_recipes(vals), "chase")

    doc = {"name": args.name, "cues": cues}
    text = json.dumps(doc, indent=2) + "\n"
    if args.out:
        args.out.write_text(text)
        lit = sum(1 for c in cues if c["recipes"])
        print(f"{len(cues)} cues ({lit} with content) -> {args.out}", file=sys.stderr)
        print(f"  looks {stats['look']}  chase steps {stats['chase']}  "
              f"effects {stats['effect']}  empty {stats['empty']}", file=sys.stderr)
        if skipped:
            print(f"{skipped} fixture entries skipped: not placed in this venue yet",
                  file=sys.stderr)
    else:
        print(text)
    return 0


if __name__ == "__main__":
    sys.exit(main())
