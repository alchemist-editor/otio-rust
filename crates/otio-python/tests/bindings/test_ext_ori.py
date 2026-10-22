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


# A timeline whose one clip has the sync GUID main/clip-1.
REVIEW_TIMELINE = json.dumps({
    "OTIO_SCHEMA": "Timeline.1", "name": "Program", "metadata": {"sync": {"guid": "main"}},
    "tracks": {"OTIO_SCHEMA": "Stack.1", "metadata": {"sync": {"guid": "main/stack"}}, "children": [{
        "OTIO_SCHEMA": "Track.1", "name": "V1", "kind": "Video",
        "metadata": {"sync": {"guid": "main/v1"}},
        "children": [{
            "OTIO_SCHEMA": "Clip.2", "name": "Beach Run",
            "metadata": {"sync": {"guid": "main/clip-1"}},
            "source_range": {
                "OTIO_SCHEMA": "TimeRange.1",
                "start_time": {"OTIO_SCHEMA": "RationalTime.1", "value": 0, "rate": 24},
                "duration": {"OTIO_SCHEMA": "RationalTime.1", "value": 48, "rate": 24},
            },
        }],
    }]},
})

# One stroke, s1.
STROKE = [
    {"OTIO_SCHEMA": "PaintStart.1", "uuid": "s1", "rgba": [1, 0, 0, 1]},
    {"OTIO_SCHEMA": "PaintPoint.1", "uuid": "s1", "points": {
        "OTIO_SCHEMA": "PaintVertices.1", "x": [0.1], "y": [0.2], "size": [0.01], "alpha": [1],
    }},
    {"OTIO_SCHEMA": "PaintEnd.1", "uuid": "s1"},
]

# Where a reviewer drew it: frame 10 of main/clip-1.
ANCHOR = {
    "sequenceId": "main", "capturedGeneration": 1, "clipGuid": "main/clip-1",
    "sourceTime": {"value": 10, "rate": 24}, "capturedSequenceTime": {"value": 10, "rate": 24},
    "capturedAt": 1787742000000,
}

# What a viewer who may comment hears on joining.
WELCOME = json.dumps({
    "v": 2, "type": "welcome", "sessionId": "s_1", "seq": 0, "ts": 0, "order": 1,
    "payload": {"selfId": "p_2", "selfUserId": "u_bea", "role": "viewer", "roster": [],
                "session": {"id": "s_1", "createdAt": 0}},
})

# The comment, as the host's session hears it from Furnace.
RELAY = {
    "type": "review_relay",
    "commit": {"clientMutationId": "m1", "anchor": ANCHOR, "bodyMarkdown": "Too dark.",
               "annotationCommands": STROKE},
    "ack": {"clientMutationId": "m1", "commentId": "c1", "serverOrder": 1, "revision": 1},
    "authorUserId": "u_bea", "authorPresenceId": "p_2", "createdAt": 0,
}

# A stroke in progress, as ORI's RV plugin sends one.
PARTIAL = {
    "type": "message", "kind": "Annotation.1/PARTIAL", "sender": "p_3",
    "payload": {"clip_guid": "main/clip-1", "frame": 10.0, "fps": 24.0,
                "events": [{"OTIO_SCHEMA": "PaintStart.1", "uuid": "s2"}]},
}


class LiveReview(unittest.TestCase):
    def setUp(self):
        ori.enable()

    def test_merges_by_uuid_and_kind(self):
        timeline = otio.adapters.read_from_string(REVIEW_TIMELINE, "otio_json")
        clip = ori.ensure_clip(ori.ensure_track(timeline), "main/clip-1", otio.opentime.RationalTime(10, 24))
        ori.set_commands(clip, [
            {"OTIO_SCHEMA": "TextAnnotation.1", "uuid": "t"},
            {"OTIO_SCHEMA": "PaintStart.1", "uuid": "a"},
        ])
        self.assertEqual(ori.merge_commands(clip, [
            {"OTIO_SCHEMA": "PaintStart.1", "uuid": "a", "brush": "gauss"},
            {"OTIO_SCHEMA": "PaintStart.1", "uuid": "b"},
        ]), (1, 1))
        self.assertEqual(ori.replace_by_kind(clip, [{"OTIO_SCHEMA": "PaintStart.1", "uuid": "c"}]), 2)
        self.assertEqual([c["uuid"] for c in json.loads(ori.commands(clip))], ["t", "c"])
        self.assertEqual(ori.merge_duplicates(timeline), 0)
        self.assertIsNone(ori.owner_of(clip, "t"))

    def test_a_reviewer_comments_and_the_host_folds_it(self):
        from opentimelineio.ext import sync

        partial = ori.read_partial(PARTIAL)
        self.assertEqual(partial["clip_guid"], "main/clip-1")
        self.assertEqual(partial["events"][0]["brush"], "circle")
        self.assertIsNone(ori.read_partial({"type": "leader", "leader": None}))

        reviewer = sync.Session(
            "join-code",
            identity={"capabilities": ["review.comment.create", "review.annotation.create"]},
            encodings=[],
        )
        reviewer.connect()
        while reviewer.poll_transmit(0) is not None:
            pass
        reviewer.receive(WELCOME)
        while reviewer.poll_event() is not None:
            pass
        source = otio.adapters.read_from_string(REVIEW_TIMELINE, "otio_json")
        mutation = ori.commit(reviewer, source, ANCHOR, "Too dark.", STROKE)
        self.assertTrue(mutation.startswith("mutation_"), mutation)
        sent = json.loads(reviewer.poll_transmit(0))
        self.assertEqual(sent["type"], "review_commit")
        self.assertEqual(sent["payload"]["annotationCommands"][0]["uuid"], "s1")

        ori.send_partial(reviewer, {"clip_guid": "main/clip-1", "frame": 10, "fps": 24, "events": []})
        with self.assertRaises(NotImplementedError):
            reviewer.poll_transmit(0)

        timeline = otio.adapters.read_from_string(REVIEW_TIMELINE, "otio_json")
        clip, already = ori.fold(timeline, RELAY)
        self.assertFalse(already)
        self.assertTrue(ori.fold(timeline, RELAY)[1])
        self.assertEqual([c["uuid"] for c in json.loads(ori.commands(clip))], ["s1"] * 3)
        self.assertEqual(ori.owner_of(clip, "s1"), "u_bea")
        self.assertEqual(clip.metadata["alchemist"]["comments"][0]["bodyMarkdown"], "Too dark.")

        # Another user may not redraw her stroke.
        other = dict(RELAY, authorUserId="u_cy",
                     ack=dict(RELAY["ack"], commentId="c2"))
        with self.assertRaises(PermissionError):
            ori.fold(timeline, other)
