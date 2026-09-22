# SPDX-License-Identifier: Apache-2.0
"""``otio_cmx3600_adapter``, as the name of this package's EDL adapter."""

import sys

from opentimelineio.adapters import cmx_3600

sys.modules[__name__ + ".cmx_3600"] = cmx_3600
