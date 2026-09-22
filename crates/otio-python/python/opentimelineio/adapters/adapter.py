# SPDX-License-Identifier: Apache-2.0
# Copyright Contributors to the OpenTimelineIO project

"""The ``Adapter`` wrapper around one adapter module.

Upstream's ``Adapter`` is a plugin record read from a JSON manifest: a name,
the suffixes it claims, and the path of a Python module that is imported the
first time it is used. Every adapter here ships inside this package, so the
record holds the module itself rather than a path to it. What a caller can
ask of one -- ``has_feature``, ``module()`` and the four read and write
methods, with upstream's fallbacks between them -- is the same.

Two things upstream's methods do that these do not: run hook scripts, of which
none can be registered here because there is no manifest to register them in,
and link media, for which there are no media linkers. The arguments for both
are still taken, so calls written against upstream keep working; asking for a
named media linker is refused rather than quietly skipped.
"""

from .. import exceptions, media_linker

# Which module functions provide which feature, as upstream spells them.
_FEATURE_MAP = {
    'read_from_file': ['read_from_file'],
    'read_from_string': ['read_from_string'],
    'read': ['read_from_file', 'read_from_string'],
    'write_to_file': ['write_to_file'],
    'write_to_string': ['write_to_string'],
    'write': ['write_to_file', 'write_to_string'],
}


class Adapter:
    """One file format this library reads, writes, or both."""

    def __init__(self, name, module, suffixes):
        self.name = name
        self.suffixes = list(suffixes)
        self._module = module

    @property
    def filepath(self):
        """The file the adapter's module was loaded from."""
        return self._module.__file__

    def module(self):
        """Returns the module that implements this adapter."""
        return self._module

    def module_abs_path(self):
        """Returns the absolute path of the module that implements this adapter."""
        return self.filepath

    def has_feature(self, feature_string):
        """Whether the adapter provides ``feature_string``.

        ``read`` and ``write`` mean either the file or the string form. Any
        string upstream does not know is not a feature.
        """
        search_strs = _FEATURE_MAP.get(feature_string.lower())
        if search_strs is None:
            return False
        return any(hasattr(self._module, s) for s in search_strs)

    def _execute_function(self, func_name, **kwargs):
        if not hasattr(self._module, func_name):
            raise exceptions.AdapterDoesntSupportFunctionError(
                f"Sorry, {self.name} doesn't support {func_name}."
            )
        return getattr(self._module, func_name)(**kwargs)

    def read_from_file(
        self,
        filepath,
        media_linker_name=media_linker.MediaLinkingPolicy.ForceDefaultLinker,
        media_linker_argument_map=None,
        hook_function_argument_map=None,
        **adapter_argument_map
    ):
        """Reads ``filepath`` with this adapter.

        An adapter that only reads strings is handed the file's text.
        """
        media_linker._refuse_named_linker(media_linker_name)
        if (
            not self.has_feature("read_from_file")
            and self.has_feature("read_from_string")
        ):
            with open(filepath, encoding="utf-8") as fo:
                contents = fo.read()
            return self._execute_function(
                "read_from_string",
                input_str=contents,
                **adapter_argument_map
            )
        return self._execute_function(
            "read_from_file",
            filepath=filepath,
            **adapter_argument_map
        )

    def write_to_file(
        self,
        input_otio,
        filepath,
        hook_function_argument_map=None,
        **adapter_argument_map
    ):
        """Writes ``input_otio`` to ``filepath`` with this adapter.

        An adapter that only writes strings has its text saved to the file,
        and the path is returned, as upstream returns it.
        """
        if (
            not self.has_feature("write_to_file")
            and self.has_feature("write_to_string")
        ):
            result = self.write_to_string(input_otio, **adapter_argument_map)
            with open(filepath, 'w', encoding="utf-8") as fo:
                fo.write(result)
            return filepath
        return self._execute_function(
            "write_to_file",
            input_otio=input_otio,
            filepath=filepath,
            **adapter_argument_map
        )

    def read_from_string(
        self,
        input_str,
        media_linker_name=media_linker.MediaLinkingPolicy.ForceDefaultLinker,
        media_linker_argument_map=None,
        hook_function_argument_map=None,
        **adapter_argument_map
    ):
        """Reads an object from ``input_str`` with this adapter."""
        media_linker._refuse_named_linker(media_linker_name)
        return self._execute_function(
            "read_from_string",
            input_str=input_str,
            **adapter_argument_map
        )

    def write_to_string(
        self,
        input_otio,
        hook_function_argument_map=None,
        **adapter_argument_map
    ):
        """Writes ``input_otio`` as a string with this adapter."""
        return self._execute_function(
            "write_to_string",
            input_otio=input_otio,
            **adapter_argument_map
        )

    def __str__(self):
        return "Adapter({}, {}, {})".format(
            repr(self.name),
            repr(self.filepath),
            repr(self.suffixes),
        )

    def __repr__(self):
        return (
            "otio.adapter.Adapter("
            "name={}, "
            "filepath={}, "
            "suffixes={}"
            ")".format(
                repr(self.name),
                repr(self.filepath),
                repr(self.suffixes),
            )
        )
