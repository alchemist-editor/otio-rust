# SPDX-License-Identifier: Apache-2.0
# Copyright Contributors to the OpenTimelineIO project

"""Python-side finishing touches on the classes the extension exports."""

import collections.abc
import copy
import types

from .. _otio import (  # noqa
    SerializableObject,
    AnyDictionary,
    AnyVector,
)


def _is_str(v):
    return isinstance(v, str)


def _is_nonstring_sequence(v):
    return isinstance(v, collections.abc.Sequence) and not _is_str(v)


_marker_ = object()


def _add_mutable_mapping_methods(mapClass):
    """Gives ``mapClass`` the rest of the ``MutableMapping`` interface.

    This is upstream's own function. The extension writes the methods a
    mapping cannot be built without (``__getitem__``, ``__setitem__``,
    ``__delitem__``, ``__iter__`` and ``__len__``); everything else a caller
    expects of a dictionary -- ``get``, ``keys``, ``items``, ``values``,
    ``update``, ``__eq__`` and the rest -- is written once in the standard
    library in terms of those, so it is borrowed rather than rewritten.

    One difference: upstream's ``__setitem__`` is written here, to convert the
    value in Python before handing it on. The extension converts values
    itself, so its own ``__setitem__`` is kept.
    """
    def __str__(self):
        return str(dict(self))

    def __repr__(self):
        return repr(dict(self))

    def setdefault(self, key, default_value):
        if key in self:
            return self[key]
        else:
            self[key] = default_value
            return self[key]

    def pop(self, key, default=_marker_):
        try:
            value = self[key]
        except KeyError:
            if default is _marker_:
                raise
            return default
        else:
            del self[key]
            return value

    def __copy__(self):
        m = mapClass()
        m.update({k: v for (k, v) in self.items()})
        return m

    def __deepcopy__(self, memo):
        m = mapClass()
        m.update({k: copy.deepcopy(v, memo) for (k, v) in self.items()})
        return m

    collections.abc.MutableMapping.register(mapClass)
    mapClass.__str__ = __str__
    mapClass.__repr__ = __repr__

    seen = set()
    for klass in (collections.abc.MutableMapping, collections.abc.Mapping):
        for name in klass.__dict__.keys():
            if name in seen:
                continue

            seen.add(name)
            func = getattr(klass, name)
            if (
                    isinstance(func, types.FunctionType)
                    and name not in klass.__abstractmethods__
            ):
                setattr(mapClass, name, func)

    mapClass.setdefault = setdefault
    mapClass.pop = pop
    mapClass.__copy__ = __copy__
    mapClass.__deepcopy__ = __deepcopy__
    return mapClass


def _add_mutable_sequence_methods(sequenceClass, side_effecting_insertions=False):
    """Gives ``sequenceClass`` the rest of the ``MutableSequence`` interface.

    This is upstream's own function, kept close to the original because the
    slicing rules below are subtle and its tests exercise them. The extension
    writes ``__len__``, ``__iter__`` and the four ``__internal_*`` methods that
    take a single index; everything a caller expects of a list is built here in
    terms of those.

    ``side_effecting_insertions`` is for a sequence where putting an object in
    changes the object -- a composition sets each child's parent -- so a failed
    slice assignment cannot be undone by writing the old values back one at a
    time, and the whole sequence is rebuilt instead.
    """

    def __add__(self, other):
        if isinstance(other, list):
            return list(self) + other
        elif _is_nonstring_sequence(other):
            return list(self) + list(other)
        else:
            raise TypeError(
                f"Cannot add types '{type(self)}' and '{type(other)}'"
            )

    def __radd__(self, other):
        return self.__add__(other)

    def __str__(self):
        return str(list(self))

    def __repr__(self):
        return repr(list(self))

    def __getitem__(self, index):
        if isinstance(index, slice):
            indices = index.indices(len(self))
            return [self.__internal_getitem__(i) for i in range(*indices)]
        else:
            return self.__internal_getitem__(index)

    # This has to handle slicing
    def __setitem__(self, index, item):
        if not isinstance(index, slice):
            self.__internal_setitem__(index, item)
            return

        if not isinstance(item, collections.abc.Iterable):
            raise TypeError("can only assign an iterable")

        indices = range(*index.indices(len(self)))

        if index.step in (1, None):
            if (
                    not side_effecting_insertions
                    and isinstance(item, collections.abc.MutableSequence)
                    and len(item) == len(indices)
            ):
                for i0, i in enumerate(indices):
                    self.__internal_setitem__(i, item[i0])
                return

            if side_effecting_insertions:
                cached_items = list(self)

            for i in reversed(indices):
                self.__internal_delitem__(i)
            insertion_index = 0 if index.start is None else index.start

            if not side_effecting_insertions:
                for e in item:
                    self.__internal_insert(insertion_index, e)
                    insertion_index += 1
                return

            try:
                for e in item:
                    self.__internal_insert(insertion_index, e)
                    insertion_index += 1
            except Exception:
                # restore the old state
                while len(self):
                    self.pop()
                self.extend(cached_items)
                raise
            return

        if not isinstance(item, collections.abc.Sequence):
            raise TypeError("can only assign a sequence")
        if len(item) != len(indices):
            raise ValueError(
                "attempt to assign sequence of size {} to extended "
                "slice of size {}".format(len(item), len(indices))
            )
        if not side_effecting_insertions:
            for i, e in enumerate(item):
                self.__internal_setitem__(indices[i], e)
            return

        cached_items = list(self)
        for i in reversed(indices):
            self.__internal_delitem__(i)
        try:
            for i, e in enumerate(item):
                self.__internal_insert(indices[i], e)
        except Exception:
            # restore the old state
            while len(self):
                self.pop()
            self.extend(cached_items)
            raise

    # This has to handle slicing
    def __delitem__(self, index):
        if not isinstance(index, slice):
            self.__internal_delitem__(index)
        else:
            for i in reversed(range(*index.indices(len(self)))):
                self.__delitem__(i)

    def insert(self, index, item):
        self.__internal_insert(index, item)

    collections.abc.MutableSequence.register(sequenceClass)
    sequenceClass.__radd__ = __radd__
    sequenceClass.__add__ = __add__
    sequenceClass.__getitem__ = __getitem__
    sequenceClass.__setitem__ = __setitem__
    sequenceClass.__delitem__ = __delitem__
    sequenceClass.insert = insert
    # A composition prints as itself rather than as a bare list, and writes
    # its own `__str__` in the extension; only a class that has none gets the
    # list one here.
    if '__str__' not in vars(sequenceClass):
        sequenceClass.__str__ = __str__
    if '__repr__' not in vars(sequenceClass):
        sequenceClass.__repr__ = __repr__

    # Everything else -- `append`, `extend`, `remove`, `pop`, `index`,
    # `count`, `__contains__`, `__iadd__`, `__reversed__` -- is written once
    # in the standard library in terms of the methods above.
    seen = set()
    for klass in (collections.abc.MutableSequence, collections.abc.Sequence):
        for name in klass.__dict__.keys():
            if name in seen:
                continue
            seen.add(name)
            func = getattr(klass, name)
            if (
                    isinstance(func, types.FunctionType)
                    and name not in klass.__abstractmethods__
                    and not hasattr(sequenceClass, name)
            ):
                setattr(sequenceClass, name, func)

    # As upstream: a sequence that is not an object copies into a new
    # free-standing one. An item's markers and effects cannot be built on
    # their own here, so their copy fails as it would without these.
    if not issubclass(sequenceClass, SerializableObject):
        def __copy__(self):
            v = sequenceClass()
            v.extend(e for e in self)
            return v

        def __deepcopy__(self, memo=None):
            v = sequenceClass()
            v.extend(copy.deepcopy(e, memo) for e in self)
            return v

        sequenceClass.__copy__ = __copy__
        sequenceClass.__deepcopy__ = __deepcopy__
    return sequenceClass


# Decorator that adds a function into a class.
def add_method(cls):
    def decorator(func):
        setattr(cls, func.__name__, func)
    return decorator
