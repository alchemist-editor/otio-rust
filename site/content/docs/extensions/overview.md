---
title: Extensions
summary: Work built on otio-rust's core that is not OpenTimelineIO, kept apart from it and off until a program turns it on.
section: Extensions
order: 1
---

Everything else on this site is OpenTimelineIO: the object model, the time
library, the file formats, ported from upstream and checked against upstream's
own tests. An **extension** is something more, built in the same repository on
the same core, that upstream OpenTimelineIO does not have.

Extensions are kept apart so nobody mistakes one for OpenTimelineIO:

- **Off until enabled.** An extension does nothing until a program turns it
  on. While it is off, any schema it owns reads as an unknown schema and is
  written back unchanged, as upstream OpenTimelineIO treats a schema whose
  plugin is not loaded. A file written with an extension on still opens in
  upstream's tools.
- **A module of its own in every language.** Importing OpenTimelineIO never
  pulls an extension in. You import the extension's module separately.
- **Its own C header.** The functions are in the same `libotio`, prefixed
  `otio_ext_<name>_`, and declared in `otio_ext_<name>.h` beside `otio.h`.

| Extension | What it does |
| --- | --- |
| [Timeline sync](/docs/extensions/timeline-sync) | Shares a timeline between programs while it is edited |

## Turning one on

Every language has the same two calls on the core, by extension id, and each
extension's module has its own `enable` and `is_enabled`:

| Language | Core | Sync's module |
| --- | --- | --- |
| Rust | `otio_ext::enable("sync")` | `otio_ext_sync::install()`, then `otio_ext::enable(otio_ext_sync::ID)` |
| Python | `opentimelineio.ext.enable("sync")` | `from opentimelineio.ext import sync; sync.enable()` |
| C | `otio_ext_enable("sync", &error)` | `otio_ext_sync_enable(&error)`, from `otio_ext_sync.h` |

Enabling an extension that is already on does nothing. Enabling one turns on
the extensions it depends on first. Enabling is for the whole process and
cannot be undone, as registering a schema cannot be.
