# otio-ext-ori-annotations

ORI Annotations, an otio-rust [extension](../..). **Not part of
OpenTimelineIO**; the schemas belong to the ORI project.

```rust
otio_ext_ori_annotations::install();
otio_ext::enable(otio_ext_ori_annotations::ID)?;
```

- `schema`: ORI's schemas, registered under ORI's own names.
- `command`: typed paint, text and shape commands, read from a document or
  built with ORI's defaults.
- `annotations`: the `Annotations` track, annotation clips by media clip GUID
  and frame, adding, replacing and removing commands by uuid, and listing by
  clip, frame or person, and the merge rules live review uses.
- `review`: live review over a sync session: committing a comment with its
  strokes, folding a relayed one into the host's timeline, and strokes in
  progress (ORI's PARTIAL).
- `coords`: ORI's height-normalised, centre-origin coordinates to and from
  pixels and OpenTimelineIO image bounds.

See [the extension's README](..) and
[ADR 0012](../../../docs/adr/0012-ori-annotations.md).

The fixtures under `tests/fixtures` are ORI's; `ori_built.otio` is
regenerated with:

```sh
pip install opentimelineio==0.18.1
OTIO_PLUGIN_MANIFEST_PATH=<ORIAnnotations>/otio_event_plugin/plugin_manifest.json \
  python tests/fixtures/make_ori_built.py <ORIAnnotations> > tests/fixtures/ori_built.otio
```
