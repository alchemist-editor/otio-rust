# Timeline sync

**An otio-rust extension, not part of OpenTimelineIO.** Sharing an
OpenTimelineIO timeline between programs while it is edited.

ORI Sync and Alchemist's Furnace already share timelines this way: objects
named by `metadata.sync.guid`, changes sent as patches addressed by GUID. This
extension is one implementation of that, in Rust, reached from every SDK, with
wire profiles for both. [ADR 0011](../../docs/adr/0011-timeline-sync.md) has
the design and epic #115 the phases.

| Folder | What it is |
| --- | --- |
| [`otio-ext-sync`](otio-ext-sync) | The extension itself, in Rust |
| [`capi`](capi) | Its C interface, `otio_ext_sync_*`, compiled into `libotio` and declared in `capi/include/otio_ext_sync.h` |

So far it is the extension and its switch: `otio_ext_sync_enable()`, or
`otio_ext::enable("sync")` from Rust. Identity and patches come next (#117).
