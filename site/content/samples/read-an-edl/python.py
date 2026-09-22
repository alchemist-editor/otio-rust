import opentimelineio as otio

# An EDL never says what rate its timecode is at, so this has to be right: a
# file read at the wrong rate puts every event in the wrong place rather than
# failing. The adapter is picked from the suffix, as upstream picks it.
timeline = otio.adapters.read_from_file("cut.edl", rate=24)

for clip in timeline.find_clips():
    print(clip.name)
