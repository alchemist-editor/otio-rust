const std = @import("std");
const otio = @import("otio");

pub fn main() !void {
    var arena = std.heap.ArenaAllocator.init(std.heap.page_allocator);
    defer arena.deinit();
    const allocator = arena.allocator();

    const document = try otio.Document.init();
    defer document.deinit();

    const timeline = try otio.Timeline.init(document, "Cut");
    const stack = try otio.Stack.init(document, "tracks");
    const track = try otio.Track.init(document, "V1", "Video");
    try timeline.setTracks(stack.asNode());
    try stack.appendChild(track.asNode());

    const one_second = otio.TimeRange{
        .start_time = .{ .value = 0, .rate = 24 },
        .duration = .{ .value = 24, .rate = 24 },
    };

    // An AAF clip is cut from media of a known length, so each clip's media
    // says how much of it there is. A new clip has no media at all, so its
    // reference goes in under upstream's key and is made the active one.
    inline for (.{ "A001C003", "A001C004" }) |name| {
        const url = "file:///media/" ++ name ++ ".mov";
        const media = try otio.ExternalReference.init(document, null, url);
        try media.setAvailableRange(one_second);

        const clip = try otio.Clip.init(document, name);
        try clip.setMediaReference("DEFAULT_MEDIA", media.asNode());
        try clip.setActiveMediaReferenceKey("DEFAULT_MEDIA");
        try clip.setSourceRange(one_second);
        try track.appendChild(clip.asNode());
    }

    try document.setRoot(timeline.asNode());

    // Every clip needs a MobID, from its metadata, its media's metadata or
    // the AAF its media names. A cut built from scratch has none, so let the
    // writer make them up rather than refuse the clip. Every other option
    // keeps its default.
    try document.writeToFile(.aaf, "cut.aaf", .{ .aaf_use_empty_mob_ids = true });

    const written = try otio.Document.readFromFile(.aaf, "cut.aaf", null);
    defer written.deinit();

    const root = (try written.root()) orelse return error.Empty;
    for (try root.findClips(allocator)) |node| {
        if (node.asClip()) |clip| {
            std.debug.print("{s}\n", .{try clip.name(allocator)});
        }
    }
}
