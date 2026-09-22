# SPDX-License-Identifier: Apache-2.0
# Copyright Contributors to the OpenTimelineIO project

"""The registry of adapters, in upstream's shape.

Upstream builds its registry from JSON plugin manifests found on
``OTIO_PLUGIN_MANIFEST_PATH`` and in installed packages, so that a third party
can ship an adapter, a media linker, a hook or a schema. Here every adapter is
part of this package and there is nothing else to find, so the manifest is
built once, in code. ``ActiveManifest()`` and the lookups on it answer as
upstream's do, for code that goes through them rather than through
``opentimelineio.adapters``.
"""

from .. import exceptions

__all__ = [
    'ActiveManifest',
    'Manifest',
]


class Manifest:
    """What is registered: the adapters, and empty lists of everything else."""

    def __init__(self, adapters):
        self.adapters = list(adapters)
        self.schemadefs = []
        self.media_linkers = []
        self.hook_scripts = []
        self.hooks = {}
        self.version_manifests = {}

    def from_filepath(self, suffix):
        """Returns the adapter that claims a file suffix, without its dot."""
        for adapter in self.adapters:
            if suffix.lower() in adapter.suffixes:
                return adapter
        raise exceptions.NoKnownAdapterForExtensionError(suffix)

    def adapter_module_from_suffix(self, suffix):
        """Returns the module of the adapter that claims a file suffix."""
        return self.from_filepath(suffix).module()

    def from_name(self, name, kind_list="adapters"):
        """Returns the plugin of kind ``kind_list`` called ``name``."""
        for thing in getattr(self, kind_list):
            if name == thing.name:
                return thing
        raise exceptions.NotSupportedError(
            "Could not find plugin: '{}' in kind_list: '{}'."
            " options: {}".format(
                name,
                kind_list,
                getattr(self, kind_list)
            )
        )

    def adapter_module_from_name(self, name):
        """Returns the module of the adapter called ``name``."""
        return self.from_name(name).module()


_MANIFEST = None


def ActiveManifest(force_reload=False):
    """Returns the manifest every lookup goes through."""
    global _MANIFEST
    if _MANIFEST is None or force_reload:
        # Imported here: the adapters package imports this one.
        from ..adapters import _builtin_adapters
        _MANIFEST = Manifest(_builtin_adapters())
    return _MANIFEST
