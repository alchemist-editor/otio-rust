import opentimelineio as otio

timeline = otio.schema.Timeline(name="Cut")
track = otio.schema.Track(name="V1", kind="Video")
timeline.tracks.append(track)

for index, name in enumerate(["A", "B", "C"]):
    clip = otio.schema.Clip(name=name)
    clip.source_range = otio.opentime.TimeRange(
        start_time=otio.opentime.RationalTime(index * 24, 24),
        duration=otio.opentime.RationalTime(24, 24),
    )
    track.append(clip)

# Three seconds of picture, written as canonical OpenTimelineIO JSON.
print(track.duration().to_seconds())
otio.adapters.otio_json.write_to_file(timeline, "cut.otio")
