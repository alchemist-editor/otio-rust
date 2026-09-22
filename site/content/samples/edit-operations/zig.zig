const std = @import("std");
const otio = @import("otio");

/// One second of picture, named.
fn second(document: *otio.Document, name: [:0]const u8) !otio.Clip {
    const clip = try otio.Clip.init(document, name);
    try clip.setSourceRange(.{
        .start_time = .{ .value = 0, .rate = 24 },
        .duration = .{ .value = 24, .rate = 24 },
    });
    return clip;
}

fn show(allocator: std.mem.Allocator, track: otio.Track) !void {
    // Anything the library hands back that has to be freed is copied into an
    // allocator you pass, and freed with `allocator.free`.
    const children = try track.children(allocator);
    defer allocator.free(children);

    for (children, 0..) |child, index| {
        const name = try child.name(allocator);
        defer allocator.free(name);
        if (index > 0) std.debug.print(" ", .{});
        std.debug.print("{s}", .{name});
    }

    const duration = try track.duration();
    std.debug.print(" - {d} frames\n", .{duration.value});
}

pub fn main() !void {
    var arena = std.heap.ArenaAllocator.init(std.heap.page_allocator);
    defer arena.deinit();
    const allocator = arena.allocator();

    const document = try otio.Document.init();
    defer document.deinit();

    const track = try otio.Track.init(document, "V1", "Video");
    for ([_][:0]const u8{ "A", "B", "C" }) |name| {
        try track.appendChild((try second(document, name)).asNode());
    }
    try show(allocator, track);

    // Insert makes room: everything from the insertion point onwards moves
    // later, and the track gets longer.
    const inserted = try second(document, "D");
    try document.insert(inserted.asNode(), track.asNode(), .{ .value = 24, .rate = 24 }, false, null);
    try show(allocator, track);

    // Overwrite does not: it lays an item over a span and whatever was in
    // that span gives way. The track is the same length afterwards.
    const laid = try second(document, "E");
    try document.overwrite(laid.asNode(), track.asNode(), .{
        .start_time = .{ .value = 48, .rate = 24 },
        .duration = .{ .value = 24, .rate = 24 },
    }, false, null);
    try show(allocator, track);
}
