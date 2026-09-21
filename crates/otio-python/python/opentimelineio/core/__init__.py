# SPDX-License-Identifier: Apache-2.0
# Copyright Contributors to the OpenTimelineIO project

"""The base classes every OTIO object is built from."""

from .. _otio import (  # noqa
    AnyDictionaryProxy,
    Color,
    Composable,
    SerializableObject,
    SerializableObjectWithMetadata,
    deserialize_json_from_string,
    serialize_json_to_string,
)

from . _core_utils import _add_mutable_mapping_methods  # noqa

_add_mutable_mapping_methods(AnyDictionaryProxy)

__all__ = [
    'Color',
    'Composable',
    'SerializableObject',
    'SerializableObjectWithMetadata',
    'deserialize_json_from_string',
    'serialize_json_to_string',
]
