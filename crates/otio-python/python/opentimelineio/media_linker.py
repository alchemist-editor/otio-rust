# SPDX-License-Identifier: Apache-2.0
# Copyright Contributors to the OpenTimelineIO project

"""Media linkers: the hook that repoints media references after a read.

Upstream finds media linkers through its plugin manifest and runs the default
one, if ``OTIO_DEFAULT_MEDIA_LINKER`` names one, after every adapter read.
There is no manifest here and so no media linkers. The policy names are kept
so that calls written against upstream still work: asking for no linking, or
for the default when none is configured, is exactly what upstream does out of
the box. Asking for a linker by name is refused, because quietly skipping it
would hand back references the caller expected to have been fixed up.
"""

import os

from . import exceptions

__all__ = [
    'MediaLinkingPolicy',
    'available_media_linker_names',
    'default_media_linker',
    'from_name',
]


class MediaLinkingPolicy:
    """Special values for ``media_linker_name``."""

    DoNotLinkMedia = "__do_not_link_media"
    ForceDefaultLinker = "__default"


def available_media_linker_names():
    """Returns the names of the media linkers there are, which is none."""
    return []


def from_name(name):
    """Returns the media linker called ``name``; there are none to return."""
    raise exceptions.NotSupportedError(
        f"media linkers are not supported yet: '{name}'"
    )


def default_media_linker():
    """Returns the default media linker, which there cannot be yet."""
    try:
        name = os.environ['OTIO_DEFAULT_MEDIA_LINKER']
    except KeyError:
        raise exceptions.NoDefaultMediaLinkerError(
            "No default Media Linker set in $OTIO_DEFAULT_MEDIA_LINKER"
        )
    return from_name(name)


def _refuse_named_linker(media_linker_name):
    """Raises if a read asks for linking this library cannot do.

    The two policies and an empty name all mean "whatever the default is",
    and with no linkers the default is to leave the references alone -- unless
    the environment names a default, which cannot be honoured either.
    """
    if media_linker_name == MediaLinkingPolicy.DoNotLinkMedia:
        return
    if media_linker_name in (None, '', MediaLinkingPolicy.ForceDefaultLinker):
        if os.environ.get('OTIO_DEFAULT_MEDIA_LINKER'):
            from_name(os.environ['OTIO_DEFAULT_MEDIA_LINKER'])
        return
    from_name(media_linker_name)
