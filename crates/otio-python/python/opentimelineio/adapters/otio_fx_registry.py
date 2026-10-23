# SPDX-License-Identifier: Apache-2.0
# Copyright Contributors to the OpenTimelineIO project

"""Reference effects linker for the ``otio.fx.*`` common effect registry.

Rewrites host-native effects into registry effects using the mapping table in
``otio_fx_registry.json``, which ships beside this file. Pass
``registry_path`` in the effects linker argument map to use another table.

Each mapping names a registry ``effect_id``, a ``match`` (all of whose keys
must hold), an optional ``fidelity`` and an optional ``parameter_map``:

.. code-block:: json

    {
        "effect_id": "otio.fx.gaussian_blur",
        "match": {"effect_name": "Gaussian Blur", "metadata_key": "fcp_xml"},
        "fidelity": "approximate",
        "parameter_map": {"amount": {"from": "radius", "scale": 0.01}}
    }

Match keys: ``effect_id``, ``effect_name`` and ``host`` compare equal;
``metadata_key`` requires that key in the effect's metadata.

A linked effect keeps what it was in ``metadata["otio.fx"]["linked_from"]``,
so a writer for the original host can still round-trip it.
"""

import json
import os

from .. import _otio

# This module is upstream's interface to the reference linker; the linker
# itself is the native one, otio_adapter::otio_fx_registry, so that Python,
# the C ABI and every SDK built on it map an effect the same way. It reads
# the same table: the JSON beside this file is byte for byte the one the
# native linker builds in, and a test checks that they agree.

LINKER_NAME = "otio_fx_registry"
REGISTRY_PREFIX = "otio.fx."
DEFAULT_REGISTRY_PATH = os.path.join(
    os.path.dirname(__file__),
    "otio_fx_registry.json"
)

_registry_cache = {}


def load_registry(path=None):
    """Return the registry document at path (default: the bundled one)."""

    path = path or DEFAULT_REGISTRY_PATH
    if path not in _registry_cache:
        with open(path, encoding="utf-8") as fo:
            _registry_cache[path] = json.load(fo)
    return _registry_cache[path]


def link_effect(in_effect, parent, effects_linker_argument_map):
    """Map a host-native effect onto its ``otio.fx`` registry entry.

    Leaves retimes (:class:`.TimeEffect`), effects already in the registry
    and effects no mapping matches alone.
    """

    return _otio.run_native_effects_linker(
        LINKER_NAME,
        in_effect,
        parent,
        dict(effects_linker_argument_map or {})
    )
