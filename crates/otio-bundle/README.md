# otio-bundle

Writes and reads OpenTimelineIO file bundles: a timeline packaged with the
media it references. This is upstream's `opentimelineio/bundle.h`, which the
Python `otioz` and `otiod` adapters call.

```
cut.otioz / cut.otiod
├── version.txt      "1.0.0"
├── content.otio     the timeline, references rewritten to media/...
└── media/
    ├── shot_010.mov
    └── render.0001.exr ...
```

An `.otioz` is a zip archive, with `content.otio` deflated and the media
stored uncompressed so it can be read in place; an `.otiod` is the same layout
as a directory.

```rust,no_run
use std::path::Path;
use otio_bundle::{ReadOptions, WriteOptions};

# fn demo(document: &otio_core::Document, timeline: otio_core::NodeId) -> otio_bundle::Result<()> {
let options = WriteOptions {
    relative_media_base_dir: Some("/show/cut".into()),
    ..WriteOptions::default()
};
otio_bundle::write_otioz(document, timeline, Path::new("cut.otioz"), &options)?;
let read = otio_bundle::read_otioz(Path::new("cut.otioz"), &ReadOptions::default())?;
# let _ = read;
# Ok(())
# }
```

What happens to a reference that is not a file on disk (a generator, or an
`http://` URL) is up to the `MediaReferencePolicy`. Every media file lands
directly under `media/`, so two files with the same name in different
directories make the write fail rather than silently dropping one, as
upstream does.

A reference's `file://` URL becomes a path through
`otio_core::bundle::file_from_url`, re-exported here, which is also what the
Python `url_utils` module calls. It decodes `%` escapes as upstream's
`std::stoi` does, quirks included: `%4g` is byte 4, a `%` with fewer than two
characters after it is kept, and a `%` followed by nothing `stoi` can read as
hex, such as `%zz`, fails the write with `Error::InvalidEscape` (`stoi`),
under every policy, because upstream decodes the URL before it looks at the
policy.

## Zip and DEFLATE

Upstream writes with minizip-ng. The workspace takes no third-party
dependencies, so this crate carries its own zip reader and writer (ZIP64
included, for bundles over 4 GiB or 65,535 files) and its own DEFLATE: a
complete inflater, and a compressor that writes one fixed-Huffman block. Files
it writes open in any zip tool, and it reads what other zip tools write,
refusing encrypted entries and compression methods other than stored and
deflate. Extraction refuses any entry whose path would land outside the
target directory.

`tests/bundle.rs` is a port of upstream's `test_bundle.cpp`. Its ZIP64 test
writes about 16 GB and is `#[ignore]`d; run it with
`cargo test -p otio-bundle --release -- --ignored`.
