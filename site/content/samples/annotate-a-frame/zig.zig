const std = @import("std");
const otio = @import("otio");
const ori = otio.ext.ori;

// A cut with one shot, which sync knows by the GUID `shot`.
const cut =
    \\{"OTIO_SCHEMA": "Timeline.1", "name": "review",
    \\ "tracks": {"OTIO_SCHEMA": "Stack.1", "children": [
    \\  {"OTIO_SCHEMA": "Track.1", "name": "V1", "kind": "Video", "children": [
    \\   {"OTIO_SCHEMA": "Clip.2", "name": "shot",
    \\    "metadata": {"sync": {"guid": "shot"}}}]}]}}
;

pub fn main(init: std.process.Init) !void {
    const allocator = init.arena.allocator();

    // ORI Annotations is an extension; enabling it enables sync too.
    try ori.enable();
    const document = try otio.Document.fromJson(cut);
    defer document.deinit();
    const timeline = (try document.root()).?;

    // The Annotations track, and the clip on it for frame 10 of the shot.
    const track = try ori.ensureTrack(document, timeline);
    const frame = try ori.ensureClip(document, track, "shot", 10, 24);

    // ORI's coordinates are height-normalised with the origin at the
    // centre, so convert from the pixels of the 1920x1080 picture.
    const pixels = [_][2]f64{ .{ 200, 200 }, .{ 960, 540 }, .{ 1720, 880 } };
    var xs: [pixels.len]f64 = undefined;
    var ys: [pixels.len]f64 = undefined;
    for (pixels, 0..) |pixel, i| {
        const point = try ori.pixelsToOri(1920, 1080, pixel[0], pixel[1]);
        xs[i] = point.x;
        ys[i] = point.y;
    }

    // A stroke is a PaintStart, its points and a PaintEnd sharing one uuid.
    // Fields left out take the defaults ORI's classes give them.
    const uuid = try ori.newUuid(allocator);
    const stroke = try std.fmt.allocPrintSentinel(allocator,
        \\[{{"OTIO_SCHEMA":"PaintStart.1","uuid":"{s}","rgba":[1,0.25,0.25,1],"participant_hash":"reviewer-1"}},
        \\ {{"OTIO_SCHEMA":"PaintPoint.1","uuid":"{s}","points":{{"OTIO_SCHEMA":"PaintVertices.1",
        \\  "x":[{d},{d},{d}],"y":[{d},{d},{d}],"size":[0.01,0.01,0.01],"alpha":[1,1,1]}}}},
        \\ {{"OTIO_SCHEMA":"PaintEnd.1","uuid":"{s}"}}]
    , .{ uuid, uuid, xs[0], xs[1], xs[2], ys[0], ys[1], ys[2], uuid }, 0);
    try ori.addCommands(document, frame, stroke);

    // Read them back: every annotation on the shot.
    for (try ori.listForClip(document, allocator, timeline, "shot")) |holder| {
        std.debug.print("{s}\n", .{try ori.describe(document, allocator, holder)});
    }

    // The file is plain OpenTimelineIO with ORI's schemas in it.
    std.debug.print("{s}\n", .{try timeline.toJson(allocator, 4)});
}
