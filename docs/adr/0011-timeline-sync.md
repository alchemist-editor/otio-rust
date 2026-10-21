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

**As built in phase 2 (#117).** The first `Transaction` does not record
patches as edits happen: it remembers the document when it begins and diffs
that against the document when it commits, so its patches are exactly what
`diff` makes, Furnace's move minimisation included. That keeps one source of
patches, checked by Furnace's goldens, while the editing API stays whatever
`otio-core` offers. Recording intent, and diffing only the objects the
journal names rather than the whole timeline, are later changes that make
the same patches faster; neither changes the wire format. Applying patches
to a timeline in a `Document` works out the patched JSON, reads it, and puts
each synced object back in the slot of the object with the same GUID, so a
program's handles to the timeline, its tracks and its items survive; only
what travels inside an item (markers, effects, media references) gets new
handles. The C ABI hands a transaction to its caller as the snapshot it
began from, so it needs no handle type of its own.

### Hooks

Through the registry from ADR 0008 and ADR 0009, so every SDK attaches them
as it attaches a media linker:

| Hook | Fires |
| --- | --- |
| `document_changed` | When a journalled edit is committed, with the touched objects' GUIDs |
| `sync_before_send` | Before local patches leave; may drop or rewrite them |
| `sync_before_apply` | Before a remote patch is applied; may reject it |
| `sync_after_apply` | After a remote patch is applied |
| `sync_joined` | After the join snapshot and replay complete |

A remote patch that inserts a clip whose media is not on this machine runs
the configured media linker.

The journal is in `otio-core`, but the hook registry is in `otio-adapter`,
which depends on the core, so sync declares `document_changed` when it is
enabled and runs it from `otio_ext_sync::journal::commit`, which takes the
journal's changes.

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

**As built in phase 3 (#118), in Rust.** The session (`otio_ext_sync::session`)
speaks in wire-free messages; `otio-ext-sync-transport` holds the Furnace v2
profile, compression, an in-memory loopback room and a blocking WebSocket
client, and a `Connection` joins a session to a profile so a program with its
own socket hands it frames. Furnace still relays whole snapshots, so the
profile sends the host's timeline as `snapshot` and a receiving session diffs
each one against the last into patches: programs see patches either way, and
phase 4 can switch the wire to patches without changing them. A patch that
no longer fits a viewer's copy (a `sync_before_apply` script refused an
earlier one, say) falls back to the whole snapshot, which keeps handles by
GUID as any apply does. Authority is the appointed host for now; ORI's
elected master arrives with ORI's profile in phase 5, its only user. Hook
scripts can stop a send or refuse an apply but not rewrite patches, since a
hook script returns an object, not data; a program that wants different
patches edits the timeline and publishes again. Compression is offered in an
`encodings` field of the hello, which Furnace ignores today, and is used only
once a welcome lists encodings back. Furnace's conformance corpus is vendored
into the transport crate's tests, and CI joins a real furnaced built from
svc-furnace when it has a token to check it out.

**As built in phase 4 (#119).** Furnace gains a host-only `patch` message
(`sequenceId`, `baseGeneration`, `generation`, `patches`), applies it to its
stored timeline with `otiodiff.Apply`, and relays the patch to clients whose
hello lists `timeline.patch.receive` and the rebuilt snapshot to everyone
else, so a late joiner's welcome and an old client are unchanged. An old
Furnace closes the connection on a message type it does not know, so the
host sends `patch` only when the welcome lists `timeline.patch` in a new
`serverCapabilities` field, and otherwise keeps sending snapshots. The host
also sends a snapshot for its first publish, every 64th change as a
checkpoint, when the change is a `replace_timeline`, and when the patches
would be bigger than the timeline. Furnace refuses a patch whose base is not
its stored generation with `stale_patch`, and one that does not apply with
`patch_rejected`; both are non-fatal, and the session answers either by
sending its whole timeline. A viewer that receives a patch from a generation
it does not hold does not guess: `receive` fails as out of sync and the
program reconnects, which brings the whole timeline in the welcome. Furnace's
conformance corpus revision 5 carries one patch and the snapshot Furnace
rebuilt from it, and the Rust tests check that applying the patch reaches
the same timeline.

**As built in phase 5 (#120).** The ORI profile speaks ORI Sync's current
envelope (`session`, `source_guid`, `payload.command_schema`,
`payload.command`) as ORI's `otio_sync_core` does at the commit its
recordings are vendored from. It discovers a master with `WHO_IS_MASTER`,
elects itself after ORI's two seconds of silence, ranks rival masters as ORI
does (a driver first), takes a master over when one stops answering, joins
by `STATE_REQUEST` and `STATE_SNAPSHOT` with ORI's buffering of what arrives
in between, settles two masters on one (one deliberate difference: where ORI
accepts a rival but keeps answering as master, the higher GUID of two equal
masters stands down, so joiners never get competing snapshots), and keeps ORI's heartbeat and peer timeouts. A peer joins with
the new `Driver` role, which may publish timelines as a host may, since ORI
has no appointed host. ORI's edits (`SET_PROPERTY`, `INSERT_CHILD`,
`MOVE_CHILD`, `REMOVE_CHILD`, `REPLACE_ANNOTATION_COMMANDS`) are applied to
the session's copy with ORI's own semantics, annotation clips merging as
ORI merges them, and handed to the session as a timeline it diffs into
patches, so programs see patches as they do over Furnace. Outgoing patches
become ORI's edits by replaying them on the previous timeline; a move
between parents becomes a remove and an insert, and what ORI's edits cannot
say (inserting a subtree, removing a property, setting a structured
property) goes as `REPLACE_TIMELINE`. Several timelines, the active one,
`PLAYBACK` (playhead, view and switching timelines) and `DISPLAY` cross
too; `SELECTION_1.0` is retired in ORI and ignored, and review comments,
leaders and muting have no ORI message, so sending them fails as
unsupported. Snapshots and messages over 1 MB can be zlib-compressed (a
`compressed` flag inside the snapshot, AMQP `content_encoding: deflate` for
a whole message), off by default because ORI's peers cannot read either yet;
compressed input is always read. The AMQP client is written on the socket
rather than taken from a crate, as the WebSocket one leans on tungstenite:
it does what ORI's pika code does (a non-durable fanout exchange per
session, an exclusive server-named queue, auto-ack) and no more. The gate
holds two ways: ORI's 26 `sync_test` recordings replay to the end state
ORI's own headless peer reaches, which CI regenerates from ORI to keep
honest, and live sessions on RabbitMQ converge between Rust peers and with
ORI's Python peer, in both directions of who is master. Choosing rustls's
`ring` provider also fixed the WebSocket client, which panicked on its
first `wss://` connection because rustls had no provider.

### What crosses the C ABI

The session, patches and identity cross, under `otio_ext_sync_*` in
`extensions/sync/capi`, declared in `otio_ext_sync.h`. Transports stay in
Rust.

**As built in phase 3 (#118), across the C ABI.** A session crosses as a
`u64` from `otio_ext_sync_session_new`, held in a registry and released with
`otio_ext_sync_session_close`, so every SDK holds one the same way and the
generator needs no new handle type. It is a `Connection` on the Furnace
profile: the program hands it frames and the time and gets frames and
events back. Settings, events and state cross as JSON objects in Furnace's
spelling (`otio_ext_sync_transport::json`), which each language reads with
its own JSON library, and `otio_ext_sync_handle` takes an event back to
apply it to a timeline. A call takes its session out of the registry while
it runs, so a hook script it runs can use other sessions without
deadlocking. libotio carries the wire profile and compression but not the
WebSocket client: the program brings its own socket. The release call is
`close`, not `free`, because the generators treat `*_free` as the
destructor of an object they wrap, and a session is a plain number. Python
binds the same `Connection` directly, behind a `Session` class.

### Security

The session carries roles and leases but makes no security claims.
Authentication and encryption belong to the transport and the server.

## Consequences

- One implementation replaces the hand-written Furnace clients in TypeScript,
  Swift and Zig, and can join ORI sessions from any language.
- `otio-core` gains the opt-in change journal, which does not alter how a
  timeline is read or written.
- Furnace can move from snapshots to patches (in svc-furnace, outside this
  repository) using the same golden corpus, with snapshots kept as the
  recovery path and for old clients.
- Conflicts rely on one ordering point (Furnace's server, or RabbitMQ's single
  exchange); there are no vector clocks in v1, and the interop tests check
  that every peer ends in the same state.
- The work is phased under one epic: framework, identity and patches,
  session with the Furnace wire and WebSocket, Furnace relaying patches, then
  the ORI wire and AMQP.
