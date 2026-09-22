---
title: How the SDKs are made
summary: One description of the C ABI, read out of its own source, becomes every binding.
section: The data model
order: 2
---

Nobody writes these SDKs by hand, and nobody writes an interface description
by hand either.

```text
crates/otio-capi/src        the C ABI, in Rust
        │
        │  otio-sdk-model reads it
        ▼
sdk/api.json                every call, its parameters, its docs, its role
        │
        │  otio-sdk-gen writes one SDK per language
        ▼
sdk/go  sdk/swift  sdk/zig  sdk/cpp  sdk/csharp  sdk/objc  …   and this site's reference pages
```

The description is committed as [`sdk/api.json`](https://github.com/alchemist-editor/otio-rust/blob/main/sdk/api.json),
so a change to the C ABI shows up as a readable diff rather than as a
surprise inside generated code. `cargo test -p otio-sdk-gen` fails when what
is checked in is not what the generator would write today, which is what CI
runs.

Seven kinds of drift fail that check rather than reaching a user: a C
function that fits no naming convention, one whose body does something the
description cannot express, a schema the generator does not know about, two
functions that would collide in a target language, a call that consumes a
document, a struct whose computed layout disagrees with the size the ABI
asserts, and generated files that differ from the ones committed.

## What "idiomatic" means here

A generated API that reads like a transliteration of C is a failure even if
every function is present. Each backend owns the shape of its own language:
Go gets methods, embedded structs for the schema ladder, slices, strings, an
`error` and a sentinel for "there is no value" — not two-pass out-parameters
and status codes.

And the shape of the API itself — what things are called, which members
exist, which are properties and which are methods — follows upstream
OpenTimelineIO's own bindings for that language rather than being invented
here. Where a generated SDK departs from upstream's shape, the departure is
written down in
[ADR 0003](https://github.com/alchemist-editor/otio-rust/blob/main/docs/adr/0003-sdk-generation.md)
with its reason.

Zig is the interesting exception, because it has no upstream binding to copy.
It is also the one target that keeps the document visible, and the reasoning
is worth reading if you want to know what the other bindings are hiding.

## Every SDK is held to the same behaviour

Each SDK tests its own surface in its own language, which catches a binding
that is broken but not one that works and quietly disagrees with the others.
So a small set of scenarios is written once, as data beside the description
([`sdk/conformance.json`](https://github.com/alchemist-editor/otio-rust/blob/main/sdk/conformance.json)),
and rendered by the generator into every SDK's own test framework: build this
timeline and it writes exactly this JSON; a removed object's handle is stale;
an object from another timeline is refused before the library is asked, and
the other timeline is left whole. Each language's CI job runs them with the
rest of its tests.

A failure in them is named by kind — the library's status, or the binding's
own refusal — never by message text, so a scenario asks the same question in
Go's `error`, Swift's `throws` and Zig's error unions. Zig keeps the document
visible, so the scenarios about what hiding it obliges a binding to do are
marked as not applying there, and the generator checks that the ones every
language runs mean the same thing on both sides of that fork.

That rule means every SDK lets you recognise the refusal of another
timeline's object without reading its message:

| SDK | How to recognise it |
| --- | --- |
| Go | `errors.Is(err, otio.ErrOtherTimeline)` |
| Swift | `error.isOtherTimeline` on `OTIOError` |
| Zig | `error.ForeignObject` |
| TypeScript | `err instanceof OtherTimelineError` |
| C++ | `catch (const otio::OtherTimelineError&)` |
| C# | `catch (OtherTimelineException)` |
| Objective-C | `OTIOIsOtherTimeline(error)` |

In Swift, C++, C# and Objective-C the refusal still carries the
invalid-argument status it always had, so code that checked for that keeps
working.

## This site is generated from the same file

The [reference section](/reference) reads `sdk/api.json` at build time: the
groups, the calls, their parameters and their documentation all come from the
description, and each C declaration is lifted verbatim out of the committed
header rather than reassembled from parts. A page here cannot describe a call
the library does not have.

The prose and the code samples are written by hand, because they are
explanation rather than interface. What keeps the samples honest is that
each one is compiled — or, for Python, run — by its own language's toolchain
against the real SDK, in the same CI job that tests that SDK. A change that
leaves a sample describing an API that no longer exists fails the build that
made it. A build-time check also requires every sample to exist in every
language the switcher offers, or to say in writing why it cannot.
