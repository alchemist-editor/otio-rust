# SPDX-License-Identifier: Apache-2.0
# Copyright Contributors to the OpenTimelineIO project

"""The base classes every OTIO object is built from."""

from .. _otio import (  # noqa
    AnyDictionaryProxy,
    AnyVectorProxy,
    Color,
    Composable,
    Item,
    SerializableObject,
    SerializableObjectWithMetadata,
    deserialize_json_from_string,
    serialize_json_to_string,
)

from . _core_utils import (  # noqa
    _add_mutable_mapping_methods,
    _add_mutable_sequence_methods,
)

_add_mutable_mapping_methods(AnyDictionaryProxy)
_add_mutable_sequence_methods(AnyVectorProxy)

__all__ = [
    'Color',
    'Composable',
    'Item',
    'SerializableObject',
    'SerializableObjectWithMetadata',
    'deserialize_json_from_string',
    'serialize_json_to_string',
]
