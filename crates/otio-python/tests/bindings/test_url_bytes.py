# SPDX-License-Identifier: Apache-2.0
# Copyright Contributors to the OpenTimelineIO project

"""Media URLs whose percent escapes spell bytes that are not UTF-8.

Upstream's C++ `bundle::file_from_url` decodes each `%XX` escape into a
`std::string`, which holds any bytes at all, so `file:///a%E9.mov` names the
file whose name has the single byte 0xE9 in it. What Python sees of that
depends on pybind11, which turns the `std::string` back into a `str` with
`PyUnicode_DecodeUTF8` and so raises `UnicodeDecodeError`; and the bundle
writer, which never hands the path to Python, finds the file (issue #93).
Each expectation here was taken from OpenTimelineIO itself, built with
pybind11 3.0.2.
"""

import os
import tempfile
import unittest
import zipfile

import opentimelineio as otio
from opentimelineio import opentime

RT = opentime.RationalTime
TR = opentime.TimeRange


class FilepathFromUrl(unittest.TestCase):
    def assertDecodeFails(self, url, args):
        with self.assertRaises(UnicodeDecodeError) as caught:
            otio.url_utils.filepath_from_url(url)
        self.assertEqual(caught.exception.args, args)

    def test_bytes_that_are_not_utf8_raise_unicode_decode_error(self):
        # pybind11 decodes the returned std::string as UTF-8 with no error
        # handler, so the exception is exactly the one `bytes.decode` gives,
        # reporting the decoded path and where in it the bad byte is.
        self.assertDecodeFails(
            "file:///tmp/a%E9.mov",
            ("utf-8", b"/tmp/a\xe9.mov", 6, 7, "invalid continuation byte"),
        )
        self.assertDecodeFails(
            "file:///tmp/%FF%FE",
            ("utf-8", b"/tmp/\xff\xfe", 5, 6, "invalid start byte"),
        )
        self.assertDecodeFails(
            "file://host/a%80",
            ("utf-8", b"//host/a\x80", 8, 9, "invalid start byte"),
        )
        # The escapes of a UTF-8 character decode to that character.
        self.assertEqual(
            otio.url_utils.filepath_from_url("file:///tmp/%C3%A9.mov"),
            "/tmp/é.mov",
        )
        # A bare path is not decoded at all.
        self.assertEqual(otio.url_utils.filepath_from_url("a%E9.mov"), "a%E9.mov")

    def test_bytes_are_taken_as_pybind11_takes_a_std_string(self):
        # pybind11 converts a bytes or bytearray argument to std::string as
        # it is, so the URL itself can hold bytes that are not UTF-8; the
        # result is still decoded as UTF-8 on the way back.
        file_from_url = otio._otio.bundle.file_from_url
        self.assertEqual(file_from_url(b"file:///x%41"), "/xA")
        self.assertEqual(file_from_url(bytearray(b"file:///y")), "/y")
        self.assertEqual(otio.url_utils.filepath_from_url(b"file:///x%41"), "/xA")
        with self.assertRaises(UnicodeDecodeError) as caught:
            file_from_url(b"/a\xe9")
        self.assertEqual(
            caught.exception.args,
            ("utf-8", b"/a\xe9", 2, 3, "unexpected end of data"),
        )
        # Not a file URL: None, so url_utils hands back what it was given.
        self.assertIsNone(file_from_url(b"http://x"))
        self.assertEqual(otio.url_utils.filepath_from_url(b"http://x"), b"http://x")

    def test_an_argument_pybind11_cannot_convert(self):
        # A str with a lone surrogate, as os.fsdecode spells a path that is
        # not UTF-8, cannot be encoded to UTF-8, so pybind11 finds no
        # overload for it, as for anything that is not text or bytes.
        for value in ("file:///a\udce9", 5, None):
            with self.assertRaises(TypeError) as caught:
                otio.url_utils.filepath_from_url(value)
            self.assertEqual(
                str(caught.exception),
                "file_from_url(): incompatible function arguments. The following "
                "argument types are supported:\n"
                "    1. (url: str) -> str | None\n\n"
                "Invoked with: " + repr(value),
            )


class BundleMediaNotUtf8(unittest.TestCase):
    def setUp(self):
        self.scratch = tempfile.TemporaryDirectory()
        self.addCleanup(self.scratch.cleanup)
        directory = os.fsencode(self.scratch.name)
        source = os.path.join(directory, b"caf\xe9.mov")
        try:
            with open(source, "wb") as media:
                media.write(b"not a movie")
        except OSError as error:
            # Apple's APFS, for one, refuses a name that is not UTF-8.
            self.skipTest(f"the filesystem refuses the name: {error}")

        self.timeline = otio.schema.Timeline()
        track = otio.schema.Track()
        self.timeline.tracks.append(track)
        track.append(
            otio.schema.Clip(
                name="clip",
                media_reference=otio.schema.ExternalReference(
                    target_url="file://" + self.scratch.name + "/caf%E9.mov"
                ),
                source_range=TR(RT(0, 24), RT(24, 24)),
            )
        )

    def path(self, name):
        return os.path.join(self.scratch.name, name)

    def test_the_file_is_found_and_bundled(self):
        # Upstream finds the file, since the bytes reach the filesystem as
        # they are. It then bundles it as `media/caf\xe9.mov`, in a zip
        # entry flagged as UTF-8 when it is not (so `zipfile` refuses the
        # whole archive) and writes the raw byte into content.otio, where
        # reading target_url back raises UnicodeDecodeError. That is
        # unsound, so these bindings spell the byte as the `%E9` escape it
        # came from: the file is bundled as `media/caf%E9.mov`, which the
        # reference names, and which upstream's reader also finds.
        bundled = "media/caf%E9.mov"
        for suffix in ("otioz", "otiod"):
            with self.subTest(suffix):
                path = self.path("bundle." + suffix)
                size = otio.adapters.write_to_file(self.timeline, path, dryrun=True)
                self.assertGreater(size, len(b"not a movie"))
                otio.adapters.write_to_file(self.timeline, path)
                if suffix == "otioz":
                    with zipfile.ZipFile(path) as archive:
                        self.assertEqual(archive.read(bundled), b"not a movie")
                else:
                    with open(os.path.join(path, bundled), "rb") as media:
                        self.assertEqual(media.read(), b"not a movie")
                result = otio.adapters.read_from_file(path)
                self.assertEqual(
                    result.find_clips()[0].media_reference.target_url, bundled
                )

    def test_every_policy_decodes_the_url_the_same_way(self):
        # Only the file itself is looked for; a policy that bundles no media
        # replaces the reference, keeping the URL it had.
        policies = otio._otio.bundle.MediaReferencePolicy
        path = self.path("missing.otiod")
        otio.adapters.write_to_file(
            self.timeline, path, media_policy=policies.all_missing
        )
        reference = (
            otio.adapters.read_from_file(path).find_clips()[0].media_reference
        )
        self.assertIsInstance(reference, otio.schema.MissingReference)
        self.assertEqual(
            reference.metadata["original_target_url"],
            "file://" + self.scratch.name + "/caf%E9.mov",
        )
        self.assertEqual(os.listdir(os.path.join(path, "media")), [])


if __name__ == "__main__":
    unittest.main()
