const std = @import("std");
const otio = @import("otio");

/// Two events, so reading it goes through the adapter, the time maths and
/// the composition walk rather than a single call into the library.
const edl =
    \\TITLE: smoke
    \\
    \\001  ShotA    V     C        01:00:00:00 01:00:01:00 00:00:00:00 00:00:01:00
    \\002  ShotB    V     C        01:00:05:00 01:00:06:00 00:00:01:00 00:00:02:00
    \\
;

pub fn main() !void {
    const document = try otio.Document.readFromBytes(.cmx3600, edl, null);
    defer document.deinit();
    const root = (try document.root()) orelse return error.Empty;
    const clips = try root.findClips(std.heap.page_allocator);
    defer std.heap.page_allocator.free(clips);
    std.debug.print("clips: {d}\n", .{clips.len});
}
