# SPDX-License-Identifier: Apache-2.0
# Copyright Contributors to the OpenTimelineIO project

"""A stand-in for the core type registry, until the real one is bound.

Upstream's plugin system is built out of Python-defined schemas: a plugin
manifest is a ``PluginManifest.1`` object, each adapter in it an
``Adapter.1``, and both are classes registered with ``core.register_type``
whose fields are declared with ``core.serializable_field``. Reading a manifest
is an ordinary ``core.deserialize_json_from_file``, and writing one an
ordinary ``core.serialize_json_to_file``, because the core's registry knows
the classes.

That registry belongs in the Rust core and its bindings, and is being built
there. Until it is, ``install()`` puts this pure-Python stand-in in its place,
so the plugin system can be written exactly as upstream writes it. The
stand-in is deliberately small and does nothing at all once ``core`` has a
``register_type`` of its own, so the day the real registry lands this module
can be deleted along with the one line in ``plugins/__init__.py`` that calls
it.

What it covers: registering a class, its serializable fields, building one
from its schema name, and reading and writing JSON that contains such objects
anywhere in it. JSON that contains none is handed to the Rust reader and
writer untouched, so everything else reads and writes exactly as before. What
it does not: upgrading or downgrading a schema, and a Python-defined object
held inside a Rust one (in a clip's metadata, say), which reads back as an
``UnknownSchema``.
"""

import collections.abc
import json

from .. import _otio

# schema name -> (class, schema version)
_REGISTRY = {}


def install():
    """Put the stand-in into ``core``, unless it has a registry already."""

    from .. import core

    if hasattr(core, "register_type"):
        return

    core.register_type = register_type
    core.serializable_field = serializable_field
    core.deprecated_field = deprecated_field
    core.instance_from_schema = instance_from_schema
    core.deserialize_json_from_string = deserialize_json_from_string
    core.deserialize_json_from_file = deserialize_json_from_file
    core.serialize_json_to_string = serialize_json_to_string
    core.serialize_json_to_file = serialize_json_to_file

    # Upstream's base class carries the field store every serializable_field
    # reads, so a Python subclass that is never registered itself (the
    # plugins' ``PythonPlugin``) can still hold fields.
    if not hasattr(_otio.SerializableObject, "_dynamic_fields"):
        _otio.SerializableObject._dynamic_fields = property(_dynamic_fields)


def _dynamic_fields(self):
    fields = self.__dict__.get("_interim_dynamic_fields")
    if fields is None:
        fields = self.__dict__["_interim_dynamic_fields"] = {}
    return fields


def _new_ignoring_arguments(cls, *args, **kwargs):
    # The extension's ``__new__`` takes no arguments; the class's own
    # ``__init__`` reads them.
    return _otio.SerializableObject.__new__(cls)


def register_type(classobj, schemaname=None):
    """Register a SerializableObject subclass under its schema name."""

    label = classobj._serializable_label
    if schemaname is None:
        schema_name, schema_version = label.split(".", 2)
    else:
        schema_name, schema_version = schemaname, 1

    _REGISTRY[schema_name] = (classobj, int(schema_version))

    if "_dynamic_fields" not in dir(classobj):
        classobj._dynamic_fields = property(_dynamic_fields)
    classobj.__new__ = _new_ignoring_arguments
    return classobj


def serializable_field(name, required_type=None, doc=None, default_value=None):
    """A property stored in the object's serialized fields; as upstream's."""

    def getter(self):
        return self._dynamic_fields.get(name, default_value)

    def setter(self, val):
        # always allow None values regardless of value of required_type
        if required_type is not None and val is not None:
            if not isinstance(val, required_type):
                raise TypeError(
                    "attribute '{}' must be an instance of '{}', not: {}".format(
                        name,
                        required_type,
                        type(val)
                    )
                )

        self._dynamic_fields[name] = val

    return property(getter, setter, doc=doc)


def deprecated_field():
    """For marking attributes on a SerializableObject deprecated."""

    def getter(self):
        raise DeprecationWarning

    def setter(self, val):
        raise DeprecationWarning

    return property(getter, setter, doc="Deprecated field, do not use.")


def _registered_label(obj):
    for klass in type(obj).__mro__:
        entry = _REGISTRY.get(
            str(getattr(klass, "_serializable_label", "")).split(".")[0]
        )
        if entry is not None and entry[0] is klass:
            return "{}.{}".format(klass._serializable_label.split(".")[0],
                                  entry[1])
    return None


def _python_fields(obj):
    """The fields a Python subclass holds, whether it is registered or not."""
    return getattr(obj, "__dict__", {}).get("_interim_dynamic_fields") or {}


def _holds_registered(value):
    if isinstance(value, _otio.SerializableObject):
        return (
            _registered_label(value) is not None
            or bool(_python_fields(value))
        )
    if isinstance(value, (str, bytes)):
        return False
    if isinstance(value, collections.abc.Mapping):
        return any(_holds_registered(v) for v in value.values())
    if isinstance(value, collections.abc.Sequence):
        return any(_holds_registered(v) for v in value)
    return False


def _to_plain(value):
    if isinstance(value, _otio.SerializableObject):
        label = _registered_label(value)
        if label is None:
            # An unregistered Python subclass writes as the schema it
            # derives from, with its own fields after the inherited ones, as
            # upstream's writer does.
            result = json.loads(_otio.serialize_json_to_string(value))
            fields = _python_fields(value)
            for key in sorted(fields):
                result[key] = _to_plain(fields[key])
            return result
        result = {"OTIO_SCHEMA": label}
        fields = value._dynamic_fields
        for key in sorted(fields):
            result[key] = _to_plain(fields[key])
        return result
    if value is None or isinstance(value, (bool, int, float, str)):
        return value
    if isinstance(value, collections.abc.Mapping):
        return {str(k): _to_plain(v) for k, v in value.items()}
    if isinstance(value, collections.abc.Sequence):
        return [_to_plain(v) for v in value]
    return json.loads(_otio.serialize_json_to_string(value))


def serialize_json_to_string(root, schema_version_targets=None, indent=4):
    """Serialize root to a json string."""

    if schema_version_targets:
        raise NotImplementedError(
            "writing an older schema version is not supported yet"
        )
    if not _holds_registered(root):
        return _otio.serialize_json_to_string(root, max(indent, 0))
    if indent < 0:
        return json.dumps(_to_plain(root), separators=(",", ":"))
    return json.dumps(_to_plain(root), indent=indent)


def serialize_json_to_file(root, filename, schema_version_targets=None, indent=4):
    """Serialize root to a json file."""

    text = serialize_json_to_string(root, schema_version_targets, indent)
    with open(filename, "w", encoding="utf-8") as fo:
        fo.write(text)
    return True


def _mentions_registered(data):
    if isinstance(data, dict):
        schema = data.get("OTIO_SCHEMA")
        if isinstance(schema, str) and schema.split(".")[0] in _REGISTRY:
            return True
        return any(_mentions_registered(v) for v in data.values())
    if isinstance(data, list):
        return any(_mentions_registered(v) for v in data)
    return False


def _from_plain(data):
    if isinstance(data, dict):
        schema = data.get("OTIO_SCHEMA")
        if isinstance(schema, str):
            entry = _REGISTRY.get(schema.split(".")[0])
            if entry is None:
                return _otio.deserialize_json_from_string(json.dumps(data))
            obj = entry[0]()
            for key, value in data.items():
                if key != "OTIO_SCHEMA":
                    obj._dynamic_fields[key] = _from_plain(value)
            return obj
        return {key: _from_plain(value) for key, value in data.items()}
    if isinstance(data, list):
        return [_from_plain(value) for value in data]
    return data


def deserialize_json_from_string(input_str):
    """Deserialize a json string into objects."""

    # Most documents hold no Python-defined schema at all; those go straight
    # to the Rust reader without being parsed twice.
    if not any(f'"{name}.' in input_str for name in _REGISTRY):
        return _otio.deserialize_json_from_string(input_str)
    data = json.loads(input_str)
    if not _mentions_registered(data):
        return _otio.deserialize_json_from_string(input_str)
    return _from_plain(data)


def deserialize_json_from_file(filename):
    """Deserialize a json file into objects."""

    with open(filename, encoding="utf-8") as fi:
        return deserialize_json_from_string(fi.read())


def instance_from_schema(schema_name, schema_version, data):
    """Build an object of a registered schema from a dictionary of fields."""

    record = dict(data)
    record["OTIO_SCHEMA"] = f"{schema_name}.{schema_version}"
    return _from_plain(record)
