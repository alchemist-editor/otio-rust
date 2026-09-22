import opentimelineio as otio

# An EDL never says what rate its timecode is at, so this has to be right: a
# file read at the wrong rate puts every event in the wrong place rather than
# failing.
timeline = otio.adapters.read_from_file("cut.edl", rate=24)

# Nothing happens in between. The timeline an EDL parses to is the same
# timeline FCP X writes out, so converting is a read and a write: the object
# model is the interchange, and the file formats are two ways of spelling it.
#
# Both calls pick their adapter from the suffix, as upstream picks it.
otio.adapters.write_to_file(timeline, "cut.fcpxml")
