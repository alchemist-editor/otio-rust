import opentimelineio as otio

# A cut of two clips: one whose media is a file beside this script, and one
# whose media is on the web.
timeline = otio.schema.Timeline(name="Cut")
track = otio.schema.Track(name="V1", kind="Video")
timeline.tracks.append(track)
for name, url in [("A001C003", "shot.mov"), ("A001C004", "https://example.com/remote.mov")]:
    track.append(otio.schema.Clip(
        name=name,
        media_reference=otio.schema.ExternalReference(target_url=url),
    ))

# Every clip whose media is a file has the file copied into the bundle and
# its reference pointed at the copy. Media that is not a file would stop the
# write, so it is made missing instead. "cut.otiod" writes the same layout as
# a directory.
otio.adapters.write_to_file(
    timeline,
    "cut.otioz",
    media_policy=otio._otio.bundle.MediaReferencePolicy.missing_if_not_file,
)

# Unpacked, the media is ready to use beside the timeline.
bundled = otio.adapters.read_from_file("cut.otioz", extract_to_directory="cut")
for clip in bundled.find_clips():
    print(clip.name, type(clip.media_reference).__name__,
          getattr(clip.media_reference, "target_url", None))
