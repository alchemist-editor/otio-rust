# ADR 0009: Media linkers and hooks across the C ABI and the SDKs

- **Status:** Accepted
- **Date:** 2026-10-08
- **Deciders:** Jeff Hodges

## Context

[ADR 0008](0008-native-linkers-and-hooks.md) put a registry of named media
linkers and hook scripts in `otio-adapter`, ran upstream's sequence around
every read and write, and left the C ABI and the SDKs for a follow-up. This
is that follow-up. A linker or a hook script is code the caller wrote, so for
the first time the library calls back into the language that called it.

## Decision

**A plugin is a C function pointer, a context, and a release function.**
`otio_register_media_linker` and `otio_register_hook_script` take an
`OtioPluginFn`, a `void *context` handed to every call, and an optional
`OtioPluginReleaseFn` the library calls once nothing will call the plugin
again: when it is unregistered, when its name is registered again, or when
the registration itself fails. That is the shape every SDK can build a
closure on, which a bare function pointer is not, and the release is what
lets a garbage-collected or reference-counted language let go of the closure
at the right moment rather than leak it or free it early.

**Linkers and scripts share one signature.** Both are handed the document,
an object (the clip, or the hook's target), the arguments, a place for the
result, and room for a message. For a linker, leaving the result as the none
handle means "leave this clip alone", which is upstream's returning `None`;
for a script it is a failure, since a hook needs something to go on with.

**The arguments are an object in the lent document.** The library puts a
temporary collection in the document whose metadata is the argument map, and
removes it once the call returns. A plugin reads its arguments with the
metadata calls every SDK already has, so there is no second value
representation to marshal, and arguments that are timeline objects arrive
as objects in the same document.

**Callers pass arguments as JSON text.** The read options gain
`media_linker`, `do_not_link_media`, `media_linker_arguments` and
`hook_arguments`; the write options gain `hook_arguments`; and
`otio_node_run_hook` takes its arguments the same way. JSON is how the C ABI
already carries a whole metadata tree in one string, and it keeps the options
structs plain values every SDK already marshals. Objects in the JSON are
moved into the document the hooks run in, and once the plugins have run,
those that nothing in the document reaches are removed again, so an argument
a plugin did not use leaves nothing behind. A read or write only does this
work, and a write only copies, when a plugin is attached to one of its own
hooks.

**A failure is the plugin's status and its own words.** A plugin that
returns anything but OK fails the read, write or `run_hook` with
`OTIO_STATUS_PLUGIN_ERROR` and the message it wrote, prefixed with its name.
An unknown linker, an undeclared hook or an unregistered script is the same
status. The message room is a fixed 1024 bytes, enough for a sentence and
cheap to put on the stack every call. Every SDK turns its language's failure
(an error value, an exception, a panic) into that status at the boundary, so
nothing unwinds through Rust.

**A write's hooks run on a copy.** Upstream's `pre_adapter_write` can change
the timeline in place, which in Python is the caller's own object. Across the
C ABI the caller's document is borrowed immutably, so the write copies it,
runs the hooks on the copy, and writes what they return. The caller's
document is left exactly as it was.

**A read keeps only what its result reaches.** A read hook may answer with a
new object in place of what the adapter parsed. In Python the parsed
timeline is then collected; across the C ABI it would stay in the document,
unseen, so the read removes whatever the new root does not reach. Nothing
else can hold a freshly read document, so nothing a caller wanted goes.

**The adapter's own arguments are empty for C callers.** Upstream hands hooks
the adapter's keyword arguments as `adapter_arguments`. The C ABI's options
are typed structs rather than a keyword bag, so the map is present and empty.

**Only registration is written by hand.** Unregistering, attaching,
detaching and `run_hook` are ordinary calls, and the generator emits them in
every SDK like any other. Registering takes a callback, which no generator
can turn into a closure in each language's idiom, so each backend writes
`register_media_linker` and `register_hook_script` by hand beside its
runtime and lists the C functions as hidden. The description still carries
them, with the plugin, context and release parameters typed, so a change to
their signatures fails the drift check like any other.

### Per language

| SDK | A linker is | A failure is | The closure is kept alive by |
| --- | --- | --- | --- |
| Go | `func(Clip, Metadata) (Node, error)` | a returned `error`, or a recovered panic | a `cgo.Handle`, deleted on release |
| C# | `Func<Clip, Metadata, MediaReference?>` | any exception | a `GCHandle`, freed on release |
| C++ | `std::function<std::optional<MediaReference>(const Clip&, const Metadata&)>` | any exception | a heap object, deleted on release |
| Objective-C | an object answering `OTIOMediaLinker`, or a block where the compiler has blocks | an `NSError`, or any raised exception | a retain, given back on release |
| Swift | `(Clip, Metadata) throws -> MediaReference?` | any thrown error | an `Unmanaged` retain, released on release |
| Zig | a context pointer and a comptime `fn (Context, Clip, Metadata) anyerror!?Node` | a returned error, named by `@errorName` | the caller, who owns the context; the release is null |
| TypeScript | `(clip, args) => MediaReference \| undefined` | anything thrown, except a wasm trap | a `Map` entry keyed by an integer context, deleted on release |

The lent document is the library's for the length of the call. An SDK wraps
it without taking ownership, never frees it, and moves a result built in
another document into it the way it moves any object between documents.
Every SDK also refuses, for the length of the call, the two ways a plugin
could free that document behind the library's back: moving its objects out
into another document, which consumes the source, and closing the caller's
own handle on it, which is the same document when `run_hook` runs a hook on
an object the caller holds.

**Objective-C plugins are objects first, blocks second.** GNUstep's GCC
runtime, which the Linux build uses, has no blocks, so the portable form is
a delegate answering a protocol, upstream's `link_media_reference` as a
method; on Apple, a block is wrapped in such an object.

**Zig has no closures**, so a plugin is a context pointer and a function
known at compile time, as `std.sort` takes them, and each registration gets
its own trampoline. A Zig panic cannot be caught, so only a returned error
becomes a plugin failure. Zig keeps its document visible, so the lent one is
guarded by a per-thread list of documents on loan rather than a flag on the
opaque `*Document`.

**WebAssembly cannot be handed a function pointer from JavaScript.** The
wasm module imports a dispatcher from the host and registers a Rust
trampoline that forwards to it, with the context an integer key into the
package's table of functions. TypeScript plugins are handed their arguments
as a plain object, upstream's `argument_map`, which stays valid after the
call, and an async plugin is refused, since a plugin runs inside a
synchronous read.

Where a language has a typed media reference, a linker answers with one
rather than with the base object type. Upstream registers plugins only
through manifests, so the register calls have no upstream name to follow;
they are named as the rest of each SDK is.

## Consequences

- Every SDK has linkers and hooks, with the same sequence and arguments
  upstream runs, and Python sees the ones registered from any language in the
  same process.
- A callback runs on whatever thread called the read or write, holding no
  library lock, so a plugin may register, unregister or run hooks itself.
- AAF's reading log, which prints as it reads, could now cross the C ABI the
  same way. It is left out until someone needs it.
