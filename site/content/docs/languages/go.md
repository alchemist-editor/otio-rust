---
title: Go
summary: A cgo package over libotio, with methods, embedded structs for the schema ladder, and an error for every failure.
section: Languages
order: 4
---

The Go package is cgo over `libotio`, the C ABI built from the Rust core. It
is generated from that ABI, so it carries the whole data model: the schemas,
the composition algorithms, the ten edit operations and the file-format
adapters. What things are called follows upstream OpenTimelineIO's own
bindings; what the package *is* follows Go, with methods, slices, strings and
an `error` rather than out-parameters and status codes.

```go
import otio "github.com/alchemist-editor/otio-rust/sdk/go"
```

## Install

The package links against a static `libotio` it expects to find in
`sdk/go/lib/`. The library is not checked in, so build it from the Rust core
and copy it there:

```sh
cargo build -p otio-capi --release
cp target/release/libotio.a sdk/go/lib/

cd sdk/go
go test ./...
```

Each [GitHub release](https://github.com/alchemist-editor/otio-rust/releases)
also carries the Go SDK's sources, laid out as in the repository with an empty
`lib/`, and a prebuilt `libotio-<version>-<target>.tar.gz` per target whose
`lib/` holds the library to put there.

## Your first program

<!-- ::sample id="read-an-edl" lang="go" -->

Reading answers with what the file was about, the root object, and there is no
container around it to hold. `otio.Open` infers the format from the filename
instead of taking one, and `otio.Save` writes back the same way. The rate in
`ReadOptions` is the one thing an EDL cannot tell you; see
[CMX 3600 EDL](/docs/formats/edl).

`otio.Filter` narrows a list of nodes with one of the `As` methods, here
keeping only the clips.

## Building a timeline

<!-- ::sample id="build-a-timeline" lang="go" -->

Objects are built on their own with `New` functions and put together
afterwards. The schemas are Go types that embed one another the way the
schemas derive from one another, so a `Clip` has every method of `Item`,
`Composable` and `Node`, and `clip.Node` is what a call taking any object
wants. Ask a node what it really is with its `As` method:

```go
if clip, ok := node.AsClip(); ok {
	reference, err := clip.MediaReference("")
}
```

## How objects and documents work here

The core keeps its objects in arenas and an object is an index into one. This
package does that bookkeeping, so there is no document to hold: a new object
gets an arena of its own, and putting it into a timeline moves it there.

A timeline is released when it is collected, so `Close` is not required. It is
worth calling anyway, because it frees a whole timeline at once and at a
moment you chose. A handle into a timeline that has been closed goes stale
rather than dangling, and reports `StatusStaleHandle`.

Appending or inserting an object that is still a child in another timeline is
refused as the library refuses it, with `StatusCoreError` and the library's
own message. Placing an object whose handle has gone stale is refused with
`StatusStaleHandle`. Both refusals come before the object's timeline is
brought over, so both timelines stay whole.

## Errors and missing values

Failure is a Go `error`. Two sentinels are worth knowing.

Where "there is nothing here" is one of the answers, such as an item with no
source range or a clip with no active media reference, the error is
`ErrNoValue`. It means the question was answered, not that something went
wrong:

```go
span, err := clip.SourceRange()
if errors.Is(err, otio.ErrNoValue) {
	// the clip is untrimmed
}
```

A call that only *names* an object rather than placing one refuses one from
another timeline before the library is asked, with `ErrOtherTimeline`.
Detaching a child that belongs to another timeline is a mistake, not an
instruction to merge the two:

```go
if err := track.DetachChild(fromSomewhereElse); errors.Is(err, otio.ErrOtherTimeline) {
	// it was never in this track
}
```

`ErrOtherTimeline` carries no `Status`, because the library was never asked.
Asking an object for something it does not have fails rather than answering
with a zero value: a clip asked for a track's kind returns an error saying so.
An absent string is the empty string, since the core draws no distinction
between an unset name and an empty one.

## Editing and algorithms

<!-- ::sample id="edit-operations" lang="go" -->

The algorithms and the ten edit operations are functions of the package rather
than methods, because each is about two objects and belongs to neither, which
is how upstream arranges them too: `otio.Insert`, `otio.Overwrite` and the
rest, beside `otio.FlattenStack`, `otio.FlattenTracks` and
`otio.TrackTrimmedToRange`. [Editing a timeline](/docs/guides/editing) says
what each edit does.

## Formats

`otio.ReadFromFile` and `otio.WriteToFile` take a format:
`FormatOTIOJSON`, `FormatALE`, `FormatCMX3600`, `FormatFcp7XML`,
`FormatFcpxXML`, `FormatAAF`, and the two bundles, `FormatOTIOZ` and
`FormatOTIOD`. A bundle is read and written through a path only.

<!-- ::sample id="write-a-bundle" lang="go" -->

See [Reading and writing files](/docs/guides/reading-and-writing),
[AAF](/docs/formats/aaf) and [Bundles](/docs/formats/bundles).

## Media linkers and hooks

A media linker is a `func(clip otio.Clip, arguments otio.Metadata) (otio.Node, error)`
registered under a name with `otio.RegisterMediaLinker`; a read names it in
`ReadOptions.MediaLinker`. Return the zero `Node` to leave a clip alone. A hook
script is a `func(target otio.Node, arguments otio.Metadata) (otio.Node, error)`
registered with `otio.RegisterHookScript` and attached to a hook with
`otio.AttachHookScript`. The arguments arrive as metadata; a read carries
them as JSON in `MediaLinkerArguments` and `HookArguments`.

A returned error, or a panic, fails the read with `StatusPluginError` and the
function's own message. The objects a plugin is handed belong to the read in
progress, so keep none of them past the call. See
[Media linkers and hooks](/docs/guides/media-linkers-and-hooks).

## Platforms

Linux and macOS, on whatever architectures the Rust core builds for. Windows
is not tested: linking a Rust static library with MSVC needs an environment CI
does not have set up.

## Reference

- [`sdk/go/README.md`](https://github.com/alchemist-editor/otio-rust/blob/main/sdk/go/README.md).
- The package documentation: every method carries the C ABI's own
  documentation, rewritten for Go, and names the C function it calls.
- The [C ABI reference](/reference) the package is generated from, and
  [ADR 0003](https://github.com/alchemist-editor/otio-rust/blob/main/docs/adr/0003-sdk-generation.md)
  for where it departs from upstream's shape.
