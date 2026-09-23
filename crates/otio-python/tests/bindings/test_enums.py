# SPDX-License-Identifier: Apache-2.0
# Copyright Contributors to the OpenTimelineIO project

"""The enums, which print, convert, compare, copy and pickle as upstream's
pybind11 enums do.

Upstream binds MissingFramePolicy, NeighborGapPolicy and
MediaReferencePolicy with pybind11's `py::enum_`; ReferencePoint, which only
these bindings expose to Python, follows them. A pybind11 enum value is built
from its number, hashes as that number, and pickles by `__getstate__` (the
number) and `__setstate__` on an instance made by `cls.__new__(cls)`, which
is also how `copy.copy` and `copy.deepcopy` copy it. The expected pickles
below are the bytes upstream's own bindings write, so a pickle written by
either loads in the other.
"""

import copy
import operator
import os
import pickle
import tempfile
import unittest

import opentimelineio as otio
from opentimelineio.opentime import RationalTime, TimeRange

MissingFramePolicy = otio.schema.ImageSequenceReference.MissingFramePolicy
NeighborGapPolicy = otio.schema.Track.NeighborGapPolicy
MediaReferencePolicy = otio._otio.bundle.MediaReferencePolicy
ReferencePoint = otio.algorithms.ReferencePoint

# Each enum, and every value it has with its number.
ENUMS = {
    MissingFramePolicy: {"error": 0, "hold": 1, "black": 2},
    NeighborGapPolicy: {"never": 0, "around_transitions": 1},
    MediaReferencePolicy: {
        "error_if_not_file": 0,
        "missing_if_not_file": 1,
        "all_missing": 2,
    },
    ReferencePoint: {"Source": 0, "Sequence": 1, "Fit": 2},
}


def values():
    for enum, numbers in ENUMS.items():
        for name, number in numbers.items():
            yield enum, getattr(enum, name), number


class CopyingAndPickling(unittest.TestCase):
    def test_copies_are_equal_values_of_the_same_enum(self):
        # As upstream's: a copy is a new object, equal to the original.
        for enum, value, number in values():
            for copier in (copy.copy, copy.deepcopy):
                copied = copier(value)
                self.assertIs(type(copied), enum)
                self.assertEqual(copied, value)
                self.assertEqual(int(copied), number)

    def test_something_holding_a_value_can_be_deep_copied(self):
        held = {"policy": MediaReferencePolicy.all_missing, "points": [ReferencePoint.Fit]}
        self.assertEqual(copy.deepcopy(held), held)

    def test_every_protocol_from_two_up_round_trips(self):
        for enum, value, number in values():
            for protocol in range(2, pickle.HIGHEST_PROTOCOL + 1):
                back = pickle.loads(pickle.dumps(value, protocol=protocol))
                self.assertIs(type(back), enum)
                self.assertEqual(back, value)

    def test_pickles_are_upstreams_bytes(self):
        # Written by upstream's bindings (OpenTimelineIO at the same commit
        # as the reference checkout, pybind11 3.0.2). The nested enums are
        # found through the class upstream nests them in.
        cases = [
            (
                MissingFramePolicy.hold,
                2,
                b"\x80\x02c__builtin__\ngetattr\nq\x00copentimelineio._otio\n"
                b"ImageSequenceReference\nq\x01X\x12\x00\x00\x00MissingFramePolicy"
                b"q\x02\x86q\x03Rq\x04)\x81q\x05K\x01b.",
            ),
            (
                MissingFramePolicy.hold,
                4,
                b"\x80\x04\x95L\x00\x00\x00\x00\x00\x00\x00\x8c\x14"
                b"opentimelineio._otio\x94\x8c)ImageSequenceReference."
                b"MissingFramePolicy\x94\x93\x94)\x81\x94K\x01b.",
            ),
            (
                NeighborGapPolicy.never,
                4,
                b"\x80\x04\x95:\x00\x00\x00\x00\x00\x00\x00\x8c\x14"
                b"opentimelineio._otio\x94\x8c\x17Track.NeighborGapPolicy"
                b"\x94\x93\x94)\x81\x94K\x00b.",
            ),
            (
                MediaReferencePolicy.missing_if_not_file,
                2,
                b"\x80\x02copentimelineio._otio.bundle\nMediaReferencePolicy"
                b"\nq\x00)\x81q\x01K\x01b.",
            ),
            (
                MediaReferencePolicy.missing_if_not_file,
                4,
                b"\x80\x04\x95>\x00\x00\x00\x00\x00\x00\x00\x8c\x1b"
                b"opentimelineio._otio.bundle\x94\x8c\x14MediaReferencePolicy"
                b"\x94\x93\x94)\x81\x94K\x01b.",
            ),
        ]
        for value, protocol, upstream in cases:
            self.assertEqual(pickle.dumps(value, protocol=protocol), upstream)
            self.assertEqual(pickle.loads(upstream), value)

    def test_protocols_zero_and_one_are_refused(self):
        # Upstream's pybind11 aborts the interpreter here, trying to
        # allocate the bare base object that protocols 0 and 1 rebuild
        # through; refusing is the sound version of that.
        for _, value, _ in values():
            for protocol in (0, 1):
                with self.assertRaises(TypeError):
                    pickle.dumps(value, protocol=protocol)


class BuildingAValue(unittest.TestCase):
    def test_from_its_number(self):
        for enum, value, number in values():
            self.assertEqual(enum(number), value)
            self.assertEqual(enum(value=number), value)

    def test_state(self):
        for enum, value, number in values():
            self.assertEqual(value.__getstate__(), number)
            fresh = enum.__new__(enum)
            fresh.__setstate__(number)
            self.assertEqual(fresh, value)

    def test_a_number_that_names_no_value_is_refused(self):
        # pybind11 keeps it as `<MediaReferencePolicy.???: 7>` and hands the
        # C++ library an enum holding a value it does not have. There is no
        # such value to keep here.
        with self.assertRaisesRegex(
            ValueError, "^7 is not a valid MediaReferencePolicy$"
        ):
            MediaReferencePolicy(7)
        fresh = MediaReferencePolicy.__new__(MediaReferencePolicy)
        with self.assertRaises(ValueError):
            fresh.__setstate__(7)

    def test_values_hash_as_their_numbers(self):
        for _, value, number in values():
            self.assertEqual(hash(value), hash(number))
        self.assertEqual(
            {MediaReferencePolicy.all_missing: "x"}[MediaReferencePolicy(2)], "x"
        )


# Each enum's `__members__`, in the order upstream binds the values, which is
# the order pybind11 lists them in. ReferencePoint is these bindings' own, in
# the order the C++ enum declares it.
MEMBERS = {
    MissingFramePolicy: ["error", "hold", "black"],
    NeighborGapPolicy: ["around_transitions", "never"],
    MediaReferencePolicy: ["error_if_not_file", "missing_if_not_file", "all_missing"],
    ReferencePoint: ["Source", "Sequence", "Fit"],
}


class WhatAValueShows(unittest.TestCase):
    # Each expectation was read from upstream's bindings (pybind11 3.0.2),
    # whose `py::enum_` gives every enum the same repr, str, name, value,
    # int and index.

    def test_repr_and_str(self):
        self.assertEqual(repr(NeighborGapPolicy.never), "<NeighborGapPolicy.never: 0>")
        self.assertEqual(str(NeighborGapPolicy.never), "NeighborGapPolicy.never")
        self.assertEqual(
            repr(MediaReferencePolicy.all_missing),
            "<MediaReferencePolicy.all_missing: 2>",
        )
        self.assertEqual(repr(MissingFramePolicy.hold), "<MissingFramePolicy.hold: 1>")
        self.assertEqual(repr(ReferencePoint.Fit), "<ReferencePoint.Fit: 2>")
        for enum, value, number in values():
            short = enum.__name__ + "." + value.name
            self.assertEqual(repr(value), f"<{short}: {number}>")
            self.assertEqual(str(value), short)
            self.assertEqual(format(value), short)
            self.assertEqual(f"{value}", short)

    def test_name_value_and_number(self):
        for enum, value, number in values():
            self.assertEqual(value.name, [n for n, v in ENUMS[enum].items() if v == number][0])
            self.assertEqual(value.value, number)
            self.assertIs(type(value.value), int)
            self.assertEqual(int(value), number)
            self.assertEqual(value.__int__(), number)
            # `__index__` makes it an integer anywhere Python wants one.
            self.assertEqual(operator.index(value), number)
            self.assertEqual(value.__index__(), number)
            self.assertEqual(hex(value), hex(number))
            self.assertEqual(float(value), float(number))
            self.assertEqual("abc"[value], "abc"[number])
            # It is not an int, and is true even when its number is 0.
            self.assertNotIsInstance(value, int)
            self.assertTrue(value)
            # A value converts to the value it is.
            self.assertEqual(enum(value), value)

    def test_equal_to_its_number_whatever_holds_it(self):
        # pybind11 compares an enum that converts to its number as
        # `int(self) == other`: equal to 1, 1.0 and True, and to a value of
        # another enum numbered the same; never to None; and with no order.
        value = MediaReferencePolicy.missing_if_not_file
        for same in (1, 1.0, True, NeighborGapPolicy.around_transitions, MediaReferencePolicy(1)):
            self.assertTrue(value == same, same)
            self.assertTrue(same == value, same)
            self.assertFalse(value != same, same)
        for other in (2, "missing_if_not_file", None, NeighborGapPolicy.never):
            self.assertFalse(value == other, other)
            self.assertFalse(other == value, other)
            self.assertTrue(value != other, other)
        for compare in (operator.lt, operator.le, operator.gt, operator.ge):
            with self.assertRaises(TypeError):
                compare(value, value)
            with self.assertRaises(TypeError):
                compare(value, 1)


class Members(unittest.TestCase):
    def test_a_dict_in_upstreams_order(self):
        for enum, names in MEMBERS.items():
            members = enum.__members__
            self.assertIs(type(members), dict)
            self.assertEqual(list(members), names)
            for name, value in members.items():
                # The very value the class holds under that name.
                self.assertIs(value, getattr(enum, name))
                self.assertEqual(value.name, name)

    def test_upstreams_members_exactly(self):
        self.assertEqual(
            repr(NeighborGapPolicy.__members__),
            "{'around_transitions': <NeighborGapPolicy.around_transitions: 1>, "
            "'never': <NeighborGapPolicy.never: 0>}",
        )
        self.assertEqual(
            repr(MediaReferencePolicy.__members__),
            "{'error_if_not_file': <MediaReferencePolicy.error_if_not_file: 0>, "
            "'missing_if_not_file': <MediaReferencePolicy.missing_if_not_file: 1>, "
            "'all_missing': <MediaReferencePolicy.all_missing: 2>}",
        )
        self.assertEqual(
            repr(MissingFramePolicy.__members__),
            "{'error': <MissingFramePolicy.error: 0>, "
            "'hold': <MissingFramePolicy.hold: 1>, "
            "'black': <MissingFramePolicy.black: 2>}",
        )

    def test_a_new_dict_each_time(self):
        # pybind11 builds it afresh on every read, so changing one changes
        # nothing else.
        members = MediaReferencePolicy.__members__
        self.assertIsNot(members, MediaReferencePolicy.__members__)
        members["extra"] = 7
        del members["all_missing"]
        self.assertEqual(list(MediaReferencePolicy.__members__), MEMBERS[MediaReferencePolicy])

    def test_read_from_a_value_too(self):
        for enum, value, _ in values():
            self.assertEqual(value.__members__, enum.__members__)


class TheAdapterLayer(unittest.TestCase):
    def test_a_media_policy_goes_through_write_to_file(self):
        # The adapter layer deep-copies its keyword arguments for the hooks,
        # so an enum that could not be copied failed here before the adapter
        # ran.
        timeline = otio.schema.Timeline()
        track = otio.schema.Track()
        timeline.tracks.append(track)
        track.append(
            otio.schema.Clip(
                media_reference=otio.schema.ExternalReference(
                    target_url="http://example.com/a.mov"
                ),
                source_range=TimeRange(RationalTime(0, 24), RationalTime(24, 24)),
            )
        )
        policy = MediaReferencePolicy.all_missing
        with tempfile.TemporaryDirectory() as directory:
            for suffix in ("otioz", "otiod"):
                path = os.path.join(directory, f"x.{suffix}")
                otio.adapters.write_to_file(timeline, path, media_policy=policy)
                read = otio.adapters.read_from_file(path)
                reference = read.tracks[0][0].media_reference
                self.assertIsInstance(reference, otio.schema.MissingReference)
                self.assertEqual(
                    reference.metadata["missing_reference_because"],
                    "'all_missing' specified as the MediaReferencePolicy",
                )

            # The default policy refuses a reference that is not a file.
            with self.assertRaises(OSError):
                otio.adapters.write_to_file(
                    timeline,
                    os.path.join(directory, "y.otioz"),
                    media_policy=MediaReferencePolicy.error_if_not_file,
                )


if __name__ == "__main__":
    unittest.main()
