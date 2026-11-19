#!/usr/bin/env python3
"""Know Me (Extended): the tempo map, the kit read from the stems and the chart.

The looks are a section each; what happens inside a section comes from the
song: the chords' own rhythm (the chorus is 4 on one, 6m pushed on the and of
four, 5 on three of the next bar), the snare on three, the kick on one and the
and of four, the b7 that ends each line, the walk-up, the band out on three.

  know_me_v7.py <chart.json> <song.json> > batch.json     (one IGNITION_BATCH)

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
BLUE, RED, GREEN = ({"ColorAdd": {"channel": c}} for c in ("Blue", "Red", "Green"))
# What happens inside a section is not written here as effects. The song's
# events (every chord change, every pushed chord, every snare and kick the
# stems hold, the landings, the stops) go into the song map as markers, which
# say what the music does and nothing about light. Further down, the show maps
# kinds of event to generic bumps from the library, section by section. Another
# show can map the same events another way.
def snare(t, depth): return None
def kick(t, depth): return None
def chords(t, depth, cool=True): return None
def verse_chords(t, depth): return None
K, SP, FI, W, BK, FL, BR, MV, AU = "Key", "Spot", "Fill", "Wash", "Back", "Floor", "Bars", "Movers", "Audience"
def faces(level): return [st(K, level, "White"), st(SP, level, "White")]   # one white, never chased
def dark(*roles): return [st(r, 0.0) for r in roles]
def lamps(level): return st(AU, level, "White")                              # the towers' warm lamps

# Home is cold (deep blue, turquoise); the chorus owns magenta; amber is the
# solo's alone; white is for faces and the last chorus. No layer runs an
# effect that steps or rolls through its lights on its own: a look holds
# still and what moves it is the song's events. The beams are the exception
# (they travel smoothly), with the candle and firelight on the lamps.
looks = {
 "Count In": dark(K, SP, FI, W, FL, BR, MV, AU) + [st(BK, 0.06, "Primary")],
 # A piano and a drum machine: the wall, the warm lamps glinting, no beams yet.
 "Intro": dark(K, SP, W, FL, BR) + [st(BK, 0.30, "Primary"), st(FI, 0.18, "Primary"), lamps(0.16), st(MV, 0.0),
          fx(BK, "back breathe", 4, None, 0.4)],
 "Verse 1": faces(0.78) + dark(FL, BR, AU) + [st(W, 0.38, "Primary"), st(BK, 0.50, "Secondary"), st(FI, 0.35, "Secondary"),
          st(MV, 0.28, "Secondary", "Band"), verse_chords(BK, 0.14),
          fx(MV, "circle", 4, "mirror", 0.5)],
 "PreChorus": faces(0.88) + dark(BR) + [st(W, 0.52, "Primary"), st(BK, 0.72, "Secondary"), st(FL, 0.30, "Secondary"),
          st(FI, 0.50, "Accent"), lamps(0.14), st(MV, 0.45, "Secondary", "Stage"),
          chords(BK, 0.12)],
 # Chorus 1 holds back the floor, the bars, the lamps, white and the fast moves.
 "Chorus": faces(0.95) + dark(FL, BR, AU) + [st(W, 0.60, "Accent"), st(BK, 0.74, "Accent"), st(FI, 0.45, "Accent"),
          st(MV, 0.70, "Primary", "Audience"), chords(FI, 0.22, cool=False), snare(BK, 0.14),
          fx(MV, "windmill", 2, "mirror", 0.6)],
 # Home again, in the other cold colour, with the band still driving under it.
 "Verse 2": faces(0.80) + dark(BR, AU) + [st(W, 0.42, "Tertiary"), st(BK, 0.45, "Primary"), st(FI, 0.38, "Tertiary"),
          st(FL, 0.18, "Primary"), st(MV, 0.32, "Tertiary", "Stage"), verse_chords(BK, 0.16), snare(FL, 0.10)],
 # Chorus 1 and two additions: the floor, and the lamps glinting through it.
 "Chorus 2": faces(1.00) + dark(BR) + [st(W, 0.78, "Accent"), st(BK, 0.90, "Accent"), st(FL, 0.62, "Accent"),
          st(FI, 0.58, "Accent"), lamps(0.14), st(MV, 0.85, "Primary", "Audience"), kick(FL, 0.18),
          snare(AU, 0.30), chords(BK, 0.16), fx(MV, "cross", 2, None, 0.7)],
 # The chart: a keytar solo, so the pool is centre (Key up, the guitarists'
 # pars down). A pool on the player and embers everywhere else: amber wall, amber fill
 # guttering, the lamps glowing low. No wash, no floor: the last chorus has
 # somewhere to come from.
 "Guitar Solo": [st(K, 0.95, "White"), st(SP, 0.30, "White"), st(BK, 0.36, "Accent 2"), st(FI, 0.36, "Accent 2"), lamps(0.16)]
          + dark(W, FL, BR) + [st(MV, 0.36, "White", "Band"),
          chords(BK, 0.16), fx(FI, "fire flicker", 2, None, 0.5), fx(AU, "fire flicker", 4, "scatter", 0.8),
          fx(MV, "circle tight", 2, "pairs", 0.6)],
 # The last chorus: the only cue at full, and everything held back is spent.
 "Chorus 3": faces(1.00) + [st(W, 1.00, "Accent"), st(BK, 1.00, "Accent"), st(FL, 0.80, "Open White"), st(BR, 0.78, "Tertiary"),
          st(FI, 1.00, "Accent"), lamps(0.50), st(MV, 1.00, "Open White", "Audience"),
          snare(AU, 0.45), kick(FL, 0.20), chords(BK, 0.14), fx(MV, "ballyhoo", 1, "scatter", 0.9)],
 # A voice and a piano: one face, the wall, the lamps like candles.
 "DownChorus": [st(K, 0.80, "White"), st(SP, 0.35, "White")] + dark(W, FL, BR) + [st(BK, 0.22, "Secondary"),
          st(FI, 0.15, "Primary"), lamps(0.16), st(MV, 0.18, "Secondary"),
          fx(AU, "wave", 2, "descend", 0.7)],     # a soft wave climbing the lamps of every tower, once every two bars
 # Sixteen bars of everything arriving: the wash filling, the fill closing in, the beams climbing.
 "Build": faces(0.90) + dark(BR) + [st(W, 0.55, "Tertiary"), st(BK, 0.60, "Secondary"), st(FL, 0.45, "Tertiary"),
          st(FI, 0.50, "Tertiary"), lamps(0.18), st(MV, 0.60, "Tertiary", "Stage"),
          fx(MV, "tilt dim", 2, None, 0.8)],
 # The band alone, with the singer talking over it (the chart): faces stay up. The chorus's colour once more, wide and rolling, under the last chorus's level.
 "Outro": [st(K, 0.95, "White"), st(SP, 0.95, "White"), st(W, 0.80, "Accent"), st(BK, 0.85, "Accent"), st(FL, 0.55, "Primary"),
          st(BR, 0.45, "Primary"), st(FI, 0.75, "Accent"), lamps(0.30), st(MV, 0.80, "Primary", "Audience"),
          snare(AU, 0.32), kick(FL, 0.15), fx(MV, "fly out", 4, None, 0.7)],
}
pre2 = faces(0.90) + dark(BR) + [st(W, 0.56, "Primary"), st(BK, 0.68, "Secondary"), st(FL, 0.40, "Secondary"),
          st(FI, 0.55, "Secondary"), lamps(0.18), st(MV, 0.50, "Secondary", "Stage")]
beat = bar_secs(0.25)
fades = {"Count In": 0.0, "Intro": bar_secs(2), "Verse 1": bar_secs(1), "PreChorus": bar_secs(1), "Chorus": beat / 2,
         "Verse 2": bar_secs(1.5), "PreChorus (2)": bar_secs(1), "Chorus 2": beat / 2, "Guitar Solo": beat / 2,
         "Chorus 3": 0.0, "DownChorus": bar_secs(2), "Build": bar_secs(12), "Outro": beat / 2}

# Where the music turns. (new name, the cue of an earlier pass it replaces, at, fade, parts)
punt = [{"look": "punt"}]
BLOCKS = ("safe", "End", "reset")
subs = [
 ("· pre riser", "· pre turn", "PreChorus+7", beat, [st(BK, 0.85, "Accent"), st(FI, 0.70, "Accent"),
          {"target": "Beams", "effect": "strobe riser", "bars": 1}]),
 ("· stop", "· stop", "Chorus+14", 0.0, dark(W, FL, MV, FI) + [st(BK, 0.15, "Accent")]),             # bar 41: the kit drops out
 ("· pre 2 riser", "· pre 2 turn", "PreChorus#2+7", beat, [st(BK, 0.90, "Accent"), st(FI, 0.75, "Accent"), lamps(0.30),
          {"target": "Beams", "effect": "strobe riser", "bars": 1}]),
 ("· Chorus 2 lift", "· Chorus 2 lift", "Chorus 2+8", beat, [st(MV, 0.90, "White", "Audience"), st(FL, 0.80, "White"), lamps(0.32),
          fx(MV, "pan fan", 2, None, 0.8)]),
 ("· solo up", "· solo up", "Guitar Solo+8", beat, [st(BK, 0.80, "Accent 2"), st(BR, 0.60, "Accent 2"), st(W, 0.30, "Accent 2"),
          lamps(0.32), fx(MV, "counter circle", 2, None, 0.7)]),
 ("· breath", "· Chorus lift", "Guitar Solo+15", 0.0, faces(0.70) + dark(W, FL, BR, MV, FI, AU) + [st(BK, 0.12, "Primary")]),
 # The second half of the last chorus: the fill goes white and moves with the chords, the bars throw
 # comets, the lamps chase. Each layer is stated again so the first half's effect on it ends here.
 ("· Chorus 3 lift", "· Chorus 3 lift", "Chorus 3+8", beat, [st(FI, 1.00, "White"), st(BR, 1.00, "Tertiary"),
          st(FL, 0.80, "Open White"), lamps(0.70), st(MV, 1.00, "Open White", "Audience"),
          fx(MV, "can can", 0.5, None, 1.0),
          st(W, 1.00, "Accent")]),
 # The chart and the kit agree: the band plays the b7 through bars 105 and 106 and is out on three of 107.
 ("· ring out", "· ring out", "Chorus 3+14", beat, [st(BK, 1.00, "Tertiary"), st(MV, 1.00, "White", "Stage")]),
 ("· band out", None, "107.3", 0.15, faces(0.55) + dark(W, FL, BR, MV, FI) + [st(BK, 0.20, "Secondary"), lamps(0.30),
          fx(AU, "drain and hold", 1, "descend", 1.0)]),   # the lamps empty from the top down as the band rings out
 # Inside the sections: the second line of a verse, the b7 that ends a pre-chorus, the second half of a chorus.
 # The intro's second line, then its last two bars: the 2m and the walk-up, rising into the verse.
 ("· intro line 2", None, "Intro+4", bar_secs(1), [st(FI, 0.26, "Secondary"), lamps(0.20)]),
 # A held 2m for a bar and a half, then the two stabs of the walk-up: so the look arrives and holds
 # still, and the stabs (a figure, placed from the chart) are the only thing that moves.
 ("· intro walk", None, "Intro+6", 2 * beat, [st(BK, 0.48, "Primary"), st(W, 0.22, "Primary"), st(MV, 0.18, "Secondary")]),
 # The verses are chill: a chord change there is a small change of state that blooms over two beats
 # and holds, never a flash. Home (the 4) is the section's look; the pushed 5 lifts a touch; the 1,
 # the chord the line resolves to, opens up the most; the pushed 3m7 leans darker and cooler.
 *[cue for line, bar in ((1, 11), (2, 15)) for cue in (
     (f"· v1 line {line} · 5", None, {"bar": bar, "beat": 4.5}, 2 * beat, [st(BK, 0.56, "Secondary"), st(FI, 0.40, "Secondary" if line == 1 else "Tertiary")]),
     (f"· v1 line {line} · 1", None, {"bar": bar + 2, "beat": 1}, 2 * beat, [st(W, 0.46, "Primary"), st(BK, 0.60, "Tertiary"), st(FI, 0.46, "Tertiary")]),
     (f"· v1 line {line} · 3m7", None, {"bar": bar + 2, "beat": 4.5}, 2 * beat, [st(W, 0.30, "Primary"), st(BK, 0.40, "Primary"), st(FI, 0.30, "Secondary")]),
     (f"· v1 line {line} · home", None, {"bar": bar + 4, "beat": 1}, 2 * beat, [st(W, 0.38, "Primary"), st(BK, 0.50, "Secondary"), st(FI, 0.35 if line == 1 else 0.45, "Secondary" if line == 1 else "Tertiary")]),
 )][:-1],
 *[cue for line, bar in ((1, 43), (2, 47)) for cue in (
     (f"· v2 line {line} · 5", None, {"bar": bar, "beat": 4.5}, 2 * beat, [st(BK, 0.52, "Primary"), st(FI, 0.44, "Tertiary")]),
     (f"· v2 line {line} · 1", None, {"bar": bar + 2, "beat": 1}, 2 * beat, [st(W, 0.52, "Tertiary"), st(BK, 0.58, "Secondary"), st(FI, 0.50, "Secondary")]),
     (f"· v2 line {line} · 3m7", None, {"bar": bar + 2, "beat": 4.5}, 2 * beat, [st(W, 0.32, "Tertiary"), st(BK, 0.36, "Primary"), st(FI, 0.30, "Tertiary")]),
     (f"· v2 line {line} · home", None, {"bar": bar + 4, "beat": 1}, 2 * beat, [st(W, 0.42, "Tertiary"), st(BK, 0.45, "Primary"), st(FI, 0.38, "Tertiary")]),
 )][:-1],
 ("· verse line 2", None, "Verse 1+4", bar_secs(1), [st(FI, 0.45, "Tertiary"), st(MV, 0.30, "Secondary", "Stage")]),
 ("· verse 2 line 2", None, "Verse 2+4", bar_secs(1), [st(FL, 0.30, "Primary"), st(MV, 0.36, "Secondary", "Band")]),
 # The b7 that ends a pre-chorus is two bars of climbing: the towers' lamps fill from the bottom
 # lamp to the top one over exactly those two bars, and are full where the chorus lands.
 ("· pre b7", None, "PreChorus+6", beat, [st(BK, 0.80, "Accent"), st(W, 0.36, "Primary"), lamps(0.45),
          fx(AU, "fill and hold", 2, "climb", 1.0)]),
 ("· pre 2 b7", None, "PreChorus#2+6", beat, [st(BK, 0.78, "Accent"), st(W, 0.40, "Primary"), lamps(0.55),
          fx(AU, "fill and hold", 2, "climb", 1.0)]),
 # The intro's walk-up: the lamps sweep up the towers across the two stabs (beat three to beat four).
 ("· intro stabs", None, {"bar": 10, "beat": 3}, 0.0, [lamps(0.42), fx(AU, "fill and hold", 0.25, "climb", 1.0)]),
 ("· chorus half", None, "Chorus+8", beat, [st(FI, 0.55, "Accent"), st(MV, 0.75, "Tertiary", "Audience")]),
 ("· chorus 2 b7", None, "Chorus 2+14", beat, [st(BK, 0.95, "Tertiary"), st(MV, 0.90, "Tertiary", "Stage")]),
 ("· down line 2", None, "DownChorus+8", bar_secs(2), [st(FI, 0.24, "Primary"), lamps(0.16)]),
 # The chart: the band comes in at the top of the build with no drums, and the drums at bar 13.
 ("· build band", None, "Build+4", bar_secs(1), [st(FL, 0.55, "Tertiary"), st(FI, 0.60, "Tertiary")]),
 ("· build 8", None, "Build+8", bar_secs(1), [st(BR, 0.35, "Tertiary"), st(W, 0.68, "Tertiary"), lamps(0.26)]),
 ("· outro half", None, "Outro+8", beat, [st(MV, 0.90, "Tertiary", "Audience"), st(BR, 0.60, "Primary")]),
 ("· build top", "· outro flip", "Build+12", bar_secs(1), [st(BK, 0.95, "Accent"), st(BR, 0.60, "Tertiary"), st(FI, 0.80, "Accent"),
          lamps(0.35)]),
 ("· button", "· voice back", "Outro+16", 0.0, faces(0.70) + [st(W, 0.90, "White"), st(BK, 0.90, "Accent"), st(FL, 0.80, "White"),
          st(FI, 0.90, "White"), lamps(0.90), st(MV, 0.80, "White", "Audience")]),
 ("End", "End", {"section": "Outro", "last_bar": True}, bar_secs(2), dark(K, SP, FI, W, FL, BR, MV, AU) + [st(BK, 0.10, "Primary")]),
 ("safe", "· down 2", "158.3", 1.0, punt),      # the look to fall back on, kept out of the count-in
 ("reset", "· riser", "158.4", 1.0, punt)]

# Accents: one a bar at most. A landing or a crash on the one is a white pop on
# the wash; the pickup out of a fill is a level bump on the bars.
hit = lambda bar, bt, k: H.get((bar, bt, k), 0.0)
lands = [secs[n][0] for n in ("Chorus", "Chorus 2", "Guitar Solo", "Chorus 3", "Outro")]
fills = [26, 42, 50, 58, 66, 74, 82, 90, 98, 104, 140, 148, 154]
quiet = set(range(*secs["DownChorus"])) | set(range(secs["Build"][0], secs["Build"][1])) | {41, 106, 108}
markers = []
for bar in range(secs["Verse 1"][0], 158):
    if bar in quiet or bar in (90,): continue                       # the breath before the last chorus stays clean
    in_verse = any(secs[n][0] <= bar <= secs[n][1] for n in ("Verse 1", "Verse 2"))
    if bar == secs["Chorus 3"][0]:
        markers.append({"kind": "drop", "bar": bar, "beat": 1})                   # the blinders, once, where the last chorus lands
    elif bar in lands or bar == 107:                                  # the last hit before the band is out
        markers.append({"kind": "land", "bar": bar, "beat": 1})
    elif bar in fills:
        best = max(((max(hit(bar, bt, "Toms"), hit(bar, bt, "Snare"), hit(bar, bt, "Cymbals")), bt) for bt in (3, 3.5, 4, 4.5)))
        if best[0] > 0.5 and not (in_verse and bar % 2): markers.append({"kind": "pickup", "bar": bar, "beat": best[1]})
    elif hit(bar, 1, "Cymbals") > 0.6 and (bar - 27) % 4 == 2 and not in_verse:
        markers.append({"kind": "crash", "bar": bar, "beat": 1})                  # the middle of each four-bar phrase
for bar in (155, 156, 157):
    markers = [m for m in markers if m["bar"] != bar]
markers.append({"kind": "button", "bar": 157, "beat": 1})                        # the last crash

# The chart, as chord events: (bar of the section from 0, beat, chord, pushed).
def line_a(o): return [(o, 1, "4", 0), (o, 4.5, "6m", 1), (o + 1, 3, "5", 0)]
def line_b(o): return [(o, 1, "4", 0), (o, 4.5, "6m", 1), (o + 1, 2.5, "5", 1)]
walk = lambda o: [(o, 1, "2m", 0), (o + 1, 3, "2m", 0), (o + 1, 3.5, "1/3", 1)]
form16 = line_a(0) + line_b(2) + line_a(4) + walk(6) + line_a(8) + line_b(10) + line_a(12) + [(14, 1, "b7", 0)]
verse8 = [e for o in (0, 4) for e in [(o, 1, "4", 0), (o, 4.5, "5", 1), (o + 2, 1, "1", 0), (o + 2, 4.5, "3m7", 1)]]
pre8 = line_a(0) + line_a(2) + line_a(4) + [(6, 1, "b7", 0)]
intro8 = [(0, 1, "4", 0), (0, 4.5, "6m", 1), (2, 1, "4", 0), (2, 4.5, "6m", 1), (3, 3, "5", 0)] + line_a(4) + walk(6)
forms = {"Intro": intro8, "Verse 1": verse8, "Verse 2": verse8, "PreChorus": pre8, "PreChorus#2": pre8, "Chorus": form16,
         "Chorus 2": form16, "Guitar Solo": form16, "Chorus 3": form16 + [(16, 1, "b7", 0)], "DownChorus": form16,
         "Build": form16, "Outro": form16}
for name, form in forms.items():
    for off, bt, chord, pushed in form:
        markers.append({"kind": "push" if pushed else "chord", "name": chord, "bar": secs[name][0] + off, "beat": bt,
                        "strength": 1.0 if pushed else 0.6})
# The walk-up that ends a line (2m as an eighth on three, 1/3 on the and of three): two stabs.
for name, form in forms.items():
    for off, bt, chord, pushed in form:
        if chord == "1/3":
            markers.append({"kind": "stab", "name": "walk-up", "bar": secs[name][0] + off, "beat": 3, "strength": 0.8})
            markers.append({"kind": "stab", "name": "walk-up", "bar": secs[name][0] + off, "beat": 3.5, "strength": 1.0})
# The kit, from the stems: every snare and kick that is really there, with how hard it was hit, so a
# show can answer the big ones one way and a light groove another (or not at all).
for (bar, bt, piece), v in sorted(H.items()):
    if piece == "Snare" and v >= 0.3: markers.append({"kind": "snare", "bar": bar, "beat": bt, "strength": round(min(v / 1.5, 1.0), 2)})
    if piece == "Kick" and v >= 0.3: markers.append({"kind": "kick", "bar": bar, "beat": bt, "strength": round(min(v / 1.5, 1.0), 2)})

# A walk-up and the fill under it are one moment: where the chart has the stabs, the drums' pickup is not a second accent.
stab_bars = {m["bar"] for m in markers if m["kind"] == "stab"}
markers = [m for m in markers if not (m["kind"] == "pickup" and m["bar"] in stab_bars)]

steps = []
def step(action, **args): steps.append({"action": "IGNITION_" + action, "args": args})
def sstep(action, **args): step(action, song=SONG, **args)
step("LIBRARY_CREATE", kind="palettes", name="Know Me Night", replace=True, entry={
    "about": "Know Me: deep blue and turquoise at home, magenta for the choruses, amber for the solo, one warm white for faces",
    "slots": {"Primary": "Deep Blue", "Secondary": "Turquoise", "Tertiary": "Cyan", "Accent": "Magenta", "Accent 2": "Amber", "White": "Warm White"}})
# Two generic ways to spread an effect, for any room with lights one above another: by real height.
step("LIBRARY_CREATE", kind="tricks", name="climb", replace=True, entry={
    "about": "phase by real height, the lowest lights first: a fill or a one-shot runs bottom to top (a running wave is seen travelling down)", "tricks": [],
    "phase": {"from": 0, "to": 0}, "phase_z": {"from": 0, "to": 300}})
step("LIBRARY_CREATE", kind="tricks", name="descend", replace=True, entry={
    "about": "phase by real height, the highest lights first: a fill or a one-shot runs top to bottom (a running wave is seen travelling up)", "tricks": [],
    "phase": {"from": 0, "to": 0}, "phase_z": {"from": 300, "to": 0}})
sstep("PALETTE_USE", palette="Know Me Night")
looks = {k: [p for p in v if p] for k, v in looks.items()}
pre2 = [p for p in pre2 if p]
subs = [(n, o, at, f, [p for p in parts if p]) for n, o, at, f, parts in subs]
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
# The show: which events this show answers, with what, where. Texture first
# (small, level only), then the accents an audience would point at.
def hits(label, kind, target, bump, depth, section=None, least=0.0):
    args = dict(target=target, kind=kind, bump=bump, depth=depth, name=label, replace=True, min_strength=least)
    if section: args["section"] = section
    sstep("FILL_HITS", **args)
hits("snare", "snare", AU, "Level", 0.40, ["Chorus 3"], 0.4)                 # the lamps take the snare when it is big
hits("snare-c2", "snare", AU, "Level", 0.30, ["Chorus 2", "Outro"], 0.4)
hits("snare-wall", "snare", BK, "Level", 0.20, ["Chorus", "Guitar Solo"], 0.4)   # held back: only the wall
hits("snare-floor", "snare", FL, "Level", 0.14, ["Verse 2", "PreChorus#2"], 0.4)
hits("kick", "kick", FL, "Level", 0.22, ["Chorus 2", "Chorus 3", "Outro"], 0.33)  # the floor takes the kick
hits("push", "push", FI, "Level", 0.30, ["PreChorus", "PreChorus#2", "Chorus", "Chorus 2", "Guitar Solo", "Chorus 3", "Build", "Outro"])
hits("chord", "chord", BK, "ColorBoost", 0.14, ["PreChorus", "PreChorus#2", "Chorus 2", "Chorus 3", "DownChorus", "Build", "Outro"])
# The intro has no kit to follow, so it follows the harmony: the wall swells on each chord, the
# colour pars and the lamps glint on the pushed ones, and the walk-up steps up the lamps into the verse.
# Under the intro, the first verse and the first pre-chorus there is a light groove from a drum
# machine: the wall barely moves with its kick and the colour pars with its snare.
hits("groove-kick", "kick", BK, "Level", 0.10, ["Intro", "Verse 1", "PreChorus"])
hits("groove-snare", "snare", FI, "Level", 0.14, ["Intro", "Verse 1", "PreChorus"])
hits("chord-intro", "chord", BK, "ColorBoost", 0.24, ["Intro"])
hits("push-fill", "push", FI, "Level", 0.30, ["Intro", "DownChorus"])
hits("push-lamps", "push", AU, "Level", 0.26, ["Intro", "DownChorus"])
hits("stab", "stab", AU, "Level", 0.30)
hits("stab-fill", "stab", FI, "Level", 0.30, ["Intro", "DownChorus", "Build"])
# The towers do nothing on their own. They move together, with the kick, in the choruses.
hits("kick-wash", "kick", W, "Level", 0.20, ["Chorus", "Chorus 2", "Chorus 3", "Outro"], 0.33)
hits("fig walk", "stab", W, "Level", 0.55, ["Intro", "Chorus 2", "Guitar Solo", "Chorus 3", "Build", "Outro"])   # the walk-up, as one figure
hits("kick-bars", "kick", BR, "Level", 0.25, ["Chorus 3"], 0.33)        # the bars take the kick too in the last chorus
hits("button", "button", AU, "White", 1.0)
hits("chorus", "land", W, "White", 0.9)
hits("crash", "crash", W, "White", 0.6)
hits("fill", "pickup", BR, "Level", 0.6)
hits("drop", "drop", AU, "White", 1.0)
print(json.dumps({"label": "Know Me: the trees in colour, the towers' lamps, an effect on every layer", "steps": steps}))
sys.stderr.write("events: %s\n" % {k: sum(1 for m in markers if m["kind"] == k) for k in sorted({m["kind"] for m in markers})})
