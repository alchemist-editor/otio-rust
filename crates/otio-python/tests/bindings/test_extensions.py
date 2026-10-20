# SPDX-License-Identifier: Apache-2.0
"""otio-rust's extensions, reached from Python.

Extensions are not part of OpenTimelineIO, so upstream's suite says nothing
about them; these check the switch that turns them on, and timeline sync's
calls once it is on.
"""

import json
import pathlib
import subprocess
import sys
import unittest

import opentimelineio as otio
from opentimelineio import ext
from opentimelineio.ext import sync

REPO = pathlib.Path(__file__).resolve().parents[4]
FIXTURES = REPO / "extensions" / "sync" / "otio-ext-sync" / "tests" / "fixtures" / "otiodiff"

# The cut site/content/samples/apply-patches opens. Nothing in it has a GUID.
CUT = """{
  "OTIO_SCHEMA": "Timeline.1",
  "name": "cut",
  "tracks": {"OTIO_SCHEMA": "Stack.1", "children": [
    {"OTIO_SCHEMA": "Track.1", "name": "V1", "kind": "Video", "children": [
      {"OTIO_SCHEMA": "Clip.2", "name": "A"},
      {"OTIO_SCHEMA": "Clip.2", "name": "B"}]}]}
}"""

FELL_BACK = {
    "generation_regression",
    "guidless_transition",
    "metadata_sync_threshold_fallback",
    "sequence_changed",
}


def read(text):
    return otio.adapters.read_from_string(text, "otio_json")


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

    def test_sync_refuses_every_call_until_enabled(self):
        # Enabling is process-wide and other tests here turn sync on, so
        # this runs in a fresh interpreter.
        program = "\n".join(
            [
                "from opentimelineio.ext import sync",
                "try:",
                "    sync.new_guid()",
                "except RuntimeError as error:",
                "    print('refused', 'enable()' in str(error))",
                "else:",
                "    print('allowed')",
            ]
        )
        result = subprocess.run(
            [sys.executable, "-c", program],
            capture_output=True,
            text=True,
            check=True,
        )
        self.assertEqual(result.stdout.strip(), "refused True")


class Sync(unittest.TestCase):
    def setUp(self):
        sync.enable()

    def test_furnace_golden_corpus(self):
        cases = 0
        for directory in sorted(FIXTURES.iterdir()):
            if not directory.is_dir():
                continue
            cases += 1
            case = directory.name
            with self.subTest(case=case):
                old = (directory / "old.json").read_text(encoding="utf-8")
                new = (directory / "new.json").read_text(encoding="utf-8")
                want = (directory / "patches.json").read_text(encoding="utf-8")

                self.assertEqual(sync.diff(old, new, pretty=True) + "\n", want)

                stats = json.loads(sync.diff_stats(old, new))
                self.assertIs(stats["fellBack"], case in FELL_BACK, stats)

                old_timeline = json.dumps(json.loads(old)["otioJson"])
                compact = sync.diff(old, new)
                applied = sync.apply(old_timeline, compact)
                self.assertEqual(json.loads(applied), json.loads(new)["otioJson"])
        self.assertEqual(cases, 11)

    def test_a_transaction_carries_an_edit_to_another_copy(self):
        timeline = read(CUT)
        before = sync.begin(timeline, "main", 1)
        snapshot = json.loads(before)
        self.assertEqual((snapshot["sequenceId"], snapshot["generation"]), ("main", 1))
        peer = read(timeline.to_json_string())
        held_clip = peer.tracks[0][0]

        timeline.name = "fine cut"
        patches = sync.commit(timeline, before)
        self.assertIn('"path":"/name"', patches)
        sync.apply_to(peer, patches)
        self.assertEqual(peer.name, "fine cut")
        self.assertIs(peer.tracks[0][0], held_clip)
        self.assertEqual(held_clip.name, "A")

        # Both copies name the timeline by the same GUID, and stamping finds
        # nothing left to do.
        guid = sync.guid(timeline)
        self.assertTrue(guid)
        self.assertIs(sync.find(peer, guid), peer)
        self.assertIs(sync.find(peer, sync.guid(held_clip)), held_clip)
        self.assertEqual(sync.stamp(peer), 0)
        self.assertIsNone(sync.find(peer, "nobody"))
        self.assertTrue(sync.new_guid())

    def test_an_object_without_a_guid_has_none_until_given_one(self):
        clip = otio.schema.Clip(name="A")
        self.assertIsNone(sync.guid(clip))
        sync.set_guid(clip, "a")
        self.assertEqual(sync.guid(clip), "a")
        self.assertEqual(clip.metadata["sync"]["guid"], "a")

    def test_rollback_and_the_journal(self):
        timeline = read(CUT)
        track = timeline.tracks[0]
        clip = track[0]
        before = sync.begin(timeline, "main", 1)
        timeline.name = "fine cut"
        track.append(otio.schema.Clip(name="C"))
        sync.rollback(timeline, before)
        self.assertEqual(timeline.name, "cut")
        self.assertEqual([child.name for child in track], ["A", "B"])
        self.assertIs(timeline.tracks[0], track)
        self.assertIs(track[0], clip)

        sync.start_journal(timeline)
        sync.start_journal(timeline)
        self.assertFalse(sync.commit_journal(timeline))
        timeline.name = "fine cut"
        self.assertTrue(sync.commit_journal(timeline))
        self.assertFalse(sync.commit_journal(timeline))
        sync.stop_journal(timeline)

    def test_text_that_is_not_a_snapshot_is_a_value_error(self):
        with self.assertRaises(ValueError):
            sync.diff("not json", "{}")
        with self.assertRaises(ValueError):
            sync.apply_to(read(CUT), "not json")


if __name__ == "__main__":
    unittest.main()
