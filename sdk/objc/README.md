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

## Media linkers and hooks

A media linker or a hook script is an object answering `OTIOMediaLinker` or
`OTIOHookScript`, registered under a name, and the library keeps it until the
name is unregistered or registered again:

```objc
@interface ProxyLinker : NSObject <OTIOMediaLinker>
@end

@implementation ProxyLinker
- (nullable OTIOMediaReference *)linkMediaReferenceForClip:(OTIOClip *)clip
                                                 arguments:(OTIOMetadata *)arguments
                                                     error:(NSError **)error {
    NSString *root = [arguments getString:@"root" error:error];
    NSString *name = [clip name:error];
    if (root == nil || name == nil) {
        return nil;
    }
    NSString *url = [NSString stringWithFormat:@"%@/%@.mov", root, name];
    return [OTIOExternalReference externalReferenceWithName:name targetURL:url error:error];
}
@end

OTIORegisterMediaLinker(@"proxies", [[ProxyLinker alloc] init], &error);

OTIOReadOptions options = OTIOReadOptionsDefault();
options.mediaLinker = @"proxies";
options.mediaLinkerArguments = @"{\"root\": \"/proxies\"}";
OTIOSerializableObject *timeline = OTIOReadFromBytes(OTIOFormatOTIOJSON, data, &options, &error);
```

A hook script is attached to a hook with `OTIOAttachHookScript`: one of the
four every read and write runs, or one of your own, which
`-runHook:arguments:error:` runs. Where the compiler has blocks, which is
always on Apple's platforms, `OTIORegisterMediaLinkerUsingBlock` and
`OTIORegisterHookScriptUsingBlock` take a block instead; GNUstep's legacy
runtime has none, so the protocols are the way in that works everywhere.

- **Failing is answering nil and setting the error.** A linker that answers
  nil and sets nothing leaves the clip as it is; a hook script must answer an
  object to go on with. The read or write fails with `OTIOStatusPluginError`
  and the plugin's own message.
- **An exception never reaches the library.** One raised in a plugin, or any
  object thrown, is caught where the library called it and becomes that
  failure.
- **What a plugin is handed is lent for the call.** It may change it, and may
  answer an object built fresh, which joins the lent timeline. Moving an
  object of that timeline into another one fails, closing it waits until the
  call is over, and an object of it kept past the call fails with
  `OTIOStatusNullPointer`, so nothing frees what the library still holds.
  That holds for `-runHook:arguments:error:` too, where the timeline lent is
  the caller's own.
