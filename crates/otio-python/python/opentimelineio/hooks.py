# SPDX-License-Identifier: Apache-2.0
# Copyright Contributors to the OpenTimelineIO project

from . import (
    _otio,
    exceptions,
    plugins,
    core,
)

__doc__ = """
HookScripts are plugins that run at defined points ("Hooks").

They expose a ``hook_function`` with signature:

.. py:function:: hook_function(timeline: opentimelineio.schema.Timeline, optional_argument_dict: dict[str, Any]) -> opentimelineio.schema.Timeline  # noqa
   :noindex:

   Hook function signature

Both hook scripts and the hooks they attach to are defined in the plugin
manifest.

Multiple scripts can be attached to a hook. They will be executed in list
order, first to last.

They are defined by the manifests :class:`HookScript`\\s and hooks areas.

.. code-block:: json

   {
       "OTIO_SCHEMA" : "PluginManifest.1",
       "hook_scripts" : [
           {
               "OTIO_SCHEMA" : "HookScript.1",
               "name" : "example hook",
               "filepath" : "example.py"
           }
       ],
       "hooks" : {
           "pre_adapter_write" : ["example hook"],
           "post_adapter_read" : []
       }
   }

The ``hook_scripts`` area loads the python modules with the ``hook_function``\\s to
call in them.  The ``hooks`` area defines the hooks (and any associated
scripts). You can further query and modify these from python.

.. code-block:: python

   import opentimelineio as otio
   hook_list = otio.hooks.scripts_attached_to("some_hook") # -> ['a','b','c']

   # to run the hook scripts:
   otio.hooks.run("some_hook", some_timeline, optional_argument_dict)

This will pass (some_timeline, optional_argument_dict) to ``a``, which will
a new timeline that will get passed into ``b`` with ``optional_argument_dict``,
etc.

To edit the order, change the order in the list:

.. code-block:: python

   hook_list[0], hook_list[2] = hook_list[2], hook_list[0]
   print hook_list # ['c','b','a']

Now ``c`` will run, then ``b``, then ``a``.

To delete a function the list:

.. code-block:: python

   del hook_list[1]

----
"""


@core.register_type
class HookScript(plugins.PythonPlugin):
    _serializable_label = "HookScript.1"

    def __init__(
        self,
        name=None,
        filepath=None,
    ):
        """HookScript plugin constructor."""

        super().__init__(name, filepath)

    def run(self, in_timeline, argument_map={}):
        """Run the hook_function associated with this plugin."""

        # @TODO: should in_timeline be passed in place?  or should a copy be
        #        made?
        return self._execute_function(
            "hook_function",
            in_timeline=in_timeline,
            argument_map=argument_map
        )

    def __str__(self):
        return "HookScript({}, {})".format(
            repr(self.name),
            repr(self.filepath)
        )

    def __repr__(self):
        return (
            "otio.hooks.HookScript("
            "name={}, "
            "filepath={}"
            ")".format(
                repr(self.name),
                repr(self.filepath)
            )
        )


def names():
    """Return a list of all the registered hooks.

    Those the manifests declare, then any declared only in Rust, through
    ``otio_adapter::plugins``.
    """

    declared = list(plugins.ActiveManifest().hooks.keys())
    declared.extend(
        hook for hook in _otio.native_hook_names() if hook not in declared
    )
    return declared


def available_hookscript_names():
    """Return the names of HookScripts that have been registered.

    Those the manifests declare, then any registered only in Rust.
    """

    names = [hs.name for hs in plugins.ActiveManifest().hook_scripts]
    names.extend(
        name for name in _otio.native_hook_script_names() if name not in names
    )
    return names


def available_hookscripts():
    """Return the HookScripts objects that have been registered."""
    return plugins.ActiveManifest().hook_scripts


def scripts_attached_to(hook):
    """Return an editable list of all the hook scripts that are attached to
    the specified hook, in execution order.  Changing this list will change the
    order that scripts run in, and deleting a script will remove it from
    executing

    For a hook a manifest declares, this is the manifest's own list, which
    ``run`` runs first; scripts attached to the same hook in Rust run after
    it and are listed by ``_otio.native_scripts_attached_to``. For a hook
    declared only in Rust, it is a copy of the scripts attached there.
    """

    # @TODO: Should this return a copy?
    manifest_hooks = plugins.ActiveManifest().hooks
    if hook not in manifest_hooks:
        native = _otio.native_scripts_attached_to(hook)
        if native is not None:
            # A copy: the scripts attached in Rust are changed there.
            return native
    return manifest_hooks[hook]


def run(hook, tl, extra_args=None):
    """Run all the scripts associated with hook, passing in tl and extra_args.

    Will return the return value of the last hook script.

    If no hookscripts are defined, returns tl.

    The scripts a manifest attaches run first, as upstream runs them; then
    any attached to the same hook in Rust, through ``otio_adapter::plugins``.
    A hook declared in neither raises ``KeyError``, as upstream's does.
    """

    manifest = plugins.ActiveManifest()
    native = _otio.native_scripts_attached_to(hook)
    if hook in manifest.hooks or native is None:
        hook_scripts = manifest.hooks[hook]
    else:
        hook_scripts = []
    for name in hook_scripts:
        try:
            hs = manifest.from_name(name, "hook_scripts")
        except exceptions.NotSupportedError:
            if name not in _otio.native_hook_script_names():
                raise
            tl = _run_native(name, tl, extra_args)
            continue
        tl = hs.run(tl, extra_args)
    for name in native or []:
        tl = _run_native(name, tl, extra_args)
    return tl


def _run_native(name, tl, extra_args):
    """Runs the hook script registered in Rust as name.

    A native script works on an OTIO object, so one handed none, as the AAF
    adapter's pre-read hook is, has nothing to run on and is skipped.
    """
    if tl is None:
        return tl
    arguments = extra_args if isinstance(extra_args, dict) else None
    return _otio.run_native_hook_script(name, tl, arguments)
