# SPDX-License-Identifier: Apache-2.0
# Copyright Contributors to the OpenTimelineIO project

"""Tests for Effect.2 and Transition.2 (effects schema proposal)."""

import json
import unittest

import opentimelineio as otio
import opentimelineio.test_utils as otio_test_utils


def _blur():
    return otio.schema.Effect(
        name="blur",
        effect_name="Gaussian Blur",
        metadata={"otio.fx": {"fidelity": "exact"}},
        effect_id="otio.fx.gaussian_blur",
        effect_version="1.0",
        host="Resolve",
        host_version="20.1",
        context="filter",
        parameters={
            "amount": 0.25,
            "edge_mode": "repeat",
            "position": {"type": "2D XY", "x": 0.5, "y": -0.25},
        },
    )


class EffectV2Test(unittest.TestCase, otio_test_utils.OTIOAssertions):
    def test_defaults(self):
        effect = otio.schema.Effect()
        self.assertEqual(effect.effect_id, "")
        self.assertEqual(effect.effect_version, "")
        self.assertEqual(effect.host, "")
        self.assertEqual(effect.host_version, "")
        self.assertEqual(effect.context, "")
        self.assertEqual(dict(effect.parameters), {})

    def test_constructor_and_setters(self):
        effect = _blur()
        self.assertEqual(effect.effect_id, "otio.fx.gaussian_blur")
        self.assertEqual(effect.effect_version, "1.0")
        self.assertEqual(effect.host, "Resolve")
        self.assertEqual(effect.host_version, "20.1")
        self.assertEqual(effect.context, "filter")
        self.assertEqual(effect.parameters["amount"], 0.25)
        self.assertEqual(effect.parameters["position"]["type"], "2D XY")

        effect.effect_id = "com.acme-vfx.lensflare"
        effect.parameters["brightness"] = 2
        self.assertEqual(effect.effect_id, "com.acme-vfx.lensflare")
        self.assertEqual(effect.parameters["brightness"], 2)

    def test_round_trip(self):
        effect = _blur()
        encoded = otio.adapters.otio_json.write_to_string(effect)
        self.assertEqual(json.loads(encoded)["OTIO_SCHEMA"], "Effect.2")
        decoded = otio.adapters.otio_json.read_from_string(encoded)
        self.assertIsOTIOEquivalentTo(effect, decoded)

    def test_read_effect_1(self):
        decoded = otio.adapters.otio_json.read_from_string(json.dumps({
            "OTIO_SCHEMA": "Effect.1",
            "name": "old",
            "effect_name": "Blur",
            "metadata": {"vendor": {"radius": 3}},
            "enabled": False,
        }))
        self.assertEqual(decoded.effect_name, "Blur")
        self.assertFalse(decoded.enabled)
        self.assertEqual(decoded.effect_id, "")
        self.assertEqual(dict(decoded.parameters), {})
        self.assertEqual(decoded.metadata["vendor"]["radius"], 3)

    def test_downgrade_stashes_fields_in_metadata(self):
        effect = _blur()
        downgraded = json.loads(
            otio.adapters.otio_json.write_to_string(
                effect, target_schema_versions={"Effect": 1}
            )
        )
        self.assertEqual(downgraded["OTIO_SCHEMA"], "Effect.1")
        for key in (
            "effect_id", "effect_version", "host", "host_version",
            "context", "parameters"
        ):
            self.assertNotIn(key, downgraded)

        otio_fx = downgraded["metadata"]["otio.fx"]
        # the existing fidelity marker survives the stash
        self.assertEqual(otio_fx["fidelity"], "exact")
        self.assertEqual(otio_fx["effect_id"], "otio.fx.gaussian_blur")
        self.assertEqual(otio_fx["host"], "Resolve")
        self.assertEqual(otio_fx["parameters"]["edge_mode"], "repeat")

    def test_downgrade_then_upgrade_is_lossless(self):
        effect = _blur()
        downgraded = otio.adapters.otio_json.write_to_string(
            effect, target_schema_versions={"Effect": 1}
        )
        upgraded = otio.adapters.otio_json.read_from_string(downgraded)
        self.assertIsOTIOEquivalentTo(effect, upgraded)
        self.assertEqual(
            dict(upgraded.metadata["otio.fx"]), {"fidelity": "exact"}
        )

    def test_downgrade_of_a_legacy_effect_adds_nothing(self):
        effect = otio.schema.Effect(name="plain", effect_name="Blur")
        downgraded = json.loads(
            otio.adapters.otio_json.write_to_string(
                effect, target_schema_versions={"Effect": 1}
            )
        )
        self.assertEqual(downgraded["metadata"], {})
        upgraded = otio.adapters.otio_json.read_from_string(
            json.dumps(downgraded)
        )
        self.assertIsOTIOEquivalentTo(effect, upgraded)

    def test_upgrade_drops_emptied_otio_fx(self):
        decoded = otio.adapters.otio_json.read_from_string(json.dumps({
            "OTIO_SCHEMA": "Effect.1",
            "name": "",
            "effect_name": "",
            "metadata": {"otio.fx": {"effect_id": "otio.fx.crop"}},
            "enabled": True,
        }))
        self.assertEqual(decoded.effect_id, "otio.fx.crop")
        self.assertNotIn("otio.fx", decoded.metadata)

    def test_subclasses_stay_at_version_1(self):
        warp = otio.schema.LinearTimeWarp(time_scalar=2.0)
        encoded = json.loads(otio.adapters.otio_json.write_to_string(warp))
        self.assertEqual(encoded["OTIO_SCHEMA"], "LinearTimeWarp.1")
        # inherited, as `enabled` was
        self.assertEqual(encoded["effect_id"], "")
        self.assertEqual(encoded["parameters"], {})


class TransitionV2Test(unittest.TestCase, otio_test_utils.OTIOAssertions):
    def _wipe(self):
        return otio.schema.Transition(
            name="Wipe to shot 12",
            transition_type=otio.schema.TransitionTypes.Custom,
            in_offset=otio.opentime.RationalTime(12, 24),
            out_offset=otio.opentime.RationalTime(12, 24),
            effect=otio.schema.Effect(
                effect_name="Wipe",
                effect_id="otio.fx.wipe",
                effect_version="1.0",
                context="transition",
                parameters={"smpte_wipe_code": 1, "reverse": False},
            ),
        )

    def test_default_effect_is_none(self):
        self.assertIsNone(otio.schema.Transition().effect)

    def test_set_and_clear_effect(self):
        trx = otio.schema.Transition()
        trx.effect = otio.schema.Effect(effect_id="otio.fx.dissolve")
        self.assertEqual(trx.effect.effect_id, "otio.fx.dissolve")
        trx.effect = None
        self.assertIsNone(trx.effect)

    def test_round_trip(self):
        trx = self._wipe()
        encoded = otio.adapters.otio_json.write_to_string(trx)
        self.assertEqual(json.loads(encoded)["OTIO_SCHEMA"], "Transition.2")
        decoded = otio.adapters.otio_json.read_from_string(encoded)
        self.assertIsOTIOEquivalentTo(trx, decoded)
        self.assertEqual(decoded.effect.parameters["smpte_wipe_code"], 1)

    def test_read_transition_1(self):
        decoded = otio.adapters.otio_json.read_from_string(json.dumps({
            "OTIO_SCHEMA": "Transition.1",
            "name": "",
            "metadata": {},
            "transition_type": "SMPTE_Dissolve",
            "in_offset": {
                "OTIO_SCHEMA": "RationalTime.1", "rate": 24, "value": 6
            },
            "out_offset": {
                "OTIO_SCHEMA": "RationalTime.1", "rate": 24, "value": 6
            },
        }))
        self.assertIsNone(decoded.effect)

    def test_downgrade_moves_effect_into_metadata(self):
        trx = self._wipe()
        downgraded = json.loads(
            otio.adapters.otio_json.write_to_string(
                trx, target_schema_versions={"Transition": 1, "Effect": 1}
            )
        )
        self.assertEqual(downgraded["OTIO_SCHEMA"], "Transition.1")
        self.assertNotIn("effect", downgraded)
        stashed = downgraded["metadata"]["otio.fx"]["effect"]
        # the stashed effect is itself downgraded
        self.assertEqual(stashed["OTIO_SCHEMA"], "Effect.1")
        self.assertEqual(
            stashed["metadata"]["otio.fx"]["effect_id"], "otio.fx.wipe"
        )

    def test_downgrade_then_upgrade_is_lossless(self):
        trx = self._wipe()
        downgraded = otio.adapters.otio_json.write_to_string(
            trx, target_schema_versions={"Transition": 1, "Effect": 1}
        )
        upgraded = otio.adapters.otio_json.read_from_string(downgraded)
        self.assertIsOTIOEquivalentTo(trx, upgraded)
        self.assertNotIn("otio.fx", upgraded.metadata)

    def test_downgrade_without_effect(self):
        trx = otio.schema.Transition(
            transition_type=otio.schema.TransitionTypes.SMPTE_Dissolve
        )
        downgraded = json.loads(
            otio.adapters.otio_json.write_to_string(
                trx, target_schema_versions={"Transition": 1}
            )
        )
        self.assertNotIn("effect", downgraded)
        self.assertEqual(downgraded["metadata"], {})

    def test_core_version_map_downgrades_both(self):
        trx = self._wipe()
        downgraded = json.loads(
            otio.adapters.otio_json.write_to_string(
                trx,
                target_schema_versions=otio.versioning.fetch_map(
                    "OTIO_CORE", "0.18.1"
                ),
            )
        )
        self.assertEqual(downgraded["OTIO_SCHEMA"], "Transition.1")
        self.assertEqual(
            downgraded["metadata"]["otio.fx"]["effect"]["OTIO_SCHEMA"],
            "Effect.1",
        )


if __name__ == '__main__':
    unittest.main()
