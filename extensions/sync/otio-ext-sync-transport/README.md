# otio-ext-sync-transport

**An otio-rust extension, not part of OpenTimelineIO.** Wire profiles and
transports for [timeline sync](../README.md): what carries an
`otio_ext_sync::session::Session` between programs.

| Module | What it is |
| --- | --- |
| `furnace` | Alchemist's Furnace protocol v2: the hello and welcome, the envelope and its numbering, and how sync's channels map to Furnace's messages |
| `compression` | gzip and Brotli, negotiated in the hello and welcome, for messages of 1 KiB or more |
| `json` | A session's settings, events and state as JSON, the shapes the C interface and every SDK use |
| `loopback` | An in-memory room that relays as Furnace does, for tests |
| `ori` | ORI Sync's wire: its envelope, master election, joins by `STATE_REQUEST` and `STATE_SNAPSHOT`, its edit, playback and display messages, and zlib compression for large snapshots |
| `websocket` | A blocking WebSocket client, with TLS through rustls (the `websocket` feature, on by default) |
| `amqp` | A blocking AMQP 0-9-1 client that joins ORI's RabbitMQ sessions on their `sync_session_<id>` fanout exchange, with TLS through rustls (the `amqp` feature) |

A `Connection` joins a session to a profile: frames and the time in, frames
and events out. A program that owns its own socket uses that; one that does
not uses `websocket::Client` for Furnace or `amqp::Client` for ORI.

## Tests

- `tests/conformance.rs` reads every frame in Furnace's conformance corpus,
  vendored under `tests/fixtures/furnace-conformance`.
- `tests/session.rs` and `tests/hooks.rs` run sessions against each other
  through the loopback room.
- `tests/furnace_live.rs` joins a real furnaced when `FURNACE_URL` names one.
- `tests/ori.rs` runs ORI peers against each other through an in-memory
  fanout exchange.
- `tests/ori_replay.rs` replays ORI's own recordings, vendored under
  `tests/fixtures/ori` (see its README), and checks the session ends where
  ORI's own peer does.
- `tests/ori_live.rs` runs sessions on a real RabbitMQ when `OTIO_AMQP_URL`
  names one, between Rust peers and, when `ORI_PYTHON` names ORI's `python`
  directory, with ORI's own Python peer.
- `examples/compression.rs` prints what each encoding saves on the
  repository's timelines.

The design is in [ADR 0011](../../../docs/adr/0011-timeline-sync.md).
