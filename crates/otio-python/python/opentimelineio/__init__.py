# SPDX-License-Identifier: Apache-2.0
# Copyright Contributors to the OpenTimelineIO project

"""An editorial interchange format and library, on a Rust core.

This package is a drop-in replacement for upstream OpenTimelineIO's Python
package, built on ``otio-rust`` instead of the C++ library. Only the parts
that are ported so far are present; see the crate README for what is missing.
"""

# flake8: noqa

from . import (
    _opentime,
    opentime,
)

__all__ = [
    'opentime',
]
