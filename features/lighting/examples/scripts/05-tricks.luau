-- One effect on twelve movers, three ways, by changing only its tricks
-- (grandMA3: MAtricks): a chase, a mirrored wave (two wings), blocks of 2.
-- The effect itself (one tilt sine, every mover in step) is never edited.
-- Uses the environment of 04-environment.luau; saves nothing.
local env = (args and args.env) or "club-stage"
local effect = { what = "tilt", shape = "sine", beats = 4, depth = 20 }
local presets = {
  { name = "in step",       tricks = {} },
  { name = "chase",         tricks = { phase = { from = 0, to = 360 } } },
  { name = "mirrored wave", tricks = { tricks = { "Mirror" }, phase = { from = 0, to = 360 } } },
  { name = "blocks of 2",   tricks = { tricks = { { Block = 2 } }, phase = { from = 0, to = 360 } } },
}
local out = {}
for _, p in ipairs(presets) do
  local units = state.query("IGNITION_TRICKS_UNITS", { env = env, target = "Movers", tricks = p.tricks })
  local shown = state.query("IGNITION_EFFECT_PREVIEW", { env = env, target = "Movers", tricks = p.tricks, bpm = 120, samples = 1,
    what = effect.what, shape = effect.shape, beats = effect.beats, depth = effect.depth })
  local phases, tilt = {}, {}
  for _, u in ipairs(units.units) do phases[#phases + 1] = string.format("%s@%d", table.concat(u.chans, "+"), math.floor(u.phase_deg + 0.5)) end
  for _, v in ipairs(shown.frames[1].values.Tilt) do tilt[#tilt + 1] = string.format("%+.0f", v) end
  out[#out + 1] = string.format("%-14s %2d units  tilt at beat 0: %s", p.name, units.count, table.concat(tilt, " "))
  out[#out + 1] = "               units@phase: " .. table.concat(phases, "  ")
end
-- Stepping through the units of blocks of 2, as MA's Next does.
local third = state.query("IGNITION_TRICKS_UNITS", { env = env, target = "Movers", tricks = { tricks = { { Block = 2 } } }, step = 2 }).step
out[#out + 1] = "step 2 of blocks of 2: channels " .. table.concat(third.chans, ", ")
return out
