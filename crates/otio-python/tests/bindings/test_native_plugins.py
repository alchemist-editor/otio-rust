# SPDX-License-Identifier: Apache-2.0
"""Media linkers and hook scripts registered in Rust, reached from Python.

Upstream's own tests cover linkers and hooks declared by manifests, and run
unmodified in ``tests/upstream``. These cover what this package adds: the
ones registered natively, through ``otio_adapter::plugins``, which the
bindings' ``_testing.register_native_example_plugins`` stands in for.
"""

import gc
import os
import unittest

import opentimelineio as otio

EDL = """TITLE: Cut
FCM: NON-DROP FRAME

001  A001     V     C        01:00:00:00 01:00:01:00 00:00:00:00 00:00:01:00
* FROM CLIP NAME:  A
002  B001     V     C        01:00:00:00 01:00:02:00 00:00:01:00 00:00:03:00
* FROM CLIP NAME:  B
"""


class NativePlugins(unittest.TestCase):
    def setUp(self):
        otio._otio._testing.register_native_example_plugins()
        self.addCleanup(
            otio._otio._testing.register_native_example_plugins, False
        )

    def test_a_native_linker_is_listed_after_the_manifests(self):
        names = otio.media_linker.available_media_linker_names()
        self.assertEqual(names[-2:], ["native_example", "native_broken"])

    def test_a_read_runs_a_native_linker_by_name(self):
        timeline = otio.adapters.read_from_string(
            EDL,
            "cmx_3600",
            media_linker_name="native_example",
            media_linker_argument_map={"studio": "north"},
        )
        references = [clip.media_reference for clip in timeline.find_clips()]
        self.assertEqual(
            [reference.name for reference in references],
            ["A_native", "B_native"],
        )
        for reference in references:
            self.assertIsInstance(reference, otio.schema.MissingReference)
            self.assertEqual(reference.metadata["studio"], "north")

    def test_a_native_linker_returning_no_media_reference_is_refused(self):
        clip = otio.schema.Clip(name="a")
        with self.assertRaises(RuntimeError) as raised:
            otio.media_linker.from_name("native_broken").link_media_reference(
                clip, {}
            )
        self.assertIn("not a media reference", str(raised.exception))
        self.assertIsInstance(clip.media_reference, otio.schema.MissingReference)

    def test_an_object_the_linker_keeps_keeps_its_wrapper(self):
        clip = otio.schema.Clip(name="a")
        marker = otio.schema.Marker(name="held")
        marker.extra = "kept"
        reference = otio.media_linker.from_name(
            "native_example"
        ).link_media_reference(clip, {"held": marker})
        del marker
        gc.collect()
        # The linker stored its arguments in the reference's metadata.
        self.assertEqual(reference.metadata["held"].extra, "kept")

    def test_the_default_linker_can_be_a_native_one(self):
        previous = os.environ.get("OTIO_DEFAULT_MEDIA_LINKER")
        os.environ["OTIO_DEFAULT_MEDIA_LINKER"] = "native_example"
        try:
            timeline = otio.adapters.read_from_string(EDL, "cmx_3600")
        finally:
            if previous is None:
                del os.environ["OTIO_DEFAULT_MEDIA_LINKER"]
            else:
                os.environ["OTIO_DEFAULT_MEDIA_LINKER"] = previous
        self.assertEqual(
            [clip.media_reference.name for clip in timeline.find_clips()],
            ["A_native", "B_native"],
        )

    def test_an_unknown_linker_still_lists_the_native_ones(self):
        with self.assertRaises(otio.exceptions.NotSupportedError) as raised:
            otio.media_linker.from_name("nowhere")
        self.assertIn("native_example", str(raised.exception))

    def test_a_native_hook_is_declared_and_runs(self):
        self.assertIn("native_example_hook", otio.hooks.names())
        self.assertEqual(
            otio.hooks.scripts_attached_to("native_example_hook"),
            ["native_example"],
        )
        self.assertIn("native_example", otio.hooks.available_hookscript_names())

        timeline = otio.schema.Timeline(name="cut")
        result = otio.hooks.run(
            "native_example_hook",
            timeline,
            {"kept": 1, "not_an_otio_value": object()},
        )
        self.assertIs(result, timeline)
        # The argument with no OTIO value was left out of what it was handed.
        self.assertEqual(list(result.metadata["native_hook"]), ["kept"])

    def test_a_native_hook_skips_what_is_not_an_object(self):
        self.assertIsNone(otio.hooks.run("native_example_hook", None, {}))

    def test_a_hook_declared_nowhere_is_still_a_key_error(self):
        with self.assertRaises(KeyError):
            otio.hooks.run("nowhere", otio.schema.Timeline())


if __name__ == "__main__":
    unittest.main()
