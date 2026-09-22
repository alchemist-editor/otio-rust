#!/usr/bin/env python3
"""Run upstream's adapter test suites against these bindings.

Each file-format adapter upstream is its own repository with its own test
suite, and those suites are vendored under `adapters/` unmodified. They drive
the adapters the way a user does -- `otio.adapters.read_from_file`, adapter
names, keyword arguments, exception types -- so they measure the Python
surface end to end rather than the Rust crate underneath, which carries its
own port of the same tests.

The suites find their fixtures in a `sample_data` directory beside the test
file. The fixtures are already vendored once, by the Rust crate that ports
the adapter, and a second copy would only drift from the first. So each suite
is copied into a scratch directory with that crate's `tests/data` beside it as
`sample_data`, and run there.

Upstream's suites use pytest, so this needs it installed as well as the wheel.
"""

import os
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

HERE = Path(__file__).parent
CRATES = HERE.parent.parent

# Each suite: its directory under adapters/, the crate whose fixtures it
# reads, and the tests left out, each with the reason it cannot pass.
SUITES = (
    ("ale", "otio-ale", {}),
    ("cmx_3600", "otio-cmx3600", {}),
    ("fcpx_xml", "otio-fcpx", {}),
    (
        "fcp_xml",
        "otio-fcp7",
        {
            # These two classes test upstream's Python implementation from
            # the inside -- its private `_Context`, `FCP7XMLParser`,
            # `_time_from_timecode_element` and so on. The format is read and
            # written in Rust here, so there is no such implementation for
            # them to reach; `otio-fcp7` ports the behaviour they pin.
            "test_fcp7_xml_adapter.py::TestFcp7XmlUtilities": "private helpers",
            "test_fcp7_xml_adapter.py::TestFcp7XmlElements": "private helpers",
            "test_fcp7_xml_adapter.py::AdaptersFcp7XmlTest::test_build_empty_file":
                "calls the private `_build_empty_file`",
        },
    ),
)


def main() -> int:
    failed = []
    env = dict(os.environ)
    env["PYTHONPATH"] = os.pathsep.join(
        [str(HERE / "adapters" / "shims")]
        + ([env["PYTHONPATH"]] if env.get("PYTHONPATH") else [])
    )
    for name, crate, excluded in SUITES:
        with tempfile.TemporaryDirectory() as scratch:
            suite = Path(scratch) / name
            shutil.copytree(HERE / "adapters" / name, suite)
            shutil.copytree(CRATES / crate / "tests" / "data", suite / "sample_data")
            command = [sys.executable, "-m", "pytest", "-v", "-p", "no:cacheprovider"]
            for test in excluded:
                command += ["--deselect", test]
            print(f"\n== {name}, fixtures from {crate}", flush=True)
            if subprocess.call(command, cwd=suite, env=env) != 0:
                failed.append(name)
    if failed:
        print(f"\nFailed: {', '.join(failed)}")
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
