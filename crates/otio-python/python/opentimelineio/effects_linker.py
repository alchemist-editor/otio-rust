# SPDX-License-Identifier: Apache-2.0
# Copyright Contributors to the OpenTimelineIO project

r"""
EffectsLinker plugins fire after an adapter has read a file (and after media
linking) in order to rewrite :class:`.Effect`\s into a common vocabulary, such
as the ``otio.fx.*`` registry, or into a site's own effect ids.

They expose a ``link_effect`` function with the signature:

.. py:function:: link_effect(in_effect: opentimelineio.schema.Effect, parent: opentimelineio.core.Composable, effects_linker_argument_map: dict) -> opentimelineio.schema.Effect  # noqa
   :noindex:

   Example link_effect function.

``parent`` is the :class:`.Item` whose ``effects`` holds the effect, or the
:class:`.Transition` whose ``effect`` it is. Effects have no parent pointer of
their own, so this is how a linker reaches the track kind, the clip's metadata
or a transition's offsets. Returning ``None`` keeps the effect as it is; any
other return value replaces it in place. A linker never removes an effect.
"""

import os
import inspect

from . import (
    _otio,
    exceptions,
    plugins,
    core,
    schema,
)


class EffectsLinkingPolicy:
    """Enum describing different effects linker policies"""
    DoNotLinkEffects = "__do_not_link_effects"
    ForceDefaultLinker = "__default"


def available_effects_linker_names():
    """Return a string list of the available effects linker plugins."""

    names = [str(adp.name) for adp in plugins.ActiveManifest().effects_linkers]
    # Linkers registered in Rust, through otio_adapter::plugins, come after
    # the ones manifests declare; a manifest's wins a name both use. The
    # built-in otio_fx_registry is not among them: the built-in manifest
    # lists it, as upstream's does.
    names.extend(
        name for name in _otio.native_effects_linker_names()
        if name not in names
    )
    return names


def from_name(name):
    """Fetch the effects linker object by the name of the linker directly."""

    if name == EffectsLinkingPolicy.ForceDefaultLinker or not name:
        name = os.environ.get("OTIO_DEFAULT_EFFECTS_LINKER", None)

    if not name:
        return None

    try:
        return plugins.ActiveManifest().from_name(
            name,
            kind_list="effects_linkers"
        )
    except exceptions.NotSupportedError:
        if name in _otio.native_effects_linker_names():
            return NativeEffectsLinker(name)
        raise exceptions.NotSupportedError(
            "effects linker not supported: {}, available: {}".format(
                name,
                available_effects_linker_names()
            )
        )


def default_effects_linker():
    try:
        return os.environ['OTIO_DEFAULT_EFFECTS_LINKER']
    except KeyError:
        raise exceptions.NoDefaultEffectsLinkerError(
            "No default Effects Linker set in $OTIO_DEFAULT_EFFECTS_LINKER"
        )


def linked_effect(
    target_effect,
    parent,
    effects_linker_name=EffectsLinkingPolicy.ForceDefaultLinker,
    effects_linker_argument_map=None
):
    """Return what the named linker makes of ``target_effect``, or ``None``
    when there is no linker or it keeps the effect."""

    effects_linker = from_name(effects_linker_name)

    if not effects_linker:
        return None

    if not effects_linker_argument_map:
        effects_linker_argument_map = {}

    return effects_linker.link_effect(
        target_effect,
        parent,
        effects_linker_argument_map
    )


def _composables_in(read_otio):
    """Yield read_otio (when it holds effects) and every descendant, in
    document order."""

    if isinstance(read_otio, schema.Timeline):
        read_otio = read_otio.tracks

    if isinstance(read_otio, (core.Item, schema.Transition)):
        yield read_otio

    find_children = getattr(read_otio, "find_children", None)
    if find_children is None:
        return

    yield from find_children()


def link_effects(
    read_otio,
    effects_linker_name=EffectsLinkingPolicy.ForceDefaultLinker,
    effects_linker_argument_map=None
):
    """Run the named effects linker over every effect in read_otio.

    Visits each entry of an Item's ``effects`` and each Transition's
    ``effect``. Makes changes in place and returns read_otio.
    """

    if not read_otio or not from_name(effects_linker_name):
        return read_otio

    for composable in _composables_in(read_otio):
        if isinstance(composable, schema.Transition):
            if composable.effect is None:
                continue
            new_effect = linked_effect(
                composable.effect,
                composable,
                effects_linker_name,
                effects_linker_argument_map
            )
            if new_effect is not None:
                composable.effect = new_effect
            continue

        effects = getattr(composable, "effects", None)
        if effects is None:
            continue
        for index, effect in enumerate(list(effects)):
            new_effect = linked_effect(
                effect,
                composable,
                effects_linker_name,
                effects_linker_argument_map
            )
            if new_effect is not None:
                effects[index] = new_effect

    return read_otio


@core.register_type
class EffectsLinker(plugins.PythonPlugin):
    _serializable_label = "EffectsLinker.1"

    def __init__(
        self,
        name=None,
        filepath=None,
    ):
        super().__init__(name, filepath)

    def link_effect(
        self,
        in_effect,
        parent=None,
        effects_linker_argument_map=None
    ):
        effects_linker_argument_map = effects_linker_argument_map or {}

        return self._execute_function(
            "link_effect",
            in_effect=in_effect,
            parent=parent,
            effects_linker_argument_map=effects_linker_argument_map
        )

    def is_default_linker(self):
        return os.environ.get("OTIO_DEFAULT_EFFECTS_LINKER", "") == self.name

    def plugin_info_map(self):
        """Adds extra linker-specific information to call to the parent fn."""

        result = super().plugin_info_map()

        fn_doc = inspect.getdoc(self.module().link_effect)
        if fn_doc:
            mod_doc = [result['doc'], ""]
            mod_doc.append(fn_doc)
            result["doc"] = "\n".join(mod_doc)

        if self.is_default_linker():
            result["** CURRENT DEFAULT EFFECTS LINKER"] = True

        return result

    def __str__(self):
        return "EffectsLinker({}, {})".format(
            repr(self.name),
            repr(self.filepath)
        )

    def __repr__(self):
        return (
            "otio.effects_linker.EffectsLinker("
            "name={}, "
            "filepath={}"
            ")".format(
                repr(self.name),
                repr(self.filepath)
            )
        )


class NativeEffectsLinker:
    """An effects linker registered in Rust rather than by a manifest.

    It answers ``link_effect`` as a manifest's :class:`EffectsLinker` does,
    so a read uses it the same way. It is handed its arguments as OTIO
    values, and an argument without one is left out.
    """

    def __init__(self, name):
        self.name = name

    def link_effect(
        self,
        in_effect,
        parent=None,
        effects_linker_argument_map=None
    ):
        return _otio.run_native_effects_linker(
            self.name,
            in_effect,
            parent,
            dict(effects_linker_argument_map or {})
        )

    def is_default_linker(self):
        return os.environ.get("OTIO_DEFAULT_EFFECTS_LINKER", "") == self.name

    def __str__(self):
        return "NativeEffectsLinker({})".format(repr(self.name))

    def __repr__(self):
        return "otio.effects_linker.NativeEffectsLinker(name={})".format(
            repr(self.name)
        )
