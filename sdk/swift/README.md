# OpenTimelineIO for Swift

Read, write and edit OpenTimelineIO timelines from Swift.

```swift
import OpenTimelineIO
```

This package is generated from the C interface of the otio-rust core, so it
carries the whole data model: the schemas, the composition algorithms, the ten
edit operations and the file-format adapters. Do not edit the `.swift` files
under `Sources/OpenTimelineIO` by hand — see [`../README.md`](../README.md)
for how they are made and regenerated.

## Building

The package links against a static `libotio` it expects to find in `lib/`,
and the linker is told where that is on the command line:

```sh
cargo build -p otio-capi --release
cp target/release/libotio.a sdk/swift/lib/

cd sdk/swift
swift test -Xlinker -L"$PWD/lib"
```

The library itself is not checked in; `lib/.gitignore` keeps it out.

## Using it

Everything lives in a `Document`, which owns the objects in it:

```swift
let document = try Document.open("cut.edl")
defer { document.close() }

let timeline = try document.root()
for clip in try timeline.findClips() {
    print(try clip.name(), try clip.duration())
}
```

An object is a class of its schema, so `as?` asks what one really is:

```swift
for child in try track.children() {
    if let clip = child as? Clip, let reference = try clip.mediaReference() as? ExternalReference {
        print(try reference.targetURL())
    }
}
```

A call that can fail throws an `OTIOError` carrying a `Status`. Where "there
is nothing here" is one of the answers — an item with no source range, a clip
with no active media reference — the call answers `nil` instead, because that
is an answer rather than a failure:

```swift
if let span = try clip.sourceRange() {
    print(span)
}
```

## What this follows, and where it differs

The shape is OpenTimelineIO's own Swift bindings: a class per schema deriving
as the schemas derive, values as structs, real enums, `throws` for failure,
and compositions that are deliberately not Swift collections. Every
deliberate departure is written down in
[ADR 0003](../../docs/adr/0003-sdk-generation.md).
