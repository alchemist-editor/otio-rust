# SPDX-License-Identifier: Apache-2.0
# Copyright Contributors to the OpenTimelineIO project

"""The adapter registry, and what the vendored adapter suites do not cover.

Upstream's adapter suites, run by `run_adapter_tests.py`, cover reading and
writing each format. What they cannot cover is the part that is new here: that
the registry answers as upstream's does without a plugin manifest behind it,
that AAF -- whose upstream suite needs pyaaf2 -- reads and writes as upstream's
adapter does, and that writing an object which lives inside a larger document
writes that object and nothing else.
"""

import contextlib
import io
import os
import pathlib
import sys
import tempfile
import unittest
import warnings

import opentimelineio as otio

CRATES = pathlib.Path(__file__).resolve().parents[3]
AAF_DATA = CRATES / "aaf" / "tests" / "data"
AAF_BASELINES = CRATES / "otio-aaf" / "tests" / "data"
AAF_WRITTEN = AAF_BASELINES / "written"

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
        self.assertTrue(aaf.has_feature("write"))
        self.assertTrue(aaf.has_feature("write_to_file"))
        self.assertFalse(aaf.has_feature("write_to_string"))
        self.assertIn("aaf", otio.adapters.suffixes_with_defined_adapters(read=True))
        self.assertIn("aaf", otio.adapters.suffixes_with_defined_adapters(write=True))


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

    def test_unknown_options_are_refused(self):
        with self.assertRaises(TypeError):
            self.read("empty.aaf", embed_essence=True)

    def test_it_bakes_keyframes_as_upstream_does(self):
        timeline = otio.adapters.read_from_file(
            str(AAF_BASELINES / "keyframed_properties.aaf"),
            bake_keyframed_properties=True,
        )
        with open(
            AAF_BASELINES / "keyframed_properties.baked.otio.json", encoding="utf-8"
        ) as f:
            expected = f.read()
        found = otio.adapters.write_to_string(timeline).splitlines()
        expected = expected.splitlines()
        self.assertEqual(len(found), len(expected))
        for want, got in zip(expected, found):
            if want == got or sys.platform == "linux":
                self.assertEqual(got, want)
                continue
            # Curves go through the platform's pow, acos and cos, which may
            # round the last place differently from the glibc the baseline
            # was made with.
            self.assertAlmostEqual(
                float(got.strip().rstrip(",")), float(want.strip().rstrip(",")), 12
            )

    def test_it_prints_the_log_upstream_prints(self):
        printed = io.StringIO()
        with contextlib.redirect_stdout(printed):
            self.read("sector_size_512.aaf", transcribe_log=True)
        with open(AAF_BASELINES / "sector_size_512.log", encoding="utf-8") as f:
            self.assertEqual(printed.getvalue(), f.read())

    def test_it_prints_nothing_unless_asked(self):
        printed = io.StringIO()
        with contextlib.redirect_stdout(printed):
            self.read("sector_size_512.aaf")
        self.assertEqual(printed.getvalue(), "")

    def test_there_are_no_string_forms(self):
        with self.assertRaises(otio.exceptions.AdapterDoesntSupportFunctionError):
            otio.adapters.write_to_string(_timeline("A"), "AAF")
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


# The upstream samples written back, whose inputs are the read baselines
# beside the samples, and the timelines the generator built, whose inputs are
# saved beside what was written from them. The same lists as
# `otio-aaf/tests/write.rs`.
WRITTEN_SAMPLES = (
    "colored_clips",
    "essence_group",
    "marker-over-transition",
    "misc_speed_effects",
    "nested_audio_dissolve",
    "nesting_test",
    "sector_size_512",
)
WRITTEN_BUILT = ("edit", "options")


@contextlib.contextmanager
def _working_directory(path):
    # `options` names an AAF to take MobIDs from by a path relative to the
    # `otio-aaf` crate, where the Rust test that shares the fixture runs.
    before = os.getcwd()
    os.chdir(path)
    try:
        yield
    finally:
        os.chdir(before)


def _written_options(name):
    """The writer's options the fixture's sidecar says it was written with."""
    options = {}
    with open(AAF_WRITTEN / f"{name}.calls.tsv", encoding="utf-8") as f:
        for line in f.read().splitlines():
            fields = line.split("\t")
            if fields[0] == "option":
                options[fields[1]] = fields[2] == "true"
    return options


def _clip(name, url):
    one_second = otio.opentime.TimeRange(
        otio.opentime.RationalTime(0, 24), otio.opentime.RationalTime(24, 24)
    )
    return otio.schema.Clip(
        name=name,
        media_reference=otio.schema.ExternalReference(
            target_url=url, available_range=one_second
        ),
        source_range=one_second,
    )


class WritingAnAaf(unittest.TestCase):
    def setUp(self):
        scratch = tempfile.TemporaryDirectory()
        self.addCleanup(scratch.cleanup)
        self.path = os.path.join(scratch.name, "cut.aaf")

    def test_every_sample_is_written_as_upstream_writes_it(self):
        # The file records when it and each mob were made and a random
        # identifier for each, so the fixture's recorded values are replayed
        # through the adapter's test hook; everything else comes through the
        # public function, options included.
        for name in WRITTEN_SAMPLES + WRITTEN_BUILT:
            with self.subTest(name=name):
                source = AAF_WRITTEN if name in WRITTEN_BUILT else AAF_BASELINES
                timeline = otio.adapters.read_from_file(
                    str(source / f"{name}.otio.json"), "otio_json"
                )
                with _working_directory(CRATES / "otio-aaf"):
                    otio.adapters.write_to_file(
                        timeline,
                        self.path,
                        _calls_tsv=AAF_WRITTEN / f"{name}.calls.tsv",
                        **_written_options(name),
                    )
                with open(self.path, "rb") as f:
                    ours = f.read()
                expected = (AAF_WRITTEN / f"{name}.aaf").read_bytes()
                self.assertEqual(len(ours), len(expected))
                self.assertTrue(ours == expected, f"{name}: the bytes differ")

    def test_a_file_written_here_reads_back(self):
        track = otio.schema.Track(name="V1")
        track.append(_clip("A", "file:///media/A.mov"))
        track.append(_clip("B", "file:///media/B.mov"))
        timeline = otio.schema.Timeline(name="Cut", tracks=[track])

        self.assertIsNone(
            otio.adapters.write_to_file(timeline, self.path, use_empty_mob_ids=True)
        )
        again = otio.adapters.read_from_file(self.path)

        self.assertIsInstance(again, otio.schema.Timeline)
        self.assertEqual(again.name, "Cut")
        self.assertEqual([clip.name for clip in again.find_clips()], ["A", "B"])
        self.assertEqual(
            again.duration(), otio.opentime.RationalTime(48, 24)
        )

    def test_a_clip_with_no_mob_id_raises_the_adapters_error(self):
        track = otio.schema.Track(name="V1")
        track.append(_clip("A", "file:///media/A.mov"))
        with self.assertRaisesRegex(
            otio.adapters.advanced_authoring_format.AAFAdapterError,
            "Cannot find mob ID",
        ):
            otio.adapters.write_to_file(
                otio.schema.Timeline(tracks=[track]), self.path
            )
        self.assertFalse(os.path.exists(self.path))

    def test_embedding_essence_is_not_implemented(self):
        with self.assertRaisesRegex(NotImplementedError, "issues/66"):
            otio.adapters.write_to_file(
                _timeline("A"), self.path, embed_essence=True
            )
        self.assertFalse(os.path.exists(self.path))

    def test_what_is_not_a_timeline_is_not_supported(self):
        with self.assertRaises(otio.exceptions.NotSupportedError):
            otio.adapters.write_to_file(_timeline("A").tracks[0], self.path)

    def test_an_option_meant_for_nothing_is_refused(self):
        with self.assertRaises(TypeError):
            otio.adapters.write_to_file(_timeline("A"), self.path, simplify=True)


if __name__ == "__main__":
    unittest.main()
