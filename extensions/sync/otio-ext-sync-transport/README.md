# otio-ext-sync-transport

**An otio-rust extension, not part of OpenTimelineIO.** Wire profiles and
transports for [timeline sync](../README.md): what carries an
`otio_ext_sync::session::Session` between programs.

| Module | What it is |
| --- | --- |
| `furnace` | Alchemist's Furnace protocol v2: the hello and welcome, the envelope and its numbering, and how sync's channels map to Furnace's messages |
| `compression` | gzip and Brotli, negotiated in the hello and welcome, for messages of 1 KiB or more |
| `loopback` | An in-memory room that relays as Furnace does, for tests |
| `websocket` | A blocking WebSocket client, with TLS through rustls (the `websocket` feature, on by default) |

A `Connection` joins a session to a profile: frames and the time in, frames
and events out. A program that owns its own socket uses that; one that does
not uses `websocket::Client`.

## Tests

- `tests/conformance.rs` reads every frame in Furnace's conformance corpus,
  vendored under `tests/fixtures/furnace-conformance`.
- `tests/session.rs` and `tests/hooks.rs` run sessions against each other
  through the loopback room.
- `tests/furnace_live.rs` joins a real furnaced when `FURNACE_URL` names one.
- `examples/compression.rs` prints what each encoding saves on the
  repository's timelines.

The design is in [ADR 0011](../../../docs/adr/0011-timeline-sync.md).
