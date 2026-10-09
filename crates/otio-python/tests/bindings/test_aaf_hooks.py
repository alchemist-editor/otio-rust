# SPDX-License-Identifier: Apache-2.0
"""The AAF adapter's four hooks, run as upstream's adapter runs them.

The first three tests are upstream's own, from ``AAFWriterTests`` in
``otio-aaf-adapter``'s ``tests/test_aaf_adapter.py``, run against its example
plugin, vendored unmodified in ``tests/adapters/aaf``. Upstream reads its
``simple.aaf``, which is not vendored here, so the read test reads another of
its sample files. The rest pin what upstream's tests leave open: what a hook
is handed, and that what it returns is what the adapter goes on with.
"""

import contextlib
import json
import os
import pathlib
import sys
import tempfile
import textwrap
import types
import unittest

import opentimelineio as otio

TESTS = pathlib.Path(__file__).resolve().parents[1]
CRATES = TESTS.parents[1]
sys.path.insert(0, str(TESTS / "adapters" / "shims"))

from otio_aaf_adapter.adapters.aaf_adapter import hooks  # noqa: E402

AAFAdapterError = otio.adapters.advanced_authoring_format.AAFAdapterError
SIMPLE_EXAMPLE_PATH = os.fspath(
    CRATES / "otio-aaf" / "tests" / "data" / "2997fps-DFTC.aaf"
)
TRANSITION_PATH = os.fspath(
    CRATES / "otio-aaf" / "tests" / "data" / "nested_audio_dissolve.aaf"
)


@contextlib.contextmanager
def with_hooks_plugin_environment():
    env_bkp = os.environ.copy()
    try:
        os.environ["OTIO_PLUGIN_MANIFEST_PATH"] = (
            os.fspath(
                TESTS / "adapters" / "aaf" / "hooks_plugin_example"
                / "plugin_manifest.json"
            )
        )
        otio.plugins.manifest.ActiveManifest(force_reload=True)
        yield
    finally:
        os.environ = env_bkp
        otio.plugins.manifest.ActiveManifest(force_reload=True)


@contextlib.contextmanager
def with_hook(hook, body):
    """Attaches a hook script whose ``hook_function`` is ``body`` to ``hook``,
    for the length of the block."""
    with tempfile.TemporaryDirectory() as folder:
        folder = pathlib.Path(folder)
        (folder / "script.py").write_text(textwrap.dedent(body))
        (folder / "plugin_manifest.json").write_text(json.dumps({
            "OTIO_SCHEMA": "PluginManifest.1",
            "hook_scripts": [{
                "OTIO_SCHEMA": "HookScript.1",
                "name": "script",
                "filepath": "script.py",
            }],
            "hooks": {hook: ["script"]},
        }))
        env_bkp = os.environ.copy()
        try:
            os.environ["OTIO_PLUGIN_MANIFEST_PATH"] = os.fspath(
                folder / "plugin_manifest.json"
            )
            otio.plugins.manifest.ActiveManifest(force_reload=True)
            yield
        finally:
            os.environ = env_bkp
            otio.plugins.manifest.ActiveManifest(force_reload=True)


# What the probing hooks below saw. They are loaded from a file by the plugin
# system, so they reach this through a module of its own rather than through
# this one, whose name depends on how the tests were started.
SEEN = {}
_probe = types.ModuleType("aaf_hook_probe")
_probe.SEEN = SEEN
sys.modules["aaf_hook_probe"] = _probe


class UpstreamHookTests(unittest.TestCase):
    def test_transcribe_hooks_registry(self):
        """Tests if the hook example correctly registers with OTIO."""
        with with_hooks_plugin_environment():
            for hook_script in ["post_aaf_write_transcribe_hook",
                                "pre_aaf_write_transcribe_hook",
                                "post_aaf_read_transcribe_hook",
                                "pre_aaf_read_transcribe_hook"]:
                self.assertIn(
                    hook_script,
                    otio.plugins.plugin_info_map()["hook_scripts"]
                )

            for hook_name in [hooks.HOOK_PRE_WRITE_TRANSCRIBE,
                              hooks.HOOK_POST_WRITE_TRANSCRIBE,
                              hooks.HOOK_PRE_READ_TRANSCRIBE,
                              hooks.HOOK_POST_READ_TRANSCRIBE]:
                self.assertIn(
                    hook_name,
                    otio.plugins.plugin_info_map()["hooks"]
                )

    def test_transcribe_write_hook_args_map(self):
        """Tests if extra arguments are correctly passed to the hooks.
        """

        tl = otio.schema.Timeline(tracks=[])
        _, tmp_aaf_path = tempfile.mkstemp(suffix='.aaf')
        self.assertTrue(otio.adapters.from_name("AAF").has_feature("hooks"))
        with with_hooks_plugin_environment():
            with self.assertRaises(AAFAdapterError):
                otio.adapters.write_to_file(
                    tl,
                    filepath=tmp_aaf_path,
                    embed_essence=True,
                    use_empty_mob_ids=True,
                    hook_function_argument_map={
                        "test_pre_hook_raise": True
                    }
                )

            with self.assertRaises(AAFAdapterError):
                otio.adapters.write_to_file(
                    tl,
                    filepath=tmp_aaf_path,
                    embed_essence=True,
                    use_empty_mob_ids=True,
                    hook_function_argument_map={
                        "test_post_hook_raise": True
                    }
                )

    def test_transcribe_read_hook_args_map(self):
        """Tests if extra arguments are correctly passed to the read hooks."""
        self.assertTrue(otio.adapters.from_name("AAF").has_feature("hooks"))
        with with_hooks_plugin_environment():
            with self.assertRaises(AAFAdapterError):
                otio.adapters.read_from_file(
                    SIMPLE_EXAMPLE_PATH,
                    hook_function_argument_map={
                        "test_pre_hook_raise": True
                    }
                )

            with self.assertRaises(AAFAdapterError):
                otio.adapters.read_from_file(
                    SIMPLE_EXAMPLE_PATH,
                    hook_function_argument_map={
                        "test_post_hook_raise": True
                    }
                )


class WhatHooksAreHanded(unittest.TestCase):
    def setUp(self):
        SEEN.clear()

    def test_the_read_hooks_are_handed_upstreams_arguments(self):
        for hook in (hooks.HOOK_PRE_READ_TRANSCRIBE,
                     hooks.HOOK_POST_READ_TRANSCRIBE):
            with self.subTest(hook=hook), with_hook(hook, """
                from aaf_hook_probe import SEEN
                def hook_function(in_timeline, argument_map=None):
                    SEEN["timeline"] = in_timeline
                    SEEN["arguments"] = dict(argument_map)
                    return in_timeline
            """):
                SEEN.clear()
                otio.adapters.read_from_file(
                    SIMPLE_EXAMPLE_PATH,
                    hook_function_argument_map={"mine": 1},
                )
                arguments = SEEN["arguments"]
                self.assertEqual(arguments["mine"], 1)
                self.assertEqual(arguments["read_filepath"], SIMPLE_EXAMPLE_PATH)
                # There is no pyaaf2 file to hand over.
                self.assertIsNone(arguments["aaf_handle"])
                if hook == hooks.HOOK_PRE_READ_TRANSCRIBE:
                    self.assertIsNone(SEEN["timeline"])
                else:
                    self.assertIsInstance(
                        SEEN["timeline"], otio.schema.SerializableCollection
                    )

    def test_the_write_hooks_are_handed_upstreams_arguments(self):
        timeline = otio.schema.Timeline(tracks=[])
        for hook in (hooks.HOOK_PRE_WRITE_TRANSCRIBE,
                     hooks.HOOK_POST_WRITE_TRANSCRIBE):
            with self.subTest(hook=hook), with_hook(hook, """
                from aaf_hook_probe import SEEN
                def hook_function(in_timeline, argument_map=None):
                    SEEN["timeline"] = in_timeline
                    SEEN["arguments"] = dict(argument_map)
                    return in_timeline
            """), tempfile.TemporaryDirectory() as folder:
                SEEN.clear()
                path = os.path.join(folder, "out.aaf")
                otio.adapters.write_to_file(
                    timeline, path, hook_function_argument_map={"mine": 1}
                )
                arguments = SEEN["arguments"]
                self.assertEqual(arguments["mine"], 1)
                self.assertEqual(arguments["write_filepath"], path)
                self.assertIsNone(arguments["aaf_handle"])
                self.assertIs(arguments["embed_essence"], False)
                self.assertIs(SEEN["timeline"], timeline)


class WhenTheBridgeIsInstalled(unittest.TestCase):
    def test_only_when_a_script_is_attached(self):
        aaf = otio.adapters.advanced_authoring_format
        # The adapter's manifest declares the hook with nothing attached.
        self.assertIn(hooks.HOOK_POST_READ_TRANSCRIBE, otio.hooks.names())
        self.assertFalse(aaf._has_scripts(hooks.HOOK_POST_READ_TRANSCRIBE))
        with with_hook(hooks.HOOK_POST_READ_TRANSCRIBE, """
            def hook_function(in_timeline, argument_map=None):
                return in_timeline
        """):
            self.assertTrue(aaf._has_scripts(hooks.HOOK_POST_READ_TRANSCRIBE))


class WhatHooksReturn(unittest.TestCase):
    def test_the_passes_run_on_what_the_post_read_hook_returns(self):
        plain = otio.adapters.read_from_file(TRANSITION_PATH)
        with with_hook(hooks.HOOK_POST_READ_TRANSCRIBE, """
            def hook_function(in_timeline, argument_map=None):
                # Before simplifying, the read is a collection of timelines.
                in_timeline[0].name = "renamed by the hook"
                return in_timeline
        """):
            hooked = otio.adapters.read_from_file(TRANSITION_PATH)
        self.assertEqual(hooked.name, "renamed by the hook")
        hooked.name = plain.name
        # Simplifying and moving transitions happened after the hook, exactly
        # as without it.
        self.assertEqual(
            otio.adapters.write_to_string(hooked),
            otio.adapters.write_to_string(plain),
        )

    def test_the_post_read_hook_can_return_another_object(self):
        with with_hook(hooks.HOOK_POST_READ_TRANSCRIBE, """
            import opentimelineio as otio
            def hook_function(in_timeline, argument_map=None):
                return otio.schema.Timeline(name="a stand-in")
        """):
            read = otio.adapters.read_from_file(SIMPLE_EXAMPLE_PATH)
        self.assertIsInstance(read, otio.schema.Timeline)
        self.assertEqual(read.name, "a stand-in")

    def test_what_the_post_read_hook_returns_sits_in_nothing(self):
        with with_hook(hooks.HOOK_POST_READ_TRANSCRIBE, """
            def hook_function(in_timeline, argument_map=None):
                return in_timeline[0].tracks[0]
        """):
            read = otio.adapters.read_from_file(SIMPLE_EXAMPLE_PATH)
        # The passes after the hook may simplify the track it returned down
        # to its one clip; either way, the result sits in nothing.
        self.assertIsInstance(read, otio.core.Item)
        self.assertIsNone(read.parent())

    def test_what_the_post_read_hook_sets_from_python_stays(self):
        with with_hook(hooks.HOOK_POST_READ_TRANSCRIBE, """
            def hook_function(in_timeline, argument_map=None):
                timeline = in_timeline[0]
                timeline.review_status = "approved"
                timeline.tracks[0].checked_by = "the hook"
                return timeline
        """):
            read = otio.adapters.read_from_file(
                SIMPLE_EXAMPLE_PATH, simplify=False
            )
        # As upstream, whose passes run on the very objects the hook returned.
        self.assertEqual(read.review_status, "approved")
        self.assertEqual(read.tracks[0].checked_by, "the hook")

    def test_an_object_the_post_read_hook_keeps_is_the_one_read(self):
        with with_hook(hooks.HOOK_POST_READ_TRANSCRIBE, """
            def hook_function(in_timeline, argument_map=None):
                timeline = in_timeline[0]
                track = timeline.tracks[0]
                timeline.favorite_track = track
                timeline.picks = [track, ("first", track)]
                timeline.by_name = {"first": track}
                timeline.by_track = {track: "first"}
                shared = [track]
                shared.append(shared)
                timeline.shared = shared
                timeline.also_shared = shared
                items = []
                cycle = (items,)
                items.append(cycle)
                timeline.cycle = cycle
                timeline.state = timeline.__dict__
                return timeline
        """):
            read = otio.adapters.read_from_file(
                SIMPLE_EXAMPLE_PATH, simplify=False
            )
        # As upstream, whose passes run on the very objects the hook kept.
        track = read.tracks[0]
        self.assertIs(read.favorite_track, track)
        self.assertIs(read.picks[0], track)
        self.assertIs(read.picks[1][1], track)
        self.assertIs(read.by_name["first"], track)
        self.assertIs(next(iter(read.by_track)), track)
        # What the hook shared stays shared, cycles included.
        self.assertIs(read.shared, read.also_shared)
        self.assertIs(read.shared[0], track)
        self.assertIs(read.shared[1], read.shared)
        self.assertIs(read.cycle[0][0], read.cycle)
        self.assertIs(read.state, read.__dict__)

    def test_a_post_read_hook_returning_no_object_is_refused(self):
        with with_hook(hooks.HOOK_POST_READ_TRANSCRIBE, """
            def hook_function(in_timeline, argument_map=None):
                return None
        """):
            with self.assertRaises(TypeError):
                otio.adapters.read_from_file(SIMPLE_EXAMPLE_PATH)

    def test_what_the_pre_write_hook_returns_is_written(self):
        # The writer refuses anything but a timeline, so the write succeeding
        # shows it wrote what the hook returned rather than what it was given.
        unwritable = otio.schema.SerializableCollection()
        with self.assertRaises(otio.exceptions.NotSupportedError), \
                tempfile.TemporaryDirectory() as folder:
            otio.adapters.write_to_file(
                unwritable, os.path.join(folder, "out.aaf"), adapter_name="AAF"
            )
        with with_hook(hooks.HOOK_PRE_WRITE_TRANSCRIBE, """
            import opentimelineio as otio
            def hook_function(in_timeline, argument_map=None):
                return otio.schema.Timeline(tracks=[])
        """), tempfile.TemporaryDirectory() as folder:
            path = os.path.join(folder, "out.aaf")
            otio.adapters.write_to_file(unwritable, path, adapter_name="AAF")
            self.assertTrue(os.path.getsize(path) > 0)

if __name__ == "__main__":
    unittest.main()
