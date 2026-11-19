-- Re-skins every song of the set by swapping the palette: nothing in any
-- cue changes, only which palette each song's slots come from. One undo step.
local show = args and args.show
local palette = (args and args.palette) or "Ice"

local before = state.query("IGNITION_SET_CHECK", { show = show })
local done = actions.run("IGNITION_SET_EACH", { show = show, action = "IGNITION_PALETTE_USE", args = { palette = palette } })
local after = state.query("IGNITION_SET_CHECK", { show = show })

local songs = {}
for _, r in ipairs(done.results) do
  local primary = "?"
  for _, s in ipairs(r.result.slots) do
    if s.slot == "Primary" then primary = string.format("%.2f %.2f %.2f", s.red, s.green, s.blue) end
  end
  songs[#songs + 1] = string.format("%-28s %s  Primary = %s", r.song, r.result.palette, primary)
end
return { palette = palette, songs = songs, problems_before = before.problems, problems_after = after.problems }
