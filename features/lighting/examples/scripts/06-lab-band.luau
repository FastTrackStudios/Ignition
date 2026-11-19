-- Room 138 for the band as it will stand, and the rig moved to suit the
-- measured room. One undo step. Positions are metres: x audience-left (-)
-- to audience-right (+), y toward the curtain (+), the curtain at y = 1.7.
--
--   singer, standing at a keyboard, centre, facing the audience
--   two guitarists audience-right, in front of their amps
--   bass audience-left, drummer on the kit where the scan has it
local env = "room138-cbu"
local function run(id, a) a.env = env; return actions.run("IGNITION_" .. id, a) end

actions.run("IGNITION_EDIT_BEGIN", { label = "lab: the band as it stands, the rig to suit the room" })

-- The air and the camera. No hazer is patched in this room, so its air is
-- nearly clear: light shows on what it lands on, hardly in between.
run("ENV_LOOK", { haze = 0.12, exposure = 0 })

-- The band. PERSON_SET places these five. The two the room file had that
-- are not in this band (a keys player at the piano, a singing bassist) were
-- taken out by hand: removing is a destructive action, and from a script
-- the app asks a person first.
--   ignition-program IGNITION_ENV_PIECE_REMOVE '{"env":"room138-cbu","in":"props","name":"Person - Keys"}'
run("PERSON_SET", { people = {
  { name = "Singer",   label = "lead vocal, keyboard, standing", position = { x = 0.0,   y = -0.75, z = 0 }, facing_deg = 0, height = 1.78 },
  { name = "Guitar 1", label = "guitar",                         position = { x = 1.35,  y = -0.05, z = 0 }, facing_deg = 0 },
  { name = "Guitar 2", label = "guitar",                         position = { x = 2.40,  y = -0.25, z = 0 }, facing_deg = 0 },
  { name = "Bass",     label = "bass",                           position = { x = -1.90, y = 0.05,  z = 0 }, facing_deg = 0 },
  { name = "Drummer",  label = "drums",                          position = { x = -0.48, y = 1.23,  z = 0 }, facing_deg = 0, height = 1.35 },
} })
-- His keyboard in front of him, the mic at it.
run("ENV_PIECE_SET", { name = "Keyboard", ["in"] = "props", position = { x = 0.0, y = -1.15, z = 0 } })
run("ENV_PIECE_SET", { name = "Mic 1",    ["in"] = "props", position = { x = 0.0, y = -1.0,  z = 0 } })
run("FOCUS_SET", { name = "Vocal Centre", target = { x = 0.0, y = -0.75, z = 1.65 } })

-- Front light: the two trees come in from x = 2.9 to 2.2 and forward to
-- y = -3.6 (the tables are gone, and at 2.9 the stage-left tree shot
-- through the PA speaker). Each par is aimed at one player; the singer
-- and the bass get one from each side.
local function tree(first, x, players)
  for i, who in ipairs(players) do
    local at = ({ Singer = { 0.0, -0.75 }, ["Guitar 1"] = { 1.35, -0.05 }, ["Guitar 2"] = { 2.40, -0.25 }, Bass = { -1.90, 0.05 }, Drummer = { -0.48, 1.23 } })[who]
    run("FIXTURE_SET", { chan = first + i - 1,
      name = string.format("Tree %d Par %d — %s", first == 27 and 1 or 2, i, who),
      position = { x = x + (i - 2.5) * 0.3, y = -3.6, z = 2.44 },
      aim = { x = at[1], y = at[2], z = who == "Drummer" and 1.1 or 1.5 } })
  end
end
tree(27, -2.2, { "Bass", "Drummer", "Singer", "Guitar 1" })
tree(31, 2.2, { "Bass", "Singer", "Guitar 1", "Guitar 2" })

-- The towers stood at x = -5.49 (behind the piano's tail), -1.83, 1.83 and
-- 5.49 (inside the diffuser booth). They go where there is floor at the
-- curtain: two left of the kit, two right, between the amps.
local towers = { -3.60, -2.05, 0.75, 2.59 }
for t, x in ipairs(towers) do
  for row = 1, 6 do
    run("FIXTURE_SET", { chan = (t - 1) * 6 + row, position = { x = x, y = 1.52, z = 0.4572 + (row - 1) * 0.2134 } })
  end
end
-- The movers had nothing to hang from (the wall there is drape). They sit
-- on the floor, upright, one at the foot of each tower, below its rings.
for i, x in ipairs(towers) do
  run("FIXTURE_SET", { chan = 34 + i, position = { x = x, y = 1.15, z = 0.0 }, aim = { x = x, y = 1.15, z = 3.0 } })
end
-- The two bars that wash the curtain from its foot: one was in the booth.
run("FIXTURE_SET", { chan = 25, position = { x = -2.83, y = 1.58, z = 0.15 } })
run("FIXTURE_SET", { chan = 26, position = { x = 1.67,  y = 1.58, z = 0.15 } })
-- The two floor bars in front of the band, in to where the band now is.
run("FIXTURE_SET", { chan = 39, position = { x = -1.5, y = -2.1, z = 0.15 } })
run("FIXTURE_SET", { chan = 40, position = { x = 1.5,  y = -2.1, z = 0.15 } })
actions.run("IGNITION_EDIT_COMMIT", {})

local patch = state.query("IGNITION_PATCH_GET", { env = env })
local people = {}
for _, p in ipairs(state.query("IGNITION_PEOPLE_LIST", { env = env }).people) do
  people[#people + 1] = string.format("%-9s %-32s x %5.2f  y %5.2f", p.name, p.label, p.position.x, p.position.y)
end
return { people = people, fixtures = #patch.patch, patch_ok = patch.ok, look = state.query("IGNITION_ENV_GET", { env = env }).look }
