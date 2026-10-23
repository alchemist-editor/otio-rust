# SPDX-License-Identifier: Apache-2.0
"""Effects linkers registered in Rust, reached from Python.

Upstream's effects schema fork tests the linkers manifests declare, and its
tests run unmodified in ``tests/upstream``. These cover what this package
adds: a linker registered natively, through ``otio_adapter::plugins``, which
``_testing.register_native_example_plugins`` stands in for, and the built-in
``otio_fx_registry``, which is native here and listed by the manifest.
"""

import unittest

import opentimelineio as otio


def cut():
    """A track holding a clip with two effects, and a transition with one."""

    clip = otio.schema.Clip(
        name="a",
        effects=[
            otio.schema.Effect(effect_name="blur"),
            otio.schema.Effect(effect_name="keep"),
        ],
    )
    transition = otio.schema.Transition(
        name="t",
        effect=otio.schema.Effect(effect_name="wipe"),
    )
    return otio.schema.Track(children=[clip, transition])


class NativeEffectsLinkers(unittest.TestCase):
    def setUp(self):
        otio._otio._testing.register_native_example_plugins()
        self.addCleanup(
            otio._otio._testing.register_native_example_plugins, False
        )

    def test_a_native_linker_is_listed_once_and_the_built_in_once(self):
        names = otio.effects_linker.available_effects_linker_names()
        self.assertEqual(names.count("otio_fx_registry"), 1)
        self.assertEqual(names[-1], "native_example")

    def test_a_read_runs_a_native_linker_by_name(self):
        text = otio.adapters.write_to_string(cut(), "otio_json")
        track = otio.adapters.read_from_string(
            text,
            "otio_json",
            effects_linker_name="native_example",
            effects_linker_argument_map={"studio": "north"},
        )
        clip, transition = track[0], track[1]
        blur, keep = clip.effects
        self.assertEqual(blur.effect_id, "native.blur")
        self.assertEqual(blur.parameters["parent"], "Clip")
        self.assertEqual(list(blur.parameters["arguments"]), ["studio"])
        self.assertEqual(keep.effect_id, "")
        self.assertEqual(transition.effect.effect_id, "native.wipe")
        self.assertEqual(transition.effect.parameters["parent"], "Transition")

    def test_asked_not_to_link_a_read_links_nothing(self):
        text = otio.adapters.write_to_string(cut(), "otio_json")
        track = otio.adapters.read_from_string(
            text,
            "otio_json",
            effects_linker_name=(
                otio.effects_linker.EffectsLinkingPolicy.DoNotLinkEffects
            ),
        )
        self.assertEqual(track[0].effects[0].effect_id, "")

    def test_an_unknown_linker_still_lists_the_native_ones(self):
        with self.assertRaises(otio.exceptions.NotSupportedError) as raised:
            otio.effects_linker.from_name("no_such_linker")
        self.assertIn("native_example", str(raised.exception))

    def test_the_built_in_linker_is_the_native_one(self):
        effect = otio.schema.Effect(effect_name="Gaussian Blur")
        linked = otio.effects_linker.from_name("otio_fx_registry").link_effect(
            effect, None, {}
        )
        self.assertIs(linked, effect)
        self.assertEqual(effect.effect_id, "otio.fx.gaussian_blur")
        self.assertEqual(
            effect.metadata["otio.fx"]["linked_from"]["linker"],
            "otio_fx_registry",
        )

    def test_post_effects_linker_is_not_a_hook_until_something_uses_it(self):
        # The library declares it for every read, but a manifest that does
        # not is not handed it: upstream's own hook tests replace the
        # manifest with one written before the hook existed.
        self.assertIn("post_effects_linker", otio.hooks.names())
        manifest = otio.plugins.manifest.Manifest()
        manifest.hooks = {"post_adapter_read": []}
        saved = otio.plugins.manifest._MANIFEST
        otio.plugins.manifest._MANIFEST = manifest
        self.addCleanup(setattr, otio.plugins.manifest, "_MANIFEST", saved)
        self.assertNotIn("post_effects_linker", otio.hooks.names())


if __name__ == "__main__":
    unittest.main()
