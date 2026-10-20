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
2. **Identity and patches**, which is what ships today: GUIDs, patches made
   by transactions or by comparing two versions of a timeline, and applying
   them.
3. **Sessions over Furnace**: a session that takes bytes in and hands bytes
   out, with no networking of its own, Furnace's wire format, a WebSocket
   client, and compression.
4. **Furnace relays patches** instead of whole timelines.
5. **ORI's wire format** over RabbitMQ, for RV and xStudio.

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
