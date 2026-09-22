#!/usr/bin/env python3
"""Run the Python test suites against these bindings.

Two of them: upstream OpenTimelineIO's own tests, vendored unmodified, which
are the parity measure; and `bindings/`, which covers what these bindings do
that upstream's C++ does not have to -- chiefly moving an object from one
document into another.

Upstream's tests import each other as the package `tests` (`from tests import
baseline_reader`) and find their fixtures beside themselves, so they are
copied into a scratch directory under that name and run with pytest from its
parent, as upstream's own CI runs them. The tests that cannot pass are
deselected, each with its reason, from the files in `excluded/`.

The wheel has to be installed first, and pytest with it; see the crate
README. This script only finds the test files and runs them, so that CI and
a developer's shell run exactly the same thing. Arguments are handed to
pytest, so `run_upstream_tests.py -k marker` narrows the run.
"""

import shutil
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

HERE = Path(__file__).parent


def excluded():
    """Yields the node ids to deselect, as `tests/test_x.py::Class::test`."""
    for listing in sorted((HERE / "excluded").glob("test_*.txt")):
        for line in listing.read_text(encoding="utf-8").splitlines():
            test = line.split("#", 1)[0].strip()
            if test:
                yield f"tests/{listing.stem}.py::{test}"


def upstream(arguments) -> int:
    with tempfile.TemporaryDirectory() as scratch:
        shutil.copytree(HERE / "upstream", Path(scratch) / "tests")
        command = [sys.executable, "-m", "pytest", "-v", "-p", "no:cacheprovider"]
        for test in excluded():
            command += ["--deselect", test]
        command += ["tests", *arguments]
        return subprocess.call(command, cwd=scratch)


def bindings() -> int:
    directory = HERE / "bindings"
    suite = unittest.defaultTestLoader.discover(
        start_dir=str(directory), pattern="test_*.py", top_level_dir=str(directory)
    )
    result = unittest.TextTestRunner(verbosity=2).run(suite)
    return 0 if result.wasSuccessful() else 1


def main() -> int:
    failures = upstream(sys.argv[1:])
    if len(sys.argv) == 1:
        failures |= bindings()
    return 1 if failures else 0


if __name__ == "__main__":
    sys.exit(main())
