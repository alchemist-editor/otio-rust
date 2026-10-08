# ADR 0007: The AAF hooks without a pyaaf2 file

- **Status:** Accepted
- **Date:** 2026-10-08
- **Deciders:** Jeff Hodges

## Context

Upstream's `otio-aaf-adapter` declares four hooks of its own, beside the
four every adapter runs:

| Hook | When upstream runs it | What it returns |
| --- | --- | --- |
| `otio_aaf_pre_read_transcribe` | the file is open, nothing transcribed yet | ignored; handed `None` as the timeline |
| `otio_aaf_post_read_transcribe` | transcribed, before `_fix_transitions`, markers and `simplify` | what the passes run on |
| `otio_aaf_pre_write_transcribe` | the output file is open, nothing written yet | what is written |
| `otio_aaf_post_write_transcribe` | everything written, the file not yet closed | ignored |

Each is handed the caller's `hook_function_argument_map`, updated in place
with `read_filepath` or `write_filepath`, with `embed_essence` on a write, and
with `aaf_handle`: the open `aaf2.file.AAFFile`, pyaaf2's object for the
file. Upstream's own example plugin uses none of `aaf_handle`; its write
hook mocks transcoding `.mov` media to `.dnx` so `embed_essence` can take it,
which is the use upstream's error message points people to.

There is no pyaaf2 here. The AAF container is the `aaf` crate (ADR 0002),
and it is not exposed to Python.

## Options

1. **Run none of them.** What this package did before. A program relying on
   a transcoding hook writes an AAF with nothing embedded, or fails.
2. **Expose the `aaf` crate to Python as an `AAFFile` look-alike.** pyaaf2's
   object model is large (every class, property and stream), so anything
   short of all of it fails on whichever part a hook touches. It is a port
   of pyaaf2's Python API, which is a project of its own.
3. **Run all four where upstream runs them, with `aaf_handle=None`.** Every
   hook that works on the timeline, which is what the example and the
   transcoding use do, works unchanged. A hook that reaches into the file
   fails on `None`, plainly and at the line that does it.

## Decision

Option 3. The hooks run at upstream's points, with upstream's arguments, and
`aaf_handle` is `None`.

Three of the points are outside the Rust read or write, so the Python adapter
module runs those hooks itself. The fourth, `post_read_transcribe`, sits
between transcription and the passes, inside the Rust read, so `otio-aaf`
takes a callback there: `ReadOptions::with_post_transcribe(PostTranscribe)`.
It is handed the document and hands one back, and the passes run on what it
hands back, which may be a different document. A failure stops the read as
`Error::Hook`; from Python the hook's own exception is raised.

## Consequences

- The write hooks run before the file is created and after it is closed,
  rather than with it open. With no handle to use, a hook cannot tell.
- `post_read_transcribe` costs a copy of the document: what the hook returns
  is copied back, since the hook may keep hold of it. Nothing is copied when
  no hook is attached.
- A hook written against pyaaf2's `aaf_handle` does not work. If one turns up
  that a user needs, option 2 is still open, and this decision does not stand
  in its way: `aaf_handle` would become that object instead of `None`.
- The C ABI and the SDKs take no hooks. They have no plugin system to name
  one from; a caller there has its timeline in hand before and after a
  write already.
