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

# What Furnace sends a viewer that joins a session whose host has shared
# one timeline: the welcome, with the timeline's snapshot.
VIEWER_WELCOME = json.dumps({
    "v": 2, "type": "welcome", "sessionId": "s_1", "seq": 0, "ts": 0, "order": 1,
    "payload": {
        "selfId": "p_2", "role": "viewer",
        "roster": [{"id": "p_1", "displayName": "Host", "role": "host",
                    "joinedAt": 0, "connectionState": "connected", "muted": False}],
        "session": {"id": "s_1", "createdAt": 0},
        "snapshot": {"sequenceId": "main", "generation": 1, "otioJson": {
            "OTIO_SCHEMA": "Timeline.1", "name": "Coastline",
            "metadata": {"sync": {"guid": "t-1"}},
            "tracks": {"OTIO_SCHEMA": "Stack.1", "metadata": {"sync": {"guid": "s-1"}},
                       "children": [{"OTIO_SCHEMA": "Track.1", "name": "V1", "kind": "Video",
                                     "metadata": {"sync": {"guid": "v-1"}},
                                     "children": [{"OTIO_SCHEMA": "Clip.2", "name": "Waves",
                                                   "metadata": {"sync": {"guid": "c-1"}}}]}]}}},
    },
})

# What Furnace sends the host of a new session.
HOST_WELCOME = json.dumps({
    "v": 2, "type": "welcome", "sessionId": "s_1", "seq": 0, "ts": 0, "order": 1,
    "payload": {"selfId": "p_1", "role": "host", "roster": [],
                "session": {"id": "s_1", "createdAt": 0}},
})

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



class Sessions(unittest.TestCase):
    def setUp(self):
        sync.enable()

    def test_a_viewer_joins_and_applies_the_timeline(self):
        session = sync.Session(token="join-code", identity={"displayName": "Ada"}, encodings=[])
        self.assertIsNone(session.poll_transmit())
        session.connect()
        hello = json.loads(session.poll_transmit(now=1))
        self.assertEqual(hello["type"], "hello")
        self.assertEqual(hello["payload"]["displayName"], "Ada")
        self.assertIsNone(session.poll_transmit())

        session.receive(VIEWER_WELCOME)
        timeline = otio.schema.Timeline()
        events = []
        while (event := session.poll_event()) is not None:
            events.append(event)
        self.assertEqual([event["type"] for event in events], ["timeline", "joined"])
        self.assertTrue(events[0]["joining"])
        self.assertEqual(sync.handle(timeline, events[0]), {"handled": "applied"})
        self.assertEqual(sync.handle(timeline, events[1]), {"handled": "joined"})
        self.assertEqual(timeline.name, "Coastline")
        self.assertEqual([clip.name for clip in timeline.tracks[0]], ["Waves"])

        state = session.state
        self.assertTrue(state["joined"])
        self.assertEqual(state["role"], "viewer")
        self.assertEqual(state["peers"][0]["displayName"], "Host")
        self.assertIn('"sequenceId":"main"', session.timeline("main"))
        self.assertIsNone(session.timeline("other"))

        # Only the host holds the timeline's lease.
        self.assertFalse(session.may_send("timeline"))
        with self.assertRaises(ValueError):
            session.publish(timeline, "main")
        session.set_playhead("main", 12, playing=True)
        presence = json.loads(session.poll_transmit())
        self.assertEqual(presence["type"], "presence")

    def test_a_host_publishes_patches(self):
        session = sync.Session(token="host-token", encodings=[])
        session.connect()
        session.poll_transmit()
        session.receive(HOST_WELCOME.encode("utf-8"), binary=False)
        self.assertEqual(session.poll_event()["role"], "host")

        timeline = read(CUT)
        self.assertIn("replace_timeline", session.publish(timeline, "main"))
        self.assertEqual(json.loads(session.poll_transmit())["type"], "snapshot")
        self.assertEqual(session.publish(timeline, "main"), "[]")
        timeline.name = "fine cut"
        self.assertIn("set_property", session.publish(timeline, "main"))

    def test_bad_settings_frames_and_events_are_refused(self):
        with self.assertRaises(ValueError):
            sync.Session(encodings=["zstd"])
        session = sync.Session()
        session.connect()
        session.poll_transmit()
        with self.assertRaises(ConnectionError):
            session.receive('{"v":2,"type":"welcome","order":7,"payload":{}}')
        with self.assertRaises(ValueError):
            sync.handle(otio.schema.Timeline(), {"type": "timeline"})
        self.assertEqual(
            sync.handle(otio.schema.Timeline(), {"type": "leader", "leader": None}),
            {"handled": "ignored"},
        )


if __name__ == "__main__":
    unittest.main()
