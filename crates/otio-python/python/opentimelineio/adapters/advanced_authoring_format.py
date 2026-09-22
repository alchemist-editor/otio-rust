# SPDX-License-Identifier: Apache-2.0
# Copyright Contributors to the OpenTimelineIO project

"""Reading Advanced Authoring Format (AAF) files.

The work is done by the ``otio-aaf`` crate, a port of upstream's
``otio-aaf-adapter`` on a Rust port of pyaaf2. It reads; it does not write
yet, so this module has no ``write_to_file`` and asking the adapter to write
raises ``AdapterDoesntSupportFunctionError``, as it does for any adapter
without that feature. As upstream's, it reads from a file only.

What is read is upstream's structural transcription. Upstream then runs
passes over it that are not ported yet -- ``simplify``, which collapses
nesting AAF has and OTIO does not need, and ``attach_markers``, which moves
each marker onto the item it points at -- so a timeline read here is what
upstream returns with ``simplify=False, attach_markers=False``. Both default
to ``True`` upstream and here; until the passes land, leaving them on warns
that they were skipped rather than quietly returning a different shape. The
third pass, which moves a transition's length onto its neighbours, upstream
always runs and takes no argument; it is not ported either.
"""

import warnings

from .. import _otio, exceptions

__all__ = [
    'AAFAdapterError',
    'read_from_file',
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
    """
    if kwargs:
        raise TypeError(
            "read_from_file() got unexpected keyword arguments: {}".format(
                ", ".join(sorted(kwargs))
            )
        )
    if transcribe_log:
        raise NotImplementedError(
            "transcribe_log is not supported by this AAF reader yet"
        )
    if bake_keyframed_properties:
        raise NotImplementedError(
            "bake_keyframed_properties is not supported by this AAF reader yet"
        )
    skipped = [
        name for name, wanted in (
            ("simplify", simplify),
            ("attach_markers", attach_markers),
        ) if wanted
    ]
    if skipped:
        warnings.warn(
            "the AAF reader does not implement {} yet, so the result is "
            "what upstream returns with {}".format(
                " or ".join(skipped),
                ", ".join(f"{name}=False" for name in skipped),
            ),
            stacklevel=2,
        )
    return _otio.read_aaf_file(str(filepath), AAFAdapterError)
