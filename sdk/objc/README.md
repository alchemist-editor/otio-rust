# OpenTimelineIO for Objective-C

Read, write and edit OpenTimelineIO timelines from Objective-C, on Apple's
runtime and on GNUstep's.

```objc
#import <OpenTimelineIO/OpenTimelineIO.h>

NSError *error = nil;
OTIOSerializableObject *timeline = OTIOOpen(@"cut.edl", &error);
for (OTIOSerializableObject *clip in [timeline findClips:&error]) {
    NSLog(@"%@", [clip name:&error]);
}
```

Building one is the other direction. Every object is made on its own and joins
a timeline when you put it into one, so nothing has to exist before the thing
it goes into:

```objc
OTIOTimeline *timeline = [OTIOTimeline timelineWithName:@"Cut" error:&error];
OTIOStack *stack = [OTIOStack stackWithName:@"tracks" error:&error];
OTIOTrack *track = [OTIOTrack trackWithName:@"V1" kind:@"Video" error:&error];

[timeline setTracks:stack error:&error];
[stack appendChild:track error:&error];
[track appendChild:[OTIOClip clipWithName:@"shot_01" error:&error] error:&error];

OTIOSave(timeline, @"cut.otio", &error);
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
- **So is an object from another timeline.** A call that only names an object
  refuses one from elsewhere before asking the library, with
  `OTIOStatusInvalidArgument`, and `OTIOIsOtherTimeline` recognises it.
- **An object that already has a parent is refused before it moves.**
  Appending or inserting one that is still a child in another timeline fails
  as the library would fail it, with `OTIOStatusCoreError` and its message,
  and placing one whose handle has gone stale fails with
  `OTIOStatusStaleHandle`, but before that timeline is brought over, so both
  stay whole.
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

There is no document in the surface. Underneath, the core keeps a timeline's
objects in an arena and an object is an index into one; this SDK does that
bookkeeping. The objects hold the arena strongly between them, so it outlives
every handle into it and goes when the last object naming it does.

An object that has not joined anything is a timeline of one. Putting it into
another moves it there, and an object from a timeline it was never put into is
refused rather than quietly dragged along with everything around it.

`-[OTIOSerializableObject close]` is there for releasing a large timeline at a
moment you chose. Every object that lived in it fails with
`OTIOStatusNullPointer` afterwards rather than reading freed memory.
