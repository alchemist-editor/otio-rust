# SPDX-License-Identifier: Apache-2.0
"""ORI Annotations, an otio-rust extension, reached from Python.

The extension is not part of OpenTimelineIO, so upstream's suite says
nothing about it. These check its calls against a file ORI's own Python
wrote, and that a program which defines ORI's classes itself, as ORI's
plugin does, still works with the extension on.
"""

import json
import pathlib
import subprocess
import sys
import unittest

import opentimelineio as otio
from opentimelineio.ext import ori

REPO = pathlib.Path(__file__).resolve().parents[4]
ORI_BUILT = (
    REPO / "extensions" / "ori" / "otio-ext-ori-annotations" / "tests" / "fixtures"
    / "ori_built.otio"
)

EMPTY = '{"OTIO_SCHEMA": "Timeline.1", "tracks": {"OTIO_SCHEMA": "Stack.1", "children": []}}'


def run(program):
    """Runs ``program`` in a fresh interpreter, where nothing is enabled yet,
    and returns what it printed."""
    result = subprocess.run(
        [sys.executable, "-c", program], capture_output=True, text=True, check=True
    )
    return result.stdout.strip()


class OriAnnotations(unittest.TestCase):
    def setUp(self):
        ori.enable()

    def test_enabling_turns_sync_on_too(self):
        self.assertTrue(ori.is_enabled())
        self.assertIn("ori-annotations", otio.ext.available())
        self.assertTrue(otio.ext.is_enabled("sync"))

    def test_refuses_every_call_until_enabled(self):
        program = "\n".join([
            "import opentimelineio as otio",
            "from opentimelineio.ext import ori",
            "t = otio.adapters.read_from_string('" + EMPTY + "', 'otio_json')",
            "try:",
            "    ori.find_track(t)",
            "except RuntimeError as error:",
            "    print('refused', 'enable()' in str(error))",
        ])
        self.assertEqual(run(program), "refused True")

    def test_reads_what_ori_wrote(self):
        timeline = otio.adapters.read_from_file(str(ORI_BUILT))
        holders = ori.list(timeline)
        self.assertEqual(len(holders), 2)
        self.assertEqual(ori.describe(holders[0]), {
            "form": "track", "clip_guid": "shot", "clip_name": None,
            "frame": {"value": 10.0, "rate": 24.0}, "note": None, "status": None,
            "command_count": 4,
        })
        track = ori.find_track(timeline)
        self.assertEqual(track.name, "Annotations")
        self.assertIs(ori.find_clip(track, "shot", 10), holders[0])
        self.assertIsNone(ori.find_clip(track, "shot", 99))
        self.assertEqual(len(ori.list_at_frame(timeline, "shot", 4)), 1)
        self.assertEqual(len(ori.list_for_clip(timeline, "shot")), 2)
        with self.assertRaises(TypeError):
            ori.describe(timeline)

    def test_annotates_a_frame_and_edits_by_uuid(self):
        built = otio.adapters.read_from_file(str(ORI_BUILT))
        commands = ori.commands(ori.list(built)[0])

        timeline = otio.adapters.read_from_string(EMPTY, "otio_json")
        track = ori.ensure_track(timeline)
        clip = ori.ensure_clip(track, "shot", otio.opentime.RationalTime(10, 24))
        ori.add_commands(clip, commands)
        self.assertIs(ori.ensure_clip(track, "shot", otio.opentime.RationalTime(10, 24)), clip)
        self.assertEqual(ori.commands(clip), commands)
        self.assertEqual(clip.name, "Annotation_10")
        self.assertEqual(clip.metadata["clip_guid"], "shot")

        self.assertTrue(ori.replace_commands(clip, "stroke-1", [
            {"OTIO_SCHEMA": "TextAnnotation.1", "uuid": "stroke-1", "text": "was a stroke"},
        ]))
        self.assertEqual(ori.remove_commands(clip, ["text-1", "nobody"]), 1)
        left = json.loads(ori.commands(clip))
        self.assertEqual([c["uuid"] for c in left], ["stroke-1"])
        self.assertEqual(left[0]["text"], "was a stroke")
        ori.set_commands(clip, [])
        self.assertEqual(ori.commands(clip), "[]")
        with self.assertRaises(ValueError):
            ori.add_commands(clip, "not json")
        with self.assertRaises(TypeError):
            ori.add_commands(timeline, [])

    def test_coordinates_are_oris(self):
        x, y = ori.pixels_to_ori(1920, 1080, 0, 0)
        self.assertAlmostEqual(x, -8 / 9, places=12)
        self.assertAlmostEqual(y, 0.5, places=12)
        px, py = ori.ori_to_pixels(1920, 1080, x, y)
        self.assertAlmostEqual(px, 0, places=9)
        self.assertAlmostEqual(py, 0, places=9)
        self.assertEqual(ori.aspect_half(1920, 0), 8 / 9)
        bounds = otio.schema.Box2d(otio.schema.V2d(-16, -9), otio.schema.V2d(16, 9))
        self.assertEqual(ori.ori_to_bounds(0.5, 0.5, bounds), (9, 9))
        self.assertEqual(ori.bounds_to_ori(9, 9, bounds), (0.5, 0.5))

    def test_oris_own_classes_and_the_extension_work_together(self):
        # ORI's plugin defines PaintStart.1 as a Python class before anything
        # enables the extension; a class defined the same way here stands in
        # for it, in a fresh interpreter. The file still reads into that
        # class, the extension finds the annotations, and it writes back
        # byte for byte.
        program = "\n".join([
            "import opentimelineio as otio",
            "from opentimelineio.ext import ori",
            "@otio.core.register_type",
            "class PaintStart(otio.core.SerializableObject):",
            "    _serializable_label = 'PaintStart.1'",
            "    rgba = otio.core.serializable_field('rgba', list)",
            "ori.enable()",
            "path = " + repr(str(ORI_BUILT)),
            "t = otio.adapters.read_from_file(path)",
            "holder = ori.list(t)[0]",
            "first = holder.metadata['annotation_commands'][0]",
            "print(type(first).__name__, first.rgba)",
            "print(ori.describe(holder)['command_count'])",
            "print(otio.adapters.write_to_string(t).rstrip() == open(path).read().rstrip())",
        ])
        self.assertEqual(run(program).splitlines(), ["PaintStart [1.0, 0.25, 0.25, 1.0]", "4", "True"])


if __name__ == "__main__":
    unittest.main()
