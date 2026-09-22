# SPDX-License-Identifier: Apache-2.0
# Copyright Contributors to the OpenTimelineIO project

"""The adapter registry, and what the vendored adapter suites do not cover.

Upstream's adapter suites, run by `run_adapter_tests.py`, cover reading and
writing each format. What they cannot cover is the part that is new here: that
the registry answers as upstream's does without a plugin manifest behind it,
that AAF -- whose upstream suite needs pyaaf2 and an AAF writer -- is
reachable at all, and that writing an object which lives inside a larger
document writes that object and nothing else.
"""

import os
import pathlib
import tempfile
import unittest
import warnings

import opentimelineio as otio

CRATES = pathlib.Path(__file__).resolve().parents[3]
AAF_DATA = CRATES / "aaf" / "tests" / "data"
AAF_BASELINES = CRATES / "otio-aaf" / "tests" / "data"

EDL = """TITLE: Cut
FCM: NON-DROP FRAME

001  A001     V     C        01:00:00:00 01:00:01:00 00:00:00:00 00:00:01:00
* FROM CLIP NAME:  A
002  B001     V     C        01:00:00:00 01:00:02:00 00:00:01:00 00:00:03:00
* FROM CLIP NAME:  B
"""


def _timeline(*names):
    track = otio.schema.Track(name="V1")
    for name in names:
        track.append(otio.schema.Clip(
            name=name,
            source_range=otio.opentime.TimeRange(
                otio.opentime.RationalTime(0, 24),
                otio.opentime.RationalTime(24, 24),
            ),
        ))
    return otio.schema.Timeline(name="Cut", tracks=[track])


class TheRegistry(unittest.TestCase):
    def test_every_adapter_is_listed_under_upstreams_name(self):
        self.assertEqual(
            sorted(otio.adapters.available_adapter_names()),
            sorted([
                "otio_json", "otioz", "otiod",
                "cmx_3600", "ale", "fcp_xml", "fcpx_xml", "AAF",
            ]),
        )

    def test_a_suffix_picks_its_adapter(self):
        for path, name in (
            ("cut.otio", "otio_json"),
            ("cut.edl", "cmx_3600"),
            ("CUT.EDL", "cmx_3600"),
            ("dailies.ale", "ale"),
            ("cut.xml", "fcp_xml"),
            ("cut.fcpxml", "fcpx_xml"),
            ("cut.aaf", "AAF"),
        ):
            with self.subTest(path=path):
                self.assertEqual(otio.adapters.from_filepath(path).name, name)

    def test_an_unknown_suffix_is_refused_by_type(self):
        with self.assertRaises(otio.exceptions.NoKnownAdapterForExtensionError):
            otio.adapters.from_filepath("cut.mov")

    def test_an_unknown_name_is_refused_by_type(self):
        with self.assertRaises(otio.exceptions.NotSupportedError):
            otio.adapters.from_name("rv_session")

    def test_the_manifest_answers_as_upstreams_does(self):
        manifest = otio.plugins.ActiveManifest()
        self.assertIs(
            manifest.adapter_module_from_name("cmx_3600"),
            otio.adapters.cmx_3600,
        )
        self.assertIs(
            manifest.adapter_module_from_suffix("fcpxml"),
            otio.adapters.fcpx_xml,
        )

    def test_features_are_what_each_module_defines(self):
        aaf = otio.adapters.from_name("AAF")
        self.assertTrue(aaf.has_feature("read"))
        self.assertTrue(aaf.has_feature("read_from_file"))
        self.assertFalse(aaf.has_feature("read_from_string"))
        self.assertFalse(aaf.has_feature("write"))
        self.assertIn("aaf", otio.adapters.suffixes_with_defined_adapters(read=True))
        self.assertNotIn(
            "aaf", otio.adapters.suffixes_with_defined_adapters(write=True)
        )


class ReadingAndWriting(unittest.TestCase):
    def test_an_edl_reads_through_the_top_level_functions(self):
        timeline = otio.adapters.read_from_string(EDL, "cmx_3600", rate=24)
        self.assertEqual([clip.name for clip in timeline.find_clips()], ["A", "B"])
        self.assertEqual(
            timeline.tracks[0][1].source_range.duration,
            otio.opentime.RationalTime(48, 24),
        )

    def test_a_file_is_read_by_its_suffix_and_a_path_object_will_do(self):
        with tempfile.TemporaryDirectory() as scratch:
            path = pathlib.Path(scratch) / "cut.edl"
            path.write_text(EDL, encoding="utf-8")
            timeline = otio.adapters.read_from_file(path, rate=24)
        self.assertEqual(len(timeline.find_clips()), 2)

    def test_writing_a_string_format_to_a_file_returns_the_path(self):
        with tempfile.TemporaryDirectory() as scratch:
            path = os.path.join(scratch, "cut.edl")
            self.assertEqual(
                otio.adapters.write_to_file(_timeline("A", "B"), path), path
            )
            again = otio.adapters.read_from_file(path)
        self.assertEqual([clip.name for clip in again.find_clips()], ["A", "B"])

    def test_an_option_meant_for_nothing_is_refused(self):
        with self.assertRaises(TypeError):
            otio.adapters.read_from_string(EDL, "cmx_3600", fps=24)
        with self.assertRaises(TypeError):
            otio.adapters.read_from_string("", "ale", rate=24)

    def test_each_format_raises_upstreams_exception_for_bad_input(self):
        with self.assertRaises(otio.adapters.cmx_3600.EDLParseError):
            otio.adapters.read_from_string("001  A001  V  C  nonsense", "cmx_3600")
        with self.assertRaises(otio.adapters.ale.ALEParseError):
            otio.adapters.read_from_string("Heading\nFPS\n", "ale")
        with self.assertRaises(ValueError):
            otio.adapters.read_from_string("<xmeml", "fcp_xml")
        with self.assertRaises(ValueError):
            otio.adapters.read_from_string("<fcpxml", "fcpx_xml")

    def test_an_unknown_edl_style_is_not_supported(self):
        with self.assertRaises(otio.exceptions.NotSupportedError):
            otio.adapters.write_to_string(
                _timeline("A"), "cmx_3600", style="final_cut"
            )

    def test_an_object_inside_a_timeline_is_written_on_its_own(self):
        # A track lives in its timeline's document, whose other objects the
        # writer must not see. Writing it as an ALE takes its clips alone,
        # and the timeline is untouched afterwards.
        timeline = _timeline("A", "B")
        other = otio.schema.Track(name="V2")
        other.append(otio.schema.Clip(name="C"))
        timeline.tracks.append(other)
        before = otio.adapters.write_to_string(timeline)

        ale = otio.adapters.write_to_string(timeline.tracks[1], "ale")
        clips = otio.adapters.read_from_string(ale, "ale")

        self.assertEqual([clip.name for clip in clips], ["C"])
        self.assertEqual(otio.adapters.write_to_string(timeline), before)

    def test_an_adapter_writes_objects_not_values(self):
        with self.assertRaises(TypeError):
            otio.adapters.write_to_string([1, 2], "cmx_3600")

    def test_media_linking_that_cannot_happen_is_refused(self):
        linker = otio.media_linker.MediaLinkingPolicy
        otio.adapters.read_from_string(
            EDL, "cmx_3600", media_linker_name=linker.DoNotLinkMedia
        )
        with self.assertRaises(otio.exceptions.NotSupportedError):
            otio.adapters.read_from_string(
                EDL, "cmx_3600", media_linker_name="my_studio_linker"
            )


class ReadingAnAaf(unittest.TestCase):
    def read(self, name, **options):
        return otio.adapters.read_from_file(str(AAF_DATA / name), **options)

    def test_it_matches_upstreams_structural_read_byte_for_byte(self):
        timeline = self.read(
            "sector_size_512.aaf", simplify=False, attach_markers=False
        )
        with open(
            AAF_BASELINES / "sector_size_512.structural.otio.json", encoding="utf-8"
        ) as f:
            expected = f.read()
        self.assertEqual(
            otio.adapters.write_to_string(timeline).splitlines(),
            expected.splitlines(),
        )

    def test_it_matches_upstreams_default_read_byte_for_byte(self):
        timeline = self.read("sector_size_512.aaf")
        with open(AAF_BASELINES / "sector_size_512.otio.json", encoding="utf-8") as f:
            expected = f.read()
        self.assertEqual(
            otio.adapters.write_to_string(timeline).splitlines(),
            expected.splitlines(),
        )

    def test_a_file_with_nothing_in_it_is_an_empty_collection(self):
        collection = self.read("empty.aaf", simplify=False, attach_markers=False)
        self.assertIsInstance(collection, otio.schema.SerializableCollection)
        self.assertEqual(len(collection), 0)

    def test_the_passes_follow_the_options_without_warning(self):
        with warnings.catch_warnings(record=True) as caught:
            warnings.simplefilter("always")
            simplified = self.read("sector_size_512.aaf")
            structural = self.read(
                "sector_size_512.aaf", simplify=False, attach_markers=False
            )
        self.assertEqual(caught, [])
        # Simplifying a collection of one timeline gives the timeline.
        self.assertIsInstance(simplified, otio.schema.Timeline)
        self.assertIsInstance(structural, otio.schema.SerializableCollection)

    def test_options_it_cannot_honour_are_refused(self):
        with self.assertRaises(NotImplementedError):
            self.read("empty.aaf", bake_keyframed_properties=True)
        with self.assertRaises(TypeError):
            self.read("empty.aaf", embed_essence=True)

    def test_writing_is_not_offered(self):
        with self.assertRaises(otio.exceptions.AdapterDoesntSupportFunctionError):
            otio.adapters.write_to_file(_timeline("A"), "cut.aaf")
        with self.assertRaises(otio.exceptions.AdapterDoesntSupportFunctionError):
            otio.adapters.read_from_string("", "AAF")

    def test_a_file_that_is_not_an_aaf_raises_the_adapters_error(self):
        with tempfile.TemporaryDirectory() as scratch:
            path = os.path.join(scratch, "cut.aaf")
            with open(path, "wb") as f:
                f.write(b"not a compound file")
            with self.assertRaises(
                otio.adapters.advanced_authoring_format.AAFAdapterError
            ):
                otio.adapters.read_from_file(
                    path, simplify=False, attach_markers=False
                )


if __name__ == "__main__":
    unittest.main()
