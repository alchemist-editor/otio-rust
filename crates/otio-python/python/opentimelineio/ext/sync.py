# SPDX-License-Identifier: Apache-2.0
"""Timeline sync: sharing a timeline between programs while it is edited.

**An otio-rust extension, not part of OpenTimelineIO.** It does nothing
until it is enabled. See ADR 0011 in the otio-rust repository.
"""

from .. import _otio

__all__ = ["ID", "enable", "is_enabled"]

#: The id sync is enabled by.
ID = "sync"


def enable():
    """Turn timeline sync on. Enabling it again does nothing."""
    _otio._ext_enable(ID)


def is_enabled():
    """Whether timeline sync is on."""
    return _otio._ext_is_enabled(ID)
