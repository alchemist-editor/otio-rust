const std = @import("std");
const otio = @import("otio");

pub fn main() !void {
    var arena = std.heap.ArenaAllocator.init(std.heap.page_allocator);
    defer arena.deinit();
    const allocator = arena.allocator();

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

    const root = (try document.root()) orelse return error.Empty;

    // Anything the library hands back that has to be freed is copied into an
    // allocator you pass, and freed with `allocator.free`.
    const clips = try root.findClips(allocator);
    defer allocator.free(clips);

    for (clips) |node| {
        if (node.asClip()) |clip| {
            const name = try clip.name(allocator);
            defer allocator.free(name);
            std.debug.print("{s}\n", .{name});
        }
    }
}
