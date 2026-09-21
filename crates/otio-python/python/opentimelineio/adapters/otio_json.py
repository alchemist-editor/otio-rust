# SPDX-License-Identifier: Apache-2.0
# Copyright Contributors to the OpenTimelineIO project

"""Adapter for reading and writing native .otio json files."""

from .. import core


def read_from_file(filepath):
    """De-serializes an OpenTimelineIO object from a file."""
    with open(filepath, encoding='utf-8') as handle:
        return read_from_string(handle.read())


def read_from_string(input_str):
    """De-serializes an OpenTimelineIO object from a json string."""
    return core.deserialize_json_from_string(input_str)


def write_to_string(input_otio, target_schema_versions=None, indent=4):
    """Serializes an OpenTimelineIO object into a json string.

    ``target_schema_versions`` is upstream's schema downgrade map. Downgrading
    is not ported yet, so anything other than ``None`` is refused rather than
    quietly ignored: a caller asking for an older schema would otherwise get a
    file that says it is current.
    """
    if target_schema_versions is not None:
        raise NotImplementedError(
            "writing an older schema version is not supported yet"
        )
    return core.serialize_json_to_string(input_otio, indent)


def write_to_file(input_otio, filepath, target_schema_versions=None, indent=4):
    """Serializes an OpenTimelineIO object into a file."""
    with open(filepath, 'w', encoding='utf-8') as handle:
        handle.write(
            write_to_string(input_otio, target_schema_versions, indent)
        )
