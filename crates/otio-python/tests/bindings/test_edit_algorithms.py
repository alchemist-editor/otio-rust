# SPDX-License-Identifier: Apache-2.0
# Copyright Contributors to the OpenTimelineIO project

"""The edit operations in `opentimelineio.algorithms`.

Upstream's C++ library has ten editing algorithms (`otio::algo`, in
editAlgorithm.h) that its Python package does not bind. These bindings export
them under the C++ names, with its parameter names and defaults in snake case.
The cases here are ported from upstream's tests/test_editAlgorithm.cpp, with
its ranges and expectations, followed by what the bindings add: the exceptions
upstream's error handler would raise, and objects an edit takes out living on
while Python holds them, as upstream's reference-counted ones do.
"""

import gc
import unittest
import weakref

import opentimelineio as otio
from opentimelineio.algorithms import ReferencePoint
from opentimelineio.opentime import RationalTime, TimeRange


def rt(value):
    return RationalTime(value, 24.0)


def tr(start, duration):
    return TimeRange(rt(start), rt(duration))


def clip(name, start, duration, media_reference=None):
    return otio.schema.Clip(
        name=name,
        media_reference=media_reference,
        source_range=tr(start, duration),
    )


def gap(name, start, duration):
    return otio.schema.Gap(name=name, source_range=tr(start, duration))


def track_of(*children):
    track = otio.schema.Track()
    for child in children:
        track.append(child)
    return track


def track_ranges(track):
    return [track.trimmed_range_of_child(child) for child in track]


def clip_ranges(track):
    return [child.trimmed_range() for child in track]


class SliceTests(unittest.TestCase):
    # test_edit_slice_1

    def slice_one_clip(self, time):
        track = track_of(clip("clip_0", 0, 24))
        otio.algorithms.slice(track, rt(time))
        return track_ranges(track)

    def test_slicing_in_the_middle(self):
        self.assertEqual(self.slice_one_clip(12), [tr(0, 12), tr(12, 12)])

    def test_slicing_at_either_end_does_nothing(self):
        self.assertEqual(self.slice_one_clip(0), [tr(0, 24)])

        # Upstream's test passes no error status here, so it does not see
        # that nothing is under the end of the track; a binding raises it.
        track = track_of(clip("clip_0", 0, 24))
        with self.assertRaisesRegex(
            ValueError, "^object is not descendent of Item type$"
        ):
            otio.algorithms.slice(track, rt(24))
        self.assertEqual(track_ranges(track), [tr(0, 24)])

    def test_slicing_near_either_end(self):
        self.assertEqual(self.slice_one_clip(1), [tr(0, 1), tr(1, 23)])
        self.assertEqual(self.slice_one_clip(23), [tr(0, 23), tr(23, 1)])

    def test_slicing_inside_a_kept_transition_is_refused(self):
        track = track_of(
            clip("clip_0", 0, 24),
            otio.schema.Transition(in_offset=rt(10), out_offset=rt(10)),
            clip("clip_1", 0, 24),
        )
        with self.assertRaisesRegex(ValueError, "^cannot trim transition$"):
            otio.algorithms.slice(track, rt(20), remove_transitions=False)


class OverwriteTests(unittest.TestCase):
    def test_overwriting_past_the_end_leaves_a_gap(self):
        # test_edit_overwrite_0
        track = track_of(clip("clip_0", 0, 24))
        clip_1 = clip("clip_1", 0, 24)

        otio.algorithms.overwrite(clip_1, track, tr(48, 24))

        self.assertEqual(len(track), 3)
        self.assertIsInstance(track[1], otio.schema.Gap)
        self.assertEqual(track.duration(), rt(72))
        self.assertEqual(clip_1.trimmed_range_in_parent(), tr(48, 24))
        self.assertIs(clip_1.parent(), track)
        self.assertIs(track[2], clip_1)

    def test_a_fill_template_fills_the_space(self):
        track = track_of(clip("clip_0", 0, 24))
        template = gap("filler", 0, 24)

        otio.algorithms.overwrite(
            clip("clip_1", 0, 24), track, tr(48, 24), True, template
        )

        self.assertIs(track[1], template)
        self.assertEqual(track[1].name, "filler")


class InsertTests(unittest.TestCase):
    def test_inserting_splits_the_clip_and_lengthens_the_track(self):
        # test_edit_insert_1
        track = track_of(clip("clip_0", 0, 24), clip("clip_1", 0, 24))

        otio.algorithms.insert(clip("insert_1", 0, 12), track, rt(12))

        self.assertEqual(len(track), 4)
        self.assertEqual(track.duration(), rt(60))
        self.assertEqual(
            track_ranges(track),
            [tr(0, 12), tr(12, 12), tr(24, 12), tr(36, 24)],
        )
        self.assertEqual(
            [child.name for child in track],
            ["clip_0", "insert_1", "clip_0", "clip_1"],
        )


class TrimTests(unittest.TestCase):
    def test_trimming_the_head_grows_the_gap_before(self):
        # test_edit_trim_1
        clip_1 = clip("clip_1", 5, 50)
        track = track_of(gap("gap_0", 0, 20), clip_1, clip("clip_2", 0, 10))
        duration = track.duration()

        otio.algorithms.trim(clip_1, rt(5), rt(0))

        self.assertEqual(track.duration(), duration)
        self.assertEqual(
            track_ranges(track), [tr(0, 25), tr(25, 45), tr(70, 10)]
        )

    def test_trimming_an_item_in_no_track_is_not_a_child(self):
        with self.assertRaisesRegex(
            otio.exceptions.NotAChildError,
            "^item is not a child of specified object$",
        ):
            otio.algorithms.trim(clip("alone", 0, 24), rt(1), rt(0))


class SlipTests(unittest.TestCase):
    # test_edit_slip

    def slip(self, delta):
        media = otio.core.MediaReference(
            name="media_0", available_range=tr(-15, 63)
        )
        clip_0 = clip("clip_0", 0, 36, media)
        otio.algorithms.slip(clip_0, rt(delta))
        return clip_0.trimmed_range()

    def test_slipping_within_the_media(self):
        self.assertEqual(self.slip(5), tr(5, 36))
        self.assertEqual(self.slip(-5), tr(-5, 36))

    def test_slipping_is_clamped_to_the_media(self):
        self.assertEqual(self.slip(20), tr(12, 36))
        self.assertEqual(self.slip(-30), tr(-15, 36))


class SlideTests(unittest.TestCase):
    # test_edit_slide

    def slide(self, delta):
        media = otio.core.MediaReference(
            name="media_0", available_range=tr(0, 48)
        )
        clip_1 = clip("clip_1", 0, 30)
        track = track_of(clip("clip_0", 0, 24, media), clip_1, clip("clip_2", 0, 40))
        otio.algorithms.slide(clip_1, rt(delta))
        return track_ranges(track)

    def test_sliding_right_stretches_the_clip_before(self):
        self.assertEqual(self.slide(12), [tr(0, 36), tr(36, 30), tr(66, 40)])

    def test_sliding_left(self):
        self.assertEqual(self.slide(-10), [tr(0, 14), tr(14, 30), tr(44, 40)])

    def test_sliding_past_the_start_changes_nothing(self):
        self.assertEqual(self.slide(-24), [tr(0, 24), tr(24, 30), tr(54, 40)])


class RippleAndRollTests(unittest.TestCase):
    def test_ripple(self):
        # test_edit_ripple_1
        clip_1 = clip("clip_1", 5, 25)
        track = track_of(gap("gap_0", 0, 20), clip_1, clip("clip_2", 5, 20))

        otio.algorithms.ripple(clip_1, rt(10), rt(0))

        self.assertEqual(
            track_ranges(track), [tr(0, 20), tr(20, 15), tr(35, 20)]
        )
        self.assertEqual(clip_ranges(track), [tr(0, 20), tr(15, 15), tr(5, 20)])

    def test_roll(self):
        # test_edit_roll_1
        clip_1 = clip("clip_1", 5, 30)
        track = track_of(gap("gap_0", 0, 20), clip_1, clip("clip_2", 5, 20))

        otio.algorithms.roll(clip_1, rt(10), rt(0))

        self.assertEqual(
            track_ranges(track), [tr(0, 30), tr(30, 20), tr(50, 20)]
        )
        self.assertEqual(clip_ranges(track), [tr(0, 30), tr(15, 20), tr(5, 20)])


class FillTests(unittest.TestCase):
    # test_edit_fill

    def fill(self, clip_range, time, *reference_point):
        track = track_of(
            clip("clip_0", 0, 20), gap("gap_0", 5, 30), clip("clip_2", 5, 20)
        )
        fill_0 = otio.schema.Clip(name="fill_0", source_range=clip_range)
        otio.algorithms.fill(fill_0, track, rt(time), *reference_point)
        return track

    def test_the_reference_point_defaults_to_source(self):
        # test_edit_fill_2
        track = self.fill(tr(0, 35), 20)
        self.assertEqual(
            track_ranges(track), [tr(0, 20), tr(20, 35), tr(55, 5)]
        )
        self.assertEqual(clip_ranges(track), [tr(0, 20), tr(0, 35), tr(20, 5)])

    def test_fit(self):
        # test_edit_fill_1
        track = self.fill(tr(0, 35), 20, ReferencePoint.Fit)
        self.assertEqual(
            track_ranges(track), [tr(0, 20), tr(20, 35), tr(55, 20)]
        )
        self.assertEqual(clip_ranges(track), [tr(0, 20), tr(0, 35), tr(5, 20)])

    def test_sequence_keeps_the_track_length(self):
        # test_edit_fill_5
        track = self.fill(tr(0, 35), 20, ReferencePoint.Sequence)
        self.assertEqual(track.duration(), rt(70))
        self.assertEqual(
            track_ranges(track), [tr(0, 20), tr(20, 30), tr(50, 20)]
        )
        self.assertEqual(clip_ranges(track), [tr(0, 20), tr(5, 30), tr(5, 20)])

    def test_filling_where_there_is_no_gap(self):
        with self.assertRaisesRegex(
            ValueError, "^object is not descendent of Gap type$"
        ):
            self.fill(tr(0, 5), 5, ReferencePoint.Source)

    def test_reference_points(self):
        self.assertEqual(
            [ReferencePoint.Source, ReferencePoint.Sequence, ReferencePoint.Fit],
            [ReferencePoint.Source, ReferencePoint.Sequence, ReferencePoint.Fit],
        )
        self.assertNotEqual(ReferencePoint.Source, ReferencePoint.Fit)


class RemoveTests(unittest.TestCase):
    def test_remove(self):
        # test_edit_remove_0
        track = track_of(
            clip("clip_0", 0, 50), clip("clip_1", 5, 50), clip("clip_2", 0, 10)
        )
        duration = track.duration()

        otio.algorithms.remove(track, rt(55))

        self.assertEqual(track.duration(), duration)
        self.assertIsInstance(track[1], otio.schema.Gap)
        self.assertEqual(
            track_ranges(track), [tr(0, 50), tr(50, 50), tr(100, 10)]
        )

        fill_0 = clip("fill_0", 0, 10)
        otio.algorithms.remove(track, rt(55), True, fill_0)

        self.assertEqual(track.duration(), rt(70))
        self.assertIs(track[1], fill_0)
        self.assertEqual(
            track_ranges(track), [tr(0, 50), tr(50, 10), tr(60, 10)]
        )
        self.assertEqual(clip_ranges(track), [tr(0, 50), tr(0, 10), tr(0, 10)])

    def test_removing_without_filling_closes_up(self):
        track = track_of(clip("clip_0", 0, 24), clip("clip_1", 0, 24))
        otio.algorithms.remove(track, rt(30), fill=False)
        self.assertEqual([child.name for child in track], ["clip_0"])

    def test_removing_where_there_is_nothing(self):
        track = track_of(clip("clip_0", 0, 24))
        with self.assertRaisesRegex(
            ValueError, "^object is not descendent of Item type$"
        ):
            otio.algorithms.remove(track, rt(100))


def holding_itself(item):
    item.metadata["cycle"] = item
    return item


class MetadataCycleTests(unittest.TestCase):
    # Upstream's "regression: slice/overwrite/insert/fill fails gracefully",
    # which expect each edit to fail on a clip that holds itself in its
    # metadata. Here they succeed, on purpose: see otio_core::edit.
    #
    # Upstream copies the leftover piece, or the clip dropped into a gap,
    # with clone(), which writes the object out and reads it back and so
    # cannot carry the cycle. From Python that is ValueError("Detected
    # SerializableObject cycle while copying/serializing: cyclically
    # encountered object has schema Clip"), and slice, overwrite and insert
    # raise it only after changing the track, leaving the clip cut short and
    # the rest of it gone. (Upstream's C++ tests check TYPE_MISMATCH instead,
    # which comes from storing a Retainer<Clip> its writer has no entry for,
    # not from the cycle.) The copy here is made in memory and keeps the
    # cycle, so each edit leaves the track as it would for a clip with no
    # cycle, and each copy holds itself.

    def test_slice(self):
        track = track_of(holding_itself(clip("big clip", 0, 24)))

        otio.algorithms.slice(track, rt(12), False)

        self.assertEqual(track_ranges(track), [tr(0, 12), tr(12, 12)])
        self.assertEqual(clip_ranges(track), [tr(0, 12), tr(12, 12)])
        self.assertIsNot(track[0], track[1])
        for piece in track:
            self.assertIs(piece.metadata["cycle"], piece)

    def test_overwrite(self):
        track = track_of(holding_itself(clip("big clip", 0, 24)))
        small = holding_itself(clip("small clip", 0, 5))

        otio.algorithms.overwrite(small, track, tr(0, 12), True, None)

        self.assertEqual(
            [child.name for child in track], ["small clip", "big clip"]
        )
        self.assertEqual(track_ranges(track), [tr(0, 5), tr(5, 12)])
        self.assertEqual(clip_ranges(track), [tr(0, 5), tr(12, 12)])
        self.assertIs(track[0], small)
        self.assertIs(track[1].metadata["cycle"], track[1])

    def test_insert(self):
        big = holding_itself(clip("big clip", 0, 24))
        track = track_of(big)
        small = holding_itself(clip("small clip", 0, 5))

        otio.algorithms.insert(small, track, rt(12), True, None)

        self.assertEqual(
            [child.name for child in track],
            ["big clip", "small clip", "big clip"],
        )
        self.assertEqual(
            track_ranges(track), [tr(0, 12), tr(12, 5), tr(17, 12)]
        )
        self.assertEqual(
            clip_ranges(track), [tr(0, 12), tr(0, 5), tr(17, 12)]
        )
        self.assertIs(track[0].metadata["cycle"], big)
        self.assertIs(track[2].metadata["cycle"], track[2])

    def test_fill(self):
        big = holding_itself(clip("big clip", 0, 24))
        track = track_of(
            holding_itself(clip("small clip", 0, 5)),
            gap("gap", 0, 20),
            holding_itself(clip("small clip 2", 0, 5)),
        )

        otio.algorithms.fill(big, track, rt(12), ReferencePoint.Sequence)

        self.assertEqual(
            [child.name for child in track],
            ["small clip", "gap", "big clip", "small clip 2"],
        )
        self.assertEqual(
            track_ranges(track), [tr(0, 5), tr(5, 7), tr(12, 13), tr(25, 5)]
        )
        self.assertEqual(
            clip_ranges(track), [tr(0, 5), tr(0, 7), tr(0, 13), tr(0, 5)]
        )
        self.assertIsNot(track[2], big)
        self.assertIs(track[2].metadata["cycle"], track[2])

    def test_the_result_still_cannot_be_written(self):
        # Allowing the edit does not make the cycle writable: JSON has no
        # way to say it, and upstream and this both refuse to write it.
        track = track_of(holding_itself(clip("big clip", 0, 24)))
        otio.algorithms.slice(track, rt(12), False)
        with self.assertRaises(ValueError):
            otio.adapters.otio_json.write_to_string(track)



def holding_one_object_twice(item):
    """Holds one object in each place an object can be held twice: under
    two metadata keys, twice in the effects, and under two media reference
    keys. Returns the three objects."""
    held = otio.core.SerializableObjectWithMetadata(name="held")
    item.metadata["a"] = held
    item.metadata["b"] = held
    effect = otio.schema.Effect(name="fx")
    item.effects.append(effect)
    item.effects.append(effect)
    reference = otio.schema.ExternalReference(target_url="file:///media.mov")
    item.set_media_references({"one": reference, "two": reference}, "one")
    return held, effect, reference


def held_pairs(item):
    references = item.media_references()
    return [
        (item.metadata["a"], item.metadata["b"]),
        (item.effects[0], item.effects[1]),
        (references["one"], references["two"]),
    ]


class CopiedPieceTests(unittest.TestCase):
    # Upstream makes the copy in each of these edits with clone(), which
    # writes the item out and reads it back. Its writer, built as it always
    # is without OTIO_INSTANCING_SUPPORT, forgets an object once written, so
    # an object the item holds in two places comes out of the copy as two.
    # Checked against an upstream build of the C++ edits: the copy holds two
    # objects where the item held one twice, for metadata, effects and media
    # references alike, and none of the originals, while the item left in
    # place keeps its own.

    def assert_copied_apart(self, copy, originals):
        for (first, second), original in zip(held_pairs(copy), originals):
            self.assertIsNot(first, second)
            self.assertIsNot(first, original)
            self.assertIsNot(second, original)
            self.assertEqual(first.name, original.name)

    def assert_still_held_twice(self, item, originals):
        for (first, second), original in zip(held_pairs(item), originals):
            self.assertIs(first, original)
            self.assertIs(second, original)

    def test_slice(self):
        big = clip("big clip", 0, 24)
        originals = holding_one_object_twice(big)
        track = track_of(big)

        otio.algorithms.slice(track, rt(12))

        self.assertIs(track[0], big)
        self.assert_still_held_twice(big, originals)
        self.assert_copied_apart(track[1], originals)

    def test_overwrite(self):
        big = clip("big clip", 0, 24)
        originals = holding_one_object_twice(big)
        track = track_of(big)

        otio.algorithms.overwrite(clip("small clip", 0, 4), track, tr(8, 4))

        self.assertEqual(
            [child.name for child in track],
            ["big clip", "small clip", "big clip"],
        )
        self.assert_still_held_twice(big, originals)
        self.assert_copied_apart(track[2], originals)

    def test_insert(self):
        big = clip("big clip", 0, 24)
        originals = holding_one_object_twice(big)
        track = track_of(big)

        otio.algorithms.insert(clip("small clip", 0, 4), track, rt(12))

        self.assert_still_held_twice(big, originals)
        self.assert_copied_apart(track[2], originals)

    def test_fill(self):
        big = clip("big clip", 0, 24)
        originals = holding_one_object_twice(big)
        track = track_of(
            clip("before", 0, 5), gap("gap", 0, 20), clip("after", 0, 5)
        )

        otio.algorithms.fill(big, track, rt(12), ReferencePoint.Sequence)

        self.assertIsNot(track[2], big)
        self.assert_still_held_twice(big, originals)
        self.assert_copied_apart(track[2], originals)

class ArgumentTests(unittest.TestCase):
    def test_arguments_of_the_wrong_kind(self):
        track = track_of(clip("clip_0", 0, 24))
        with self.assertRaises(TypeError):
            otio.algorithms.overwrite(track, clip("c", 0, 1), tr(0, 1))
        with self.assertRaises(TypeError):
            otio.algorithms.slice(clip("c", 0, 1), rt(0))
        with self.assertRaises(TypeError):
            otio.algorithms.slip(otio.schema.Marker(), rt(0))


class LifetimeTests(unittest.TestCase):
    def test_a_clip_taken_out_lives_on_while_python_holds_it(self):
        clip_1 = clip("clip_1", 0, 24)
        track = track_of(clip("clip_0", 0, 24), clip_1)

        otio.algorithms.remove(track, rt(30))

        self.assertIsNone(clip_1.parent())
        self.assertEqual(clip_1.name, "clip_1")
        self.assertEqual(clip_1.trimmed_range(), tr(0, 24))
        # And can be used again.
        otio.algorithms.overwrite(clip_1, track, tr(24, 24))
        self.assertIs(track[1], clip_1)

    def test_a_clip_taken_out_that_nothing_holds_is_freed(self):
        track = track_of(clip("clip_0", 0, 24), clip("clip_1", 0, 24))
        held = weakref.ref(track[1])
        gc.collect()
        self.assertIsNotNone(held())

        otio.algorithms.overwrite(clip("over", 0, 24), track, tr(24, 24))
        gc.collect()

        self.assertIsNone(held())
        self.assertEqual([child.name for child in track], ["clip_0", "over"])

    def test_the_incoming_item_is_owned_by_the_track(self):
        track = track_of(clip("clip_0", 0, 24))
        otio.algorithms.insert(clip("new", 0, 12), track, rt(0))
        gc.collect()
        self.assertEqual(track[0].name, "new")
        self.assertIs(track[0].parent(), track)


if __name__ == "__main__":
    unittest.main()
