# SPDX-License-Identifier: Apache-2.0
"""``otio_aaf_adapter``, as the name of this package's AAF adapter.

Upstream's hook examples import ``AAFAdapterError`` from
``otio_aaf_adapter.adapters.aaf_adapter.aaf_writer``, and its tests take the
hook names from ``otio_aaf_adapter.adapters.aaf_adapter.hooks``. Here both
are ``opentimelineio.adapters.advanced_authoring_format``.
"""

import sys
import types

from opentimelineio.adapters import advanced_authoring_format

_adapters = types.ModuleType(__name__ + ".adapters")
_aaf_adapter = types.ModuleType(__name__ + ".adapters.aaf_adapter")
_adapters.advanced_authoring_format = advanced_authoring_format
_adapters.aaf_adapter = _aaf_adapter
_aaf_adapter.aaf_writer = advanced_authoring_format
_aaf_adapter.hooks = advanced_authoring_format
adapters = _adapters

sys.modules[__name__ + ".adapters"] = _adapters
sys.modules[__name__ + ".adapters.advanced_authoring_format"] = (
    advanced_authoring_format
)
sys.modules[__name__ + ".adapters.aaf_adapter"] = _aaf_adapter
sys.modules[__name__ + ".adapters.aaf_adapter.aaf_writer"] = (
    advanced_authoring_format
)
sys.modules[__name__ + ".adapters.aaf_adapter.hooks"] = advanced_authoring_format
