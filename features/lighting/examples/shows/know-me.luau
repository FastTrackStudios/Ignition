-- Know Me (Extended), second pass (2026-11-19), from the owner's notes:
-- nothing changes on the quarter note; the looks move on phrases (2, 4 and
-- 8 bars) and change on section boundaries; the tree pars carry colour and
-- change with the song; the bars are bright enough to read. One undo step.
local song = "know-me-extended"
local function run(id, a) a.song = a.song or song; return actions.run("IGNITION_" .. id, a) end
local function ask(id, a) a.song = a.song or song; return state.query("IGNITION_" .. id, a) end

actions.run("IGNITION_EDIT_BEGIN", { label = "Know Me: looks on phrases, the trees in colour" })

actions.run("IGNITION_LIBRARY_CREATE", { kind = "palettes", name = "Know Me Night", replace = true, entry = {
  about = "Know Me: deep blue and turquoise beds, magenta for the choruses, amber for the solo",
  slots = { Primary = "Deep Blue", Secondary = "Turquoise", Tertiary = "Cyan", Accent = "Magenta", ["Accent 2"] = "Amber", White = "Open White" },
} })
run("PALETTE_USE", { palette = "Know Me Night" })

-- A static part, and a moving one: `bars` is how long one pass takes.
local function st(target, dimmer, color, focus) return { target = target, dimmer = dimmer, color = color, focus = focus } end
local function fx(target, effect, bars, tricks, depth) return { target = target, effect = effect, bars = bars, tricks = tricks, depth = depth } end
-- Targets: the singer's two pars, the band's six, the towers behind (Wash),
-- the bars on the curtain (Back), the bars at the front (Floor), the movers.
local S, B, T, W, F, M = "Key Singer", "Key Band", "Wash", "Back", "Floor", "Movers"
local off = function(t) return st(t, 0.0) end

local looks = {
  ["Count In"] = { st(S, 0.25, "White"), off(B), st(T, 0.08, "Primary"), st(W, 0.35, "Primary"), off(F), off(M) },
  ["Intro"] = { st(S, 0.55, "White"), st(B, 0.35, "Secondary"), st(T, 0.30, "Primary"), st(W, 0.80, "Primary"),
    st(F, 0.35, "Secondary"), st(M, 0.45, "Secondary", "Band"),
    fx(T, "breathe", 8, "centre out", 0.5), fx(M, "circle breathe", 8, "mirror", 0.6) },
  ["Verse 1"] = { st(S, 0.85, "White"), st(B, 0.45, "Primary"), st(T, 0.25, "Secondary"), st(W, 0.70, "Secondary"),
    st(F, 0.45, "Primary"), st(M, 0.30, "Primary", "Stage"),
    fx(T, "wave", 8, "centre out", 0.5), fx(M, "sway", 8, "mirror", 0.5) },
  ["PreChorus"] = { st(S, 0.90, "White"), st(B, 0.65, "Accent"), st(T, 0.45, "Primary"), st(W, 0.90, "Accent"),
    st(F, 0.60, "Accent"), st(M, 0.70, "Primary", "Audience"),
    fx(T, "build", 8, "ends in", 0.8), fx(M, "tilt fan", 4, "fan from centre", 0.7) },
  ["Chorus"] = { st(S, 1.00, "White"), st(B, 0.95, "Accent"), st(T, 0.85, "Primary"), st(W, 1.00, "Accent"),
    st(F, 0.90, "Primary"), st(M, 1.00, "Accent", "Audience"),
    fx(T, "wave", 2, "mirror", 0.6), fx(W, "comet", 2, nil, 0.8), fx(M, "pan fan", 4, "mirror", 0.8) },
  ["Guitar Solo"] = { st(S, 0.30, "White"), st(B, 0.50, "Accent 2"), st("Key Guitar", 1.00, "White"),
    st(T, 0.45, "Accent 2"), st(W, 0.90, "Accent 2"), st(F, 0.70, "Accent 2"), st(M, 1.00, "White", "Band"),
    fx(T, "breathe", 4, "halves", 0.6), fx(W, "comet", 4, nil, 0.8), fx(M, "circle tight", 4, "pairs", 0.7) },
  ["DownChorus"] = { st(S, 0.85, "White"), st(B, 0.15, "Primary"), st(T, 0.12, "Secondary"), st(W, 0.45, "Secondary"),
    st(F, 0.20, "Primary"), st(M, 0.25, "Secondary", "Band"),
    fx(W, "breathe", 8, nil, 0.5), fx(M, "sway", 8, "mirror", 0.4) },
  ["Build"] = { st(S, 0.90, "White"), st(B, 0.60, "Tertiary"), st(T, 0.50, "Tertiary"), st(W, 0.90, "Accent"),
    st(F, 0.60, "Tertiary"), st(M, 0.80, "Tertiary", "Audience"),
    fx(T, "build", 16, "ends in", 0.9), fx(B, "build", 16, nil, 0.6), fx(M, "tilt wave", 4, "centre out", 0.8) },
  ["Outro"] = { st(S, 1.00, "White"), st(B, 0.95, "Accent"), st(T, 0.90, "Tertiary"), st(W, 1.00, "Accent"),
    st(F, 0.90, "White"), st(M, 1.00, "White", "Audience"),
    fx(T, "wave", 2, "mirror", 0.7), fx(W, "comet", 2, nil, 0.9), fx(M, "figure eight", 4, "mirror", 0.9) },
}
looks["Verse 2"] = { st(S, 0.85, "White"), st(B, 0.50, "Tertiary"), st(T, 0.30, "Primary"), st(W, 0.75, "Tertiary"),
  st(F, 0.45, "Secondary"), st(M, 0.35, "Tertiary", "Stage"),
  fx(T, "wave", 8, "ends in", 0.5), fx(M, "sway", 8, "mirror", 0.5) }
looks["Chorus 2"], looks["Chorus 3"] = looks["Chorus"], looks["Chorus"]
run("FILL_SECTIONS", { looks = looks, replace = true })

-- The second half of each chorus: the same shape in another colour, wider.
local lift = { st(S, 1.00, "White"), st(B, 0.95, "Tertiary"), st(T, 0.95, "Accent"), st(W, 1.00, "Tertiary"),
  st(F, 0.95, "White"), st(M, 1.00, "White", "Audience"),
  fx(T, "wave", 2, "centre out", 0.7), fx(W, "comet", 2, nil, 0.9), fx(M, "figure eight", 4, "mirror", 0.9) }
-- (Removing things asks a person: the lifts are rewritten in place by
-- know-me-lifts.json, run after this, instead of deleted and added.)
LIFT = lift
-- The last bar: out, slowly.
run("CUE_ADD", { name = "End", at = { section = "Outro", last_bar = true }, fade_secs = 3.5,
  parts = { st(S, 0.2, "White"), off(B), off(T), st(W, 0.15, "Primary"), off(F), off(M) } })

-- One accent per chorus: the towers flash white where it lands.
local markers = {}
for _, s in ipairs(ask("SONG_GET", {}).sections) do
  local n = string.lower(s.name)
  if (string.find(n, "chorus") and not string.find(n, "down") and not string.find(n, "pre")) or n == "outro" then
    markers[#markers + 1] = { kind = "hit", name = s.name .. " lands", bar = s.bar, beat = 1 }
  end
end
-- (Clearing triggers asks a person; one silent marker lets the old 132 be
-- replaced by a single trigger of no depth instead.)
markers[#markers + 1] = { kind = "snare", bar = 1, beat = 1 }
run("SONG_MAP_SET", { markers = markers, replace_markers = true })
-- The backbeat accents of the first pass go.
run("FILL_HITS", { target = W, kind = "snare", name = "snare", depth = 0.0, replace = true })
local hits = run("FILL_HITS", { target = T, kind = "hit", bump = "White", depth = 0.9, name = "chorus", replace = true })
actions.run("IGNITION_EDIT_COMMIT", {})

local rows = {}
for _, c in ipairs(ask("SONG_GET", {}).cues) do rows[#rows + 1] = string.format("bar %3d  %-18s %d parts", c.bar or 0, c.name, #c.parts) end
return { cues = rows, hits = hits.added, problems = state.query("IGNITION_SET_CHECK", {}).problems }
