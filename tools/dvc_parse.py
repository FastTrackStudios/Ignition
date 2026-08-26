#!/usr/bin/env python3
"""Reads a Daslight 5 / myDMX 5 showfile (`.dvc`) and dumps its patch.

The format is packed, not proprietary. A `.dvc` is plain XML rooted at
`<DLMFILE TYPE="Daslight" VERSION="5">`; the bulk payloads (`PATCHS`, the
scene tree, the touch UI) live in ordinary XML attributes as

    base64( uint32be(uncompressed_length) + raw zlib stream )

A few smaller attributes (`TOUCH_DOCK_MANAGER`, per-fixture `OUTMODE` /
`FLAG`) use the same container hex-encoded instead of base64. Both are
handled here. Fixture profiles are embedded inline (`SSLLIBRARY` /
`SSLMODE` / `SSLCHANNEL`), so a showfile is self-contained — the `.ssl2`
library files are not needed to recover a channel map.

Everything this does is read-only. It never writes to the showfile and
never talks to the desk.

Usage:
    tools/dvc_parse.py SHOW.dvc --dump out/     # every payload, decoded
    tools/dvc_parse.py SHOW.dvc --patch         # the patch, as CSV
    tools/dvc_parse.py SHOW.dvc --channel-map   # one row per DMX channel
    tools/dvc_parse.py SHOW.dvc --json out.json # the lot, structured
"""

import argparse
import base64
import binascii
import csv
import json
import pathlib
import struct
import sys
import xml.etree.ElementTree as ET
import zlib


class NotPacked(Exception):
    """The attribute is ordinary text, not a packed payload."""


def unpack(text):
    """Decode one packed attribute to bytes.

    Accepts either encoding of the container. Raises `NotPacked` for
    anything that is not one — attributes are tried speculatively, so
    "this is just a string" has to be a cheap, non-fatal answer.
    """
    if not text or len(text) < 8:
        raise NotPacked
    raw = None
    stripped = "".join(text.split())
    # Hex first: it is the stricter grammar, so a hex payload can never be
    # mistaken for base64, while the reverse is possible for short even-
    # length strings drawn from [0-9a-f].
    if len(stripped) % 2 == 0 and all(c in "0123456789abcdefABCDEF" for c in stripped):
        try:
            raw = binascii.unhexlify(stripped)
        except binascii.Error:
            raw = None
    if raw is None:
        try:
            raw = base64.b64decode(stripped, validate=True)
        except (binascii.Error, ValueError):
            raise NotPacked
    if len(raw) < 5:
        raise NotPacked
    (declared,) = struct.unpack(">I", raw[:4])
    # A sane length guard: the declared size is what tells us this really
    # is the container and not a coincidence.
    if declared == 0 or declared > 512 * 1024 * 1024:
        raise NotPacked
    try:
        body = zlib.decompress(raw[4:])
    except zlib.error:
        raise NotPacked
    if len(body) != declared:
        raise NotPacked
    return body


def pack(body):
    """The inverse of `unpack`, base64 flavour. Only used by the tests."""
    return base64.b64encode(struct.pack(">I", len(body)) + zlib.compress(body)).decode()


def payloads(path):
    """Every packed attribute in the file, decoded.

    Yields `(element_path, attribute_name, bytes)`. Walks every attribute
    rather than a hard-coded list of names, because the interesting ones
    differ between showfile versions and a missed payload is invisible.
    """
    tree = ET.parse(path)
    for parents, el in _walk(tree.getroot(), ()):
        where = "/".join(parents + (el.tag,))
        for name, value in el.attrib.items():
            try:
                yield where, name, unpack(value)
            except NotPacked:
                continue


def _walk(el, parents):
    yield parents, el
    for child in el:
        yield from _walk(child, parents + (el.tag,))


def as_xml(body):
    """Parse a payload as XML, or return None if it is not."""
    try:
        return ET.fromstring(body.decode("utf-8", "replace"))
    except ET.ParseError:
        return None


def _attr(el, *names, default=None):
    """First matching attribute, case-insensitively."""
    lowered = {k.lower(): v for k, v in el.attrib.items()}
    for n in names:
        if n.lower() in lowered:
            return lowered[n.lower()]
    return default


def _int(value, default=None):
    try:
        return int(str(value).strip())
    except (TypeError, ValueError):
        return default


def collect(path):
    """Structured extract: fixtures, profiles and their channel maps.

    The real inner schema, confirmed against a Daslight 5 showfile:

        PATCH NBFIXTURE=n
          FIXTURES                      one block per fixture *type*
            SSLLIBRARY  SSLNAME=".../Solena ... .ssl2"  SSLFIXUID=...
            SSLMODES > SSLMODE SSLNBCHANNEL=n
                       > SSLCHANNEL SSLCHANNELNAME="Dimmer" ...
            FIXTURE NAME=... ADDRESS=n UNIVERS=n POSX=n POSY=n ANGLE=n

    `POSX`/`POSY` are the desk's own 2D stage-view coordinates. They are
    schematic, not metric — the operator arranges icons to look like the
    room — so they are reliable for *ordering and grouping* (which pole,
    which row, left-to-right) and not for distances.
    """
    profiles, fixtures = [], []
    for _, _, body in payloads(path):
        root = as_xml(body)
        if root is None or root.tag.upper() != "PATCH":
            continue
        for block in root.findall(".//FIXTURES"):
            lib = block.find(".//SSLLIBRARY")
            raw = _attr(lib, "SSLNAME", default="") if lib is not None else ""
            # "_varied/Solena Professional Max Par 54 RGB.ssl2" -> the name
            name = raw.split("/")[-1]
            name = name[:-5] if name.lower().endswith(".ssl2") else name
            mode = block.find(".//SSLMODE")
            channels = []
            if mode is not None:
                for i, ch in enumerate(mode.findall(".//SSLCHANNEL")):
                    # Document order is the DMX offset. `SSLCHANNELTYPEINDEX`
                    # is NOT an offset — it disambiguates several channels
                    # of the same type (two "Color Macros" channels are
                    # type 0 index 0 and type 0 index 1). `SSLCHANNELMSB` /
                    # `SSLCHANNELLSB` flag the coarse and fine halves of a
                    # 16-bit pair rather than carrying a number.
                    channels.append({
                        "offset": i,
                        "name": _attr(ch, "SSLCHANNELNAME", default=""),
                        "type": _int(_attr(ch, "SSLCHANNELTYPE")),
                        "type_index": _int(_attr(ch, "SSLCHANNELTYPEINDEX"), 0),
                        "coarse": _attr(ch, "SSLCHANNELMSB") == "1",
                        "fine": _attr(ch, "SSLCHANNELLSB") == "1",
                    })
            footprint = _int(_attr(mode, "SSLNBCHANNEL") if mode is not None else None,
                             len(channels))
            profiles.append({"profile": name, "uid": _attr(lib, "SSLFIXUID", default="")
                             if lib is not None else "",
                             "library": raw, "footprint": footprint,
                             "channels": channels})
            for f in block.findall(".//FIXTURE"):
                addr = _int(_attr(f, "ADDRESS"))
                if addr is None:
                    continue
                fixtures.append({
                    "address": addr,
                    "universe": _int(_attr(f, "UNIVERS", "UNIVERSE"), 1),
                    "name": _attr(f, "NAME", default=""),
                    "profile": name,
                    "footprint": footprint,
                    "posx": _int(_attr(f, "POSX")),
                    "posy": _int(_attr(f, "POSY")),
                    "angle": _int(_attr(f, "ANGLE"), 0),
                    "uid": _attr(f, "DASUID", default=""),
                })
    fixtures.sort(key=lambda f: (f["universe"], f["address"]))
    # One entry per distinct type, not one per block.
    seen, unique = set(), []
    for p in profiles:
        if p["profile"] not in seen:
            seen.add(p["profile"])
            unique.append(p)
    return {"fixtures": fixtures, "profiles": unique, "groups": []}


def channel_rows(show):
    """One row per occupied DMX channel: address -> fixture, function."""
    by_profile = {p["profile"]: p for p in show["profiles"]}
    rows = []
    for f in sorted(show["fixtures"], key=lambda f: (f["universe"], f["address"])):
        profile = by_profile.get(f["profile"])
        span = f["footprint"] or (len(profile["channels"]) if profile else 0)
        for i in range(span):
            fn = ""
            if profile:
                match = [c for c in profile["channels"] if c["offset"] == i]
                fn = match[0]["name"] if match else ""
            rows.append({"universe": f["universe"], "address": f["address"] + i,
                         "offset": i, "fixture": f["name"],
                         "profile": f["profile"], "function": fn})
    return rows


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__,
                                 formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("showfile", type=pathlib.Path)
    ap.add_argument("--dump", type=pathlib.Path,
                    help="write every decoded payload here, for inspection")
    ap.add_argument("--patch", action="store_true", help="patch as CSV on stdout")
    ap.add_argument("--channel-map", action="store_true",
                    help="one CSV row per DMX channel, on stdout")
    ap.add_argument("--json", type=pathlib.Path, help="everything, structured")
    args = ap.parse_args(argv)

    if args.dump:
        args.dump.mkdir(parents=True, exist_ok=True)
        n = 0
        for where, name, body in payloads(args.showfile):
            stem = f"{where.replace('/', '.')}.{name}"
            suffix = ".xml" if as_xml(body) is not None else ".bin"
            (args.dump / (stem + suffix)).write_bytes(body)
            n += 1
        print(f"{n} payloads -> {args.dump}", file=sys.stderr)

    if not (args.patch or args.channel_map or args.json):
        return 0

    show = collect(args.showfile)
    print(f"{len(show['fixtures'])} fixtures, {len(show['profiles'])} profiles",
          file=sys.stderr)
    if not show["fixtures"]:
        print("no fixtures recognised - run with --dump and check the inner "
              "schema against `collect()`'s tag list", file=sys.stderr)

    if args.patch:
        w = csv.DictWriter(sys.stdout, ["universe", "address", "footprint",
                                        "name", "profile", "posx", "posy", "angle"],
                           extrasaction="ignore")
        w.writeheader()
        for f in sorted(show["fixtures"], key=lambda f: (f["universe"], f["address"])):
            w.writerow(f)
    if args.channel_map:
        rows = channel_rows(show)
        w = csv.DictWriter(sys.stdout, ["universe", "address", "offset",
                                        "fixture", "profile", "function"])
        w.writeheader()
        w.writerows(rows)
    if args.json:
        args.json.write_text(json.dumps(show, indent=2) + "\n")
    return 0


if __name__ == "__main__":
    sys.exit(main())
