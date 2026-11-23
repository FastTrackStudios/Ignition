-- A small library for a set: colours, three palettes, a look, two effects
-- made from parameters, and three trick presets. One undo step.
--
-- Run it against any show (default: the recital):
--   <app> actions run SCRIPT_RUN --json '{"source": "<this file>"}'
local show = args and args.show
local function run(id, a) a.show = a.show or show; return actions.run("IGNITION_" .. id, a) end

actions.run("IGNITION_EDIT_BEGIN", { label = "library: palettes, effects, tricks" })

-- Colours the profile does not ship.
local colours = {
  Ember    = { red = 1.00, green = 0.28, blue = 0.02, tags = { "warm" }, folder = "Warm" },
  Rose     = { red = 1.00, green = 0.20, blue = 0.45, tags = { "warm" }, folder = "Warm" },
  Glacier  = { red = 0.35, green = 0.75, blue = 1.00, tags = { "cool" }, folder = "Cool" },
  Abyss    = { red = 0.02, green = 0.08, blue = 0.90, tags = { "cool" }, folder = "Cool" },
  Acid     = { red = 0.55, green = 1.00, blue = 0.05, tags = { "loud" }, folder = "Loud" },
  ["Hot Pink"] = { red = 1.00, green = 0.05, blue = 0.60, tags = { "loud" }, folder = "Loud" },
}
for name, entry in pairs(colours) do
  run("LIBRARY_CREATE", { kind = "colors", name = name, entry = entry, replace = true })
end

-- Palettes: the same slots, so any one re-skins a show written against another.
local palettes = {
  Sunset = { Primary = "Ember", Secondary = "Rose", Tertiary = "Amber", Accent = "Magenta", White = "Warm White" },
  Ice    = { Primary = "Glacier", Secondary = "Abyss", Tertiary = "Cyan", Accent = "Lavender", White = "Cool White" },
  Neon   = { Primary = "Hot Pink", Secondary = "Acid", Tertiary = "Cyan", Accent = "Purple", White = "Open White" },
}
for name, slots in pairs(palettes) do
  run("LIBRARY_CREATE", { kind = "palettes", name = name, replace = true,
    entry = { slots = slots, about = name .. ": five slots", tags = { "skin" }, folder = "Skins" } })
end

-- A look written against slots, not colours.
run("LIBRARY_CREATE", { kind = "looks", name = "slot wash", replace = true, entry = {
  kind = "Full", about = "faces white, the wash in Primary, the back in Secondary", tags = { "base" },
  recipes = {
    { target = { Role = "Key" },  steps = { { apply = { { Dimmer = 0.8 }, { Color = "White" } }, width = 1, transition = 0, ease = "Linear" } } },
    { target = { Role = "Wash" }, steps = { { apply = { { Dimmer = 0.6 }, { Color = "Primary" } }, width = 1, transition = 0, ease = "Linear" } } },
    { target = { Role = "Back" }, steps = { { apply = { { Dimmer = 0.7 }, { Color = "Secondary" } }, width = 1, transition = 0, ease = "Linear" } } },
  } } })

-- Effects from parameters: what moves, its shape, its rate in beats, its spread.
run("EFFECT_MAKE", { name = "slow breathe", target = "Wash", what = "dimmer", shape = "sine",
  beats = 8, depth = 0.25, about = "the wash swelling over two bars", tags = { "bed" }, replace = true })
run("EFFECT_MAKE", { name = "slot chase", target = "Bars", what = "color", shape = "square",
  colors = { "Primary", "Accent" }, beats = 2, spread_deg = 180, about = "Primary and Accent trading places", tags = { "drive" }, replace = true })
run("EFFECT_MAKE", { name = "mover wave", target = "Movers", what = "tilt", shape = "sine",
  beats = 4, depth = 18, about = "one tilt sine, in step: the tricks make it a chase or a wave", tags = { "movers" }, replace = true })

-- Trick presets (grandMA3: MAtricks). Any of them goes on any selection.
local tricks = {
  chase           = { about = "phase 0 to 360 across the selection: one effect becomes a chase", phase = { from = 0, to = 360 } },
  ["mirrored wave"] = { about = "two wings: the ends meet in the middle (MA Wings 2)", tricks = { "Mirror" }, phase = { from = 0, to = 360 } },
  ["blocks of 2"] = { about = "pairs act as one, chasing (MA Block 2)", tricks = { { Block = 2 } }, phase = { from = 0, to = 360 } },
  ["odds and evens"] = { about = "every second fixture, trading places (MA Groups 2)", tricks = { { Group = 2 } }, phase = { from = 0, to = 180 } },
  ["speeding up"] = { about = "each unit faster than the last: rate 1 to 2", phase = { from = 0, to = 90 }, speed = { from = 1, to = 2 } },
  wipe            = { about = "a static look arriving left to right: delay 0 to 2 beats", delay = { from = 0, to = 2 } },
}
for name, entry in pairs(tricks) do
  entry.tags = { "tricks" }
  run("LIBRARY_CREATE", { kind = "tricks", name = name, entry = entry, replace = true })
end

run("PALETTE_USE", { palette = "Sunset" })
actions.run("IGNITION_EDIT_COMMIT", {})

local now = state.query("IGNITION_LIBRARY_LIST", { show = show, layer = "set" })
local out = { palette = now.palette }
for kind, rows in pairs(now.entries) do
  local names = {}
  for _, r in ipairs(rows) do names[#names + 1] = r.name end
  table.sort(names)
  out[kind] = table.concat(names, ", ")
end
return out
