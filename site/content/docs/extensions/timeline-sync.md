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

1. **The extension itself**, which is what ships today: sync can be enabled,
   and every SDK has its module.
2. **Identity and patches**: GUIDs, patches made by transactions or by
   comparing two versions of a timeline, and applying them.
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
| Swift | `import OTIOExtSync` | see the Swift SDK's README |
| TypeScript | `@alchemist-edit/otio/ext/sync` | `enable()` |
| C++ | `opentimelineio/ext/sync.hpp` | `otio::ext::sync::enable();` |
| C# | `using OpenTimelineIO.Ext.Sync;` | `SyncExtension.Enable();` |
| Objective-C | `OTIOExtSync.h` | see the Objective-C SDK's README |
| Zig | `otio.ext.sync` | `try otio.ext.sync.enable();` |

The design is in
[ADR 0011](https://github.com/alchemist-editor/otio-rust/blob/main/docs/adr/0011-timeline-sync.md).
