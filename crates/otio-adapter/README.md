# otio-adapter

The trait that every OpenTimelineIO file-format adapter in this workspace
implements, together with the error type they share and the pieces of
metadata more than one of them writes.

Upstream OpenTimelineIO makes an adapter a Python plugin with four loosely
specified entry points and a bag of keyword arguments. Here it is one trait,
`Adapter`, with each format's read and write options named and typed, so that
an option meant for one format cannot be quietly handed to another.

`Adapter` is stated over bytes, since AAF is a binary container. Formats that
really are text — ALE, EDL, and the two FCP XML flavours — also implement
`TextAdapter`, which works in `&str` and `String`.

See `crates/otio-ale` for a worked example.

## Media linkers and hooks

`plugins` holds upstream's two plugin points as functions registered under a
name in a process-wide registry. A media linker is handed each clip a read
produced and returns the media reference to use instead; a hook script is
handed the whole timeline at a named hook and returns what to go on with.
`after_read`, `before_write` and `after_write` run upstream's sequence:
`post_adapter_read`, the linker on every clip, `post_media_linker`, and
`pre_adapter_write` and `post_adapter_write` around a write, with
upstream's arguments and its default linker from `OTIO_DEFAULT_MEDIA_LINKER`.

The registry holds functions, not manifests: a manifest names Python files,
which only the Python bindings can load. Those keep upstream's manifests and
find what is registered here by name. See
[ADR 0008](../../docs/adr/0008-native-linkers-and-hooks.md).
