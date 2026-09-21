#!/usr/bin/env python3
"""Run the Python test suites against these bindings.

Two of them: upstream OpenTimelineIO's own tests, vendored unmodified, which
are the parity measure; and `bindings/`, which covers what these bindings do
that upstream's C++ does not have to — chiefly moving an object from one
document into another.

The wheel has to be installed first; see the crate README. This script only
finds the test files and runs them, so that CI and a developer's shell run
exactly the same thing.
"""

import sys
import unittest
from pathlib import Path

SUITES = (
    Path(__file__).parent / "upstream",
    Path(__file__).parent / "bindings",
)


def main() -> int:
    suite = unittest.TestSuite(
        unittest.defaultTestLoader.discover(
            start_dir=str(directory), pattern="test_*.py", top_level_dir=str(directory)
        )
        for directory in SUITES
    )
    result = unittest.TextTestRunner(verbosity=2).run(suite)
    return 0 if result.wasSuccessful() else 1


if __name__ == "__main__":
    sys.exit(main())
