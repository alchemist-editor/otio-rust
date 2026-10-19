# SPDX-License-Identifier: Apache-2.0
"""otio-rust's extensions. **Not part of OpenTimelineIO.**

An extension is work built on this package's Rust core that upstream
OpenTimelineIO does not have, such as timeline sync. Each one does nothing
until it is enabled, and nothing in :mod:`opentimelineio` imports this
package, so a program that never asks for an extension never sees one.

.. code-block:: python

    from opentimelineio.ext import sync

    sync.enable()
"""

from .. import _otio

__all__ = ["available", "enable", "enabled", "is_enabled"]


def enable(id):
    """Turn on the extension ``id``, and every extension it depends on first.

    Enabling one that is already on does nothing. Raises :class:`ValueError`
    for an id this package was not built with.
    """
    _otio._ext_enable(id)


def is_enabled(id):
    """Whether the extension ``id`` is on."""
    return _otio._ext_is_enabled(id)


def enabled():
    """The ids of the extensions that are on, in the order they were enabled."""
    return list(_otio._ext_enabled())


def available():
    """The ids of every extension this package was built with."""
    return list(_otio._ext_available())
