# SPDX-License-Identifier: Apache-2.0
# Copyright Contributors to the OpenTimelineIO project

"""Reading and writing OTIO objects in other file formats.

Upstream discovers adapters through a plugin manifest, so that a third party
can ship one. Only the native json adapter is ported so far, so the lookup
here is a dictionary rather than a manifest; the calling code is the same
either way.
"""

from . import otio_json  # noqa

_ADAPTERS = {
    'otio_json': otio_json,
}


def from_name(name):
    """Returns the adapter registered under ``name``."""
    try:
        return _ADAPTERS[name]
    except KeyError:
        raise ValueError(
            "Adapter not supported: {}, options: {}".format(
                name, sorted(_ADAPTERS)
            )
        )


def read_from_string(input_str, adapter_name='otio_json', **adapter_argument_map):
    """De-serializes an OpenTimelineIO object from a string."""
    return from_name(adapter_name).read_from_string(
        input_str, **adapter_argument_map
    )


def write_to_string(input_otio, adapter_name='otio_json', **adapter_argument_map):
    """Serializes an OpenTimelineIO object into a string."""
    return from_name(adapter_name).write_to_string(
        input_otio, **adapter_argument_map
    )


def read_from_file(filepath, adapter_name='otio_json', **adapter_argument_map):
    """De-serializes an OpenTimelineIO object from a file."""
    return from_name(adapter_name).read_from_file(
        filepath, **adapter_argument_map
    )


def write_to_file(
    input_otio, filepath, adapter_name='otio_json', **adapter_argument_map
):
    """Serializes an OpenTimelineIO object into a file."""
    return from_name(adapter_name).write_to_file(
        input_otio, filepath, **adapter_argument_map
    )


__all__ = [
    'from_name',
    'otio_json',
    'read_from_file',
    'read_from_string',
    'write_to_file',
    'write_to_string',
]
