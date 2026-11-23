-- Programs one song from its section map with the library of 01-library.luau:
-- a look per section by what the section is, an effect on the choruses, a
-- wave on the movers, accents on its snare markers. Reviewable first
-- (dry_run), then written, as one undo step.
local show = args and args.show
local song = (args and args.song) or "Fire"
local function run(id, a) a.show = a.show or show; a.song = a.song or song; return actions.run("IGNITION_" .. id, a) end
local function ask(id, a) a.show = a.show or show; a.song = a.song or song; return state.query("IGNITION_" .. id, a) end

local before = ask("SONG_GET", {})
local function part(target, dimmer, color) return { target = target, dimmer = dimmer, color = color } end
local looks = {
  intro  = { part("Key", 0.45, "White"), part("Wash", 0.30, "Secondary"), part("Back", 0.60, "Secondary"), part("Movers", 0.0) },
  verse  = { part("Key", 0.75, "White"), part("Wash", 0.40, "Primary"),   part("Back", 0.50, "Secondary"), part("Movers", 0.25, "Tertiary") },
  pre    = { part("Key", 0.85, "White"), part("Wash", 0.60, "Primary"),   part("Back", 0.70, "Accent"),    part("Movers", 0.50, "Accent") },
  chorus = { part("Key", 1.00, "White"), part("Wash", 0.90, "Primary"),   part("Back", 0.90, "Accent"),    part("Movers", 0.85, "Primary"),
             { target = "Wash", effect = "slow breathe" },
             { target = "Movers", effect = "mover wave", tricks = "mirrored wave" } },
  bridge = { part("Key", 0.60, "White"), part("Wash", 0.35, "Tertiary"),  part("Back", 0.80, "Tertiary"),  part("Movers", 0.40, "Secondary"),
             { target = "Movers", effect = "mover wave", tricks = "chase", bars = 2 } },
  solo   = { part("Key", 0.90, "White"), part("Wash", 0.70, "Accent"),    part("Back", 0.80, "Primary"),   part("Movers", 0.80, "Accent"),
             { target = "Movers", effect = "mover wave", tricks = "blocks of 2" } },
  outro  = { part("Key", 0.50, "White"), part("Wash", 0.30, "Secondary"), part("Back", 0.40, "Secondary"), part("Movers", 0.0) },
  ["*"]  = { part("Key", 0.70, "White"), part("Wash", 0.50, "Primary"),   part("Back", 0.60, "Secondary") },
}

-- The plan, written nowhere: what each section would get.
local plan = run("FILL_SECTIONS", { looks = looks, replace = true, dry_run = true }).plan

actions.run("IGNITION_EDIT_BEGIN", { label = "program " .. song .. " from its sections" })
run("PALETTE_USE", { palette = "Sunset" })
run("FILL_SECTIONS", { looks = looks, replace = true, block = true })

-- Accents: a marker on beats 2 and 4 of every chorus bar (a stand-in for the
-- stems' snare hits, which the session primitive supplies), then a bump on each.
local markers = {}
for _, s in ipairs(ask("SONG_GET", {}).sections) do
  if string.find(string.lower(s.name), "chorus") then
    for bar = s.bar, s.bar + s.bars - 1 do
      markers[#markers + 1] = { kind = "snare", bar = bar, beat = 2 }
      markers[#markers + 1] = { kind = "snare", bar = bar, beat = 4 }
    end
  end
end
run("SONG_MAP_SET", { markers = markers, replace_markers = true })
local hits = run("FILL_HITS", { target = "Back", kind = "snare", bump = "Level", depth = 0.5, name = "snare", replace = true })

-- The last chorus is the first chorus, and hotter.
local choruses = {}
for _, row in ipairs(plan) do if row.kind == "chorus" then choruses[#choruses + 1] = row.section end end
if #choruses > 1 then
  run("SECTION_COPY", { from = choruses[1], to = choruses[#choruses] })
  run("SECTION_APPLY", { section = choruses[#choruses], parts = { { target = "Wash", dimmer = 1.0 }, { target = "Bars", dimmer = 0.9, color = "Accent" }, { target = "Bars", effect = "slot chase" } } })
end
actions.run("IGNITION_EDIT_COMMIT", {})

local after = ask("SONG_GET", {})
local rows = {}
for _, c in ipairs(after.cues) do
  rows[#rows + 1] = string.format("bar %3d  %-14s %d parts", c.bar or 0, c.name, #c.parts)
end
return { song = song, sections = #after.sections, cues_before = #before.cues, cues = rows, accents = hits.added, palette = after.palette }
