---
title: Timeline sync
summary: Sharing an OpenTimelineIO timeline between programs while it is edited, over Alchemist's Furnace or ORI Sync.
section: Extensions
order: 2
---

Timeline sync keeps one timeline the same in several programs while people
edit it: an editor in the browser, a reviewer on an iPad, a desktop editor,
RV or xStudio. It is otio-rust's first extension.

Two systems already do this. ORI Sync, from the ORI Annotations project, runs
between OpenRV and xStudio over RabbitMQ. Alchemist's Furnace is a WebSocket
server between Alchemist's clients. Both name every object with a GUID in
`metadata.sync.guid` and send changes as patches addressed by GUID. Timeline
sync is one implementation of that model, written once in Rust and reached
from every language, that speaks both systems' wire formats.

## Where it stands

Sync is being built in phases, tracked in
[epic #115](https://github.com/alchemist-editor/otio-rust/issues/115):

1. **The extension itself**: sync can be enabled, and every SDK has its
   module.
2. **Identity and patches**: GUIDs, patches made by transactions or by
   comparing two versions of a timeline, and applying them.
3. **Sessions over Furnace**: a session that takes bytes in and hands bytes
   out, with no networking of its own, Furnace's wire format, a WebSocket
   client, and compression, in Rust and, through the C interface, in every
   SDK.
4. **Furnace relays patches** instead of whole timelines.
5. **ORI's wire format** over RabbitMQ, for RV and xStudio, in Rust.

## Turning it on

| Language | Import | Enable |
| --- | --- | --- |
| Rust | `otio-ext-sync` | `otio_ext_sync::install(); otio_ext::enable(otio_ext_sync::ID)?;` |
| Python | `from opentimelineio.ext import sync` | `sync.enable()` |
| C | `#include "otio_ext_sync.h"` | `otio_ext_sync_enable(&error);` |
| Go | `github.com/alchemist-editor/otio-rust/sdk/go/ext/sync` | `sync.Enable()` |
| Swift | `import OTIOExtSync` | `try Sync.enable()` |
| TypeScript | `import * as sync from "@alchemist-edit/otio/ext/sync"` | `await init(); sync.enable();` |
| C++ | `opentimelineio/ext/sync.hpp` | `otio::ext::sync::enable();` |
| C# | `using OpenTimelineIO.Ext.Sync;` | `SyncExtension.Enable();` |
| Objective-C | `#import <OpenTimelineIO/OTIOExtSync.h>` | `OTIOExtSyncEnable(&error);` |
| Zig | `otio.ext.sync` | `try otio.ext.sync.enable();` |

## Identity

Every object sync addresses carries a GUID in `metadata.sync.guid`: the
timeline, its stack, and every track, stack and item under it. Markers,
effects and media references travel inside the item that holds them. A GUID
is ordinary metadata, so a file carrying them opens anywhere OpenTimelineIO
does.

Sync gives an object a GUID when it needs one. Beginning a transaction
stamps every object that lacks one, and an object that shares a GUID with
one met earlier (usually a copy made with its metadata) gets a new one; the
first keeps it. You can also stamp a timeline yourself, read or set one
object's GUID, make a new GUID, and find an object by its GUID.

## Patches

A change to a timeline travels as a list of patches, in Alchemist's Furnace
format: `insert_child`, `move_child`, `remove_child` and `set_property`,
each naming the objects it is about by GUID. A change too big for patches
(more than 500 of them, or more than 40% of the timeline's objects), a change
between two sequences, or one that goes back a generation, is sent as one
`replace_timeline` patch holding the whole timeline instead.

There are two ways to get patches, and both give the same ones.

**Compare two versions.** Diffing two snapshots, each a timeline with its
sequence id and generation as Furnace sends it, gives the patches between
them. It is a port of Furnace's own diff, checked against Furnace's golden
files in Rust and in every SDK, so both ends agree byte for byte.

<!-- ::sample id="diff-two-timelines" -->

**Record a transaction.** Begin a transaction on a timeline, edit it with
any of the usual calls, and commit it for the patches. Rolling back puts the
timeline back as it was. The transaction remembers the timeline as it began
and diffs it against the timeline as it is, so its patches are exactly what
comparing the two versions gives.

Applying patches to a timeline changes it in place. The timeline keeps its
identity, and so does every track and item the patches keep, so anything
your program holds onto still refers to the same object afterwards. An
object the patches removed is gone, and one they added can be found by its
GUID.

<!-- ::sample id="apply-patches" -->

## Noticing changes

A program that edits a timeline through code it does not own, such as an
editing operation or a script, can turn on the document's change journal.
The journal records which objects any edit touched. Committing it runs the
`document_changed` hook, with the GUIDs of the touched objects as its
`touched` argument, so a hook script can send the change on. The journal is
off unless you turn it on, and costs nothing while it is off.

The design is in
[ADR 0011](https://github.com/alchemist-editor/otio-rust/blob/main/docs/adr/0011-timeline-sync.md).

## Sessions

A session joins a shared timeline and keeps a copy of it in step. It does no
networking itself: a program hands it what arrived and the session hands back
what to send and what happened. The `otio-ext-sync-transport` crate puts it on
a wire. Its first wire format is Furnace's protocol v2, and it has a blocking
WebSocket client that runs the whole loop:

```rust
use std::time::Duration;
use otio_ext_sync::session::{self, ApplyOptions, Config, Event, Session};
use otio_ext_sync_transport::websocket::Client;

let config = Config { token: join_token, ..Config::default() };
let mut client = Client::connect(&ws_url, Session::new(config))?;
while let Some(event) = client.next_event(Duration::from_secs(1))? {
    // Applies timeline changes to `timeline` in place and runs the hooks.
    session::handle(&mut document, timeline, &event, &ApplyOptions::default())?;
}
```

The host publishes with `session.publish(&mut document, timeline, "main")`,
which works out the patches since the last publish and sends them. Viewers
receive each change as patches and apply them in place, so handles to the
timeline, its tracks and its items keep working. A host that reconnects to a
Furnace holding an older generation than its own, because the connection
dropped as an edit went out, sends its timeline again.

### Patches on the wire

When Furnace's welcome lists `timeline.patch`, the host sends each change as
a `patch` message holding the patches and the generations they go from and
to. Furnace applies them to its copy, so anyone who joins later still gets
the whole timeline in their welcome, and relays them to every client whose
hello listed `timeline.patch.receive` (sessions list it unless told not to).
Everyone else, older clients included, gets the timeline Furnace rebuilt, as
before. The host still sends the whole timeline for its first publish, every
64th change after that as a checkpoint, when the patches would be bigger
than the timeline, when the change replaces the timeline outright, and to a
Furnace that does not list `timeline.patch`.

If Furnace's copy is not the generation a patch starts from, or the patch
does not apply to it, Furnace answers `stale_patch` or `patch_rejected`, and
the host sends its whole timeline in reply. A viewer that receives patches
from a generation it does not have is out of sync: `receive` fails with an
out-of-sync error (a `ConnectionError` in Python, `IoError` over the C
interface), and the program reconnects to get the whole timeline in the
welcome. Furnace's conformance corpus, revision 5, holds a patch and the
timeline it makes, and Go and Rust must both reach the same timeline.

Everything a session sends travels on a channel with a delivery rule:

| Channel | Rule | Over Furnace |
| --- | --- | --- |
| timeline | in order | `patch`, or `snapshot`, from the host only |
| playback | newest wins: a playhead not yet sent is replaced | `presence` |
| presence | newest wins | `welcome` and `roster` |
| review | kept until acknowledged, refused or cancelled, sent again after a reconnect | `review_commit` |
| signal | sent once, never replayed | `signal` |

Only the host may change the timeline, and only a client with the
`review.comment.create` capability may commit a review comment. A session
refuses these itself rather than letting Furnace close the connection. For
the same reason `session.send` carries only Furnace's message types that have
no call of their own (`signal`, and from the host only `bin` and the LAN
messages), and review
comments are refused when an older Furnace chooses protocol v1, which has no
review ledger.
Selections and other extensions' channels need Furnace to relay them, which
it does not do yet.

Four hooks run around a session: `sync_before_send` before the host's
changes leave (a script that fails stops them), `sync_before_apply` before a
remote change is applied (a script that fails rejects it), `sync_after_apply`
after, and `sync_joined` once the join is done. The media linker runs on the
clips a remote change inserts.

### From other languages

Every SDK has the same session, over the C interface. The program owns the
socket: open a WebSocket to Furnace with whatever your language offers,
connect the session, and then keep three things going. Send every frame the
session gives you (text frames as text, binary ones as binary), hand it every
frame that arrives, and act on each event it reports. Passing a `timeline` or
`joined` event to `handle` with your copy of the timeline applies it in
place and runs the hooks, as `session::handle` does in Rust.

A session is a number from `session_new`, released with `session_close`.
Its settings, its events and its state are JSON objects, in the field names
Furnace uses:

| | Fields |
| --- | --- |
| Settings | `token`, `identity` (`displayName`, `app`, `appVersion`, `credential`, `deviceId`, `clientKind`, `formFactor`, `deviceName`, `capabilities`), `encodings` (`br`, `gzip`); all optional |
| Event | `type`: `joined`, `timeline`, `timeline_removed`, `playhead`, `selection`, `roster`, `leader`, `review_relay`, `review_committed`, `review_rejected`, `error`, `ended` or `message`, and that event's fields |
| State | `joined`, `ended`, `sessionId`, `selfId`, `role`, `leader`, `peers`, `activeSequence`, `sequences`, `pendingReviews`, `version`, `encodings` |

Python wraps this in a `Session` class that takes and returns dictionaries,
and `poll_transmit` there returns `str` for a text frame and `bytes` for a
binary one.

<!-- ::sample id="join-a-session" -->

### Compression

Each side lists the encodings it accepts when it joins: the client in its
hello, the server in its welcome. Messages of 1 KiB or more go as Brotli or
gzip when the other side accepts it, in a binary frame whose first byte names
the encoding (1 for gzip, 2 for Brotli); everything else stays JSON text.
Furnace does not list any encodings yet, so today every frame is text; the
in-memory test room does, and CI publishes a table of what each encoding
saves on the repository's timelines. A 103 KiB timeline goes over the wire
as about 3 KiB.

### Checked against Furnace

Furnace's conformance corpus, every frame it says a client or server sends,
is vendored into the crate's tests: each client frame is read and written
again and must match it value for value, and each server frame is read as what it
says. A CI job also builds furnaced and has a host and a viewer join a real
session.


## Sessions over ORI Sync

The ORI profile speaks ORI Sync's wire, so a session can join the same
RabbitMQ session as OpenRV and xStudio running ORI's plugins. It is in Rust
for now. There is no server: every peer binds a queue to the session's
fanout exchange, `sync_session_<id>`, and hears everything. The first peer
finds no master within two seconds and elects itself; later peers ask the
master for its state and join with it, and if the master stops answering
another peer takes over.

```rust
use std::time::Duration;
use otio_ext_sync::session::{self, ApplyOptions, Config, Session};
use otio_ext_sync_transport::{amqp::Client, ori};

let options = ori::Options::new("review-42");
let mut client = Client::connect("amqp://guest:guest@localhost:5672/", Session::new(Config::default()), options)?;
loop {
    // `None` is a quiet second, not the end: keep going so the client can
    // elect itself, answer joiners and send its heartbeat.
    if let Some(event) = client.next_event(Duration::from_secs(1))? {
        session::handle(&mut document, timeline, &event, &ApplyOptions::default())?;
    }
}
```

Any peer may change the timelines, as in ORI. What ORI's peers send arrives
as patches like Furnace's, and what a session publishes goes out as ORI's
own edits (`SET_PROPERTY`, `INSERT_CHILD`, `MOVE_CHILD`, `REMOVE_CHILD`),
or as the whole timeline when ORI's edits cannot say it, such as inserting a
track with clips in it. Several timelines, which one is active, the
playhead and ORI's display settings cross too. Selections are retired in
ORI, and review comments, leaders and muting have no ORI message, so sending
them fails as unsupported.

Snapshots and messages over 1 MB can be zlib-compressed, but ORI's own peers
cannot read either yet, so both are off unless `Options` turns them on.
Compressed input is always read.

### Checked against ORI

ORI's own recordings of real sessions are vendored into the crate's tests
and replayed as ORI's recording player replays them; the session must end
with the same timelines, active timeline, playhead and display settings as
ORI's own peer, which CI checks against ORI itself. CI also runs sessions on
a real RabbitMQ, between Rust peers and with ORI's Python peer, with each
side as master.
