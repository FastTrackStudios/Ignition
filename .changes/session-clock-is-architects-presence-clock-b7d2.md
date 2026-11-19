---
"ignition" = "patch"
"ignition-pane" = "minor"
---

The room and the Lighting pane follow a session through Architect's presence clock (`PresencePeer::clock`), since `session::clock` is gone from the session primitive (session#189): `ignition_pane::session::follow_session` returns the peer alone and `leader_now` takes it alone. `session` is at its main (`f186f34b`, the revision Studio and Keyflow pin). The crates that came from Studio's repo at a saved revision (`utils`, `daw`, `daw-proto`, `daw-control`, `dawfile-reaper`, `audio`, `fts-sample`, `dsp`, `ableton`) come from where they live now: the daw repo, fts-fx and studio-adapters, at Studio's revisions
