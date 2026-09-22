# SPDX-License-Identifier: Apache-2.0
# Copyright Contributors to the OpenTimelineIO project

"""Reading and writing Final Cut Pro X XML (``fcpxml``) files.

The work is done by the ``otio-fcpx`` crate. This module keeps upstream's
``otio-fcpx-xml-adapter`` functions. Two of upstream's behaviours are
deliberately not reproduced, because each loses or scrambles an edit: every
event in a library is read rather than only the first, and lanes composite
in numeric rather than alphabetical order. The crate documents both.
"""

import os
import subprocess
from urllib.parse import unquote

from .. import _otio

__all__ = [
    'format_name',
    'read_from_string',
    'write_to_string',
]


def format_name(frame_rate, path):
    """Returns the name FCP X gives the video format of the media at ``path``.

    Upstream's helper, kept with its behaviour: it asks ``ffprobe`` for the
    frame size and returns ``""`` when ``ffprobe`` is missing or the file is
    not on disk. The naming rule itself is the Rust crate's. Unlike upstream,
    the writer never calls this, so every format it writes is unnamed -- which
    is what upstream writes too whenever the media is not on the machine.
    """
    path = unquote(path.replace("file://", ""))
    if not os.path.exists(path):
        return ""
    try:
        frame_size = subprocess.check_output(
            [
                "ffprobe",
                "-v",
                "error",
                "-select_streams",
                "v:0",
                "-show_entries",
                "stream=height,width",
                "-of",
                "csv=s=x:p=0",
                path
            ]
        ).decode("utf-8")
    except (subprocess.CalledProcessError, OSError):
        frame_size = ""
    return _otio.fcpx_format_name(int(frame_rate), frame_size)


def read_from_string(input_str):
    """Reads an FCP X XML document from a string."""
    return _otio.read_fcpx_xml(input_str, ValueError)


def write_to_string(input_otio):
    """Writes a timeline, or a collection of them, as FCP X XML."""
    return _otio.write_fcpx_xml(input_otio, ValueError)
