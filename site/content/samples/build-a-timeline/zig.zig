const std = @import("std");
const otio = @import("otio");

pub fn main() !void {
    // A document is an arena, and it is held in the open the way a Zig
    // programmer holds any other arena.
    const document = try otio.Document.init();
    defer document.deinit();

    const timeline = try otio.Timeline.init(document, "Cut");
    const stack = try otio.Stack.init(document, "tracks");
    const track = try otio.Track.init(document, "V1", "Video");
    try timeline.setTracks(stack.asNode());
    try stack.appendChild(track.asNode());

    for ([_][:0]const u8{ "A", "B", "C" }, 0..) |name, index| {
        const clip = try otio.Clip.init(document, name);
        try clip.setSourceRange(.{
            .start_time = .{ .value = @floatFromInt(index * 24), .rate = 24 },
            .duration = .{ .value = 24, .rate = 24 },
        });
        try track.appendChild(clip.asNode());
    }

    try document.setRoot(timeline.asNode());

    // Three seconds of picture, written as canonical OpenTimelineIO JSON.
    const duration = try track.duration();
    std.debug.print("{d}\n", .{duration.toSeconds()});
    try document.save("cut.otio");
}
