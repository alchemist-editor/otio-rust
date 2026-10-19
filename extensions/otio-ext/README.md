# otio-ext

The framework otio-rust's [extensions](..) are built on. Extensions are not
part of OpenTimelineIO.

An extension implements `Extension`: a stable id, a version, the extensions
it depends on, the schema names it owns, and `register()`, which adds its
schemas and hooks to the core's registries. `install` makes one available;
`enable` turns it on, dependencies first, and is process-wide and permanent,
like the core's schema registry. Until an extension is enabled its schemas
read as `UnknownSchema` and round-trip unchanged.

See [ADR 0010](../../docs/adr/0010-extensions.md).
