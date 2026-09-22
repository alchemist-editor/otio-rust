# OpenTimelineIO for C#

Read, write and edit OpenTimelineIO timelines from C#.

```csharp
using OpenTimelineIO;
```

This project is generated from the C interface of the otio-rust core, so it
carries the whole data model: the schemas, the composition algorithms, the ten
edit operations and the file-format adapters. Do not edit the `.cs` files under
`OpenTimelineIO/` by hand — see [`../README.md`](../README.md) for how they are
made and regenerated.

## Building

The assembly loads a native `libotio` it expects to find beside itself, which
the project copies out of `lib/`:

```sh
cargo build -p otio-capi --release
cp target/release/libotio.so sdk/csharp/lib/     # libotio.dylib on macOS

cd sdk/csharp
dotnet run --project tests
```

The library itself is not checked in; `lib/.gitignore` keeps it out.

## Using it

Reading a file hands back the object it is about:

```csharp
var timeline = Otio.Open("cut.edl");

foreach (var child in timeline.FindClips())
{
    var clip = (Clip)child;
    Console.WriteLine($"{clip.Name()} {clip.Duration()}");
}
```

Building one is the other direction. Every object is made on its own and joins
a timeline when you put it into one, so nothing has to exist before the thing
it goes into:

```csharp
var timeline = new Timeline("Cut");
var stack = new Stack("tracks");
var track = new Track("V1", "Video");

timeline.SetTracks(stack);
stack.AppendChild(track);
track.AppendChild(new Clip("shot_01"));

Otio.Save(timeline, "cut.otio");
```

An object that has not joined anything is a timeline of one. Putting it into
another moves it there, and an object from a timeline it was never put into is
refused rather than quietly dragged along with everything around it.

An object is a class of its schema, so a cast asks what one really is:

```csharp
foreach (var child in track.Children())
{
    if (child is Clip clip && clip.MediaReference(null) is ExternalReference reference)
    {
        Console.WriteLine(reference.TargetUrl());
    }
}
```

A call that can fail throws an `OtioException` carrying a `Status`. Where
"there is nothing here" is one of the answers — an item with no source range, a
clip with no active media reference — the call answers `null` instead, because
that is an answer rather than a failure:

```csharp
if (clip.SourceRange() is TimeRange span)
{
    Console.WriteLine(span);
}
```

## What this follows, and where it differs

C# has no upstream OpenTimelineIO binding to copy, so what things are *called*
follows upstream's Python and C++ — the schema names, the member names, the
bare-noun getter and the `Set` prefix — spelled the way .NET spells names, and
what the binding *is* follows upstream's Java bindings, which are the nearest
thing upstream has to a managed language.

There is no document in the surface, as there is none in upstream's own
bindings. Underneath, the core keeps a timeline's objects in an arena and an
object is an index into one; this SDK does that bookkeeping. The objects hold
the arena between them, so it goes when the last of them does and there is
nothing to dispose. `Close()` is there for releasing a large timeline at a
moment you chose; every object that lived in it fails afterwards rather than
reading freed memory.

Every deliberate departure is written down in
[ADR 0003](../../docs/adr/0003-sdk-generation.md).
