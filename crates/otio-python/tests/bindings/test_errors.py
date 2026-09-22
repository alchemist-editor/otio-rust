# SPDX-License-Identifier: Apache-2.0
# Copyright Contributors to the OpenTimelineIO project

"""Error messages, and the exceptions that carry them, as upstream raises them.

Upstream's C++ reports a failure as an outcome and some details, and its
Python bindings turn that into an exception of a type chosen by the outcome,
with a message that ends in `str()` of the object concerned. Code in the wild
matches on both, so both are pinned here, character for character. Each
expected message was taken from OpenTimelineIO itself.

Where a message names an object, the expectation is built from `str()` of
that object here, so that these check the shape of the message rather than
how the object prints, which is a separate matter.
"""

import unittest

import opentimelineio as otio
from opentimelineio import opentime

RT = opentime.RationalTime
TR = opentime.TimeRange


def span(start, duration):
    return TR(RT(start, 24), RT(duration, 24))


class ObjectModelErrors(unittest.TestCase):
    def assertRaisesWith(self, kind, message, call):
        with self.assertRaises(kind) as caught:
            call()
        self.assertEqual(str(caught.exception), message)

    def test_an_item_with_no_parent(self):
        clip = otio.schema.Clip(name="c")
        for call in (clip.range_in_parent, clip.trimmed_range_in_parent):
            self.assertRaisesWith(
                otio.exceptions.NotAChildError,
                f"item has no parent: {clip}",
                call,
            )

    def test_a_transition_with_no_parent_says_what_it_could_not_compute(self):
        transition = otio.schema.Transition(name="t")
        self.assertRaisesWith(
            otio.exceptions.NotAChildError,
            "item has no parent: cannot compute range in parent because item "
            f"has no parent: {transition}",
            transition.range_in_parent,
        )
        self.assertRaisesWith(
            otio.exceptions.NotAChildError,
            "item has no parent: cannot compute trimmed range in parent "
            f"because item has no parent: {transition}",
            transition.trimmed_range_in_parent,
        )

    def test_a_clip_with_no_available_range(self):
        clip = otio.schema.Clip(name="c")
        expected = (
            "Cannot compute available range: No available_range set on "
            f"media reference on clip: {clip}"
        )
        for call in (clip.available_range, clip.duration):
            self.assertRaisesWith(
                otio.exceptions.CannotComputeAvailableRangeError, expected, call
            )

        # A track passes its clip's error on, naming the clip.
        track = otio.schema.Track()
        track.append(clip)
        self.assertRaisesWith(
            otio.exceptions.CannotComputeAvailableRangeError,
            expected.replace(str(clip), str(track[0])),
            track.available_range,
        )

    def test_a_clip_with_no_image_bounds(self):
        clip = otio.schema.Clip()
        with self.assertRaises(ValueError) as caught:
            clip.available_image_bounds
        self.assertEqual(
            str(caught.exception),
            "cannot compute image bounds: No image bounds set on media "
            f"reference on clip: {clip}",
        )

    def test_what_a_base_class_leaves_to_its_subclasses(self):
        for call in (
            otio.schema.Gap().available_range,
            otio.core.Item().available_range,
            otio.core.Composition().range_of_all_children,
            lambda: otio.core.Composition().range_of_child_at_index(0),
            lambda: otio.schema.Gap().available_image_bounds,
        ):
            self.assertRaisesWith(
                NotImplementedError, "method not implemented for this class", call
            )

    def test_an_index_past_the_end(self):
        for call in (
            lambda: otio.schema.Track().range_of_child_at_index(5),
            lambda: otio.schema.Stack().trimmed_range_of_child_at_index(5),
            lambda: otio.schema.Track().__setitem__(3, otio.schema.Clip()),
            lambda: otio.schema.Track().__delitem__(3),
        ):
            self.assertRaisesWith(IndexError, "illegal index", call)

        # Reading one raises a bare IndexError, with no message at all.
        for call in (
            lambda: otio.schema.Track()[3],
            lambda: otio.schema.SerializableCollection()[3],
        ):
            self.assertRaisesWith(IndexError, "", call)

    def test_an_object_not_below_a_composition(self):
        track = otio.schema.Track(name="t")
        elsewhere = otio.schema.Track(name="t2")
        clip = otio.schema.Clip(name="c", source_range=span(0, 10))
        elsewhere.append(clip)
        for call in (track.range_of_child, track.trimmed_range_of_child):
            self.assertRaisesWith(
                otio.exceptions.NotAChildError,
                f"item is not a descendent of specified object: {track}",
                lambda: call(clip),
            )

        stranger = otio.schema.Clip(name="c")
        for call in (track.handles_of_child, track.neighbors_of):
            self.assertRaisesWith(
                otio.exceptions.NotAChildError,
                f"item is not a child of specified object: {track}",
                lambda: call(stranger),
            )

    def test_a_child_in_two_places(self):
        track = otio.schema.Track()
        clip = otio.schema.Clip()
        track.append(clip)
        self.assertRaisesWith(
            ValueError,
            "child already has a parent",
            lambda: otio.schema.Track().append(clip),
        )

        # Assigning it over another child refuses before removing anything.
        track.append(otio.schema.Clip(name="second"))
        with self.assertRaises(ValueError):
            track[1] = clip
        self.assertEqual([child.name for child in track], ["", "second"])

        # Putting a child back where it already is does nothing.
        track[0] = clip
        self.assertIs(track[0], clip)

    def test_a_trim_that_leaves_nothing(self):
        track = otio.schema.Track(source_range=span(100, 10))
        clip = otio.schema.Clip(source_range=span(0, 10))
        track.append(clip)
        for call in (
            lambda: track.trimmed_range_of_child_at_index(0),
            lambda: track.trimmed_range_of_child(clip),
            clip.trimmed_range_in_parent,
        ):
            self.assertRaisesWith(
                ValueError, "computed time range would be invalid", call
            )

    def test_media_reference_keys(self):
        def set_key():
            otio.schema.Clip().active_media_reference_key = "nope"

        self.assertRaisesWith(
            ValueError, "The media references do not contain the active key", set_key
        )
        self.assertRaisesWith(
            ValueError,
            "The media references do not contain the active key",
            lambda: otio.schema.Clip().set_media_references(
                {"a": otio.schema.MissingReference()}, "b"
            ),
        )
        self.assertRaisesWith(
            ValueError,
            "The media references contain an empty key",
            lambda: otio.schema.Clip().set_media_references(
                {"": otio.schema.MissingReference()}, ""
            ),
        )

    def test_colours(self):
        for text in ("zz", "#12345", ""):
            self.assertRaisesWith(
                ValueError,
                "Invalid hex format",
                lambda: otio.core.Color.from_hex(text),
            )
        # A component std::stoi cannot read.
        self.assertRaisesWith(ValueError, "stoi", lambda: otio.core.Color.from_hex("#zzz"))
        self.assertRaisesWith(
            ValueError,
            "List must have exactly 3 or 4 elements",
            lambda: otio.core.Color.from_float_list([1, 2]),
        )


class ReadingErrors(unittest.TestCase):
    def assertReadFails(self, text, message):
        with self.assertRaises(ValueError) as caught:
            otio.adapters.read_from_string(text, "otio_json")
        self.assertEqual(str(caught.exception), message)

    def test_json_syntax(self):
        self.assertReadFails(
            "{",
            "JSON parse error while reading: JSON parse error on input string: "
            "Missing a name for object member. (line 1, column 1)",
        )
        self.assertReadFails(
            '{\n  "a": 1\n  "b": 2\n}',
            "JSON parse error while reading: JSON parse error on input string: "
            "Missing a comma or '}' after an object member. (line 3, column 2)",
        )

    def test_a_field_of_the_wrong_type(self):
        self.assertReadFails(
            '{\n"OTIO_SCHEMA": "Clip.2",\n"name": "shot",\n"enabled": 3,\n'
            '"media_references": {}, "active_media_reference_key": ""\n}',
            "type mismatch while decoding: While reading object named 'shot' "
            "(of type 'N14opentimelineio5v0_194ClipE'): expected type b under "
            "key 'enabled': found type l instead (near line 6)",
        )

    def test_a_child_of_the_wrong_kind(self):
        self.assertReadFails(
            '{"OTIO_SCHEMA": "Track.1", "name": "V1", "kind": "", "children": [5]}',
            "type mismatch while decoding: While reading object named 'V1' "
            "(of type 'N14opentimelineio5v0_195TrackE'): expected to read a "
            "N14opentimelineio5v0_1910ComposableE, found a l instead "
            "(near line 1)",
        )

    def test_inside_a_value_type_only_the_line_is_given(self):
        self.assertReadFails(
            '{"OTIO_SCHEMA": "Clip.2", "source_range": {\n'
            '"OTIO_SCHEMA": "TimeRange.1", "start_time": 3}}',
            "type mismatch while decoding: near line 2",
        )

    def test_a_malformed_schema(self):
        self.assertReadFails(
            '{"OTIO_SCHEMA": "Track.1", "children": [\n{"OTIO_SCHEMA": "Gap"}\n]}',
            "Illegal/malformed schema: near line 2",
        )

    def test_an_unresolved_reference(self):
        self.assertReadFails(
            '{"OTIO_SCHEMA": "Clip.2", "media_references": {}, '
            '"active_media_reference_key": "", "metadata": {"x": '
            '{"OTIO_SCHEMA": "SerializableObjectRef.1", "id": "nope"}}}',
            "Unresolved object reference while reading: nope (near line 1)",
        )

    def test_an_unknown_missing_frame_policy(self):
        self.assertReadFails(
            '{"OTIO_SCHEMA": "ImageSequenceReference.1", "name": "", '
            '"target_url_base": "", "name_prefix": "", "name_suffix": "", '
            '"start_frame": 1, "frame_step": 1, "rate": 24.0, '
            '"frame_zero_padding": 0, "missing_frame_policy": "zzz"}',
            "JSON parse error while reading: While reading object named '' "
            "(of type 'N14opentimelineio5v0_1922ImageSequenceReferenceE'): "
            "Unknown missing_frame_policy: zzz (near line 1)",
        )


class VersioningErrors(unittest.TestCase):
    def assertRaisesWith(self, kind, message, call):
        with self.assertRaises(kind) as caught:
            call()
        self.assertEqual(str(caught.exception), message)

    def test_a_schema_version_newer_than_registered(self):
        self.assertRaisesWith(
            otio.exceptions.UnsupportedSchemaError,
            "unsupported schema version: Schema Clip has highest version 2, "
            "but the requested schema version 99 is even greater.",
            lambda: otio.core.instance_from_schema("Clip", 99, {}),
        )
        # Read from a file, upstream gives only the line.
        self.assertRaisesWith(
            otio.exceptions.UnsupportedSchemaError,
            "unsupported schema version: near line 3",
            lambda: otio.adapters.read_from_string(
                '{\n"OTIO_SCHEMA": "Clip.99"\n}', "otio_json"
            ),
        )

    def test_a_downgrade_with_no_function(self):
        self.assertRaisesWith(
            ValueError,
            'Internal error (aka "this is a bug"):No downgrader function '
            "available for going from version 1 to version 0.",
            lambda: otio.adapters.write_to_string(
                otio.schema.Clip(), "otio_json", target_schema_versions={"Clip": 0}
            ),
        )

    def test_an_object_that_holds_itself(self):
        clip = otio.schema.Clip()
        clip.metadata["self"] = clip
        message = (
            "Detected SerializableObject cycle while copying/serializing: "
            "cyclically encountered object has schema Clip"
        )
        self.assertRaisesWith(
            ValueError,
            message,
            lambda: otio.adapters.write_to_string(clip, "otio_json"),
        )
        self.assertRaisesWith(ValueError, message, clip.clone)


if __name__ == "__main__":
    unittest.main()
