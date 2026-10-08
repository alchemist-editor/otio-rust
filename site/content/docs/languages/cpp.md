---
title: C++
summary: Header-only C++17 over libotio, shaped like upstream's own C++ API, with objects as small values over an arena handle.
section: Languages
order: 6
---

The C++ SDK is a set of C++17 headers over `libotio`, the C ABI built from the
Rust core. It is generated from that ABI, so it carries the whole data model:
the schemas, the composition algorithms, the ten edit operations and the
file-format adapters. Its shape is upstream OpenTimelineIO's own C++ API: a
class per schema deriving as the schemas derive, `snake_case` members, values
as structs, and no document in the surface. Everything lives in namespace
`otio`.

```cpp
#include <opentimelineio/otio.hpp>
```

## Install

The headers are header-only. They call a static `libotio`, which the CMake
target expects to find in `sdk/cpp/lib/`. The library is not checked in, so
build it from the Rust core first:

```sh
cargo build -p otio-capi --release
cp target/release/libotio.a sdk/cpp/lib/

cmake -S sdk/cpp -B sdk/cpp/build
cmake --build sdk/cpp/build
ctest --test-dir sdk/cpp/build --output-on-failure
```

Each [GitHub release](https://github.com/alchemist-editor/otio-rust/releases)
also carries the C++ SDK's sources, laid out as in the repository with an
empty `lib/`, and a prebuilt `libotio-<version>-<target>.tar.gz` per target
whose `lib/` holds the library to put there.

## Your first program

<!-- ::sample id="read-an-edl" lang="cpp" -->

Reading hands back the file's root object, with no container around it to
hold. `otio::open` infers the format from the filename instead of taking one,
and `otio::save` writes back the same way. The rate is the one option an EDL
cannot do without; see [CMX 3600 EDL](/docs/formats/edl).

What an object really is, the library knows rather than the compiler:
`node.is<otio::Clip>()` asks, and `node.as<otio::Clip>()` hands back an
`std::optional<otio::Clip>` holding the clip where the answer is yes. They
take the place of `dynamic_cast`, because an object here is a value rather
than a retained pointer.

## Building a timeline

<!-- ::sample id="build-a-timeline" lang="cpp" -->

Each object is made on its own with a static `create` and joins a timeline
when you put it in one. `RationalTime` and `TimeRange` are structs.

## How objects and documents work here

An object is a small value over an arena handle. Underneath, the core keeps a
timeline's objects in an arena; the SDK does that bookkeeping, and objects keep
their timeline alive between them, so there is nothing to close. `close()`
exists for releasing a large one early, and every object that lived in it then
fails with `Status::NULL_POINTER` rather than reading freed memory.

Objects made apart stay apart until one takes the other in. Appending or
inserting an object that is still a child in another timeline is refused as
the library refuses it, with `Status::CORE_ERROR` and the library's own
message, and so is placing one whose handle has gone stale, with
`Status::STALE_HANDLE`. Both are refused before that timeline is brought over,
so both timelines stay whole and releasing one leaves the other working.

## Errors and missing values

A call that can fail throws an `otio::Error` carrying a `Status`, where
upstream's C++ takes an `ErrorStatus *` out-parameter.

A call that only *names* an object, such as `detach_child`, `index_of_child`
or `has_child`, refuses one that belongs to a different timeline before
asking the library, because merging the two and failing afterwards would
already have done the damage. That refusal is an `otio::OtherTimelineError`:
an `otio::Error` with `Status::INVALID_ARGUMENT` and a type of its own, so it
can be caught apart from the library's failures with
`catch (const otio::OtherTimelineError&)`.

Where "there is nothing here" is one of the answers, such as an item with no
source range or a clip with no active media reference, the call answers an
empty `std::optional`, because that is an answer rather than a failure:

```cpp
if (std::optional<otio::TimeRange> span = clip.source_range()) {
    std::cout << span->duration.value << "\n";
}
```

## Editing and algorithms

<!-- ::sample id="edit-operations" lang="cpp" -->

The ten edit operations and the algorithms are free functions in `otio`, such
as `otio::insert`, `otio::overwrite` and `otio::flatten_tracks`, because each
is about two objects and belongs to neither.
[Editing a timeline](/docs/guides/editing) says what each edit does.

## Formats

`otio::read_from_file` and `otio::write_to_file` take an `otio::Format`:
`OTIO_JSON`, `ALE`, `CMX_3600`, `FCP7_XML`, `FCPX_XML`, `AAF`, and the two
bundles, `OTIOZ` and `OTIOD`, which are read and written through a path only.

<!-- ::sample id="convert-a-format" lang="cpp" -->

See [Reading and writing files](/docs/guides/reading-and-writing),
[AAF](/docs/formats/aaf) and [Bundles](/docs/formats/bundles).

## Media linkers and hooks

<!-- ::sample id="link-media-and-run-hooks" lang="cpp" -->

A media linker and a hook script are `std::function`s, registered by name
with `otio::register_media_linker` and `otio::register_hook_script`. A linker
is handed each clip a read produced and the read's
`media_linker_arguments` as `otio::Metadata`, and answers an
`std::optional<otio::MediaReference>`: the reference the clip should use, or
empty to leave the clip alone. A hook script is handed what the hook runs on
and the `hook_arguments`, and answers the object to go on with, which may be
the one it was handed or another built fresh. Hook scripts run only at the
hooks `otio::attach_hook_script` attaches them to; `run_hook` runs a hook of
your own on any object.

An exception a linker or a script throws stops the read or write there, which
then throws an `otio::Error` with `Status::PLUGIN_ERROR` and the exception's
`what()`; nothing unwinds into the library. The registry is the whole
process's, so a name registered again replaces what was there, and the
`std::function` and whatever it captured are released when the name is
replaced or unregistered. A read may run on any thread, so a plugin must be
safe to call from any of them.

What a plugin is handed belongs to the timeline being read, and is valid only
for the call: keep nothing from it. That timeline is the library's to free
while the call runs, even where it is your own, as it is for `run_hook`. So
moving it into another, by appending what the plugin was handed to a track
built inside the call, is refused, and closing it does nothing.

## Platforms

CI builds and tests the SDK on Linux and macOS. Windows is left out, as it is
for the other SDKs that link the Rust static library through the platform's
C/C++ toolchain.

## Reference

- [`sdk/cpp/README.md`](https://github.com/alchemist-editor/otio-rust/blob/main/sdk/cpp/README.md).
- The [C ABI reference](/reference) the headers are generated from, and
  [ADR 0003](https://github.com/alchemist-editor/otio-rust/blob/main/docs/adr/0003-sdk-generation.md)
  for every deliberate departure from upstream's C++ API.
