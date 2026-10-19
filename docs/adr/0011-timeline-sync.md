# ADR 0011: Timeline sync as an extension

- **Status:** Accepted
- **Date:** 2026-10-19
- **Deciders:** Jeff Hodges

## Context

Two working systems share an OpenTimelineIO timeline between programs while
it is edited. ORI Sync (the ORI Annotations project, in Python) runs over a
RabbitMQ fanout exchange between OpenRV and xStudio plugins. Alchemist's
Furnace (svc-furnace, in Go) is a WebSocket server between Alchemist's web,
Apple and native clients, each of which implements its protocol by hand.

They agree on the core idea: a shared OTIO timeline is the source of truth,
objects are identified by `metadata.sync.guid`, and changes travel as
GUID-addressed patches (`insert_child`, `remove_child`, `move_child`,
`set_property`, `replace_timeline`). They differ in the envelope, the network,
and who is in charge: ORI elects a master among its peers, Furnace's server
appoints a host. Furnace relays whole snapshots today; its `internal/otiodiff`
package has the same patch operations and 11 golden fixtures, but nothing
calls it yet.

Jeff asked for timeline sync to be built in otio-rust, as an extension
([ADR 0010](0010-extensions.md)), so every language shares one
implementation, and for the Furnace wire protocol and WebSocket to come first.

## Decisions

### Identity: `metadata.sync.guid`

`NodeId` does not survive a write and a read, and the core has no GUIDs. ORI
and Furnace both use `metadata.sync.guid`, so sync does too: a helper stamps
missing GUIDs and an index maps GUID to node. If upstream OpenTimelineIO adds
a native object id, as ORI has asked, we follow it in the core and stop
inventing one in metadata.

### Patches: transactions, plus a change journal feeding a diff

A patch list addressed by GUID is made in one of two ways:

- **A `Transaction`** applies edits and records the patches as it goes, which
  keeps the edit's intent (a move is a move).
- **`diff(old, new)`** works out the patches between two versions, so any
  editing code, including the existing `edit::*` operations, syncs without
  being rewritten. It is a port of Furnace's `otiodiff`: the same move
  minimisation, the same per-namespace metadata diff, and the same fallback
  to `replace_timeline` past 500 ops or 40% of nodes. Furnace's goldens are
  its tests.

We considered three ways to learn what a program changed: diffing whole
snapshots (simple, but costs the whole timeline per edit), a full change
observer in the core (exact, but every edit path must report, and one that
forgets is a silent desync), and a **change journal plus diff**. We chose the
journal. `otio-core` keeps every object in the `Document` arena, whose
storage is private; the only ways to change an object are `insert`,
`remove`, `get_mut`, `try_get_mut`, `absorb` and `set_root`. Marking the
object id at those six entry points catches every edit, including ones made
through the C ABI and Python, and the diff then compares only what was
touched. The journal is off unless a program turns it on, so a program that
does not sync pays nothing. It says which objects changed, not how; a
program that needs exact intent uses a `Transaction`.

### Hooks

Through the registry from ADR 0008 and ADR 0009, so every SDK attaches them
as it attaches a media linker:

| Hook | Fires |
| --- | --- |
| `document_changed` (core) | When a journalled edit is committed, with the touched ids |
| `sync_before_send` | Before local patches leave; may drop or rewrite them |
| `sync_before_apply` | Before a remote patch is applied; may reject it |
| `sync_after_apply` | After a remote patch is applied |
| `sync_joined` | After the join snapshot and replay complete |

A remote patch that inserts a clip whose media is not on this machine runs
the configured media linker.

### A sans-IO session with channels

The session is a state machine with no networking of its own: a program
feeds in bytes and the time, and gets back bytes to send and events to act
on. It covers joining (snapshot, then replay of what was buffered), authority
in either shape (ORI's elected master or Furnace's appointed host), several
timelines per session with one active, and roles and per-channel write
leases enforced on send, as ORI does.

Everything travels on a named channel with a delivery rule: **latest-wins**
(playhead, selection, presence), **ordered** (timeline patches),
**transient** (never stored or replayed) or an **idempotent ledger** (a
commit acknowledged once, as Furnace does with `clientMutationId` and
`serverOrder`). Sync defines the timeline, playback, selection and presence
channels; other extensions register their own.

### Wire profiles and transports

A wire profile turns session messages into bytes and back; a transport moves
the bytes. Both live in `otio-ext-sync-transport`. The profiles are Furnace
v2 (first), ORI's current envelope (second), and ORI's v1 proposal when ORI
ships it. The Rust transports are an in-memory loopback for tests, a
WebSocket client (first, for Furnace) and AMQP (second, for ORI). Other
languages bring their own socket and hand the session bytes.

Compression is negotiated in the wire profile, so every SDK has it: peers
list the encodings they accept (`br`, `gzip`, `identity`), messages under
about 1 KiB go uncompressed, and compressed messages travel as binary
WebSocket frames with a one-byte encoding tag while plain JSON stays text.
The ORI profile zlib-compresses snapshots over 1 MB, as ORI's v1 proposal
does.

### What crosses the C ABI

The session, patches and identity cross, under `otio_ext_sync_*` in
`extensions/sync/capi`, declared in `otio_ext_sync.h`. Transports stay in
Rust.

### Security

The session carries roles and leases but makes no security claims.
Authentication and encryption belong to the transport and the server.

## Consequences

- One implementation replaces the hand-written Furnace clients in TypeScript,
  Swift and Zig, and can join ORI sessions from any language.
- `otio-core` gains the opt-in change journal and the `document_changed`
  hook; neither alters how a timeline is read or written.
- Furnace can move from snapshots to patches (in svc-furnace, outside this
  repository) using the same golden corpus, with snapshots kept as the
  recovery path and for old clients.
- Conflicts rely on one ordering point (Furnace's server, or RabbitMQ's single
  exchange); there are no vector clocks in v1, and the interop tests check
  that every peer ends in the same state.
- The work is phased under one epic: framework, identity and patches,
  session with the Furnace wire and WebSocket, Furnace relaying patches, then
  the ORI wire and AMQP.
