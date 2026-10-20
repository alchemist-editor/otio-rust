# SPDX-License-Identifier: Apache-2.0
"""otio-rust's extensions, reached from Python.

Extensions are not part of OpenTimelineIO, so upstream's suite says nothing
about them; these check the switch that turns them on.
"""

import subprocess
import sys
import unittest

from opentimelineio import ext
from opentimelineio.ext import sync


class Extensions(unittest.TestCase):
    def test_sync_is_available_and_turns_on(self):
        self.assertIn("sync", ext.available())
        sync.enable()
        sync.enable()
        self.assertTrue(sync.is_enabled())
        self.assertTrue(ext.is_enabled("sync"))
        self.assertIn("sync", ext.enabled())

    def test_an_unknown_extension_is_a_value_error_naming_it(self):
        with self.assertRaises(ValueError) as caught:
            ext.enable("no-such-extension")
        self.assertIn("no-such-extension", str(caught.exception))
        self.assertFalse(ext.is_enabled("no-such-extension"))

    def test_importing_opentimelineio_does_not_import_the_extensions(self):
        program = (
            "import sys, opentimelineio; "
            "print('opentimelineio.ext' in sys.modules)"
        )
        result = subprocess.run(
            [sys.executable, "-c", program],
            capture_output=True,
            text=True,
            check=True,
        )
        self.assertEqual(result.stdout.strip(), "False")


if __name__ == "__main__":
    unittest.main()
