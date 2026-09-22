# OpenTimelineIO for Go

Read, write and edit OpenTimelineIO timelines from Go.

```go
import otio "github.com/alchemist-editor/otio-rust/sdk/go"
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

An object is a `Node`, and objects are built on their own and put together
afterwards:

```go
track, err := otio.NewTrack("V1", "Video")
clip, err := otio.NewClip("shot_01")
err = track.AppendChild(clip.Node)
```

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

Reading answers with what the file was about, and writing starts wherever it
is pointed:

```go
root, err := otio.ReadFromFile(otio.FormatCMX3600, "cut.edl", nil)
if err != nil {
	return err
}
defer root.Close()

clips, err := root.FindClips()
```

`otio.Open` infers the format from the filename and `otio.Save` writes it
back the same way. A timeline is released when it is collected, so `Close` is
not required; it is worth calling anyway, because it frees a whole timeline at
once and at a moment you chose.

The algorithms and the ten edit operations are functions of the package,
because each is about two objects and belongs to neither — which is how
upstream arranges them too:

```go
err = otio.Insert(clip.Node, track.Node, at, false, nil)
flat, err := otio.FlattenTracks([]otio.Node{lower, upper})
```

### Objects from another timeline

The core keeps its objects in arenas and an object is an index into one. This
package does that bookkeeping, so it is not something to hold: a new object
gets an arena of its own, and putting it into a timeline moves it there.
An object that is still a child in another timeline is the exception:
appending or inserting it is refused as the library refuses it, with
`StatusCoreError` and the library's own message. So is placing an object whose
handle has gone stale, with `StatusStaleHandle`. Both refusals come before the
object's timeline is brought over, so both timelines stay whole and closing
one leaves the other working.

What is left visible is `ErrOtherTimeline`, for the calls that only *name* an
object rather than placing one. Detaching a child that belongs to another
timeline is a mistake, not an instruction to merge the two, so it is refused
before the library is asked:

```go
if err := track.DetachChild(fromSomewhereElse); errors.Is(err, otio.ErrOtherTimeline) {
	// it was never in this track
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
A handle into a timeline that has been closed goes stale rather than
dangling, and reports `StatusStaleHandle`.

The package documentation is the reference; every method carries the C ABI's
own documentation, rewritten for Go, and names the C function it calls.

## Following upstream

What things are called and which members exist follow upstream
OpenTimelineIO's own bindings rather than being invented here: a stored field
is a property and a computed one is a method, getters are bare nouns and
setters take a `Set` prefix, and the schema names and member names are
upstream's.

Two things deliberately differ, because Go is not Python:

- Failure is a Go `error`, not an exception or an out-parameter, and "no
  value" is the `ErrNoValue` sentinel rather than `None`. So is refusing an
  object from another timeline, which is `ErrOtherTimeline` and carries no
  `Status`, because the library was never asked.
- An absent string is the empty string. Go has no `Optional[str]` worth
  imposing on a caller, and the core draws no distinction between an unset
  name and an empty one.

[ADR 0003](../../docs/adr/0003-sdk-generation.md) records these in full.

## Supported platforms

Linux and macOS, on whatever architectures the Rust core builds for. Windows
is not tested: linking a Rust static library with MSVC needs an environment
CI does not have set up, and the C ABI job leaves it out for the same reason.
