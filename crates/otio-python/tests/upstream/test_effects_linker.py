# SPDX-License-Identifier: Apache-2.0
# Copyright Contributors to the OpenTimelineIO project

"""Tests for effects linker plugins and the otio_fx_registry linker."""

import json
import os
import tempfile
import unittest

from tests import baseline_reader

import opentimelineio as otio
from opentimelineio.adapters import otio_fx_registry
from tests import utils

LINKER_PATH = "effects_linker_example"
MANIFEST_PATH = "effects_linker_plugin_manifest.plugin_manifest"


class _ManifestTestCase(unittest.TestCase):
    def setUp(self):
        self.bak = otio.plugins.ActiveManifest()
        self.man = utils.create_manifest(MANIFEST_PATH)
        otio.plugins.manifest._MANIFEST = self.man
        self.env_bak = os.environ.pop("OTIO_DEFAULT_EFFECTS_LINKER", None)

    def tearDown(self):
        otio.plugins.manifest._MANIFEST = self.bak
        utils.remove_manifest(self.man)
        os.environ.pop("OTIO_DEFAULT_EFFECTS_LINKER", None)
        if self.env_bak is not None:
            os.environ["OTIO_DEFAULT_EFFECTS_LINKER"] = self.env_bak


class TestPluginEffectsLinker(_ManifestTestCase):
    def setUp(self):
        super().setUp()
        self.jsn = baseline_reader.json_baseline_as_string(LINKER_PATH)
        self.eln = otio.adapters.otio_json.read_from_string(self.jsn)
        self.eln._json_path = os.path.join(
            baseline_reader.MODPATH,
            "baselines",
            LINKER_PATH
        )

    def test_plugin(self):
        self.assertIsInstance(self.eln, otio.effects_linker.EffectsLinker)
        self.assertEqual(self.eln.name, "example_effects_linker")
        self.assertEqual(self.eln.filepath, "effects_linker_example.py")
        self.assertTrue(hasattr(self.eln.module(), "link_effect"))

    def test_run_linker(self):
        clip = otio.schema.Clip(name="foo")
        effect = otio.schema.Effect(effect_name="Blur")
        linked = self.eln.link_effect(effect, clip, {"extra_data": True})
        self.assertEqual(linked.effect_id, "test.fx.blur")
        self.assertEqual(linked.context, "filter")
        self.assertEqual(linked.metadata["linked_on"], "foo")
        self.assertEqual(linked.metadata["extra_data"], True)

    def test_serialize(self):
        self.assertEqual(
            str(self.eln),
            "EffectsLinker({}, {})".format(
                repr(self.eln.name),
                repr(self.eln.filepath)
            )
        )
        self.assertEqual(
            repr(self.eln),
            "otio.effects_linker.EffectsLinker("
            "name={}, "
            "filepath={}"
            ")".format(
                repr(self.eln.name),
                repr(self.eln.filepath)
            )
        )

    def test_available_effects_linker_names(self):
        self.assertEqual(
            otio.effects_linker.available_effects_linker_names(),
            ["example_effects_linker"]
        )

    def test_from_name(self):
        self.assertEqual(
            otio.effects_linker.from_name("example_effects_linker").name,
            "example_effects_linker"
        )
        # no environment default means no linker
        self.assertIsNone(
            otio.effects_linker.from_name(
                otio.effects_linker.EffectsLinkingPolicy.ForceDefaultLinker
            )
        )
        with self.assertRaises(otio.exceptions.NotSupportedError):
            otio.effects_linker.from_name("should not exist")

    def test_default_effects_linker(self):
        os.environ['OTIO_DEFAULT_EFFECTS_LINKER'] = 'foo'
        self.assertEqual(otio.effects_linker.default_effects_linker(), 'foo')
        with self.assertRaises(otio.exceptions.NoDefaultEffectsLinkerError):
            del os.environ['OTIO_DEFAULT_EFFECTS_LINKER']
            otio.effects_linker.default_effects_linker()


class TestEffectsLinkerReadSequence(_ManifestTestCase):
    def _read(self, **kwargs):
        return otio.adapters.read_from_string(
            "tl", adapter_name="effects_example", **kwargs
        )

    def _effects(self, tl):
        track = tl.tracks[0]
        clip = track[0]
        transition = track[1]
        return track.effects, clip.effects, transition.effect

    def test_nothing_runs_by_default(self):
        tl = self._read()
        track_fx, clip_fx, trx_fx = self._effects(tl)
        self.assertEqual(track_fx[0].effect_id, "")
        self.assertEqual(clip_fx[0].effect_id, "")
        self.assertEqual(trx_fx.effect_id, "")

    def test_named_linker_visits_items_and_transitions(self):
        tl = self._read(
            effects_linker_name="example_effects_linker",
            effects_linker_argument_map={"site": "test"},
        )
        track_fx, clip_fx, trx_fx = self._effects(tl)

        self.assertEqual(track_fx[0].effect_id, "test.fx.trackglow")
        self.assertEqual(track_fx[0].metadata["linked_on"], "V1")

        self.assertEqual(clip_fx[0].effect_id, "test.fx.blur")
        self.assertEqual(clip_fx[0].metadata["site"], "test")

        self.assertEqual(trx_fx.effect_id, "test.fx.wipe")
        self.assertEqual(trx_fx.context, "transition")

    def test_none_keeps_the_effect(self):
        tl = self._read(effects_linker_name="example_effects_linker")
        _, clip_fx, _ = self._effects(tl)
        self.assertEqual(len(clip_fx), 3)
        self.assertEqual(clip_fx[1].effect_name, "Leave Me")
        self.assertEqual(clip_fx[1].effect_id, "")
        self.assertIsInstance(clip_fx[2], otio.schema.LinearTimeWarp)

    def test_environment_default(self):
        os.environ["OTIO_DEFAULT_EFFECTS_LINKER"] = "example_effects_linker"
        tl = self._read()
        _, clip_fx, _ = self._effects(tl)
        self.assertEqual(clip_fx[0].effect_id, "test.fx.blur")

    def test_do_not_link_policy(self):
        os.environ["OTIO_DEFAULT_EFFECTS_LINKER"] = "example_effects_linker"
        tl = self._read(
            effects_linker_name=(
                otio.effects_linker.EffectsLinkingPolicy.DoNotLinkEffects
            )
        )
        _, clip_fx, _ = self._effects(tl)
        self.assertEqual(clip_fx[0].effect_id, "")

    def test_post_effects_linker_hook_runs_after_linking(self):
        tl = self._read(
            effects_linker_name="example_effects_linker",
            effects_linker_argument_map={"site": "test"},
        )
        seen = tl.metadata["post_effects_linker"]
        self.assertIn("test.fx.blur", seen["effect_ids"])
        self.assertIn("test.fx.wipe", seen["effect_ids"])
        self.assertIn("test.fx.trackglow", seen["effect_ids"])
        self.assertEqual(seen["argument_map"]["site"], "test")

    def test_read_from_file(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            path = os.path.join(tmpdir, "tl.effects_example")
            with open(path, "w") as fo:
                fo.write("tl")
            tl = otio.adapters.read_from_file(
                path, effects_linker_name="example_effects_linker"
            )
        _, clip_fx, _ = self._effects(tl)
        self.assertEqual(clip_fx[0].effect_id, "test.fx.blur")
        self.assertIn(
            "test.fx.blur", tl.metadata["post_effects_linker"]["effect_ids"]
        )

    def test_link_effects_on_a_bare_clip(self):
        clip = otio.schema.Clip(name="solo")
        clip.effects.append(otio.schema.Effect(effect_name="Blur"))
        otio.effects_linker.link_effects(clip, "example_effects_linker")
        self.assertEqual(clip.effects[0].effect_id, "test.fx.blur")


class TestRegistryLinker(unittest.TestCase):
    def setUp(self):
        self.linker = otio.plugins.ActiveManifest().from_name(
            "otio_fx_registry", kind_list="effects_linkers"
        )

    def test_is_builtin_but_not_default(self):
        self.assertIn(
            "otio_fx_registry",
            otio.effects_linker.available_effects_linker_names()
        )
        self.assertIn("post_effects_linker", otio.hooks.names())

    def test_registry_document(self):
        registry = otio_fx_registry.load_registry()
        ids = [e["effect_id"] for e in registry["effects"]]
        self.assertEqual(len(ids), 17)
        self.assertIn("otio.fx.gaussian_blur", ids)
        for mapping in registry["mappings"]:
            self.assertIn(mapping["effect_id"], ids)

    def test_maps_by_name(self):
        effect = otio.schema.Effect(
            effect_name="Gaussian Blur",
            host="Premiere Pro",
            parameters={"Blurriness": 30.0},
        )
        linked = self.linker.link_effect(effect, otio.schema.Clip(), {})
        self.assertIs(linked, effect)
        self.assertEqual(linked.effect_id, "otio.fx.gaussian_blur")
        self.assertEqual(linked.effect_version, "1.0")
        self.assertEqual(linked.context, "filter")
        # host provenance is untouched
        self.assertEqual(linked.host, "Premiere Pro")
        # parameters start from the registry defaults
        self.assertEqual(linked.parameters["amount"], 0.0)
        self.assertEqual(linked.parameters["edge_mode"], "transparent")

        otio_fx = linked.metadata["otio.fx"]
        self.assertEqual(otio_fx["fidelity"], "approximate")
        self.assertEqual(otio_fx["linked_from"]["effect_id"], "")
        self.assertEqual(
            otio_fx["linked_from"]["parameters"]["Blurriness"], 30.0
        )
        self.assertEqual(otio_fx["linked_from"]["linker"], "otio_fx_registry")

    def _custom_registry(self, mappings):
        registry = dict(otio_fx_registry.load_registry())
        registry["mappings"] = mappings
        fd, path = tempfile.mkstemp(suffix=".json")
        with os.fdopen(fd, "w") as fo:
            json.dump(registry, fo)
        self.addCleanup(os.remove, path)
        return {"registry_path": path}

    def test_parameter_map_and_metadata_key(self):
        args = self._custom_registry([
            {
                "effect_id": "otio.fx.gaussian_blur",
                "match": {"effect_name": "Blur", "metadata_key": "fcp_xml"},
                "fidelity": "exact",
                "parameter_map": {
                    "amount": {"from": "radius", "scale": 0.01, "offset": 0.5},
                    "horizontal": {"from": "h"},
                    "edge_mode": {"from": "edges"},
                },
            },
        ])

        # without the metadata key, no match
        plain = otio.schema.Effect(effect_name="Blur")
        self.assertIsNone(self.linker.link_effect(plain, None, args))
        self.assertEqual(plain.effect_id, "")

        effect = otio.schema.Effect(
            effect_name="Blur",
            metadata={"fcp_xml": {"effectid": "blur"}},
            parameters={"radius": 50, "h": False, "edges": "repeat"},
        )
        linked = self.linker.link_effect(effect, None, args)
        self.assertEqual(linked.effect_id, "otio.fx.gaussian_blur")
        self.assertAlmostEqual(linked.parameters["amount"], 1.0)
        # bools are copied, not scaled
        self.assertIs(linked.parameters["horizontal"], False)
        self.assertEqual(linked.parameters["edge_mode"], "repeat")
        self.assertEqual(linked.parameters["vertical"], True)
        self.assertEqual(linked.metadata["otio.fx"]["fidelity"], "exact")
        # native metadata is left in place
        self.assertEqual(linked.metadata["fcp_xml"]["effectid"], "blur")

    def test_unknown_match_key_never_matches(self):
        args = self._custom_registry([
            {
                "effect_id": "otio.fx.crop",
                "match": {"effect_name": "Crop", "plugin_vendor": "Acme"},
            },
        ])
        effect = otio.schema.Effect(effect_name="Crop")
        self.assertIsNone(self.linker.link_effect(effect, None, args))

    def test_skips_time_effects_and_registry_effects(self):
        warp = otio.schema.LinearTimeWarp(time_scalar=2.0)
        warp.effect_name = "Gaussian Blur"
        self.assertIsNone(self.linker.link_effect(warp, None, {}))

        already = otio.schema.Effect(
            effect_name="Gaussian Blur",
            effect_id="otio.fx.gaussian_blur",
            parameters={"amount": 0.3},
        )
        self.assertIsNone(self.linker.link_effect(already, None, {}))
        self.assertEqual(already.parameters["amount"], 0.3)

    def test_unmapped_effect_is_kept(self):
        effect = otio.schema.Effect(effect_name="Lens Flare")
        self.assertIsNone(self.linker.link_effect(effect, None, {}))

    def test_through_read_sequence(self):
        tl = otio.schema.Timeline()
        track = otio.schema.Track()
        tl.tracks.append(track)
        clip = otio.schema.Clip(name="c")
        clip.effects.append(otio.schema.Effect(effect_name="Gaussian Blur"))
        track.append(clip)
        track.append(
            otio.schema.Transition(
                effect=otio.schema.Effect(effect_name="Cross Dissolve")
            )
        )
        track.append(otio.schema.Clip(name="d"))

        read = otio.adapters.read_from_string(
            otio.adapters.write_to_string(tl),
            effects_linker_name="otio_fx_registry",
        )
        self.assertEqual(
            read.tracks[0][0].effects[0].effect_id, "otio.fx.gaussian_blur"
        )
        self.assertEqual(read.tracks[0][1].effect.effect_id, "otio.fx.dissolve")
        self.assertEqual(read.tracks[0][1].effect.context, "transition")


if __name__ == '__main__':
    unittest.main()
