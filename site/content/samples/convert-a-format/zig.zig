const std = @import("std");
const otio = @import("otio");

pub fn main() !void {
    // An EDL never says what rate its timecode is at, so this has to be
    // right: a file read at the wrong rate puts every event in the wrong
    // place rather than failing.
    const options = otio.ReadOptions{
        .rate = 24,
        .name_column = null,
        .ignore_timecode_mismatch = false,
    };

    const document = try otio.Document.readFromFile(.cmx3600, "cut.edl", options);
    defer document.deinit();

    // Nothing happens in between. The timeline an EDL parses to is the same
    // timeline FCP X writes out, so converting is a read and a write: the
    // object model is the interchange, and the file formats are two ways of
    // spelling it.
    try document.writeToFile(.fcpx_xml, "cut.fcpxml", null);
}
