const std = @import("std");
const otio = @import("otio");

fn frames(label: []const u8, span: otio.TimeRange) void {
    std.debug.print("{s} {d} for {d}\n", .{ label, span.start_time.value, span.duration.value });
}

pub fn main() !void {
    const document = try otio.Document.init();
    defer document.deinit();

    // Ten seconds of rushes on disk. The available range belongs to the
    // media, not to the clip: it is what the file offers, whoever uses it.
    const media = try otio.ExternalReference.init(document, "A001", "file:///A001.mov");
    try media.setAvailableRange(.{
        .start_time = .{ .value = 0, .rate = 24 },
        .duration = .{ .value = 240, .rate = 24 },
    });

    // Three seconds of it, starting two seconds in. A source range is in the
    // media's clock, which is why it starts at 48 rather than at 0.
    const clip = try otio.Clip.init(document, "shot");
    try clip.setMediaReference("DEFAULT_MEDIA", media.asNode());
    try clip.setSourceRange(.{
        .start_time = .{ .value = 48, .rate = 24 },
        .duration = .{ .value = 72, .rate = 24 },
    });

    // A second of black in front of it, so the clip does not start the track.
    const head = try otio.Gap.init(document, null);
    try head.setSourceRange(.{
        .start_time = .{ .value = 0, .rate = 24 },
        .duration = .{ .value = 24, .rate = 24 },
    });

    const track = try otio.Track.init(document, "V1", "Video");
    try track.appendChild(head.asNode());
    try track.appendChild(clip.asNode());

    // The same clip, asked four questions. The first three answer in the
    // media's clock; the last answers in the track's.
    frames("available:", try clip.availableRange());
    frames("trimmed:  ", try clip.trimmedRange());
    frames("visible:  ", try clip.visibleRange());
    frames("in parent:", try clip.rangeInParent());
}
