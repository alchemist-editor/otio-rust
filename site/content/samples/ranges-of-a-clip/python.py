import opentimelineio as otio

# Ten seconds of rushes on disk. `available_range` belongs to the media, not
# to the clip: it is what the file offers, whoever uses it.
media = otio.schema.ExternalReference(
    target_url="file:///A001.mov",
    available_range=otio.opentime.TimeRange(
        otio.opentime.RationalTime(0, 24), otio.opentime.RationalTime(240, 24)
    ),
)

# Three seconds of it, starting two seconds in. A source range is in the
# media's clock, which is why it starts at 48 rather than at 0.
clip = otio.schema.Clip(name="shot", media_reference=media)
clip.source_range = otio.opentime.TimeRange(
    otio.opentime.RationalTime(48, 24), otio.opentime.RationalTime(72, 24)
)

track = otio.schema.Track(name="V1", kind="Video")
# A second of black in front of it, so the clip does not start the track.
track.append(
    otio.schema.Gap(
        source_range=otio.opentime.TimeRange(
            otio.opentime.RationalTime(0, 24), otio.opentime.RationalTime(24, 24)
        )
    )
)
track.append(clip)


def frames(span):
    return f"{span.start_time.value} for {span.duration.value}"


# The same clip, asked four questions. The first three answer in the media's
# clock; the last answers in the track's.
print("available:", frames(clip.available_range()))
print("trimmed:  ", frames(clip.trimmed_range()))
print("visible:  ", frames(clip.visible_range()))
print("in parent:", frames(clip.range_in_parent()))
