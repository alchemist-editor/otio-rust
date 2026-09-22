# SPDX-License-Identifier: Apache-2.0
# Copyright Contributors to the OpenTimelineIO project

"""Reading Advanced Authoring Format (AAF) files.

The work is done by the ``otio-aaf`` crate, a port of upstream's
``otio-aaf-adapter`` on a Rust port of pyaaf2. It reads; it does not write
yet, so this module has no ``write_to_file`` and asking the adapter to write
raises ``AdapterDoesntSupportFunctionError``, as it does for any adapter
without that feature. As upstream's, it reads from a file only.

Reading runs upstream's passes as upstream does: ``simplify``, which
collapses nesting AAF has and OTIO does not need, and ``attach_markers``,
which moves each marker onto the item it points at, both on by default, and
the pass that moves a transition's length onto its neighbours, which always
runs. Either way, what is read matches upstream's adapter byte for byte on
every sample file in its test suite.
"""

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
    return _otio.read_aaf_file(
        str(filepath), AAFAdapterError, bool(simplify), bool(attach_markers)
    )
