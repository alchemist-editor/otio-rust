# SPDX-License-Identifier: Apache-2.0
# Copyright Contributors to the OpenTimelineIO project

"""Plugin system for OTIO"""

# flake8: noqa

# The plugin classes below are Python-defined schemas, registered with
# ``core.register_type``. Until the core type registry is bound, a stand-in
# supplies it; see ``core/_interim_registry.py``. It does nothing once
# ``core`` has a registry of its own.
from ..core import _interim_registry
_interim_registry.install()

from .python_plugin import (
    plugin_info_map,
    PythonPlugin,
)

from .manifest import (
    manifest_from_file,
    ActiveManifest,
)
