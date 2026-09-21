#!/usr/bin/env python3
"""Run upstream OpenTimelineIO's own Python tests against these bindings.

The wheel has to be installed first; see the crate README. This script only
finds the vendored test files and runs them, so that CI and a developer's
shell run exactly the same thing.
"""

import sys
import unittest
from pathlib import Path

UPSTREAM = Path(__file__).parent / "upstream"


def main() -> int:
    tests = unittest.defaultTestLoader.discover(
        start_dir=str(UPSTREAM), pattern="test_*.py"
    )
    result = unittest.TextTestRunner(verbosity=2).run(tests)
    return 0 if result.wasSuccessful() else 1


if __name__ == "__main__":
    sys.exit(main())
