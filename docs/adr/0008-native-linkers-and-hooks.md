# ADR 0008: Media linkers and hooks in the core

- **Status:** Accepted
- **Date:** 2026-10-08
- **Deciders:** Jeff Hodges

## Context

Upstream OpenTimelineIO has two plugin points around every read and write.
A media linker is handed each clip a read produced and returns the media
reference to use instead. A hook script is handed the timeline at a named
hook (`post_adapter_read`, `post_media_linker`, `pre_adapter_write`,
`post_adapter_write`, or one an adapter declares) and returns what to go on
with. Both are Python modules, found through JSON plugin manifests.

The Python bindings already carried upstream's plugin system unchanged, and
upstream's tests for it pass. Jeff asked for linkers and hooks to be native
to the Rust core and reachable from every SDK, with Python still matching
upstream's approach and tests exactly.

## Options

1. **Manifests in the core.** The core reads `plugin_manifest.json` and
   `OTIO_PLUGIN_MANIFEST_PATH`. But a manifest names `.py` files, which a Go,
   Swift or C program cannot load, so the core would read manifests only to
   find nothing it could run.
2. **Python plugins run through the core.** Python registers each linker
   and hook it loads into the core, and reads go through the core's
   sequence. Upstream hands hooks arbitrary Python objects (its AAF adapter
   passes an open pyaaf2 file) and the caller's own dictionary, which a hook
   may change in place for the next one to see. Neither survives a trip
   through OTIO values, so upstream's behaviour would change.
3. **A registry of named functions in the core, with Python beside it.** The
   core registers linkers and hook scripts as functions and runs upstream's
   sequence. Python keeps upstream's manifests and runs its own plugins as
   upstream does, and also finds the native ones by name.

## Decision

Option 3. `otio_adapter::plugins` holds a process-wide `Registry`, as
upstream's active manifest is process-wide: named `MediaLinker`s, named
`HookScript`s, and declared hooks each with an ordered list of scripts. The
four adapter hooks are declared from the start, as upstream's built-in
manifest declares them. `after_read`, `before_write` and `after_write` run
upstream's `Adapter` sequence with upstream's arguments, quirk included:
`post_media_linker` is handed the linker's arguments, not the hook ones. The
default linker is the one `OTIO_DEFAULT_MEDIA_LINKER` names; an unknown one
is refused with upstream's message.

In Python, `media_linker.from_name`, `available_media_linker_names`,
`hooks.names`, `hooks.run` and the rest consult the manifests first, as
upstream does, then the native registry. A native plugin is handed its
arguments as OTIO values, leaving out any that have none.

## Consequences

- Python's behaviour with only manifest plugins is upstream's, and its tests
  run unmodified.
- The registry is locked only to look things up, never while a plugin runs,
  so a hook can register or run another.
- The C ABI and the SDKs reach the registry by registering a linker or hook
  script as a function pointer and context, and name a linker and argument
  maps in the read and write options; see
  [ADR 0009](0009-linkers-and-hooks-across-the-c-abi.md).
- Manifests stay Python's. A program in another language registers its
  plugins in code, which is how it would load them anyway.
