-- The lab's rig as the owner asked (2026-11-19): the two trees out beside
-- the PA speakers and further out front, each a 2 x 2 of pars aimed at the
-- people it lights; the movers off the towers' feet. One undo step.
local function run(id, a) return actions.run("IGNITION_" .. id, a) end
actions.run("IGNITION_EDIT_BEGIN", { label = "lab rig: trees beside the PA as 2x2, movers placed" })

local head = {}
for _, p in ipairs(state.query("IGNITION_PEOPLE_LIST", {}).people) do head[p.name] = p.head end

-- chan = { tree x, who it is aimed at }; a tree is two pars over two.
local trees = {
  { x = -5.50, y = -2.70, pars = { { 27, "Bass" }, { 28, "Drummer" }, { 29, "Singer" }, { 30, "Guitar 1" } } },
  { x =  3.85, y = -2.70, pars = { { 31, "Bass" }, { 32, "Singer" }, { 33, "Guitar 1" }, { 34, "Guitar 2" } } },
}
local dx, top, bottom = 0.19, 2.50, 2.12
for _, t in ipairs(trees) do
  for i, par in ipairs(t.pars) do
    local col = (i % 2 == 1) and -dx or dx
    local z = (i <= 2) and top or bottom
    run("FIXTURE_SET", { chan = par[1], position = { x = t.x + col, y = t.y, z = z }, aim = head[par[2]] })
  end
end

-- Movers: two upstage in the gaps between the towers, raised to a metre so
-- their beams clear the amps; two downstage at the sides for cross-light.
local movers = {
  { 35, { x = -4.10, y = -1.55, z = 0.0 } },
  { 36, { x = -2.83, y =  1.30, z = 1.0 } },
  { 37, { x =  1.67, y =  1.30, z = 1.0 } },
  { 38, { x =  2.45, y = -1.55, z = 0.0 } },
}
for _, m in ipairs(movers) do run("FIXTURE_SET", { chan = m[1], position = m[2] }) end

-- Who each par lights, as groups a show can name.
run("GROUP_SET", { name = "Key Singer", chans = { 29, 32 } })
run("GROUP_SET", { name = "Key Guitar", chans = { 30, 33, 34 } })
run("GROUP_SET", { name = "Key Rhythm", chans = { 27, 28, 31 } })
run("GROUP_SET", { name = "Key Band", chans = { 27, 28, 30, 31, 33, 34 } })
actions.run("IGNITION_EDIT_COMMIT", {})
local out = {}
for _, f in ipairs(state.query("IGNITION_FIXTURE_LIST", {}).fixtures) do
  if f.chan >= 27 and f.chan <= 38 then out[#out + 1] = string.format("%d %s (%.2f, %.2f, %.2f)", f.chan, f.name, f.position.x, f.position.y, f.position.z) end
end
return out
