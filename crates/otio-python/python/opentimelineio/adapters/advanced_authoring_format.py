# SPDX-License-Identifier: Apache-2.0
# Copyright Contributors to the OpenTimelineIO project

"""Reading and writing Advanced Authoring Format (AAF) files.

The work is done by the ``otio-aaf`` crate, a port of upstream's
``otio-aaf-adapter`` on a Rust port of pyaaf2. As upstream's, it reads from a
file and writes to one, and has no string forms.

Reading runs upstream's passes as upstream does: ``simplify``, which
collapses nesting AAF has and OTIO does not need, and ``attach_markers``,
which moves each marker onto the item it points at, both on by default, and
the pass that moves a transition's length onto its neighbours, which always
runs. Either way, what is read matches upstream's adapter byte for byte on
every sample file in its test suite.

Writing makes the same pyaaf2 operations upstream's writer makes, in the
same order, so given the same times and identifiers the file is the one
upstream writes, byte for byte. ``prefer_file_mob_id``,
``use_empty_mob_ids``, ``embed_essence`` and ``create_edgecode`` behave as
upstream's do.

Upstream's four hooks run where upstream runs them, with the arguments it
passes: ``otio_aaf_pre_read_transcribe`` before the file is transcribed,
``otio_aaf_post_read_transcribe`` on what was transcribed before any pass,
``otio_aaf_pre_write_transcribe`` on the timeline before it is written, which
is where media can be transcoded into something ``embed_essence`` can embed,
and ``otio_aaf_post_write_transcribe`` once it has been. Upstream also hands
each hook the open pyaaf2 file as ``aaf_handle``. There is no pyaaf2 file
here, so ``aaf_handle`` is ``None``, and the write hooks run before the file
is created and after it is closed, rather than while it is open.
"""

from .. import _otio, exceptions, hooks

__all__ = [
    'AAFAdapterError',
    'adapter_hook_names',
    'read_from_file',
    'write_to_file',
]

# Upstream's hook names, from its ``aaf_adapter/hooks.py``.
HOOK_PRE_READ_TRANSCRIBE = "otio_aaf_pre_read_transcribe"
HOOK_POST_READ_TRANSCRIBE = "otio_aaf_post_read_transcribe"
HOOK_PRE_WRITE_TRANSCRIBE = "otio_aaf_pre_write_transcribe"
HOOK_POST_WRITE_TRANSCRIBE = "otio_aaf_post_write_transcribe"


class AAFAdapterError(exceptions.OTIOError):
    """Raised for AAF adapter-specific errors."""


def read_from_file(
    filepath,
    simplify=True,
    transcribe_log=False,
    attach_markers=True,
    bake_keyframed_properties=False,
    hook_function_argument_map=None,
    **kwargs
):
    """Reads an AAF file as a ``Timeline``, or a ``SerializableCollection``
    of them when the file holds several.

    ``transcribe_log`` prints a line for each thing the reader makes, as
    upstream's does, and ``bake_keyframed_properties`` records each
    keyframed effect parameter's value at every frame of its effect as
    ``keyframe_baked_values``.

    ``hook_function_argument_map`` is what the adapter hands its hooks, and
    the read hooks are handed it with ``read_filepath`` and ``aaf_handle``
    added, as upstream's are.
    """
    if kwargs:
        raise TypeError(
            "read_from_file() got unexpected keyword arguments: {}".format(
                ", ".join(sorted(kwargs))
            )
        )
    # As upstream, the hooks are handed the caller's own dictionary, with
    # the read's arguments added to it.
    extra_args = {} if hook_function_argument_map is None else (
        hook_function_argument_map
    )
    if HOOK_PRE_READ_TRANSCRIBE in hooks.names():
        extra_args.update({"read_filepath": filepath, "aaf_handle": None})
        hooks.run(HOOK_PRE_READ_TRANSCRIBE, tl=None, extra_args=extra_args)

    post_transcribe = None
    if HOOK_POST_READ_TRANSCRIBE in hooks.names():
        def post_transcribe(timeline):
            extra_args.update({"read_filepath": filepath, "aaf_handle": None})
            return hooks.run(
                HOOK_POST_READ_TRANSCRIBE, tl=timeline, extra_args=extra_args
            )

    return _otio.read_aaf_file(
        str(filepath),
        AAFAdapterError,
        bool(simplify),
        bool(attach_markers),
        bool(transcribe_log),
        bool(bake_keyframed_properties),
        post_transcribe=post_transcribe,
    )


def write_to_file(
    input_otio,
    filepath,
    prefer_file_mob_id=False,
    use_empty_mob_ids=False,
    embed_essence=False,
    create_edgecode=False,
    hook_function_argument_map=None,
    **kwargs
):
    """Writes ``input_otio``, a ``Timeline``, as an AAF file at ``filepath``.

    ``prefer_file_mob_id`` looks for each clip's Mob ID in the AAF file its
    media names before its metadata; ``use_empty_mob_ids`` makes one up for a
    clip that has none anywhere, where otherwise such a clip raises
    ``AAFAdapterError``; and ``create_edgecode`` gives each master mob an
    edge code slot, which Media Composer shows as Frame Count Start and End.

    ``embed_essence`` embeds each clip's media, found by the path its URL
    names: essence copied out of an ``.aaf`` with the master mob the clip's
    Mob ID names, or, on a video track, a raw DNxHD stream in a ``.dnx``
    file imported. As upstream's, it raises ``FileNotFoundError`` for media
    that is not there, ``AAFAdapterError`` for any other kind of file or an
    AAF without that master mob, ``TypeError`` for a ``.dnx`` or ``.wav`` on
    an audio track, which upstream fails on, and ``ValueError`` for a file
    the DNxHD import cannot read, a ``.wav`` among them.

    A timeline that is not one AAF can hold raises ``NotSupportedError``, and
    one missing what the writer needs, such as a rate every item agrees on,
    raises ``AAFAdapterError`` listing everything it lacks. The file is only
    created once the whole AAF has been built.

    The write hooks are handed ``hook_function_argument_map`` with
    ``write_filepath``, ``aaf_handle`` and ``embed_essence`` added, as
    upstream's are. What ``otio_aaf_pre_write_transcribe`` returns is what is
    written.
    """
    # For this package's tests only: replays a fixture's recorded times and
    # identifiers, so the file can be compared with upstream's byte for byte.
    calls_tsv = kwargs.pop("_calls_tsv", None)
    if kwargs:
        raise TypeError(
            "write_to_file() got unexpected keyword arguments: {}".format(
                ", ".join(sorted(kwargs))
            )
        )
    extra_args = {} if hook_function_argument_map is None else (
        hook_function_argument_map
    )

    def write_args():
        extra_args.update({
            "write_filepath": filepath,
            "aaf_handle": None,
            "embed_essence": embed_essence,
        })
        return extra_args

    if HOOK_PRE_WRITE_TRANSCRIBE in hooks.names():
        input_otio = hooks.run(HOOK_PRE_WRITE_TRANSCRIBE, input_otio, write_args())

    _otio.write_aaf_file(
        input_otio,
        str(filepath),
        AAFAdapterError,
        bool(prefer_file_mob_id),
        bool(use_empty_mob_ids),
        bool(embed_essence),
        bool(create_edgecode),
        _calls_tsv=None if calls_tsv is None else str(calls_tsv),
    )

    if HOOK_POST_WRITE_TRANSCRIBE in hooks.names():
        hooks.run(HOOK_POST_WRITE_TRANSCRIBE, input_otio, write_args())


def adapter_hook_names():
    """Returns names of custom hooks implemented by this adapter."""
    return [
        HOOK_POST_READ_TRANSCRIBE,
        HOOK_POST_WRITE_TRANSCRIBE,
        HOOK_PRE_READ_TRANSCRIBE,
        HOOK_PRE_WRITE_TRANSCRIBE,
    ]
