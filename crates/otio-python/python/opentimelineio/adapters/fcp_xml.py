# SPDX-License-Identifier: Apache-2.0
# Copyright Contributors to the OpenTimelineIO project

"""Reading and writing Final Cut Pro 7 XML (``xmeml``) files.

The work is done by the ``otio-fcp7`` crate. This module keeps upstream's
``otio-fcp-adapter`` functions and their behaviour: a file with one sequence
reads as a ``Timeline``, and one with several as a
``SerializableCollection`` of them. A file that does not parse raises ``ValueError``,
as upstream's does.
"""

from .. import _otio

__all__ = [
    'read_from_string',
    'write_to_string',
]


def read_from_string(input_str):
    """Reads an FCP 7 XML document from a string."""
    return _otio.read_fcp_xml(input_str, ValueError)


def write_to_string(input_otio):
    """Writes a timeline, or a collection of them, as FCP 7 XML."""
    return _otio.write_fcp_xml(input_otio, ValueError)
