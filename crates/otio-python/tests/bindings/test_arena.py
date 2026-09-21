# SPDX-License-Identifier: Apache-2.0
# Copyright Contributors to the OpenTimelineIO project

"""What happens when one object is put inside another.

Upstream's C++ objects are reference counted, so appending one to another is a
pointer copy. Here every object starts in a document of its own and has to be
moved. That move is invisible from Python, and these tests are what says so.
"""

import unittest

import opentimelineio as otio


class MovingBetweenDocuments(unittest.TestCase):
    def test_an_appended_marker_keeps_its_identity(self):
        item = otio.core.Item(name="shot")
        marker = otio.schema.Marker(name="note")

        item.markers.append(marker)

        self.assertIs(item.markers[0], marker)
        self.assertEqual(item.markers[0].name, "note")

    def test_the_original_handle_still_works_after_the_move(self):
        # The object the caller is holding was built in its own document and
        # now lives in the item's. Reading and writing through the name they
        # already have has to go to its new home, not the old one.
        item = otio.core.Item(name="shot")
        marker = otio.schema.Marker(name="note")
        item.markers.append(marker)

        marker.name = "renamed"
        marker.metadata["k"] = "v"

        self.assertEqual(item.markers[0].name, "renamed")
        self.assertEqual(item.markers[0].metadata["k"], "v")

    def test_everything_under_the_moved_object_moves_too(self):
        item = otio.core.Item(name="shot")
        inner = otio.schema.Marker(name="inner")
        outer = otio.schema.Marker(name="outer")
        outer.metadata["held"] = inner

        item.markers.append(outer)

        self.assertIs(item.markers[0].metadata["held"], inner)
        self.assertEqual(inner.name, "inner")

    def test_the_lists_write_through(self):
        item = otio.core.Item(name="shot")
        item.effects.append(otio.schema.Effect(effect_name="blur"))
        item.effects.append(otio.schema.Effect(effect_name="flop"))

        self.assertEqual(len(item.effects), 2)
        self.assertEqual(item.effects[-1].effect_name, "flop")

        del item.effects[0]
        self.assertEqual([e.effect_name for e in item.effects], ["flop"])

        item.effects.insert(0, otio.schema.Effect(effect_name="first"))
        self.assertEqual([e.effect_name for e in item.effects], ["first", "flop"])

    def test_a_list_can_be_serialized_on_its_own(self):
        item = otio.core.Item(name="shot")
        item.markers.append(otio.schema.Marker(name="note"))

        encoded = otio.adapters.otio_json.write_to_string(item.markers)
        self.assertIn('"Marker.3"', encoded)
        self.assertIn('"note"', encoded)

    def test_a_plain_value_can_be_serialized_on_its_own(self):
        # Upstream's writer takes anything its metadata can hold, and its own
        # tests compare a bare boolean this way.
        self.assertEqual(
            otio.adapters.otio_json.write_to_string(True).strip(), "true"
        )


class MetadataHoldingObjects(unittest.TestCase):
    def test_an_object_in_metadata_round_trips(self):
        item = otio.core.Item(name="shot")
        item.metadata["marker"] = otio.schema.Marker(name="note")

        encoded = otio.adapters.otio_json.write_to_string(item)
        decoded = otio.adapters.otio_json.read_from_string(encoded)

        self.assertTrue(decoded.is_equivalent_to(item))
        self.assertEqual(decoded.metadata["marker"].name, "note")


if __name__ == "__main__":
    unittest.main()
