# ADR 0010: Extensions that are not OpenTimelineIO

- **Status:** Accepted
- **Date:** 2026-10-19
- **Deciders:** Jeff Hodges

## Context

otio-rust is a port of OpenTimelineIO. Everything under `crates/` is either
OpenTimelineIO itself or one of its file-format adapters, and the C ABI and
seven generated SDKs expose exactly that.

Two pieces of work want to live beside it: timeline sync (sharing a timeline
between programs as it is edited, as ORI Sync and Alchemist's Furnace already
do; [ADR 0011](0011-timeline-sync.md)) and ORI Annotations (the review schemas
from the ORI Annotations project). More will follow, such as an effects
registry or OCIO colour. They should be built here, against the same arena and
through the same C ABI and SDK generator, so every language gets them. But
nobody reading the code, the docs or an SDK should mistake them for
OpenTimelineIO, and a program that does not ask for them should behave exactly
as it does today.

## Options

1. **Inside the existing crates.** Sync goes in `otio-core`, its C functions
   in `otio-capi`, its SDK surface beside the core's. Least work, but nothing
   marks the boundary, and the core grows schemas and behaviour upstream does
   not have.
2. **A separate repository per extension.** The boundary is obvious, but each
   one needs its own C library, its own copy of the SDK generator and its own
   packages, and it can only see what the core's public C ABI shows.
3. **Extensions in this repository, kept apart and switched off by default.**
   Each extension gets its own folder, crates, C header and SDK module, links
   into the one `libotio`, and does nothing until a program turns it on.

## Decision

Option 3.

- **Layout.** `extensions/<name>/` holds an extension's crates, named
  `otio-ext-<name>-*` (for sync: `otio-ext-sync`, `otio-ext-sync-transport`,
  `otio-ext-sync-capi`). `crates/` stays OpenTimelineIO only.
- **A framework crate.** `extensions/otio-ext` defines the trait every
  extension implements: a stable id (`sync`, `ori-annotations`) and version;
  the extensions it depends on; `register()`, which adds its schemas,
  upgrade and downgrade functions and hooks to the existing registries; and
  the schema names it owns, so a schema can be traced to the extension that
  brought it.
- **Off unless enabled.** A program calls `otio_ext::enable("sync")`, or
  `otio_ext_enable` through the C ABI. Enabling an extension enables what it
  depends on first. `otio_ext::enabled()` lists what is on. Enabling is
  process-wide, like the schema registry, and cannot be undone.
- **Off means upstream behaviour.** While an extension is off its schemas read
  as `UnknownSchema` and are written back unchanged, exactly as upstream
  OpenTimelineIO treats a schema whose schemadef plugin is not loaded.
- **No new `Node` variants.** As [ADR 0006](0006-subclassing-built-in-schemas.md)
  decided, an extension's schemas register as dynamic schemas or as
  subclasses of built-in ones, with typed Rust views on top.
- **One library, a module per SDK.** Extension C functions are compiled into
  the same `libotio` behind a Cargo feature that release builds turn on, under
  the prefix `otio_ext_<name>_*` and a header of their own. The SDK model
  learns modules: it scans a list of source folders, each tagged with a module
  name, and every backend writes an extension as its own module (Go
  `otio/ext/sync`, Swift `OTIOExtSync`, TypeScript
  `@alchemist-edit/otio/ext/sync`, C++ `otio::ext::sync`, C# `Otio.Ext.Sync`,
  Objective-C `OTIOExtSync*`, Zig `otio.ext.sync`). Importing OTIO never pulls
  in an extension by accident.
- **Python** links the Rust crates directly and exposes an extension as
  `opentimelineio.ext.<name>`.
- **Docs.** Extensions get their own section of the docs site, and every page
  in it says that extensions are not part of OpenTimelineIO.
- **CI.** `.github/ci/select-jobs.sh` gains an `extensions/*` arm, so a change
  to an extension alone does not run the format adapters' jobs.

## Consequences

- The boundary shows in the folder tree, the crate names, the C prefix, the
  header, every SDK's module path and the docs.
- A program that never calls `enable` sees no change, and files written by
  one that does still open in upstream OpenTimelineIO.
- The C ABI needs two general additions that extensions rely on:
  `otio_ext_enable`/`otio_ext_enabled`, and field access on dynamic and
  extension schemas, which ADR 0006 notes is missing.
- The SDK generator and `sdk/api.json` grow a module concept, which every
  backend must honour; drift still fails `cargo test`.
- An extension can reach the arena directly rather than through the C ABI, so
  it can do what the core cannot yet expose; the core's own invariants still
  apply to it.
