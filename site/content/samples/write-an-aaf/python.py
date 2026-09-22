import opentimelineio as otio

# An AAF clip is cut from media of a known length, so each clip's media says
# how much of it there is. A cut read from an AAF already carries that, and
# what the reader found under metadata["AAF"], and both are written back.
timeline = otio.schema.Timeline(name="Cut")
track = otio.schema.Track(name="V1", kind="Video")
timeline.tracks.append(track)

for name in ["A001C003", "A001C004"]:
    one_second = otio.opentime.TimeRange(
        start_time=otio.opentime.RationalTime(0, 24),
        duration=otio.opentime.RationalTime(24, 24),
    )
    track.append(otio.schema.Clip(
        name=name,
        media_reference=otio.schema.ExternalReference(
            target_url=f"file:///media/{name}.mov",
            available_range=one_second,
        ),
        source_range=one_second,
    ))

# Every clip needs a MobID, from its metadata, its media's metadata or the
# AAF its media names. A cut built from scratch has none, so let the writer
# make them up rather than refuse the clip.
otio.adapters.write_to_file(timeline, "cut.aaf", use_empty_mob_ids=True)

print([clip.name for clip in otio.adapters.read_from_file("cut.aaf").find_clips()])
