# SPDX-License-Identifier: Apache-2.0
# Copyright Contributors to the OpenTimelineIO project

"""What a copy shares: `clone`, `copy.deepcopy` and the algorithms that copy.

Upstream copies an object with `clone()`, which writes it out and reads it
back. Its writer has a way to refer back to an object met a second time
(`OTIO_REF_ID`), but only when built with `OTIO_INSTANCING_SUPPORT`, which
its build never defines; without it, the writer forgets an object once it
has written it. So an object held in two places comes out of a copy as two
objects, and one that holds itself is refused as a cycle. Every expectation
here was checked against an upstream build.
"""

import copy
import unittest

import opentimelineio as otio
from opentimelineio.opentime import RationalTime, TimeRange


def tr(start, duration):
    return TimeRange(RationalTime(start, 24), RationalTime(duration, 24))


def clip_holding_one_object_twice(name, duration):
    clip = otio.schema.Clip(name=name, source_range=tr(0, duration))
    held = otio.core.SerializableObjectWithMetadata(name="held")
    clip.metadata["a"] = held
    clip.metadata["b"] = held
    effect = otio.schema.Effect(name="fx")
    clip.effects.append(effect)
    clip.effects.append(effect)
    reference = otio.schema.ExternalReference(target_url="file:///media.mov")
    clip.set_media_references({"one": reference, "two": reference}, "one")
    return clip


def held_pairs(clip):
    references = clip.media_references()
    return [
        (clip.metadata["a"], clip.metadata["b"]),
        (clip.effects[0], clip.effects[1]),
        (references["one"], references["two"]),
    ]


def track_sharing_one_object():
    """Two clips in a track, the first holding objects twice and the second
    holding the first's metadata object as well."""
    first = clip_holding_one_object_twice("A", 24)
    second = otio.schema.Clip(name="B", source_range=tr(0, 24))
    second.metadata["a"] = first.metadata["a"]
    track = otio.schema.Track()
    track.append(first)
    track.append(second)
    return track


def holding_itself(clip):
    clip.metadata["self"] = clip
    return clip


class CopyTests(unittest.TestCase):
    def assert_copied_apart(self, copied, original):
        for (first, second), (held, _) in zip(
            held_pairs(copied), held_pairs(original)
        ):
            self.assertIsNot(first, second)
            self.assertIsNot(first, held)
            self.assertIsNot(second, held)
            self.assertEqual(first.name, held.name)

    def assert_one_per_clip(self, copied_track, original_track):
        self.assertIsNot(
            copied_track[0].metadata["a"], copied_track[1].metadata["a"]
        )
        self.assertIsNot(
            copied_track[1].metadata["a"], original_track[1].metadata["a"]
        )


class CloneTests(CopyTests):
    def test_clone_deepcopy_and_copy_deepcopy_copy_a_twice_held_object_twice(
        self,
    ):
        original = clip_holding_one_object_twice("A", 24)
        for copied in (
            original.clone(),
            original.deepcopy(),
            copy.deepcopy(original),
        ):
            self.assert_copied_apart(copied, original)
        # The original keeps its sharing.
        for first, second in held_pairs(original):
            self.assertIs(first, second)

    def test_copy_deepcopy_of_a_container_keeps_the_sharing(self):
        # Deep-copying a metadata dictionary or an effect list, rather than
        # an object, is Python's copy.deepcopy walking the container, and its
        # memo hands the second holder the copy made for the first. Upstream
        # behaves the same way.
        original = clip_holding_one_object_twice("A", 24)

        metadata = copy.deepcopy(original.metadata)
        self.assertIs(metadata["a"], metadata["b"])
        self.assertIsNot(metadata["a"], original.metadata["a"])

        effects = copy.deepcopy(original.effects)
        self.assertIs(effects[0], effects[1])
        self.assertIsNot(effects[0], original.effects[0])


class TrackTrimmedToRangeTests(CopyTests):
    def test_an_object_held_twice_is_copied_twice(self):
        track = track_sharing_one_object()

        trimmed = otio.algorithms.track_trimmed_to_range(track, tr(0, 48))

        self.assert_copied_apart(trimmed[0], track[0])
        self.assert_one_per_clip(trimmed, track)

    def test_timeline_trimmed_to_range_copies_the_same_way(self):
        timeline = otio.schema.Timeline()
        timeline.tracks.append(track_sharing_one_object())

        trimmed = otio.algorithms.timeline_trimmed_to_range(
            timeline, tr(0, 48)
        )

        self.assert_copied_apart(trimmed.tracks[0][0], timeline.tracks[0][0])
        self.assert_one_per_clip(trimmed.tracks[0], timeline.tracks[0])

    def test_a_clip_that_holds_itself_is_refused(self):
        # Upstream's Python copies the track with copy.deepcopy, and its C++
        # with clone(); both refuse the cycle.
        track = otio.schema.Track()
        track.append(
            holding_itself(otio.schema.Clip(name="A", source_range=tr(0, 24)))
        )
        with self.assertRaisesRegex(
            ValueError,
            "^Detected SerializableObject cycle while copying/serializing: "
            "cyclically encountered object has schema Clip$",
        ):
            otio.algorithms.track_trimmed_to_range(track, tr(0, 12))
        self.assertEqual(len(track), 1)


class FlattenStackTests(CopyTests):
    def test_an_object_held_twice_is_copied_twice(self):
        track = track_sharing_one_object()
        stack = otio.schema.Stack()
        stack.append(track)

        for flat in (
            otio.algorithms.flatten_stack(stack),
            otio.algorithms.flatten_stack([track]),
        ):
            self.assert_copied_apart(flat[0], track[0])
            self.assert_one_per_clip(flat, track)

    def test_a_clip_that_holds_itself_is_refused(self):
        # Upstream's flatten_stack sets the cycle error when it fails to copy
        # the clip, then goes on to use the copy it did not get and crashes
        # the interpreter. The error it had set is raised here instead.
        track = otio.schema.Track()
        track.append(
            holding_itself(otio.schema.Clip(name="A", source_range=tr(0, 24)))
        )
        with self.assertRaisesRegex(
            ValueError, "^Detected SerializableObject cycle"
        ):
            otio.algorithms.flatten_stack([track])


if __name__ == "__main__":
    unittest.main()
