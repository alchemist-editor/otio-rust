# SPDX-License-Identifier: Apache-2.0
# Copyright Contributors to the OpenTimelineIO project

"""The time classes, under the name upstream gives them.

Upstream builds two extension modules, ``_opentime`` and ``_otio``, because
its C++ library is two libraries. This port has one, so this module re-exports
the time half of it under the name callers expect. The classes themselves
report ``opentimelineio._opentime`` as their ``__module__``, so a ``repr()``
here reads the same as upstream's.
"""

# flake8: noqa

from . _otio import (  # noqa
    RationalTime,
    TimeRange,
    TimeTransform,
    _testing,
)

__all__ = ['RationalTime', 'TimeRange', 'TimeTransform']
