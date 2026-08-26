# DMX output

The wire. Everything above this resolves to one byte per channel per
universe; this is how those bytes leave the machine, and how the visualizer
is held to the same bytes.

Implemented by `crates/ignition-io` (`ignition_io::Sender`, with the packet
builders in `ignition_io::sacn` and `ignition_io::artnet`). The venue carries
the configuration under a `"dmx"` key in `venue.ig-venue`, deserialised as
`ignition_io::OutputConfig`:

```json
"dmx": {
  "universes": {
    "1": {
      "sacn":   { "priority": 100, "multicast": true, "unicast": ["10.0.0.20:5568"] },
      "artnet": { "net": 0, "subnet": 0, "universe": 0, "broadcast": true, "unicast": [] },
      "enabled": true, "max_hz": 44, "keepalive_hz": 1
    }
  }
}
```

Every key but the universe number is optional; a universe with neither
`sacn` nor `artnet` is not sent.

r[dmx.protocols]
The engine MUST transmit **sACN (E1.31)** and **Art-Net 4** — both, selectable
per universe, since rooms have nodes of either kind and a touring rig meets
both in a week. A universe MAY be sent on both at once.

r[dmx.one-frame]
Every protocol MUST send the same frame: the encoder's universes, after curves,
multipatch and parked bytes. There is one source of truth for a byte and it is
the encoder; a protocol adapter reorders and wraps, it never decides a value.

r[dmx.rate]
Frames MUST go out at a bounded rate — at most 44 Hz per universe, the DMX
refresh ceiling — and a universe whose bytes have not changed MUST still be
sent at a keep-alive rate of at least once a second, because a node that stops
hearing a source drops to its own fallback. A change that arrives inside the
rate window is owed, not dropped: it goes out when the window opens, even if
the bytes have since reverted. Both rates are per universe and venue-settable
(`max_hz`, `keepalive_hz`).

r[dmx.sacn.priority]
sACN frames MUST carry a per-universe **priority** (0–200, default 100) and a
stable source CID and name, so a house desk and Ignition can share a universe
with the rule decided by the node, not by whoever sent last. The CID is
derived from the host name and the source name, so it survives a restart
(E1.31 asks a source to keep its CID) and differs between two machines.

r[dmx.sacn.addressing]
sACN MUST multicast by default to the universe's group and MAY unicast to a
list of addresses per universe. A universe MUST be able to be **terminated**
on stop (stream-terminated flag sent three times), so nodes release it at once.

r[dmx.artnet.addressing]
Art-Net MUST send `ArtDmx` with a 15-bit port address (net / sub-net /
universe), to the subnet broadcast by default and to unicast nodes when
configured; MUST answer `ArtPoll` with an `ArtPollReply` naming the source,
so controllers and nodes can discover it.

r[dmx.sequence]
Every frame MUST carry a per-universe sequence number that increments and
wraps, so a receiver can discard out-of-order packets. The counter is per
universe *per protocol*: sACN wraps 255 → 0; Art-Net wraps 255 → 1, because
0 there means "no sequence".

r[dmx.venue-config]
Which universes go where — protocol, priority, unicast targets, Art-Net port
address, enabled — MUST live in the **venue** (`venue.ig-venue` or a sibling
file), because it is a property of the room's network, and a show MUST NOT
carry any of it.

r[dmx.output-toggle]
Output MUST be switchable off and on at the desk without stopping the engine,
and its state — sending, which universes, at what rate, and any socket error —
MUST be visible. A desk that is silently not sending is worse than one that
is not connected. Opening the sockets MUST NOT fail the engine: a socket that
would not bind is an entry in that visible state, and the other protocol
still runs. Turning output off silences keep-alives too; turning it back on
resends every universe at once.

r[dmx.loopback]
The engine MUST be able to feed its own output frame back into the
visualizer's receive path, byte for byte, so what is on screen is what left
the socket (`r[viz.driven-by-dmx]`). A loopback MUST reproduce the engine's
attribute values within one byte's resolution per channel. The loopback is
a sink handed the same 512-byte slot array the packets carried — never a
re-encoding — and it is fed whether or not a socket accepted the packet,
because the picture must show what the engine meant to send.
