#!/usr/bin/env python3
"""The programming actions' catalogue (pane/actions.json) written where it
is read: the extension's manifest (`[[actions]]` between the markers in
pane/extension.toml) and the docs' table (docs/lighting/guides/actions.md).

    python3 -I tools/program_actions.py          # write both
    python3 -I tools/program_actions.py --check  # fail if either is stale
"""
import json
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent.parent
CATALOGUE = HERE / "pane" / "actions.json"
MANIFEST = HERE / "pane" / "extension.toml"
DOC = HERE.parent.parent / "docs" / "lighting" / "guides" / "actions.md"
BEGIN = "# >>> programming actions (tools/program_actions.py writes this from actions.json)"
END = "# <<< programming actions"
TYPES = {"any": None, "string": "string", "number": "number", "integer": "integer",
         "boolean": "boolean", "object": "object", "array": "array"}
GROUPS = [
    ("History and batching", ("ACTIONS", "HISTORY", "UNDO", "REDO", "EDIT_", "BATCH")),
    ("Library", ("LIBRARY_", "PALETTE_")),
    ("Effects and tricks", ("EFFECT_", "TRICKS_")),
    ("A song", ("SONG_", "CUE_", "SECTION_", "FILL_", "TRIGGER_")),
    ("A set", ("SET_",)),
    ("Environments", ("ENV_",)),
    ("People and focus", ("PERSON_", "PEOPLE_", "BLOCKING_", "FOCUS_")),
    ("Fixtures, groups and roles", ("FIXTURE_", "GROUP_", "ROLE_")),
    ("The patch", ("PATCH_",)),
]


def schema(action):
    props = {}
    for name, arg in action["args"].items():
        prop = {"description": arg["about"]}
        if TYPES[arg["type"]]:
            prop["type"] = TYPES[arg["type"]]
        props[name] = prop
    out = {"type": "object", "properties": props}
    if action["required"]:
        out["required"] = action["required"]
    return json.dumps(out, separators=(",", ":"), ensure_ascii=False)


def toml_str(s):
    return json.dumps(s, ensure_ascii=False)


def manifest_block(actions):
    lines = [BEGIN]
    for a in actions:
        lines += ["", "[[actions]]", f'id = "{a["id"]}"', f"name = {toml_str(a['name'])}",
                  f"description = {toml_str(a['about'])}", 'group = "Lighting"',
                  f'effect = "{a["effect"]}"']
        if a["args"]:
            lines.append(f"params = {toml_str(schema(a))}")
    lines += ["", END]
    return "\n".join(lines)


def doc(actions):
    out = ["# The programming actions", "",
           "Generated from `features/lighting/pane/actions.json` by",
           "`features/lighting/tools/program_actions.py`; edit the catalogue, not this page.",
           "How they fit together: [programming.md](programming.md).", "",
           "Every action also takes `root` (the lighting data root, default `IGNITION_ROOT`).",
           "**bold** arguments are required.", ""]
    left = list(actions)
    for title, prefixes in GROUPS:
        rows = [a for a in left if a["id"].removeprefix("IGNITION_").startswith(prefixes)]
        left = [a for a in left if a not in rows]
        if not rows:
            continue
        out += [f"## {title}", "", "| Action | Effect | What it does | Arguments |", "|---|---|---|---|"]
        for a in rows:
            args = ", ".join(f"**{n}**" if n in a["required"] else n for n in a["args"] if n != "root")
            about = a["about"].replace("|", "\\|")
            out.append(f"| `{a['id']}` | {a['effect'].replace('_', ' ')} | {about} | {args} |")
        out.append("")
    assert not left, [a["id"] for a in left]
    return "\n".join(out)


def main():
    actions = json.loads(CATALOGUE.read_text())
    text = MANIFEST.read_text()
    block = manifest_block(actions)
    if BEGIN in text:
        head, rest = text.split(BEGIN, 1)
        tail = rest.split(END, 1)[1]
        new = head + block + tail
    else:
        new = text.rstrip("\n") + "\n\n" + block + "\n"
    page = doc(actions) + "\n"
    if "--check" in sys.argv:
        stale = [p.name for p, want in ((MANIFEST, new), (DOC, page)) if not p.exists() or p.read_text() != want]
        if stale:
            sys.exit(f"stale: {stale}; run tools/program_actions.py")
        return
    MANIFEST.write_text(new)
    DOC.parent.mkdir(parents=True, exist_ok=True)
    DOC.write_text(page)
    print(f"{len(actions)} actions -> {MANIFEST.name}, {DOC.name}")


if __name__ == "__main__":
    main()
