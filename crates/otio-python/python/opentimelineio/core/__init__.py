# SPDX-License-Identifier: Apache-2.0
# Copyright Contributors to the OpenTimelineIO project

"""The base classes every OTIO object is built from."""

from .. _otio import (  # noqa
    AnyDictionaryProxy,
    AnyVectorProxy,
    Color,
    Composable,
    Composition,
    Item,
    MediaReference,
    SerializableObject,
    SerializableObjectWithMetadata,
    Track,
    deserialize_json_from_string,
    serialize_json_to_string,
)

from . _core_utils import (  # noqa
    _add_mutable_mapping_methods,
    _add_mutable_sequence_methods,
)

_add_mutable_mapping_methods(AnyDictionaryProxy)
_add_mutable_sequence_methods(AnyVectorProxy)
# Putting a child into a composition sets that child's parent, so a slice
# assignment that fails part way cannot be undone by writing the old children
# back one at a time; the whole composition is rebuilt instead.
_add_mutable_sequence_methods(Composition, side_effecting_insertions=True)

__all__ = [
    'Color',
    'Composable',
    'Composition',
    'Item',
    'MediaReference',
    'SerializableObject',
    'SerializableObjectWithMetadata',
    'Track',
    'deserialize_json_from_string',
    'serialize_json_to_string',
]
