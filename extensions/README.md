# Extensions

**Nothing in this folder is part of OpenTimelineIO.**

`crates/` is OpenTimelineIO: the time library, the object model, the file
formats and the C ABI over them, ported from upstream. `extensions/` is work
built on top of it in this repository, against the same arena and through the
same C ABI and SDK generator, so every language gets it, but which upstream
OpenTimelineIO does not have and which no OpenTimelineIO file needs.

| Extension | What it is | Epic |
| --- | --- | --- |
| [`sync`](sync) | Timeline sync: sharing a timeline between programs while it is edited, over Alchemist's Furnace or ORI Sync | #115 |

Every extension follows [ADR 0010](../docs/adr/0010-extensions.md):

- It lives in `extensions/<name>/`, its crates are named `otio-ext-<name>-*`,
  and it implements the `Extension` trait from [`otio-ext`](otio-ext).
- It does nothing until a program enables it, with `otio_ext::enable` in Rust
  or `otio_ext_enable` in C. While it is off, any schemas it owns read as
  `UnknownSchema` and are written back unchanged, as upstream does.
- Its C functions are compiled into `libotio`, prefixed `otio_ext_<name>_`,
  and declared in a header of their own, `otio_ext_<name>.h`.
- Every SDK writes it as a module of its own, so importing OpenTimelineIO
  never pulls it in: Go `otio/ext/sync`, Swift `OTIOExtSync`, TypeScript
  `@alchemist-edit/otio/ext/sync`, C++ `otio::ext::sync`, C#
  `OpenTimelineIO.Ext.Sync`, Zig `otio.ext.sync`.
