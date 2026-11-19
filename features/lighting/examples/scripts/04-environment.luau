-- An environment from nothing: a room with a stage, two trusses and an
-- audience; a five-piece band placed on it; a small rig hung, grouped,
-- bound to roles and patched; one fixture re-patched (a conflict is
-- refused first); the front movers focused on the singer. One undo step.
local env = (args and args.env) or "club-stage"
local function run(id, a) a.env = a.env or env; return actions.run("IGNITION_" .. id, a) end
local function ask(id, a) a.env = a.env or env; return state.query("IGNITION_" .. id, a) end

actions.run("IGNITION_EDIT_BEGIN", { label = "environment " .. env })
actions.run("IGNITION_ENV_CREATE", { name = env, title = "Club Stage", width = 12, depth = 9, height = 4.5, upstage = 3.0,
  stage = { width = 8, depth = 4, height = 0.4, y = 0.5 }, audience = { depth = 4 },
  truss = { { name = "Front", y = -2.5, z = 4.0, width = 8 }, { name = "Back", y = 2.0, z = 4.0, width = 8 } } })
run("ENV_PIECE_SET", { name = "Drum Riser", ["in"] = "props", kind = "riser",
  position = { x = 0, y = 1.6, z = 0.4 }, size = { x = 2.4, y = 2.0, z = 0.3 } })

-- The band, standing on the stage (0.4 m up).
run("PERSON_SET", { people = {
  { name = "Singer",  label = "vocals", position = { x = 0.0,  y = -0.8, z = 0.4 } },
  { name = "Guitar",  label = "guitar", position = { x = -2.2, y = -0.3, z = 0.4 } },
  { name = "Bass",    label = "bass",   position = { x = 2.2,  y = -0.3, z = 0.4 } },
  { name = "Keys",    label = "keys",   position = { x = -3.0, y = 1.2,  z = 0.4 }, facing_deg = 30 },
  { name = "Drummer", label = "drums",  position = { x = 0.0,  y = 1.6,  z = 0.7 }, height = 1.4 },
} })

-- The rig: six pars on the front truss aimed at the band, six on the back
-- aimed at the stage floor, twelve movers along the back truss hanging
-- straight down (a fixture with no aim does).
run("FIXTURE_ADD", { type = "36 LED Par Can", name = "Front Par", count = 6, groups = { "Front" }, tags = { "par" },
  position = { x = -3.0, y = -2.5, z = 3.8 }, step = { x = 1.2, y = 0, z = 0 }, aim = { x = 0, y = 0.2, z = 1.5 } })
run("FIXTURE_ADD", { type = "36 LED Par Can", name = "Back Par", count = 6, groups = { "Back Pars" }, tags = { "par" },
  position = { x = -3.0, y = 2.3, z = 3.8 }, step = { x = 1.2, y = 0, z = 0 }, aim = { x = 0, y = 0.5, z = 0.4 } })
run("FIXTURE_ADD", { type = "150W Moving Head Beam", name = "Mover", count = 12, groups = { "Movers" }, tags = { "mover" },
  position = { x = -3.85, y = 2.0, z = 3.7 }, step = { x = 0.7, y = 0, z = 0 } })
run("ROLE_BIND", { role = "Key", to = "Front" })
run("ROLE_BIND", { role = "Wash", to = "Back Pars" })
run("ROLE_BIND", { role = "Back", to = "Back Pars" })
run("ROLE_BIND", { role = "Movers", to = "Movers" })
run("ROLE_BIND", { role = "Beams", to = "Movers" })

-- Patch on the fly. An address on top of another fixture is refused whole…
local refused = pcall(function() run("PATCH_SET", { chan = 2, universe = 1, address = 5 }) end) == false
-- …so the movers go to a universe of their own, packed end to end, and that
-- universe is routed to Art-Net and sACN.
run("PATCH_PACK", { target = "Movers", universe = 2, address = 1 })
run("PATCH_ROUTE", { universe = 2, artnet = { net = 0, subnet = 0, universe = 1 }, sacn = { priority = 100 } })
-- One mover was hung a truss lower and one par swapped to another address.
run("FIXTURE_SET", { chan = 13, move = { x = 0, y = 0, z = -0.3 } })
run("PATCH_SET", { chan = 6, universe = 1, address = 200 })

-- Focus: the four middle movers on the singer. The answer is each one's pan and tilt.
local focus = run("FOCUS_ON", { target = { 17, 18, 19, 20 }, person = "Singer" })
-- Re-hang: the front pars a little lower and aimed at the singer's feet.
run("FIXTURE_SET", { target = "Front", move = { x = 0, y = 0, z = -0.2 }, aim = { x = 0, y = -0.6, z = 0.4 } })
actions.run("IGNITION_EDIT_COMMIT", {})

local patch = ask("PATCH_GET", {})
local unis = {}
for _, u in ipairs(patch.universes) do unis[#unis + 1] = string.format("universe %d: %d channels used", u.universe, u.used) end
local aim = {}
for _, f in ipairs(focus.fixtures) do aim[#aim + 1] = string.format("chan %d: pan %.1f tilt %.1f (%.1f m)", f.chan, f.pan_deg, f.tilt_deg, f.throw_m) end
return { env = env, conflict_refused = refused, patch_ok = patch.ok, universes = unis, focus = aim,
  people = #ask("PEOPLE_LIST", {}).people, fixtures = ask("FIXTURE_LIST", {}).count }
