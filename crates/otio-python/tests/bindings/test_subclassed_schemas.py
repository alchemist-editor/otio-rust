# SPDX-License-Identifier: Apache-2.0
# Copyright Contributors to the OpenTimelineIO project

"""Subclasses of concrete schemas, such as `Clip`, registered with
`register_type`.

Upstream's C++ holds such an object as an instance of the concrete class
whose type record names the subclass, with the subclass's fields among its
dynamic fields. Here the core holds the built-in object with an extension
naming the subclass, so it is a clip to every algorithm, and is written under
the subclass's name. Upstream's own tests only register subclasses of the two
root classes; these pin the rest.
"""

import json
import unittest

import opentimelineio as otio
from opentimelineio.core import (
    register_type,
    serializable_field,
    upgrade_function_for,
)
from opentimelineio.opentime import RationalTime, TimeRange


def seconds(start, duration, rate=24):
    return TimeRange(RationalTime(start * rate, rate), RationalTime(duration * rate, rate))


@register_type
class TakeClip(otio.schema.Clip):
    _serializable_label = "BindingsTakeClip.1"

    take = serializable_field("take", int, "Which take this is.")

    def __init__(self, name="", take=1, **kwargs):
        super().__init__(name=name, **kwargs)
        self.take = take


@register_type
class CircledTakeClip(TakeClip):
    _serializable_label = "BindingsCircledTakeClip.1"

    circled = serializable_field("circled", bool)

    def __init__(self, name="", take=1, circled=True, **kwargs):
        super().__init__(name=name, take=take, **kwargs)
        self.circled = circled


@register_type
class LaneTrack(otio.schema.Track):
    _serializable_label = "BindingsLaneTrack.1"

    lane = serializable_field("lane", int)


@register_type
class ReelStack(otio.schema.Stack):
    _serializable_label = "BindingsReelStack.1"

    reel = serializable_field("reel", str)


@register_type
class SlugGap(otio.schema.Gap):
    _serializable_label = "BindingsSlugGap.1"

    slug = serializable_field("slug", str)


@register_type
class NoteMarker(otio.schema.Marker):
    _serializable_label = "BindingsNoteMarker.1"

    author = serializable_field("author", str)


@register_type
class GradeEffect(otio.schema.Effect):
    _serializable_label = "BindingsGradeEffect.1"

    lut = serializable_field("lut", str)


@register_type
class CheckedReference(otio.schema.ExternalReference):
    _serializable_label = "BindingsCheckedReference.2"

    checksum = serializable_field("checksum", str)


@upgrade_function_for(CheckedReference, 2)
def _md5_to_checksum(data):
    upgraded = dict(data)
    upgraded["checksum"] = "md5:" + upgraded.pop("md5")
    return upgraded


def round_trip(obj):
    return otio.adapters.read_from_string(
        otio.adapters.write_to_string(obj, "otio_json"), "otio_json"
    )


def edit():
    """A timeline using every subclass above."""
    clip = TakeClip(
        "shot",
        take=3,
        source_range=seconds(0, 2),
        media_reference=CheckedReference(target_url="shot.mov"),
    )
    clip.media_reference.checksum = "sha1:abc"
    clip.markers.append(NoteMarker(name="note", marked_range=seconds(0, 1)))
    clip.markers[0].author = "ed"
    clip.effects.append(GradeEffect(effect_name="grade"))
    clip.effects[0].lut = "show.cube"
    gap = SlugGap(source_range=seconds(0, 1))
    gap.slug = "black"
    track = LaneTrack(name="V1", children=[clip, gap])
    track.lane = 2
    stack = ReelStack(children=[track])
    stack.reel = "A001"
    timeline = otio.schema.Timeline("cut")
    timeline.tracks = stack
    return timeline


class ASubclassIsItsBuiltIn(unittest.TestCase):
    def test_it_is_built_and_named_as_the_subclass(self):
        clip = TakeClip("shot", take=3, source_range=seconds(0, 2))
        self.assertIsInstance(clip, TakeClip)
        self.assertIsInstance(clip, otio.schema.Clip)
        self.assertEqual((clip.name, clip.take), ("shot", 3))
        self.assertEqual(clip.source_range, seconds(0, 2))
        self.assertEqual(clip.schema_name(), "BindingsTakeClip")
        self.assertEqual(clip.schema_version(), 1)
        self.assertFalse(clip.is_unknown_schema)
        # A clip with no reference still gets upstream's default one.
        self.assertIsInstance(clip.media_reference, otio.schema.MissingReference)

    def test_it_takes_part_in_compositions_and_algorithms(self):
        timeline = edit()
        track = timeline.tracks[0]
        clip = track[0]
        self.assertIsInstance(clip, TakeClip)
        self.assertIs(clip.parent(), track)
        self.assertEqual(track.duration(), RationalTime(72, 24))
        self.assertEqual(clip.range_in_parent(), seconds(0, 2))
        self.assertEqual(list(timeline.find_clips()), [clip])
        self.assertEqual(track.composition_kind, "Track")

        flat = otio.algorithms.flatten_stack(timeline.tracks)
        self.assertIsInstance(flat[0], TakeClip)
        self.assertEqual(flat[0].take, 3)

    def test_a_copy_keeps_its_class_and_fields(self):
        clip = TakeClip("shot", take=3)
        copied = clip.deepcopy()
        self.assertIsInstance(copied, TakeClip)
        self.assertEqual(copied.take, 3)
        self.assertTrue(copied.is_equivalent_to(clip))

    def test_a_subclass_of_a_subclass_derives_from_the_same_built_in(self):
        clip = CircledTakeClip("shot", take=2, circled=False)
        self.assertEqual(clip.schema_name(), "BindingsCircledTakeClip")
        read = round_trip(clip)
        self.assertIsInstance(read, CircledTakeClip)
        self.assertEqual((read.take, read.circled), (2, False))


class ASubclassRoundTrips(unittest.TestCase):
    def test_every_subclass_comes_back_as_itself(self):
        read = round_trip(edit())
        self.assertTrue(read.is_equivalent_to(edit()))
        stack = read.tracks
        track = stack[0]
        clip, gap = track
        self.assertIsInstance(stack, ReelStack)
        self.assertIsInstance(track, LaneTrack)
        self.assertIsInstance(clip, TakeClip)
        self.assertIsInstance(gap, SlugGap)
        self.assertIsInstance(clip.markers[0], NoteMarker)
        self.assertIsInstance(clip.effects[0], GradeEffect)
        self.assertIsInstance(clip.media_reference, CheckedReference)
        self.assertEqual(
            (
                stack.reel,
                track.lane,
                clip.take,
                gap.slug,
                clip.markers[0].author,
                clip.effects[0].lut,
                clip.media_reference.checksum,
            ),
            ("A001", 2, 3, "black", "ed", "show.cube", "sha1:abc"),
        )
        # And the built-in's own fields came back with them.
        self.assertEqual(track.name, "V1")
        self.assertEqual(clip.markers[0].marked_range, seconds(0, 1))
        self.assertEqual(clip.media_reference.target_url, "shot.mov")

    def test_it_is_written_under_its_own_name_with_its_fields_first(self):
        text = otio.adapters.write_to_string(TakeClip("shot", take=3), "otio_json")
        self.assertEqual(
            list(json.loads(text))[:4],
            ["OTIO_SCHEMA", "take", "metadata", "name"],
        )
        self.assertEqual(json.loads(text)["OTIO_SCHEMA"], "BindingsTakeClip.1")

    def test_an_older_version_is_upgraded_under_the_subclasss_name(self):
        read = otio.adapters.read_from_string(
            json.dumps(
                {
                    "OTIO_SCHEMA": "BindingsCheckedReference.1",
                    "target_url": "a.mov",
                    "md5": "123",
                }
            ),
            "otio_json",
        )
        self.assertIsInstance(read, CheckedReference)
        self.assertEqual((read.target_url, read.checksum), ("a.mov", "md5:123"))
        self.assertEqual(read.schema_version(), 2)

    def test_an_unregistered_subclass_reads_as_an_unknown_schema(self):
        text = otio.adapters.write_to_string(TakeClip("shot", take=3), "otio_json")
        text = text.replace("BindingsTakeClip.1", "BindingsNobodysClip.1")
        read = otio.adapters.read_from_string(text, "otio_json")
        self.assertTrue(read.is_unknown_schema)
        self.assertEqual(read.data["take"], 3)
        self.assertEqual(read.data["name"], "shot")


class UnregisteredSubclasses(unittest.TestCase):
    def test_a_plain_subclass_passes_its_arguments_on(self):
        class Lane(otio.schema.Track):
            def __init__(self, name, lane):
                super().__init__(name, kind=otio.schema.TrackKind.Audio)
                self.lane = lane

        lane = Lane("A1", 4)
        self.assertEqual((lane.name, lane.kind, lane.lane), ("A1", "Audio", 4))
        self.assertEqual(lane.schema_name(), "Track")

    def test_a_subclass_with_no_init_takes_the_built_ins_arguments(self):
        class Plain(otio.schema.Clip):
            pass

        plain = Plain("p", source_range=seconds(0, 1))
        self.assertEqual((plain.name, plain.source_range), ("p", seconds(0, 1)))
        self.assertEqual(plain.schema_name(), "Clip")


class DynamicFieldsOnABuiltIn(unittest.TestCase):
    def test_any_object_with_a_name_keeps_dynamic_fields_across_a_file(self):
        clip = otio.schema.Clip("c")
        clip._dynamic_fields["extra"] = {"a": 1}
        self.assertEqual(clip.schema_name(), "Clip")
        read = round_trip(clip)
        self.assertEqual(type(read), otio.schema.Clip)
        self.assertEqual(dict(read._dynamic_fields), {"extra": {"a": 1}})

    def test_registering_a_subclass_of_unknown_schema_is_refused(self):
        class Odd(otio._otio.UnknownSchema):
            _serializable_label = "BindingsOdd.1"

        with self.assertRaises(NotImplementedError):
            register_type(Odd)


if __name__ == "__main__":
    unittest.main()
