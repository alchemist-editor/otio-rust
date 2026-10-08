---
title: Media linkers and hooks
summary: Pointing every clip a read produces at the right media, and running code of your own at fixed points around a read or a write.
section: Guides
order: 3
---

A file that comes from somewhere else rarely points at media where it lives
here. An EDL names a reel, an AAF names a MobID, an `.otio` from another
facility names paths on its disks. Upstream OpenTimelineIO answers that with
two plugin points, and every language here has both.

A **media linker** is handed each clip a read produced, and answers with the
media reference that clip should use instead, or with nothing to leave it
alone. A read names the linker it wants.

A **hook script** is handed the whole result at a named point, and answers
with what the read or write goes on with: the same object changed, or another
one. Scripts are attached to a **hook**, in order, and each is handed what the
one before it returned.

<!-- ::sample id="link-media-and-run-hooks" -->

## Where they run

A read parses the file, then runs, in this order, which is upstream's:

| Step | Handed |
| --- | --- |
| `post_adapter_read` hook | the hook arguments, plus `adapter_arguments` and `media_linker_argument_map` |
| the media linker, once per clip | the media linker arguments |
| `post_media_linker` hook | the media linker arguments, not the hook ones, as upstream hands them |

A write runs `pre_adapter_write` before it writes and `post_adapter_write`
after. Writing to a file adds the path as `_filepath`. The pre-write hook
works on a copy, so the object you asked to write is left exactly as it was,
and what the hook returns is what gets written.

Any other hook name is one of your own. Attaching a script to it declares it,
and `run_hook` on an object runs it there and then.

## Choosing a linker

A read with no linker named uses the one the `OTIO_DEFAULT_MEDIA_LINKER`
environment variable names, or none, as upstream does. Naming one that was
never registered fails the read rather than skipping the step, because a
conform that silently did not link is worse than one that stopped. Asking for
no linking at all (`do_not_link_media`, upstream's
`MediaLinkingPolicy.DoNotLinkMedia`) leaves every clip alone, and the hooks
still run.

## Arguments

Each read or write can carry two argument maps: one for the linker and one
for the hooks. Outside Rust and Python they cross as JSON text, an object
whose values may be anything OTIO metadata can hold, timeline objects
included. Your function receives them as metadata it can read with the
metadata calls it already knows. A timeline object among the arguments is
moved into the document the plugin works on; one the plugin puts into the
timeline stays there, and the rest are removed once it has run.

## Failures

A linker or script that fails stops the read or write, and the error carries
its own message, prefixed with the plugin's name. In every language a
failure is whatever that language fails with, an error, an exception, a
panic, and it is turned into the library's `PLUGIN_ERROR` at the boundary
rather than crossing into code that cannot handle it.

## Registering

Plugins live in one registry per process, as upstream's do. Registering a
name again replaces what it named; unregistering says whether there was
anything to remove. A script that is unregistered while a hook still lists it
fails that hook when it next runs, as upstream fails one its manifest names
and cannot find, so detach it as well.

Python finds plugins the way upstream does, through plugin manifests that
name `.py` modules, and also sees every linker and script registered from any
other language in the same process. Every other language registers functions
in code, which is how a program in that language would load them anyway.
[ADR 0008](https://github.com/alchemist-editor/otio-rust/blob/main/docs/adr/0008-native-linkers-and-hooks.md)
and [ADR 0009](https://github.com/alchemist-editor/otio-rust/blob/main/docs/adr/0009-linkers-and-hooks-across-the-c-abi.md)
say why.
