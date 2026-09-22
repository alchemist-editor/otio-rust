# SPDX-License-Identifier: Apache-2.0
# Copyright Contributors to the OpenTimelineIO project

"""Classes registered from Python with `register_type`.

Upstream keeps such an object as a C++ object that points back at its Python
instance. Here it is a plain node holding a schema name, a version and a field
map, and the Python class is found again by schema name whenever the node is
read. These tests pin the parts of that which upstream's own tests don't reach:
constructors that take arguments, objects nested in each other's fields, and
version functions that fail in Python.
"""

import unittest

import opentimelineio as otio
from opentimelineio.core import (
    register_type,
    serializable_field,
    upgrade_function_for,
)


@register_type
class Labelled(otio.core.SerializableObjectWithMetadata):
    _serializable_label = "BindingsLabelled.1"

    def __init__(self, name="", metadata=None, weight=3):
        super().__init__(name, metadata)
        self.weight = weight

    weight = serializable_field("weight", int)


@register_type
class Holder(otio.core.SerializableObject):
    _serializable_label = "BindingsHolder.1"

    def __init__(self):
        otio.core.SerializableObject.__init__(self)
        self.items = []

    items = serializable_field("items", list)


@register_type
class Failing(otio.core.SerializableObject):
    _serializable_label = "BindingsFailing.2"


@upgrade_function_for(Failing, 2)
def _fail(data):
    raise KeyError("raised in python")


def round_trip(obj):
    return otio.adapters.read_from_string(
        otio.adapters.write_to_string(obj, "otio_json"), "otio_json"
    )


class ConstructorArguments(unittest.TestCase):
    def test_a_subclass_passes_its_own_arguments_to_its_init(self):
        labelled = Labelled("n", {"a": 1}, weight=5)
        self.assertEqual(labelled.name, "n")
        self.assertEqual(dict(labelled.metadata), {"a": 1})
        self.assertEqual(labelled.weight, 5)

    def test_the_base_class_itself_still_takes_no_arguments(self):
        with self.assertRaises(TypeError):
            otio.core.SerializableObject(1)

    def test_a_read_object_keeps_the_values_written(self):
        read = round_trip(Labelled("n", {"a": 1}, weight=5))
        self.assertIsInstance(read, Labelled)
        self.assertFalse(read.is_unknown_schema)
        self.assertEqual(
            (read.name, dict(read.metadata), read.weight), ("n", {"a": 1}, 5)
        )


class NestedRegisteredObjects(unittest.TestCase):
    def test_objects_in_a_list_field_come_back_as_their_class(self):
        holder = Holder()
        holder.items = holder.items + [Labelled("inner", weight=7)]
        read = round_trip(holder)
        self.assertIsInstance(read, Holder)
        self.assertIsInstance(read.items[0], Labelled)
        self.assertEqual((read.items[0].name, read.items[0].weight), ("inner", 7))


class VersionFunctions(unittest.TestCase):
    def test_an_exception_raised_in_python_reaches_the_reader(self):
        with self.assertRaises(KeyError):
            otio.core.deserialize_json_from_string(
                '{"OTIO_SCHEMA": "BindingsFailing.1"}'
            )


if __name__ == "__main__":
    unittest.main()
