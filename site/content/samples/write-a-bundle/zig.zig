const std = @import("std");
const otio = @import("otio");

pub fn main() !void {
    var arena = std.heap.ArenaAllocator.init(std.heap.page_allocator);
    defer arena.deinit();
    const allocator = arena.allocator();

    const document = try otio.Document.init();
    defer document.deinit();

    const timeline = try otio.Timeline.init(document, "Cut");
    const stack = ((try timeline.tracks()) orelse return error.Empty).asStack().?;
    const track = try otio.Track.init(document, "V1", "Video");
    try stack.appendChild(track.asNode());

    // A cut of two clips: one whose media is a file beside the program, and
    // one whose media is on the web.
    inline for (.{
        .{ "A001C003", "shot.mov" },
        .{ "A001C004", "https://example.com/remote.mov" },
    }) |source| {
        const media = try otio.ExternalReference.init(document, null, source[1]);
        const clip = try otio.Clip.init(document, source[0]);
        try clip.setMediaReference("DEFAULT_MEDIA", media.asNode());
        try clip.setActiveMediaReferenceKey("DEFAULT_MEDIA");
        try track.appendChild(clip.asNode());
    }

    // A bundle holds a timeline, and what is written is the document's root.
    try document.setRoot(timeline.asNode());

    // Every clip whose media is a file has the file copied into the bundle
    // and its reference pointed at the copy. Media that is not a file would
    // stop the write, so it is made missing instead. `.otiod` writes the same
    // layout as a directory.
    try document.writeToFile(.otioz, "cut.otioz", .{ .bundle_media_policy = .missing_if_not_file });

    // Unpacked, with each reference made absolute, the media is ready to use.
    const bundled = try otio.Document.readFromFile(.otioz, "cut.otioz", .{
        .bundle_extract_path = "cut",
        .bundle_absolute_media_paths = true,
    });
    defer bundled.deinit();

    const root = (try bundled.root()) orelse return error.Empty;
    for (try root.findClips(allocator)) |node| {
        const media = (try node.asClip().?.mediaReference(null)) orelse continue;
        if (media.asExternalReference()) |external| {
            std.debug.print("{s}\n", .{try external.targetUrl(allocator)});
        } else {
            std.debug.print("missing\n", .{});
        }
    }
}
