# SPDX-License-Identifier: Apache-2.0
# Copyright Contributors to the OpenTimelineIO project

"""Reading and writing CMX 3600 Edit Decision Lists (EDLs).

The work is done by the ``otio-cmx3600`` crate, which carries upstream's
``otio-cmx3600-adapter`` test suite across. This module keeps that adapter's
functions, arguments, defaults and exception types, so code written against
``otio_cmx3600_adapter.cmx_3600`` works against it unchanged.
"""

from .. import _otio, exceptions

__all__ = [
    'EDLParseError',
    'read_from_string',
    'write_to_string',
]


class EDLParseError(exceptions.OTIOError):
    """The input is not an EDL this adapter can read."""


def read_from_string(input_str, rate=24, ignore_timecode_mismatch=False):
    """Reads a CMX Edit Decision List (EDL) from a string.

    An EDL does not say what rate its timecode is at, so ``rate`` has to be
    right: read at the wrong rate, every event lands in the wrong place rather
    than failing.

    By default a file whose record timecode does not add up -- an event that
    overlaps the one before it, say -- is refused with ``EDLParseError``.
    Plenty of real files break that rule; ``ignore_timecode_mismatch=True``
    believes the source timecode instead and slides events along to keep the
    track in order.
    """
    return _otio.read_cmx_3600(
        input_str, float(rate), bool(ignore_timecode_mismatch), EDLParseError
    )


def write_to_string(input_otio, rate=None, style='avid', reelname_len=8):
    """Writes a timeline with a single video track as an EDL.

    ``style`` is one of ``'avid'``, ``'nucoda'`` and ``'premiere'``, whose
    conventions for naming a clip's media differ and are unreadable to one
    another. ``reelname_len`` pads or truncates reel names to that many
    characters; ``None`` writes them in full.
    """
    return _otio.write_cmx_3600(
        input_otio,
        None if rate is None else float(rate),
        style,
        reelname_len,
        EDLParseError,
    )
