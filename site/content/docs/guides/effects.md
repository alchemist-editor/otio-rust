---
title: Effects (prototype)
summary: A prototype of a proposed OpenTimelineIO effects schema. Effects say what they are and hold their parameters, transitions carry an effect, and effects linkers map one host's effects onto another's.
section: Guides
order: 4
---

> **Prototype.** This page describes a proposal, not OpenTimelineIO. The
> schema it adds (`Effect.2`, `Transition.2`) and the effects linker plugin
> point are not in any upstream release, and they can still change. Files
> written with them read in upstream OpenTimelineIO only once they are
> downgraded (see [Older readers](#older-readers)). The decision record is
> ADR 0013 in the repository.

In upstream OpenTimelineIO an effect has a name and an `effect_name`, and
nothing else. A blur from one editor and a blur from another are two strings
that happen to read alike. Their settings travel, if at all, in metadata that
only the host that wrote them reads.

The proposal gives an effect what it needs to be recognised and rebuilt
elsewhere, and adds a plugin point that maps one vocabulary onto another.

<!-- ::sample id="link-effects" -->

## What an effect carries

`Effect.2` keeps every `Effect.1` field and adds six. All of them default to
empty, and all of them are always written.

| Field | Holds |
| --- | --- |
| `effect_id` | A stable identifier that does not depend on the host: an OpenFX plugin identifier such as `com.acme-vfx.lensflare`, an `otio.fx.*` registry identifier, or a vendor's own. |
| `effect_version` | The version of the effect that wrote `parameters`. |
| `host` | The application that wrote `parameters`. |
| `host_version` | That application's version. |
| `context` | Empty, or `filter`, `transition` or `generator`. |
| `parameters` | The effect's values, keyed by stable parameter name. A point, a colour or a curve is a dictionary with a `type` key. |

An effect with all six empty is an `Effect.1` as it always was. Retimes
(`TimeEffect`, `LinearTimeWarp`, `FreezeFrame`) keep their version 1 schema
names, and carry the same fields because they are effects.

## A transition's effect

`Transition.2` adds `effect`: the effect the transition performs, such as an
`otio.fx.wipe` with its direction and softness, or none. Without one, a
transition means what its `transition_type` has always meant, so a
`SMPTE_Dissolve` with no effect is still a dissolve.

A transition owns its effect the way a timeline owns its stack. Replacing it
does not destroy the old one; that effect stays in the document, held by
nothing.

## Effects linkers

An **effects linker** is handed each effect a read produced, along with what
holds it (the item whose effects list it is in, or the transition whose
effect it is), and the effects linker arguments. It answers with the effect
to use, or nothing to keep the one it was handed. It changes an effect in
place or swaps it for another, and it never removes one.

A read now runs, in this order:

| Step | Handed |
| --- | --- |
| `post_adapter_read` hook | the hook arguments, plus `adapter_arguments`, `media_linker_argument_map` and `effects_linker_argument_map` |
| the media linker, once per clip | the media linker arguments |
| `post_media_linker` hook | the media linker arguments |
| the effects linker, once per effect | the effects linker arguments |
| `post_effects_linker` hook | the effects linker arguments |

A linker is chosen the way a media linker is: named on the read, or by the
`OTIO_DEFAULT_EFFECTS_LINKER` environment variable, or none. Naming one that
is not registered fails the read. Asking for none (`do_not_link_effects`,
`EffectsLinkingPolicy.DoNotLinkEffects` in Python) skips linking, and the
hook still runs.

In Rust, effects linkers go in the same registry as media linkers
(`register_effects_linker`). In Python they are listed in a plugin manifest
under `effects_linkers`, as upstream lists media linkers, and expose
`link_effect(in_effect, parent, effects_linker_argument_map)`. From C, the
callback is `OtioEffectsLinkerFn`, registered with
`otio_register_effects_linker`. In this prototype the other SDKs can choose
an effects linker for a read and use the fields above, but they cannot yet
register an effects linker written in their own language.

## The reference linker

The library comes with one effects linker, `otio_fx_registry`. It maps
effects onto the `otio.fx.*` registry, a shared set of common effects (blurs,
dissolves, wipes, basic colour corrections), using the mapping table that
comes with the registry. It is built in but never the default, so a read
runs it only when it names it.

Each mapping names a registry effect, a `match` whose keys must all hold
(`effect_id`, `effect_name` or `host` equal, or `metadata_key` present), an
optional `fidelity`, and an optional `parameter_map` that converts the
effect's own values with a scale and an offset. A match key the linker does
not know never matches, so a newer table cannot match more than it means to.
Pass `registry_path` in the effects linker arguments to use a table of your
own.

A linked effect keeps what it was in `metadata["otio.fx"]["linked_from"]`
(its old `effect_id`, `effect_version` and `parameters`), and how close the
mapping is in `metadata["otio.fx"]["fidelity"]`. Its `host` and
`host_version` are left alone, so a writer for that host can still rebuild
the original. It leaves retimes alone, and effects that are already in the
registry, and effects no mapping matches.

## Older readers

Upstream OpenTimelineIO 0.18 knows only `Effect.1` and `Transition.1`.
Writing with the 0.18.1 release's schema versions downgrades both. Each
non-empty new field goes into `metadata["otio.fx"]`, next to whatever was
there already, such as a `fidelity`, and an empty field is left out. A
transition's effect goes there too. Reading such a file with this library
moves them back into their fields, and drops `otio.fx` if nothing else is
left in it. An older reader keeps the values as metadata, so a file
survives a trip through one.
