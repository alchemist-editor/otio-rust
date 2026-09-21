# SPDX-License-Identifier: Apache-2.0
# Copyright Contributors to the OpenTimelineIO project

"""Python-side finishing touches on the classes the extension exports."""

import collections.abc


def _add_mutable_mapping_methods(cls):
    """Gives ``cls`` the rest of the ``MutableMapping`` interface.

    The extension writes the six methods a mapping cannot be built without
    (``__getitem__``, ``__setitem__``, ``__delitem__``, ``__iter__``,
    ``__len__`` and ``__contains__``); everything else a caller expects of a
    dictionary -- ``get``, ``keys``, ``items``, ``values``, ``update``,
    ``pop``, ``setdefault`` and the rest -- is written once in the standard
    library in terms of those, so it is borrowed rather than rewritten.

    Upstream does the same thing to its pybind11 ``AnyDictionary`` for the
    same reason; both are types that cannot inherit from a Python class.
    """
    for name in (
        'get',
        'keys',
        'items',
        'values',
        'pop',
        'popitem',
        'clear',
        'update',
        'setdefault',
    ):
        setattr(cls, name, getattr(collections.abc.MutableMapping, name))
    collections.abc.MutableMapping.register(cls)
    return cls
