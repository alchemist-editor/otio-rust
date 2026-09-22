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
    schemadef,
    plugins,
    media_linker,
    adapters,
    hooks,
    versioning,
)

__all__ = [
    'adapters',
    'core',
    'exceptions',
    'hooks',
    'media_linker',
    'opentime',
    'plugins',
    'schema',
    'schemadef',
    'versioning',
]

# Upstream stamps its release into this file when it builds. This package
# answers with the upstream version whose Python API it reproduces, which is
# what code checking ``otio.__version__`` is asking about.
__version__ = "0.19.0.dev1"
