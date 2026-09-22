# OpenTimelineIO for Go

Read, write and edit OpenTimelineIO timelines from Go.

```go
import otio "github.com/jhodges10/otio-rust/sdk/go"
```

This package is generated from the C interface of the otio-rust core, so it
carries the whole data model: the schemas, the composition algorithms, the ten
edit operations and the file-format adapters. Do not edit the `.go` files here
by hand — see [`../README.md`](../README.md) for how they are made and
regenerated.

## Building

The package is cgo over `libotio`, and it links against a static library it
expects to find in `lib/`:

```sh
cargo build -p otio-capi --release
cp target/release/libotio.a sdk/go/lib/

cd sdk/go
go test ./...
```

The library itself is not checked in; `lib/.gitignore` keeps it out.

## Using it

Everything lives in a `Document`, which owns the objects in it:

```go
document, err := otio.ReadFromFile(otio.FormatCMX3600, "cut.edl", nil)
if err != nil {
	return err
}
defer document.Close()

root, err := document.Root()
if err != nil {
	return err
}
clips, err := root.FindClips()
```

`otio.Open` infers the format from the filename, and `document.Save` writes it
back the same way. A document is freed when it is collected, so `Close` is not
required; it is worth calling anyway, because it frees a whole timeline at
once and at a moment you chose.

An object is a `Node`: a handle, and the document it can be resolved against.
The schemas are Go types that embed one another the way the schemas derive
from one another, so a `Clip` has every method of `Item`, `Composable` and
`Node`. Ask a node what it is with its `As` method, and narrow a list with
`Filter`:

```go
if clip, ok := node.AsClip(); ok {
	reference, err := clip.MediaReference("")
}

for _, clip := range otio.Filter(clips, otio.Node.AsClip) {
	name, _ := clip.Name()
}
```

Where "there is nothing here" is one of the answers — an item with no source
range, a clip with no active media reference — the error is `ErrNoValue`, and
it means the question was answered rather than that something went wrong:

```go
span, err := clip.SourceRange()
if errors.Is(err, otio.ErrNoValue) {
	// the clip is untrimmed
}
```

Asking an object for something it does not have fails rather than answering
with a zero value: a clip asked for a track's kind returns an error saying so.
A handle into a document that has been closed goes stale rather than
dangling, and reports `StatusStaleHandle`.

The package documentation is the reference; every method carries the C ABI's
own documentation, rewritten for Go, and names the C function it calls.

## Following upstream

What things are called and which members exist follow upstream
OpenTimelineIO's own bindings rather than being invented here: a stored field
is a property and a computed one is a method, getters are bare nouns and
setters take a `Set` prefix, and the schema names and member names are
upstream's.

Three things deliberately differ, because Go is not Python:

- There is a `Document`. Upstream's bindings hand out reference-counted
  objects; this core owns its objects in an arena, so something has to own
  that arena and it is visible here.
- Failure is a Go `error`, not an exception or an out-parameter, and "no
  value" is the `ErrNoValue` sentinel rather than `None`.
- An absent string is the empty string. Go has no `Optional[str]` worth
  imposing on a caller, and the core draws no distinction between an unset
  name and an empty one.

[ADR 0003](../../docs/adr/0003-sdk-generation.md) records these in full.

## Supported platforms

Linux and macOS, on whatever architectures the Rust core builds for. Windows
is not tested: linking a Rust static library with MSVC needs an environment
CI does not have set up, and the C ABI job leaves it out for the same reason.
