import opentimelineio as otio


def second(name):
    """One second of picture, named."""
    return otio.schema.Clip(
        name=name,
        source_range=otio.opentime.TimeRange(
            otio.opentime.RationalTime(0, 24), otio.opentime.RationalTime(24, 24)
        ),
    )


def show(track):
    names = " ".join(child.name for child in track)
    print(names, "-", track.duration().value, "frames")


track = otio.schema.Track(name="V1", kind="Video")
for name in ["A", "B", "C"]:
    track.append(second(name))
show(track)

# Insert makes room: everything from the insertion point onwards moves later,
# and the track gets longer.
otio.algorithms.insert(
    second("D"), track, otio.opentime.RationalTime(24, 24), remove_transitions=False
)
show(track)

# Overwrite does not: it lays an item over a span and whatever was in that
# span gives way. The track is the same length afterwards.
otio.algorithms.overwrite(
    second("E"),
    track,
    otio.opentime.TimeRange(
        otio.opentime.RationalTime(48, 24), otio.opentime.RationalTime(24, 24)
    ),
    remove_transitions=False,
)
show(track)
