# SPDX-License-Identifier: Apache-2.0
# Copyright Contributors to the OpenTimelineIO project

"""Reading and writing Avid Log Exchange (ALE) files.

The work is done by the ``otio-ale`` crate. This module keeps upstream's
``otio-ale-adapter`` functions, arguments, defaults and exception type, so
code written against ``otio_ale_adapter.ale`` works against it unchanged.

Reading returns a ``SerializableCollection`` of clips, with the file's
heading and column order under ``metadata["ALE"]``.
"""

from .. import _otio, exceptions

__all__ = [
    'ALEParseError',
    'read_from_string',
    'write_to_string',
]


class ALEParseError(exceptions.OTIOError):
    """The input is not an ALE this adapter can read."""


def read_from_string(input_str, fps=24, **adapter_argument_map):
    """Reads an ALE from a string.

    ``fps`` is used when the file's heading states no ``FPS``; when it does,
    the heading wins. ``ale_name_column_key`` names the column a clip takes
    its name from, ``"Name"`` unless given. Upstream takes it through the
    keyword arguments rather than as a parameter of its own, and so does this;
    any other keyword is refused rather than ignored.
    """
    name_column = adapter_argument_map.pop("ale_name_column_key", "Name")
    if adapter_argument_map:
        raise TypeError(
            "read_from_string() got unexpected keyword arguments: {}".format(
                ", ".join(sorted(adapter_argument_map))
            )
        )
    return _otio.read_ale(input_str, float(fps), name_column, ALEParseError)


def write_to_string(input_otio, columns=None, fps=None, video_format=None):
    """Writes the clips below ``input_otio`` as an ALE.

    ``columns`` fixes the column order; by default it is the one the object
    was read with, followed by any column a clip has that it does not
    mention. ``fps`` defaults to the heading's, and ``video_format`` to the
    heading's or to one guessed from the clips' ``Image Size``.
    """
    return _otio.write_ale(
        input_otio,
        None if columns is None else list(columns),
        None if fps is None else float(fps),
        video_format,
        ALEParseError,
    )
