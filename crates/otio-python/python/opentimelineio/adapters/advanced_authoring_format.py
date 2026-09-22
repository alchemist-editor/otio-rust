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
upstream's do. Upstream's pre- and post-write hooks are plugins handed the
open pyaaf2 file, and there is no such file here, so none run; in
particular, there is no ``otio_aaf_pre_write_transcribe`` hook to make
embeddable media of other files.
"""

from .. import _otio, exceptions

__all__ = [
    'AAFAdapterError',
    'read_from_file',
    'write_to_file',
]


class AAFAdapterError(exceptions.OTIOError):
    """Raised for AAF adapter-specific errors."""


def read_from_file(
    filepath,
    simplify=True,
    transcribe_log=False,
    attach_markers=True,
    bake_keyframed_properties=False,
    **kwargs
):
    """Reads an AAF file as a ``Timeline``, or a ``SerializableCollection``
    of them when the file holds several.

    ``transcribe_log`` prints a line for each thing the reader makes, as
    upstream's does, and ``bake_keyframed_properties`` records each
    keyframed effect parameter's value at every frame of its effect as
    ``keyframe_baked_values``.
    """
    if kwargs:
        raise TypeError(
            "read_from_file() got unexpected keyword arguments: {}".format(
                ", ".join(sorted(kwargs))
            )
        )
    return _otio.read_aaf_file(
        str(filepath),
        AAFAdapterError,
        bool(simplify),
        bool(attach_markers),
        bool(transcribe_log),
        bool(bake_keyframed_properties),
    )


def write_to_file(
    input_otio,
    filepath,
    prefer_file_mob_id=False,
    use_empty_mob_ids=False,
    embed_essence=False,
    create_edgecode=False,
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
