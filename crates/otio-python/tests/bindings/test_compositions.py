# SPDX-License-Identifier: Apache-2.0
# Copyright Contributors to the OpenTimelineIO project

"""What a composition has to do that upstream's C++ gets for free.

A child appended to a track starts life in a document of its own and has to
be moved into the track's; the move is what these check is invisible. The
slicing and the write-through metadata are here for the same reason: they are
built on top of a handful of single-index methods, so it is worth saying
plainly what the pieces add up to.
"""

import unittest

import opentimelineio as otio


class ChildrenMoveIntoTheirComposition(unittest.TestCase):
    def test_an_appended_clip_keeps_its_identity(self):
        track = otio.schema.Track(name="V1")
        clip = otio.schema.Clip(name="A")

        track.append(clip)

        self.assertIs(track[0], clip)
        self.assertIs(clip.parent(), track)

    def test_the_original_handle_still_works_after_the_move(self):
        track = otio.schema.Track(name="V1")
        clip = otio.schema.Clip(name="A")
        track.append(clip)

        clip.name = "renamed"
        clip.media_reference = otio.schema.ExternalReference("/a.mov")

        self.assertEqual(track[0].name, "renamed")
        self.assertEqual(track[0].media_reference.target_url, "/a.mov")

    def test_a_whole_track_moves_into_a_timeline(self):
        clip = otio.schema.Clip(name="A")
        track = otio.schema.Track(name="V1", children=[clip])
        timeline = otio.schema.Timeline(name="cut", tracks=[track])

        self.assertIs(timeline.tracks[0], track)
        self.assertIs(timeline.tracks[0][0], clip)
        self.assertEqual(timeline.find_clips()[0].name, "A")

    def test_a_child_cannot_be_in_two_compositions_at_once(self):
        clip = otio.schema.Clip(name="A")
        first = otio.schema.Track(name="V1", children=[clip])
        second = otio.schema.Track(name="V2")

        with self.assertRaises(ValueError):
            second.append(clip)

        # Taking it out of the first makes the move legal, and the wrapper
        # the caller is holding is still the one the second track reports.
        del first[0]
        second.append(clip)
        self.assertIs(second[0], clip)


class CompositionsBehaveLikeLists(unittest.TestCase):
    def track(self):
        return otio.schema.Track(
            children=[otio.schema.Clip(name=name) for name in "ABCD"]
        )

    def test_a_slice_reads_back(self):
        track = self.track()
        self.assertEqual([clip.name for clip in track[1:3]], ["B", "C"])
        self.assertEqual([clip.name for clip in track[::2]], ["A", "C"])
        self.assertEqual(track[-1].name, "D")

    def test_a_slice_can_be_replaced(self):
        track = self.track()
        track[1:3] = [otio.schema.Clip(name="X")]
        self.assertEqual([clip.name for clip in track], ["A", "X", "D"])

    def test_a_slice_can_be_deleted(self):
        track = self.track()
        del track[1:3]
        self.assertEqual([clip.name for clip in track], ["A", "D"])

    def test_the_list_methods_are_all_there(self):
        track = self.track()
        clip = track[2]
        self.assertEqual(len(track), 4)
        self.assertEqual(track.index(clip), 2)
        self.assertIn(clip, track)
        self.assertIs(track.pop(2), clip)
        self.assertEqual([c.name for c in track], ["A", "B", "D"])

    def test_a_failed_slice_assignment_leaves_the_track_as_it_was(self):
        # Putting a child in sets that child's parent, so a half-applied
        # slice would leave objects parented to a track they are not in.
        # The one that fails here is already in another track.
        track = self.track()
        taken = otio.schema.Track(children=[otio.schema.Clip(name="Z")])

        with self.assertRaises(ValueError):
            track[1:3] = [otio.schema.Clip(name="X"), taken[0]]

        self.assertEqual([clip.name for clip in track], ["A", "B", "C", "D"])


class MetadataNests(unittest.TestCase):
    def test_a_nested_dictionary_writes_through(self):
        clip = otio.schema.Clip(name="A")
        clip.metadata["shot"] = {"lens": {}}

        clip.metadata["shot"]["lens"]["mm"] = 50

        encoded = otio.adapters.otio_json.write_to_string(clip)
        decoded = otio.adapters.otio_json.read_from_string(encoded)
        self.assertEqual(decoded.metadata["shot"]["lens"]["mm"], 50)

    def test_a_generators_parameters_write_through(self):
        reference = otio.schema.GeneratorReference(generator_kind="SMPTEBars")
        reference.parameters["width"] = 1920

        encoded = otio.adapters.otio_json.write_to_string(reference)
        decoded = otio.adapters.otio_json.read_from_string(encoded)
        self.assertEqual(decoded.parameters["width"], 1920)


class CopyingAnObject(unittest.TestCase):
    def test_a_copy_shares_nothing_with_the_original(self):
        track = otio.schema.Track(
            name="V1", children=[otio.schema.Clip(name="A")]
        )
        track[0].metadata["k"] = "v"

        copy = track.deepcopy()

        self.assertIsNot(copy, track)
        self.assertIsNot(copy[0], track[0])
        self.assertIsNone(copy.parent())

        copy[0].metadata["k"] = "changed"
        self.assertEqual(track[0].metadata["k"], "v")

    def test_a_copy_of_a_timeline_carries_its_tracks(self):
        timeline = otio.schema.timeline_from_clips(
            [otio.schema.Clip(name="A")]
        )
        copy = timeline.deepcopy()

        self.assertEqual(len(copy.find_clips()), 1)
        self.assertIsNot(copy.find_clips()[0], timeline.find_clips()[0])


if __name__ == "__main__":
    unittest.main()
