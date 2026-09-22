# SPDX-License-Identifier: Apache-2.0
"""``otio_fcpx_xml_adapter``, as the name of this package's FCP X adapter."""

import sys

from opentimelineio.adapters import fcpx_xml

sys.modules[__name__ + ".fcpx_xml"] = fcpx_xml
