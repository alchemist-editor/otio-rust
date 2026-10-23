# ADR 0013: An effects schema, as a prototype

- **Status:** Proposed. This is a prototype on a branch, kept off `main` while
  feedback comes in.
- **Date:** 2026-10-23
- **Deciders:** Jeff Hodges

## Context

In upstream OpenTimelineIO an effect is a name and an `effect_name`. Nothing
says which effect it is apart from the host that wrote it, and its settings
travel, if at all, in that host's metadata. A transition has a
`transition_type` and nothing else. So an effect cannot be recognised in
another host, let alone rebuilt there.

Jeff has proposed a schema that fixes this ("OpenTimelineIO Effects Schema
Proposal"), and it is being prototyped twice against one shared spec: in
C++ and Python on a fork of OpenTimelineIO (branch `jh/effects-schema-v1`),
and here. Both prototypes vendor the same registry document,
`otio_fx_registry.json`, byte for byte. The spec is authoritative: where
this ADR and the spec disagree, the spec wins and this ADR is wrong.

Briefly, the spec says:

- `Effect.2` adds `effect_id`, `effect_version`, `host`, `host_version`,
  `context` and `parameters`. All default to empty, are always written after
  `enabled` in that order, and are read if present. `TimeEffect`,
  `LinearTimeWarp` and `FreezeFrame` stay at version 1 and carry the fields
  because they inherit them.
- `Transition.2` adds `effect`, an `Effect` or null, written after `enabled`.
- Going down to version 1, non-empty values move into `metadata["otio.fx"]`
  next to what is already there. Going up, they move back, and `otio.fx` is
  dropped if it is left empty. A round trip loses nothing.
- An effects linker plugin point mirrors media linkers name for name:
  `EffectsLinker.1`, `link_effect(in_effect, parent,
  effects_linker_argument_map)`, effects linking after media linking, and a
  `post_effects_linker` hook after that.
- A reference linker, `otio_fx_registry`, maps effects onto the `otio.fx.*`
  registry. It is built in and is not the default.

## Decisions

### The core

The six fields are plain fields on `EffectData`, so every kind of effect
has them. `Transition` gains `effect: Option<NodeId>`, an owned link like a
timeline's stack. Ownership, cloning, removal and every walk over what an
object holds (`visit_links_mut`, `visit_owned`, the reachability walks in the
AAF simplifier and in the C ABI's plugin code) include it. Like a stack
`set_tracks` replaces, an effect a transition lets go of stays in the
document, held by nothing.

The version changes are ordinary registered upgrade and downgrade functions
in `otio_core::upgrade`, over the field maps, as every other schema change
here is. The release map's development label is `0.19.0.dev1`, to match the
fork's `CORE_VERSION_MAP`, with `Effect` 2, `Transition` 2 and
`EffectsLinker` 1. Writing with the `0.18.1` map downgrades both. A
downgraded object is written with its keys in sorted order, which is how
this library writes every downgraded object.

The context values are not checked, as the spec says.

### The plugin point

Effects linkers go in the process-wide registry of ADR 0008, beside media
linkers and hook scripts. An `EffectsLinker` is handed the document, the
effect, what holds it (or nothing), and the arguments. It answers with the
effect to use, or nothing to keep the one it was handed. A linker that
answers with something that is not an effect fails the read, as a media
linker that answers with something other than media does.
`plugins::link_effects` is the walk the spec describes.

`after_read` runs the effects linker after `post_media_linker` and then the
`post_effects_linker` hook, which is handed the effects linker arguments, as
`post_media_linker` is handed the media linker ones. The hook arguments gain
`effects_linker_argument_map`, as upstream's `hook_function_argument_map`
does. The hook is one of the five the library declares for every read and
write.

The reference linker is written in Rust, in
`otio_adapter::otio_fx_registry`, and the registry document is compiled in.
It is registered when the registry is first used, which is why it is built
in, but `LinkerChoice::Default` never picks it unless
`OTIO_DEFAULT_EFFECTS_LINKER` names it. A Rust test checks that the copy
compiled in and the copy the Python package ships are the same bytes.

### Python

The Python package mirrors the fork's API exactly. `opentimelineio.adapters`,
`adapter.py`, `exceptions.py`, the plugin manifest class, the built-in
manifest and the plugin documentation generator take the fork's changes
word for word, and `effects_linker.py` is the fork's module with one
addition: as `media_linker.py` does here, it also finds linkers registered
in Rust, by name, after the ones manifests declare.

Three things differ from the fork, deliberately:

- `adapters/otio_fx_registry.py` keeps the fork's interface (`LINKER_NAME`,
  `REGISTRY_PREFIX`, `DEFAULT_REGISTRY_PATH`, `load_registry`), its
  docstrings and the JSON beside it, but its `link_effect` hands the effect
  to the native linker. So there is one implementation of the mapping for
  Python, the C ABI and every SDK, and they cannot drift apart. The cost is
  one behaviour: a registry entry missing `effect_version` or `context` is
  read as empty here, where the fork's Python raises `KeyError`.
- Python reaches the built-in linker through the manifest entry that lists
  it, as upstream reaches its built-ins. The list of linkers registered in
  Rust that Python sees leaves the built-in out, so it is not listed twice,
  and a test that replaces the manifest does not find it anyway.
- `hooks.names()` lists a hook the library declares for every read and
  write only when a manifest declares it or Rust has attached a script to
  it. Without that, every manifest would gain `post_effects_linker`,
  including the ones upstream's own hook tests use, which were written
  before the hook existed. That changes nothing a read does: a hook with no
  scripts runs nothing.

The fork's new tests (`test_effect_v2.py`, `test_effects_linker.py`), their
baselines, its `utils.py` and its updated `empty_effect.json` and
`empty_transition.json` are vendored unmodified into `tests/upstream`, with
the fork's `CORE_VERSION_MAP.cpp` and generated schema and plugin documents
in `tests/upstream_root`.

### The C ABI

- `otio_effect_*` getters and setters for the five strings, and
  `otio_effect_parameters` / `otio_effect_set_parameters`, which carry the
  dictionary as JSON text. That is how the C ABI already carries argument
  maps; there is no per-key parameters API, because generator parameters
  have none either.
- `otio_transition_effect` (`OTIO_STATUS_NO_VALUE` when there is none) and
  `otio_transition_set_effect`, which takes `otio_node_none` to clear it.
- `OtioEffectsLinkerFn`, a callback of its own, registered with
  `otio_register_effects_linker` and removed with
  `otio_unregister_effects_linker`. It is the media linker callback plus a
  `parent` argument, because upstream's `link_effect` takes the parent and a
  linker needs it to reach the track or the clip. The context and release
  convention is ADR 0009's.
- `otio_node_link_effects` runs a named linker over an object outside a
  read, as `plugins::link_effects` does.
- `OtioReadOptions` gains `effects_linker`, `do_not_link_effects` and
  `effects_linker_arguments` at its end. A caller compiled against the older
  struct passes the older `struct_size`, and the new fields take their
  defaults.

### The SDKs

All seven SDKs are regenerated from the C ABI, so each has the Effect
fields, a transition's effect, the new read options and
`link_effects` without any hand-written code. The generator learns the new
callback type as `plugin<effects_linker>` in `sdk/api.json`.

What is left out: no SDK can yet register an effects linker written in its
own language. Each backend lists `otio_register_effects_linker` as handled
elsewhere, with that reason, so the omission is recorded rather than silent.
Media linkers and hook scripts are registered by hand-written code in each
SDK (ADR 0009), and doing the same for effects linkers seven times is
better left until the schema has settled.

## Consequences

- Every `Effect.1` and `Transition.1` read by this library is written back
  as version 2. Expected outputs that this changes were regenerated by
  reading each one and writing it again, and every changed line is a schema
  label, one of the new empty fields, or the comma before them. That is
  the AAF crate's baselines; no vendored input fixture changed.
- An upstream 0.18 reader cannot read version 2. A file meant for one has
  to be written with the `0.18.1` schema versions, and then it survives the
  trip.
- The schema is not upstream's. If it changes before it is proposed
  upstream, both prototypes change with it, and so does this ADR.
