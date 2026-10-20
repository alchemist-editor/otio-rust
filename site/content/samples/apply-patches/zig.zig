const std = @import("std");
const otio = @import("otio");
const sync = otio.ext.sync;

// A cut, as an .otio file would hold it. Nothing in it has a GUID yet.
const cut =
    \\{"OTIO_SCHEMA": "Timeline.1", "name": "cut",
    \\ "tracks": {"OTIO_SCHEMA": "Stack.1", "children": [
    \\  {"OTIO_SCHEMA": "Track.1", "name": "V1", "kind": "Video", "children": [
    \\   {"OTIO_SCHEMA": "Clip.2", "name": "A"},
    \\   {"OTIO_SCHEMA": "Clip.2", "name": "B"}]}]}}
;

pub fn main() !void {
    var arena = std.heap.ArenaAllocator.init(std.heap.page_allocator);
    defer arena.deinit();
    const allocator = arena.allocator();

    try sync.enable();

    // Beginning a transaction gives every object a GUID, and writes the
    // timeline as it was as a snapshot: that snapshot is the transaction.
    // A second program starts from the same stamped timeline.
    const editor = try otio.Document.fromJson(cut);
    defer editor.deinit();
    const timeline = (try editor.root()) orelse return error.Empty;
    const before = try sync.begin(editor, allocator, timeline, "main", 1);
    const json = try editor.toJson(allocator, 0);
    const viewer = try otio.Document.fromJson(try allocator.dupeZ(u8, json));
    defer viewer.deinit();
    const shown = (try viewer.root()) orelse return error.Empty;

    // Edit as usual, then commit for the patches that say what changed.
    try timeline.setName("fine cut");
    const patches = try sync.commit(editor, allocator, timeline, try allocator.dupeZ(u8, before));
    std.debug.print("{s}\n", .{patches});

    // The other copy applies them in place: what it held still names the
    // same objects.
    try sync.applyTo(viewer, shown, try allocator.dupeZ(u8, patches));
    std.debug.print("the viewer now shows {s}\n", .{try shown.name(allocator)});
}
