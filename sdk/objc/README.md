# OpenTimelineIO for Objective-C

Read, write and edit OpenTimelineIO timelines from Objective-C, on Apple's
runtime and on GNUstep's.

```objc
#import <OpenTimelineIO/OpenTimelineIO.h>

NSError *error = nil;
OTIODocument *document = [OTIODocument open:@"cut.edl" error:&error];
OTIOSerializableObject *root = [document root:&error];
if ([root isKindOfClass:[OTIOTimeline class]]) {
    OTIOTimeline *timeline = (OTIOTimeline *)root;
    for (OTIOSerializableObject *clip in [timeline findClips:&error]) {
        NSLog(@"%@", [clip name:&error]);
    }
}
[document close];
```

This code is generated from the C interface in `crates/otio-capi` by
`otio-sdk-gen`, so nothing here is edited by hand. What things are called
follows upstream OpenTimelineIO; what the binding *is* follows Cocoa, and
every deliberate departure is written down in
`docs/adr/0003-sdk-generation.md`.

## What it looks like

- **A class per schema**, deriving as the schemas derive, prefixed `OTIO`
  because the language has no namespaces. Every handle the library hands back
  arrives as the class its schema names, so `isKindOfClass:` tells the truth.
- **Values are C structs**, as `NSRange` and `CGRect` are, with
  `OTIORationalTimeMake` to build one and C functions to compute with one.
- **Failure is an `NSError` out-parameter**, in the `OTIOErrorDomain`, whose
  code is the `OTIOStatus`. A call that can fail answers `NO` or `nil`.
- **"There is nothing here" is a failure you can tell apart**: it fails with
  `OTIOStatusNoValue`, which `OTIOIsNoValue` recognises.
- **ARC, and also manual retain and release.** Everything the SDK owns is
  confined to the runtime, so the same sources build both ways.

## Building

The Rust library it calls into is built by cargo and copied into `lib/`:

```sh
cargo build -p otio-capi --release
cp target/release/libotio.a sdk/objc/lib/
```

Then, from this directory:

```sh
make          # build/libOpenTimelineIO.a
make check    # build and run the tests
```

On Linux this needs GNUstep's base library and a clang that can build
Objective-C: `gnustep-devel` and `clang` on Debian and Ubuntu. On macOS it
needs nothing but Xcode's command line tools.

## Memory

An `OTIODocument` owns every object in it, and every object holds its document,
so the arena outlives the handles into it. `-close` frees it at a moment you
chose; letting go of the last reference does the same thing later.
