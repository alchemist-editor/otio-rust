---
title: Bundles (.otioz and .otiod)
summary: Packaging a timeline with its media, and what happens to media that is not a file.
section: Formats
order: 5
---

A bundle is how a cut travels with its media: the timeline and every file it
references, in one archive or one directory, so it can be moved to another
machine and opened there.

```text
cut.otioz / cut.otiod
├── version.txt      "1.0.0"
├── content.otio     the timeline, references rewritten to media/...
└── media/
    ├── shot_010.mov
    └── render.0001.exr ...
```

An `.otioz` is a zip archive, with `content.otio` deflated and the media
stored uncompressed so it can be read in place; an `.otiod` is the same
layout as a directory. The `otio-bundle` crate ports upstream's `bundle.cpp`.

## Writing a bundle

A bundle is how a cut travels with its media. Writing one copies every file
the timeline's media references name into `media/` and points each reference
at its copy, so the bundle can be moved to another machine and opened there.
Every file lands directly under `media/`, so two media files with the same
name in different directories cannot both go in, and the write fails rather
than dropping one. Nor is a bundle ever written over: a path that already
exists stops the write.

<!-- ::sample id="write-a-bundle" -->

Media that is not a file on disk, such as a URL on the web or a generator,
is what upstream's media reference policy is for. By default it stops the
write; told to, the writer replaces each such reference with a missing
reference instead, or replaces every reference and bundles no media at all.
A relative media path is found from the working directory unless you name
another. Reading an `.otioz` reads only the timeline out of the archive
unless you ask for it to be unpacked into a directory, which must not exist
yet; either bundle can then have its references rewritten as absolute paths
into it.

From Rust these are `otio_bundle::WriteOptions` and `ReadOptions`, and from
Python upstream's `media_policy`, `relative_media_base_dir` and
`extract_to_directory` keyword arguments. From C and the SDKs, the bundle
formats are `OTIO_FORMAT_OTIOZ` and `OTIO_FORMAT_OTIOD`, and the options are
fields of the read and write options: `bundle_media_policy` and
`bundle_media_base_dir` for writing, `bundle_extract_path` and
`bundle_absolute_media_paths` for reading. A bundle is read and written
through a path, never as bytes, and the calls that take bytes refuse one.
The TypeScript package has no file system to keep a bundle on, so it has no
bundle formats at all.

## Media URLs with unusual characters

Media URLs are percent-decoded exactly as upstream decodes them, so a URL
with a `%` that is not followed by a hex digit, such as `a%zz.mov`, fails the
write (`ValueError("stoi")` from Python) whatever the policy, as it does
upstream.

An escape that spells a byte that is not UTF-8, such as `%E9` in
`caf%E9.mov`, names a file whose name has that raw byte, and on Linux (or any
Unix filesystem that allows such a name) that file is found and bundled.
Inside the bundle it is named `caf%E9.mov`, with the escape spelled out,
because upstream's raw name makes `content.otio` invalid JSON. From Python,
`url_utils.filepath_from_url` on such a URL raises `UnicodeDecodeError`, as
upstream's does.

The [crate's README](https://github.com/alchemist-editor/otio-rust/blob/main/crates/otio-bundle/README.md)
covers the zip implementation and the Rust options.
