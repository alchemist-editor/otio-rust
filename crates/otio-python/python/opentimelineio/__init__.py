# SPDX-License-Identifier: Apache-2.0
# Copyright Contributors to the OpenTimelineIO project

"""An editorial interchange format and library, on a Rust core.

This package is a drop-in replacement for upstream OpenTimelineIO's Python
package, built on ``otio-rust`` instead of the C++ library. Only the parts
that are ported so far are present; see the crate README for what is missing.
"""

# flake8: noqa

# in dependency hierarchy, as upstream orders it
from . import (
    _opentime,
    opentime,
    core,
    exceptions,
    schema,
    plugins,
    media_linker,
    adapters,
    url_utils,
)

__all__ = [
    'adapters',
    'core',
    'exceptions',
    'media_linker',
    'opentime',
    'plugins',
    'schema',
    'url_utils',
]
