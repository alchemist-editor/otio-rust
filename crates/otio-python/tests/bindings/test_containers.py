# SPDX-License-Identifier: Apache-2.0
# Copyright Contributors to the OpenTimelineIO project

"""Where the containers now behave as upstream's do.

Three things upstream's reference counting and pybind11 bindings give it for
free, which here have to be done on purpose: an object taken out of metadata
is freed once nothing holds it; an item's markers and effects are lists of
their own classes; and a list or dict read from JSON at the top level is a
free-standing `AnyVector` or `AnyDictionary`.
"""

import copy
import gc
import unittest
import weakref

import opentimelineio as otio
from opentimelineio import _otio

document_size = _otio._testing._document_size


class ObjectsLetGoOfByMetadataAreFreed(unittest.TestCase):
    def test_deleting_an_entry_frees_its_object(self):
        item = otio.core.Item()
        item.metadata["marker"] = otio.schema.Marker(name="note")
        held = weakref.ref(item.metadata["marker"])
        before = document_size(item)
        gc.collect()
        # While the entry holds it, the object keeps its wrapper.
        self.assertIsNotNone(held())

        del item.metadata["marker"]
        gc.collect()

        self.assertIsNone(held())
        self.assertEqual(document_size(item), before - 1)

    def test_replacing_an_entry_frees_what_it_held(self):
        item = otio.core.Item()
        item.metadata["marker"] = otio.schema.Marker(name="old")
        held = weakref.ref(item.metadata["marker"])
        before = document_size(item)

        item.metadata["marker"] = "a string now"
        gc.collect()

        self.assertIsNone(held())
        self.assertEqual(document_size(item), before - 1)

    def test_everything_the_entry_held_goes_with_it(self):
        item = otio.core.Item()
        outer = otio.schema.Marker(name="outer")
        outer.metadata["inner"] = otio.schema.Marker(name="inner")
        item.metadata["nested"] = {"deeper": [outer]}
        held = weakref.ref(item.metadata["nested"]["deeper"][0].metadata["inner"])
        del outer
        before = document_size(item)

        item.metadata.clear()
        gc.collect()

        self.assertIsNone(held())
        self.assertEqual(document_size(item), before - 2)

    def test_an_any_vector_item_is_freed_when_deleted_or_replaced(self):
        item = otio.core.Item()
        item.metadata["list"] = [
            otio.schema.Marker(name="a"),
            otio.schema.Marker(name="b"),
        ]
        vector = item.metadata["list"]
        a = weakref.ref(vector[0])
        b = weakref.ref(vector[1])
        before = document_size(item)

        del vector[0]
        gc.collect()
        self.assertIsNone(a())
        self.assertEqual(document_size(item), before - 1)

        vector[0] = 7
        gc.collect()
        self.assertIsNone(b())
        self.assertEqual(document_size(item), before - 2)

    def test_replacing_the_whole_metadata_frees_what_it_held(self):
        item = otio.core.Item()
        item.metadata["marker"] = otio.schema.Marker(name="old")
        item.metadata["kept"] = otio.schema.Marker(name="kept")
        old = weakref.ref(item.metadata["marker"])
        kept = item.metadata["kept"]
        before = document_size(item)

        item.metadata = {"kept": kept}
        gc.collect()

        self.assertIsNone(old())
        self.assertIs(item.metadata["kept"], kept)
        self.assertEqual(document_size(item), before - 1)

    def test_an_object_held_twice_lives_until_the_last_holder_lets_go(self):
        item = otio.core.Item()
        marker = otio.schema.Marker(name="shared")
        item.metadata["a"] = marker
        item.metadata["b"] = [marker]
        held = weakref.ref(marker)
        del marker

        del item.metadata["a"]
        gc.collect()
        self.assertIsNotNone(held())
        self.assertEqual(item.metadata["b"][0].name, "shared")

        del item.metadata["b"]
        gc.collect()
        self.assertIsNone(held())

    def test_an_object_python_holds_outlives_its_entry(self):
        item = otio.core.Item()
        marker = otio.schema.Marker(name="kept")
        item.metadata["marker"] = marker
        before = document_size(item)

        del item.metadata["marker"]
        gc.collect()

        self.assertEqual(marker.name, "kept")
        self.assertEqual(document_size(item), before)
        # And can be put somewhere else.
        item.markers.append(marker)
        self.assertIs(item.markers[0], marker)

        # Once Python lets go as well, it is freed.
        item.markers.pop()
        held = weakref.ref(marker)
        del marker
        gc.collect()
        self.assertIsNone(held())
        self.assertEqual(document_size(item), before - 1)

    def test_a_clip_a_track_also_holds_stays_in_the_track(self):
        # The freed object's metadata held a clip that sits in a track as
        # well: the clip is the track's still.
        track = otio.schema.Track()
        track.append(otio.schema.Clip(name="clip"))
        holder = otio.schema.Marker(name="holder")
        holder.metadata["clip"] = track[0]
        item = otio.core.Item()
        item.metadata["holder"] = holder
        del holder
        gc.collect()

        del item.metadata["holder"]
        gc.collect()

        self.assertEqual(track[0].name, "clip")
        self.assertIs(track[0].parent(), track)

    def test_an_object_a_dynamic_field_also_holds_is_kept(self):
        holder = otio.core.SerializableObjectWithMetadata()
        marker = otio.schema.Marker(name="in a field")
        holder._dynamic_fields["field"] = marker
        holder.metadata["entry"] = marker
        held = weakref.ref(marker)
        del marker

        del holder.metadata["entry"]
        gc.collect()

        self.assertIsNotNone(held())
        self.assertEqual(holder._dynamic_fields["field"].name, "in a field")

    def test_a_free_standing_container_frees_what_it_held(self):
        vector = otio.core.AnyVector()
        vector.append(otio.schema.Marker(name="in the vector"))
        held = weakref.ref(vector[0])
        gc.collect()
        self.assertIsNotNone(held())

        del vector
        gc.collect()

        self.assertIsNone(held())

    def test_a_free_standing_container_leaves_shared_objects_alone(self):
        track = otio.schema.Track()
        track.append(otio.schema.Clip(name="clip"))
        vector = otio.core.AnyVector()
        vector.append(track[0])
        before = document_size(track)

        del vector
        gc.collect()

        self.assertEqual(track[0].name, "clip")
        # The vector's hidden holder is gone; the clip is not.
        self.assertEqual(document_size(track), before - 1)

    def test_a_clip_taken_out_of_a_track_stays_in_the_list_that_holds_it(self):
        read = otio.core.deserialize_json_from_string(
            '[{"OTIO_SCHEMA": "Clip.2", "name": "c"}]'
        )
        track = otio.schema.Track()
        track.append(read[0])
        gc.collect()

        del track[0]
        gc.collect()

        self.assertEqual(read[0].name, "c")
        self.assertIsNone(read[0].parent())

    def test_dropping_a_track_leaves_a_clip_metadata_also_holds(self):
        track = otio.schema.Track()
        track.append(otio.schema.Clip(name="clip"))
        item = otio.core.Item()
        item.metadata["clip"] = track[0]
        gc.collect()

        del track
        gc.collect()

        self.assertEqual(item.metadata["clip"].name, "clip")
        self.assertIsNone(item.metadata["clip"].parent())


class MarkerAndEffectVectors(unittest.TestCase):
    def test_each_list_has_upstreams_class(self):
        item = otio.core.Item()
        self.assertIs(type(item.markers), _otio.MarkerVector)
        self.assertIs(type(item.effects), _otio.EffectVector)
        self.assertEqual(_otio.MarkerVector.__module__, "opentimelineio._otio")
        self.assertEqual(_otio.EffectVector.__module__, "opentimelineio._otio")
        self.assertIsNot(_otio.MarkerVector, _otio.EffectVector)
        self.assertEqual(
            type(iter(item.markers)).__name__, "MarkerVectorIterator"
        )
        self.assertEqual(
            type(iter(item.effects)).__name__, "EffectVectorIterator"
        )

    def test_each_list_takes_only_its_own_kind(self):
        item = otio.core.Item()
        with self.assertRaises(TypeError):
            item.markers.append(otio.schema.Effect())
        with self.assertRaises(TypeError):
            item.effects.append(otio.schema.Marker())
        with self.assertRaises(TypeError):
            item.markers.append(None)
        with self.assertRaises(TypeError):
            otio.core.Item(markers=[otio.schema.Effect()])
        # A kind of effect is an effect.
        item.effects.append(otio.schema.LinearTimeWarp())
        self.assertEqual(len(item.effects), 1)
        self.assertEqual(len(item.markers), 0)

    def test_a_list_can_be_built_on_its_own(self):
        markers = _otio.MarkerVector()
        markers.append(otio.schema.Marker(name="a"))
        markers.insert(0, otio.schema.Marker(name="b"))
        self.assertEqual([m.name for m in markers], ["b", "a"])

        effects = _otio.EffectVector()
        effects.extend([otio.schema.Effect(effect_name="blur")])
        self.assertEqual(effects[0].effect_name, "blur")

    def test_a_copy_holds_the_same_objects(self):
        item = otio.core.Item(markers=[otio.schema.Marker(name="a")])
        copied = copy.copy(item.markers)
        self.assertIs(type(copied), _otio.MarkerVector)
        self.assertIs(copied[0], item.markers[0])

        deep = copy.deepcopy(item.markers)
        self.assertIsNot(deep[0], item.markers[0])
        self.assertEqual(deep[0].name, "a")

    def test_a_marker_held_by_another_list_survives_removal(self):
        item = otio.core.Item(markers=[otio.schema.Marker(name="a")])
        copied = copy.copy(item.markers)
        held = weakref.ref(copied[0])

        del item.markers[0]
        gc.collect()

        self.assertIsNotNone(held())
        self.assertEqual(copied[0].name, "a")

    def test_a_removed_marker_nothing_holds_is_freed(self):
        item = otio.core.Item(markers=[otio.schema.Marker(name="a")])
        held = weakref.ref(item.markers[0])
        before = document_size(item)

        item.markers.pop()
        gc.collect()

        self.assertIsNone(held())
        self.assertEqual(document_size(item), before - 1)

    def test_the_list_keeps_its_item_alive(self):
        markers = otio.core.Item(
            markers=[otio.schema.Marker(name="a")]
        ).markers
        gc.collect()
        self.assertEqual(markers[0].name, "a")

    def test_indices_follow_upstream(self):
        item = otio.core.Item()
        for name in "abc":
            item.markers.append(otio.schema.Marker(name=name))
        # Upstream's insert appends for an index past either end, and its
        # delete takes the last for one past either end.
        item.markers.insert(-10, otio.schema.Marker(name="d"))
        self.assertEqual([m.name for m in item.markers], list("abcd"))
        item.markers.__internal_delitem__(10)
        self.assertEqual([m.name for m in item.markers], list("abc"))
        with self.assertRaises(IndexError):
            item.markers[3]
        self.assertEqual(item.markers[-1].name, "c")

    def test_a_list_compares_equal_only_to_itself(self):
        # Upstream's classes have no `__eq__`.
        item = otio.core.Item(markers=[otio.schema.Marker()])
        markers = item.markers
        self.assertEqual(markers, markers)
        self.assertNotEqual(markers, list(markers))


class TopLevelValuesAreFreeStandingContainers(unittest.TestCase):
    def test_a_list_reads_as_an_any_vector(self):
        read = otio.core.deserialize_json_from_string('[1, {"a": [2]}]')
        self.assertIs(type(read), otio.core.AnyVector)
        self.assertIs(type(read[1]), otio.core.AnyDictionary)
        self.assertIs(type(read[1]["a"]), otio.core.AnyVector)
        self.assertEqual(read[0], 1)

    def test_a_dict_reads_as_an_any_dictionary(self):
        read = otio.core.deserialize_json_from_string('{"a": 1, "b": "c"}')
        self.assertIs(type(read), otio.core.AnyDictionary)
        self.assertEqual(read, {"a": 1, "b": "c"})

    def test_anything_else_reads_as_itself(self):
        self.assertEqual(otio.core.deserialize_json_from_string("3"), 3)
        read = otio.core.deserialize_json_from_string(
            '{"OTIO_SCHEMA": "Marker.2", "name": "m"}'
        )
        self.assertIsInstance(read, otio.schema.Marker)

    def test_the_adapter_reads_the_same(self):
        read = otio.adapters.read_from_string(
            '[{"OTIO_SCHEMA": "Clip.2", "name": "c"}]', "otio_json"
        )
        self.assertIs(type(read), otio.core.AnyVector)
        self.assertEqual(read[0].name, "c")
        self.assertIs(read[0], read[0])

    def test_the_file_reader_reads_the_same(self):
        import os
        import tempfile

        with tempfile.TemporaryDirectory() as directory:
            path = os.path.join(directory, "list.otio")
            with open(path, "w") as handle:
                handle.write('{"k": [1, 2]}')
            read = otio.core.deserialize_json_from_file(path)
        self.assertIs(type(read), otio.core.AnyDictionary)
        self.assertEqual(list(read["k"]), [1, 2])

    def test_the_container_writes_through_and_round_trips(self):
        read = otio.core.deserialize_json_from_string(
            '[{"OTIO_SCHEMA": "Marker.2", "name": "m"}]'
        )
        read.append("more")
        read[0].name = "renamed"
        again = otio.core.deserialize_json_from_string(
            otio.core.serialize_json_to_string(read)
        )
        self.assertEqual(again[0].name, "renamed")
        self.assertEqual(again[1], "more")

    def test_an_object_taken_out_outlives_the_container(self):
        read = otio.core.deserialize_json_from_string(
            '[{"OTIO_SCHEMA": "Clip.2", "name": "c"}]'
        )
        track = otio.schema.Track()
        track.append(read[0])
        del read
        gc.collect()

        self.assertEqual(track[0].name, "c")
        self.assertIs(track[0].parent(), track)

    def test_an_unknown_schemas_data_is_a_free_standing_copy(self):
        unknown = otio.core.deserialize_json_from_string(
            '{"OTIO_SCHEMA": "Mystery.1", "n": 1,'
            ' "m": {"OTIO_SCHEMA": "Marker.2", "name": "inner"}}'
        )
        data = unknown.data
        self.assertIs(type(data), otio.core.AnyDictionary)
        self.assertIs(data["m"], unknown.data["m"])
        data["n"] = 2
        self.assertEqual(unknown.data["n"], 1)
        del data
        gc.collect()
        self.assertEqual(unknown.data["m"].name, "inner")


if __name__ == "__main__":
    unittest.main()
