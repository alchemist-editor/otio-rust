# SPDX-License-Identifier: Apache-2.0
# Copyright Contributors to the OpenTimelineIO project

"""The schemas a .otio file is made of."""

from .. _otio import (  # noqa
    Box2d,
    Color,
    V2d,
)

MarkerColor = Color  # for backwards compatibility, as upstream does

__all__ = [
    'Box2d',
    'MarkerColor',
    'V2d',
]
