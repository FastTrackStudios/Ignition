#!/usr/bin/env python3
"""Know Me (Extended), written to the cue design guide and held to its lint.

  know_me_v4.py <chart.json> <song.json> > batch.json     (one IGNITION_BATCH)

chart.json is the kit charted on the set's own tempo map (chart.py, from
`lintshow --grid`). Roles only; the library by name; sections block; one
accent a bar at most; effect periods of 4, 2, 1 or half a bar.
"""
import json, sys
chart = json.load(open(sys.argv[1])); song = json.load(open(sys.argv[2]))
H = {(b, be, k): v for b, be, k, v in chart["hits"]}
SONG = "know-me-extended"
have = {c["name"] for c in song["cues"]}
secs = {}
for s_ in chart["sections"]:
    secs[s_["name"] if s_["name"] not in secs else s_["name"] + "#2"] = (s_["bar"], s_["end"] - 1)
def bar_secs(n=1.0): return n * 4 * 60 / 127.0

def st(t, d, c=None, f=None):
    p = {"target": t, "dimmer": d}
    if c: p["color"] = c
    if f: p["focus"] = f
    return p
def fx(t, e, bars, tricks=None, depth=None):
    p = {"target": t, "effect": e, "bars": bars}
    if tricks: p["tricks"] = tricks
    if depth is not None: p["depth"] = depth
    return p
K, SP, W, BK, FL, BR, MV, AU = "Key", "Spot", "Wash", "Back", "Floor", "Bars", "Movers", "Audience"
def faces(level): return [st(K, level, "White"), st(SP, level, "White")]   # one white, never chased
def dark(*roles): return [st(r, 0.0) for r in roles]

# Home is cold (deep blue, turquoise); the chorus owns magenta; amber is the
# solo's alone; white is for faces and the last chorus.
looks = {
 "Count In": dark(K, SP, W, FL, BR, MV) + [st(BK, 0.06, "Primary")],
 # A piano and a drum machine: the back wall and the movers on it, nothing else.
 "Intro": dark(K, SP, W, FL, BR) + [st(BK, 0.28, "Primary"), st(MV, 0.25, "Secondary", "Back Wall"),
          fx(BK, "breathe", 4, None, 0.3)],
 "Verse 1": faces(0.60) + dark(FL, BR) + [st(W, 0.35, "Primary"), st(BK, 0.50, "Secondary"),
          st(MV, 0.25, "Secondary", "Band"), fx(W, "breathe", 4, "halves", 0.3)],
 "PreChorus": faces(0.70) + dark(BR) + [st(W, 0.50, "Primary"), st(BK, 0.72, "Secondary"), st(FL, 0.30, "Secondary"),
          st(MV, 0.45, "Secondary", "Stage"), fx(W, "build", 4, "ends in", 0.6)],
 # Chorus 1 holds back the floor, the bars, the blinders, white and the fast moves.
 "Chorus": faces(0.75) + dark(FL, BR) + [st(W, 0.65, "Accent"), st(BK, 0.85, "Accent"),
          st(MV, 0.70, "Primary", "Audience"), fx(W, "wave", 2, "mirror", 0.5), fx(MV, "windmill", 2, "mirror", 0.6)],
 # Home again, in the other cold colour, with the band still driving under it.
 "Verse 2": faces(0.62) + dark(FL, BR) + [st(W, 0.40, "Tertiary"), st(BK, 0.50, "Primary"),
          st(MV, 0.30, "Tertiary", "Stage"), fx(W, "offset breathe", 4, "thirds", 0.3)],
 # Chorus 1 and one addition: the floor.
 "Chorus 2": faces(0.78) + dark(BR) + [st(W, 0.78, "Accent"), st(BK, 0.95, "Accent"), st(FL, 0.70, "Accent"),
          st(MV, 0.85, "Primary", "Audience"), fx(W, "wave", 2, "mirror", 0.6), fx(MV, "windmill", 2, "mirror", 0.7)],
 # A pool on the guitarist, amber behind him, the movers on the band: fewest
 # fixtures of any full section, so the last chorus has somewhere to come from.
 "Guitar Solo": [st(K, 0.35, "White"), st(SP, 0.90, "White"), st(BK, 0.55, "Accent 2")] + dark(W, FL, BR) + [
          st(MV, 0.55, "White", "Band"), fx(MV, "circle tight", 2, "pairs", 0.6)],
 # The last chorus: the only cue at full, and everything held back is spent.
 "Chorus 3": faces(0.80) + [st(W, 1.00, "Accent"), st(BK, 1.00, "Accent"), st(FL, 1.00, "Open White"), st(BR, 1.00, "Tertiary"),
          st(MV, 1.00, "Open White", "Audience"), fx(W, "wave", 1, "mirror", 0.7), fx(BR, "strip chase", 1, None, 0.9),
          fx(MV, "ballyhoo", 1, "scatter", 0.9)],
 # A voice and a piano.
 "DownChorus": [st(K, 0.60, "White"), st(SP, 0.25, "White")] + dark(W, FL, BR) + [st(BK, 0.20, "Secondary"),
          st(MV, 0.15, "Secondary", "Back Wall")],
 "Build": faces(0.75) + dark(BR) + [st(W, 0.60, "Tertiary"), st(BK, 0.85, "Secondary"), st(FL, 0.50, "Tertiary"),
          st(MV, 0.70, "Tertiary", "Stage"), fx(W, "build", 4, "ends in", 0.7)],
 # The band alone: the chorus's colour once more, under the last chorus's level.
 "Outro": [st(K, 0.55, "White"), st(SP, 0.55, "White"), st(W, 0.50, "Accent"), st(BK, 0.62, "Accent"), st(FL, 0.40, "Primary"),
          st(BR, 0.0), st(MV, 0.60, "Primary", "Audience"), fx(W, "wave", 2, "mirror", 0.5), fx(MV, "figure eight", 2, "mirror", 0.6)],
}
pre2 = faces(0.72) + dark(BR) + [st(W, 0.55, "Primary"), st(BK, 0.72, "Secondary"), st(FL, 0.40, "Secondary"),
          st(MV, 0.50, "Secondary", "Stage"), fx(W, "build", 4, "ends in", 0.7)]
beat = bar_secs(0.25)
fades = {"Count In": 0.0, "Intro": bar_secs(2), "Verse 1": bar_secs(1), "PreChorus": bar_secs(1), "Chorus": beat / 2,
         "Verse 2": bar_secs(1.5), "PreChorus (2)": bar_secs(1), "Chorus 2": beat / 2, "Guitar Solo": beat / 2,
         "Chorus 3": 0.0, "DownChorus": bar_secs(2), "Build": bar_secs(12), "Outro": beat / 2}

# Where the music turns. (new name, the cue of an earlier pass it replaces, at, fade, parts)
punt = [{"look": "punt"}]
BLOCKS = ("safe", "End", "reset")
subs = [
 ("safe", "· down 2", "Count In", 0.5, punt),
 ("· pre riser", "· pre turn", "PreChorus+7", beat, [st(BK, 0.85, "Accent"), {"target": "Beams", "effect": "strobe riser", "bars": 1}]),
 ("· stop", "· stop", "Chorus+14", 0.0, dark(W, FL, MV) + [st(BK, 0.15, "Accent")]),                 # bar 41: the kit drops out
 ("· pre 2 riser", "· pre 2 turn", "PreChorus#2+7", beat, [st(BK, 0.90, "Accent"), {"target": "Beams", "effect": "strobe riser", "bars": 1}]),
 ("· Chorus 2 lift", "· Chorus 2 lift", "Chorus 2+8", beat, [st(MV, 0.90, "White", "Audience"), st(FL, 0.80, "White")]),
 ("· solo up", "· solo up", "Guitar Solo+8", beat, [st(BK, 0.75, "Accent 2"), st(BR, 0.60, "Accent 2"), fx(BR, "comet", 2, None, 0.8)]),
 ("· breath", "· Chorus lift", "Guitar Solo+15", 0.0, faces(0.70) + dark(W, FL, BR, MV) + [st(BK, 0.12, "Primary")]),  # the bar before the last chorus
 ("· Chorus 3 lift", "· Chorus 3 lift", "Chorus 3+8", beat, [st(W, 1.00, "White"), fx(MV, "ballyhoo", 0.5, "scatter", 1.0)]),
 ("· ring out", "· ring out", "Chorus 3+14", bar_secs(4), dark(FL, BR, MV) + [st(W, 0.08, "Primary"), st(BK, 0.30, "Secondary")]),
 ("· build top", "· outro flip", "Build+12", bar_secs(1), [st(BK, 0.95, "Accent"), st(BR, 0.60, "Tertiary"), fx(BR, "strip fill", 4, None, 0.9)]),
 ("· button", "· voice back", "Outro+16", 0.0, faces(0.70) + [st(W, 0.90, "White"), st(BK, 0.90, "Accent"), st(FL, 0.80, "White"), st(MV, 0.80, "White", "Audience")]),
 ("End", "End", {"section": "Outro", "last_bar": True}, bar_secs(2), dark(K, SP, W, FL, BR, MV) + [st(BK, 0.10, "Primary")]),
 ("reset", "· riser", "158.4", 1.0, punt),
]

# Accents: one a bar at most. A landing or a crash on the one is a white pop on
# the wash; the pickup out of a fill is a level bump on the bars.
hit = lambda bar, bt, k: H.get((bar, bt, k), 0.0)
lands = [secs[n][0] for n in ("Chorus", "Chorus 2", "Guitar Solo", "Chorus 3", "Outro")]
fills = [26, 42, 50, 58, 66, 74, 82, 90, 98, 104, 140, 148, 154]
quiet = set(range(*secs["DownChorus"])) | set(range(secs["Build"][0], secs["Build"][1])) | {41, 105, 106, 107, 108}
markers = []
for bar in range(secs["Verse 1"][0], 158):
    if bar in quiet or bar in (90,): continue                       # the breath before the last chorus stays clean
    in_verse = any(secs[n][0] <= bar <= secs[n][1] for n in ("Verse 1", "Verse 2"))
    if bar == secs["Chorus 3"][0]:
        markers.append({"kind": "drop", "bar": bar, "beat": 1})                   # the blinders, once, where the last chorus lands
    elif bar in lands:
        markers.append({"kind": "land", "bar": bar, "beat": 1})
    elif bar in fills:
        best = max(((max(hit(bar, bt, "Toms"), hit(bar, bt, "Snare"), hit(bar, bt, "Cymbals")), bt) for bt in (3, 3.5, 4, 4.5)))
        if best[0] > 0.5 and not (in_verse and bar % 2): markers.append({"kind": "pickup", "bar": bar, "beat": best[1]})
    elif hit(bar, 1, "Cymbals") > 0.6 and (bar - 27) % 4 == 2 and not in_verse:
        markers.append({"kind": "crash", "bar": bar, "beat": 1})                  # the middle of each four-bar phrase
for bar in (155, 156, 157):
    markers = [m for m in markers if m["bar"] != bar]
markers.append({"kind": "button", "bar": 157, "beat": 1})                        # the last crash

steps = []
def step(action, **args): steps.append({"action": "IGNITION_" + action, "args": args})
def sstep(action, **args): step(action, song=SONG, **args)
step("ROLE_BIND", role="Spot", to="Key Guitar")                                  # this room's soloist light: the guitarist's pars
step("LIBRARY_CREATE", kind="palettes", name="Know Me Night", replace=True, entry={
    "about": "Know Me: deep blue and turquoise at home, magenta for the choruses, amber for the solo, one warm white for faces",
    "slots": {"Primary": "Deep Blue", "Secondary": "Turquoise", "Tertiary": "Cyan", "Accent": "Magenta", "Accent 2": "Amber", "White": "Warm White"}})
sstep("PALETTE_USE", palette="Know Me Night")
sstep("FILL_SECTIONS", looks=looks, replace=True, block=True)
sstep("CUE_UPDATE", cue="PreChorus (2)", set={"recipes": [], "block": True})
sstep("CUE_UPDATE", cue="PreChorus (2)", parts=pre2)
for name, f in fades.items(): sstep("CUE_UPDATE", cue=name, set={"fade_secs": round(f, 3), "block": True})
for name, old, at, fade, parts in subs:
    if name in have:
        sstep("CUE_UPDATE", cue=name, at=at, set={"recipes": [], "fade_secs": round(fade, 3), "block": name in BLOCKS})
    elif old in have:
        sstep("CUE_UPDATE", cue=old, at=at, set={"name": name, "recipes": [], "fade_secs": round(fade, 3), "block": name in BLOCKS})
    else:
        sstep("CUE_ADD", name=name, at=at, fade_secs=round(fade, 3), parts=parts); continue
    sstep("CUE_UPDATE", cue=name, parts=parts)
sstep("SONG_MAP_SET", markers=markers, replace_markers=True)
sstep("FILL_HITS", target=W, kind="button", bump="White", depth=0.9, name="snare", replace=True)
sstep("FILL_HITS", target=W, kind="land", bump="White", depth=0.9, name="chorus", replace=True)
sstep("FILL_HITS", target=W, kind="crash", bump="White", depth=0.6, name="crash", replace=True)
sstep("FILL_HITS", target=BR, kind="pickup", bump="Level", depth=0.6, name="fill", replace=True)
sstep("FILL_HITS", target=AU, kind="drop", bump="White", depth=1.0, name="kick", replace=True)
print(json.dumps({"label": "Know Me: to the design guide (roles, blocking sections, one accent a bar)", "steps": steps}))
sys.stderr.write("markers: %s\n" % {k: sum(1 for m in markers if m["kind"] == k) for k in ("land", "crash", "pickup")})
