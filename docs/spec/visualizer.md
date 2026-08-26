# Visualizer

The visualizer is a fixture-level simulation, not a picture of the cue list.

r[viz.driven-by-dmx]
The visualizer MUST render from the **DMX bytes** the engine outputs — the same
universes a rig would receive — decoded through each fixture's channel map,
never from the engine's attribute values directly. A visualizer that reads
attributes cannot show a patch mistake, a curve, a multipatch, or a fine
channel; one that reads bytes shows exactly what the lights would.

r[viz.export]
The visualizer MUST be able to render a show to a **video file** offline —
frame by frame against the song's clock, at a chosen size — so a look can be
reviewed away from the desk and sent to someone who has no console.

r[viz.gobo-raster]
A fixture with a gobo selected SHOULD render that gobo's pattern in its beam
and on the surface it lands on. (Deferred until the beam material supports a
projected texture; noted, not built.)
