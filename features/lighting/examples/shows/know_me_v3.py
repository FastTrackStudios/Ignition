#!/usr/bin/env python3
"""Know Me (Extended), third pass: written from the song itself.

Reads the stem analysis (kick, snare, toms, cymbals on the eighth-note grid
and loudness per bar, from analyse2.py) and the cue design guide's rules
(docs/lighting/domain/cue-design-guide.md), and prints one IGNITION_BATCH:
a look per section with its fade, sub-cues where the music turns (the
pre-chorus's last bars, the stop in bar 41, the solo's second half, the
ring-out, the build's last two bars), and accents on what the drums play:
chorus landings, crash downbeats, the hits of each fill, a downbeat pulse.

  know_me_v3.py <raw2.json> <song.json>  > batch.json
"""
import json, sys
raw = json.load(open(sys.argv[1])); song = json.load(open(sys.argv[2]))
H = {(b, be, k): v for b, be, k, v in raw["hits"]}
have = {c["name"] for c in song["cues"]}
SONG = "know-me-extended"; BAR = 4 * 60 / 127.0
S, B, G, T, W, F, M = "Key Singer", "Key Band", "Key Guitar", "Wash", "Back", "Floor", "Movers"

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
off = lambda t: st(t, 0.0)

# Home is cool (deep blue, turquoise); the chorus owns magenta; amber is the
# solo's alone; white is spent at the peaks. Faces are white and never chased.
looks = {
 "Count In": [off(S), off(B), off(T), st(W, 0.20, "Primary"), off(F), off(M)],
 # Piano and a drum machine, no voice: no key, movers on the back wall.
 "Intro": [off(S), off(B), st(T, 0.22, "Primary"), st(W, 0.45, "Primary"), off(F),
           st(M, 0.30, "Secondary", "Back Wall"), fx(T, "breathe", 4, "centre out", 0.3)],
 # Home. The voice arrives: key on, everything else holds still.
 "Verse 1": [st(S, 0.65, "White"), st(B, 0.18, "Primary"), st(T, 0.30, "Primary"), st(W, 0.55, "Secondary"),
             off(F), st(M, 0.25, "Secondary", "Band"), fx(T, "breathe", 4, "halves", 0.25)],
 # One layer added and a level that climbs for eight bars; no new colour yet.
 "PreChorus": [st(S, 0.70, "White"), st(B, 0.30, "Primary"), st(T, 0.42, "Primary"), st(W, 0.70, "Secondary"),
               st(F, 0.35, "Secondary"), st(M, 0.45, "Secondary", "Stage"),
               fx(T, "build", 8, "ends in", 0.6), fx(M, "tilt fan", 8, "fan from centre", 0.5)],
 # The colour flips, the level jumps, the movers go out to the room. White,
 # the blinders and the fast moves are held back for later.
 "Chorus": [st(S, 0.75, "White"), st(B, 0.60, "Accent"), st(T, 0.62, "Primary"), st(W, 0.90, "Accent"),
            st(F, 0.50, "Primary"), st(M, 0.70, "Accent", "Audience"),
            fx(T, "wave", 2, "mirror", 0.5), fx(M, "pan fan", 2, "mirror", 0.6)],
 # Home again with the band still driving: one thing kept from the chorus.
 "Verse 2": [st(S, 0.68, "White"), st(B, 0.28, "Tertiary"), st(T, 0.35, "Primary"), st(W, 0.60, "Secondary"),
             st(F, 0.25, "Primary"), st(M, 0.35, "Secondary", "Band"),
             fx(T, "wave", 4, "mirror", 0.35)],
 "Chorus 2": [st(S, 0.78, "White"), st(B, 0.75, "Accent"), st(T, 0.75, "Primary"), st(W, 1.00, "Accent"),
              st(F, 0.75, "Tertiary"), st(M, 0.85, "Accent", "Audience"),
              fx(T, "wave", 2, "mirror", 0.6), fx(W, "comet", 2, None, 0.7), fx(M, "can can", 2, "mirror", 0.7)],
 # The third colour, the guitarist lit, the singer let go: there is no voice.
 "Guitar Solo": [st(S, 0.15, "White"), st(B, 0.30, "Accent 2"), st(G, 0.90, "White"), st(T, 0.50, "Accent 2"),
                 st(W, 0.90, "Accent 2"), st(F, 0.45, "Accent 2"), st(M, 0.90, "White", "Guitar 1"),
                 fx(T, "wave", 2, "ends in", 0.5), fx(M, "circle tight", 2, "pairs", 0.6)],
 "Chorus 3": [st(S, 0.80, "White"), st(B, 0.85, "Accent"), st(T, 0.85, "Primary"), st(W, 1.00, "Accent"),
              st(F, 0.85, "Tertiary"), st(M, 0.95, "Accent", "Audience"),
              fx(T, "wave", 2, "mirror", 0.7), fx(W, "comet", 2, None, 0.8), fx(M, "can can", 2, "mirror", 0.8)],
 # A voice and a piano: the darkest the stage has been since the top.
 "DownChorus": [st(S, 0.60, "White"), off(B), st(T, 0.05, "Primary"), st(W, 0.22, "Secondary"), off(F),
                st(M, 0.18, "Secondary", "Vocal Stage Left")],
 # Sixteen bars of one climb, arriving on the outro.
 "Build": [st(S, 0.75, "White"), st(B, 0.55, "Tertiary"), st(T, 0.55, "Tertiary"), st(W, 0.85, "Tertiary"),
           st(F, 0.50, "Tertiary"), st(M, 0.70, "Tertiary", "Stage"),
           fx(T, "build", 16, "ends in", 0.8), fx(M, "tilt wave", 4, "centre out", 0.6)],
 # The furthest from home: everything on, white in it, the fastest moves.
 "Outro": [st(S, 0.80, "White"), st(B, 1.00, "Accent"), st(T, 1.00, "White"), st(W, 1.00, "Accent"),
           st(F, 1.00, "White"), st(M, 1.00, "White", "Audience"),
           fx(T, "wave", 1, "mirror", 0.8), fx(W, "strip chase", 1, None, 0.9), fx(M, "ballyhoo", 2, "scatter", 0.9)],
}
fades = {"Count In": 0.0, "Intro": 2 * BAR, "Verse 1": 2 * BAR, "PreChorus": BAR, "Chorus": 0.0, "Verse 2": 2 * BAR,
         "PreChorus#2": BAR, "Chorus 2": 0.0, "Guitar Solo": 0.0, "Chorus 3": 0.0, "DownChorus": 2 * BAR,
         "Build": 14 * BAR, "Outro": 0.0}
looks["PreChorus#2"] = [st(S, 0.72, "White"), st(B, 0.38, "Tertiary"), st(T, 0.48, "Primary"), st(W, 0.75, "Secondary"),
                        st(F, 0.45, "Secondary"), st(M, 0.50, "Secondary", "Stage"),
                        fx(T, "build", 8, "ends in", 0.7), fx(M, "tilt fan", 8, "fan from centre", 0.6)]

# Sub-cues, only where the music turns. (name, at, fade, parts)
subs = [
 ("· pre turn", "PreChorus+6", BAR, [st(W, 0.9, "Accent"), st(F, 0.5, "Accent")]),            # the chorus colour, two bars early
 ("· Chorus lift", "Chorus+8", 0.5, [st(T, 0.70, "Tertiary"), st(F, 0.60, "Tertiary")]),
 ("· stop", "Chorus+14", 0.0, [off(B), off(T), st(W, 0.15, "Accent"), off(F), st(M, 0.0, "Accent", "Band")]),  # bar 41: the kit drops out
 ("· pre 2 turn", "PreChorus#2+6", BAR, [st(W, 0.95, "Accent"), st(F, 0.6, "Accent"), st(M, 0.6, "Accent", "Stage")]),
 ("· Chorus 2 lift", "Chorus 2+8", 0.5, [st(T, 0.85, "Tertiary"), st(F, 0.85, "White"), st(B, 0.8, "Tertiary")]),
 ("· solo up", "Guitar Solo+8", 0.5, [st(T, 0.70, "Accent 2"), st(F, 0.75, "White"), fx(W, "comet", 2, None, 0.8)]),
 ("· voice back", "Guitar Solo+13", BAR, [st(S, 0.75, "White")]),                               # the singer comes back in bar 88
 ("· Chorus 3 lift", "Chorus 3+8", 0.5, [st(T, 0.95, "Tertiary"), st(F, 0.95, "White"), st(B, 0.9, "Tertiary"),
                                          fx(M, "figure eight", 2, "mirror", 0.8)]),
 ("· ring out", "Chorus 3+14", 4 * BAR, [st(B, 0.10, "Primary"), st(T, 0.08, "Primary"), st(W, 0.30, "Secondary"),
                                         off(F), st(M, 0.15, "Secondary", "Band")]),             # bars 105-108: the kit is gone
 ("· down 2", "DownChorus+8", 4 * BAR, [st(T, 0.10, "Primary"), st(W, 0.30, "Secondary"), fx(T, "breathe", 8, "centre out", 0.4)]),
 ("· riser", "Build+14", 0.3, [st(T, 0.9, "White"), fx(T, "strobe", 0.5, None, 0.8), st(M, 1.0, "White", "Audience")]),  # the fill into the outro
 ("· outro flip", "Outro+8", 0.3, [st(B, 1.0, "Tertiary"), st(W, 1.0, "Tertiary"), fx(M, "ballyhoo", 1, "scatter", 1.0)]),
 ("End", {"section": "Outro", "last_bar": True}, 3 * BAR, [st(S, 0.2, "White"), off(B), off(T), st(W, 0.12, "Primary"), off(F), off(M)]),
]

# Accents, from what the drums play. One per bar at most outside the fills.
secs = {}
for s_ in song["sections"]:   # a name used twice: the second is "name#2"
    key = s_["name"] if s_["name"] not in secs else s_["name"] + "#2"
    secs[key] = (s_["bar"], s_["bar"] + int(s_["bars"]) - 1)
def inside(*names): return [b for n in names for b in range(secs[n][0], secs[n][1] + 1)]
hit = lambda bar, beat, k: H.get((bar, beat, k), 0.0)
markers = [{"kind": "snare", "bar": 1, "beat": 1}]   # inert: see the first pass
for n in ("Chorus", "Chorus 2", "Guitar Solo", "Chorus 3", "Outro"):
    markers.append({"kind": "land", "name": n + " lands", "bar": secs[n][0], "beat": 1})
fills = [26, 42, 50, 57, 58, 66, 73, 74, 81, 82, 89, 90, 97, 98, 104]   # bars the kit plays a fill (toms and snare busy)
for bar in fills:
    for i in range(8):
        beat = 1 + i / 2
        if max(hit(bar, beat, "Toms"), hit(bar, beat, "Snare")) > 0.5 or hit(bar, beat, "Cymbals") > 0.75:
            markers.append({"kind": "fill", "bar": bar, "beat": beat})
lands = {secs[n][0] for n in ("Chorus", "Chorus 2", "Guitar Solo", "Chorus 3", "Outro")}
crash = []
for bar in inside("Chorus", "Verse 2", "PreChorus#2", "Chorus 2", "Guitar Solo", "Chorus 3"):
    if bar in lands or bar in fills or bar >= 105: continue
    if hit(bar, 1, "Cymbals") > 0.6 and bar % 2 == 1:
        crash.append(bar); markers.append({"kind": "crash", "bar": bar, "beat": 1})
for bar in inside("Chorus 2", "Chorus 3"):
    if bar in lands or bar in fills or bar in crash or bar >= 105: continue
    if hit(bar, 1, "Kick") > 0.5: markers.append({"kind": "kick", "bar": bar, "beat": 1})

steps = []
def step(action, **args): steps.append({"action": "IGNITION_" + action, "args": {"song": SONG, **args}})
pre2 = looks.pop("PreChorus#2")
step("FILL_SECTIONS", looks=looks, replace=True)
step("CUE_UPDATE", cue="PreChorus (2)", set={"recipes": []})
step("CUE_UPDATE", cue="PreChorus (2)", parts=pre2)
for name, f in fades.items(): step("CUE_UPDATE", cue=name.replace("PreChorus#2", "PreChorus (2)"), set={"fade_secs": round(f, 3)})
for name, at, fade, parts in subs:
    if name in have:
        step("CUE_UPDATE", cue=name, at=at, set={"recipes": [], "fade_secs": round(fade, 3)})
        step("CUE_UPDATE", cue=name, parts=parts)
    else:
        step("CUE_ADD", name=name, at=at, fade_secs=round(fade, 3), parts=parts)
step("SONG_MAP_SET", markers=markers, replace_markers=True)
step("FILL_HITS", target=T, kind="snare", name="snare", depth=0.0, replace=True)
step("FILL_HITS", target=T, kind="land", bump="White", depth=1.0, name="chorus", replace=True)   # every landing: the towers, white
step("FILL_HITS", target=T, kind="crash", bump="ColorBoost", depth=0.6, name="crash", replace=True)
step("FILL_HITS", target=M, kind="fill", bump="Burst", depth=0.8, name="fill", replace=True)       # each hit of a fill: the movers
step("FILL_HITS", target=F, kind="kick", bump="Level", depth=0.4, name="kick", replace=True)       # a low pulse at the front
print(json.dumps({"label": "Know Me: written from the stems (landings, crashes, fills, the stop, the ring-out)", "steps": steps}))
sys.stderr.write("markers: %s\n" % {k: sum(1 for m in markers if m["kind"] == k) for k in ("land", "crash", "fill", "kick")})
