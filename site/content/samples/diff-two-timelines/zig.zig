const std = @import("std");
const otio = @import("otio");
const sync = otio.ext.sync;

// Two versions of one sequence, as Furnace sends them: in the second, clip B
// has moved in front of clip A. Every object carries its GUID in
// `metadata.sync.guid`, which is how a patch names it.
const before =
    \\{"sequenceId": "main", "generation": 1, "otioJson": {
    \\  "OTIO_SCHEMA": "Timeline.1", "name": "cut", "metadata": {"sync": {"guid": "cut"}},
    \\  "tracks": {"OTIO_SCHEMA": "Stack.1", "metadata": {"sync": {"guid": "tracks"}}, "children": [
    \\    {"OTIO_SCHEMA": "Track.1", "name": "V1", "kind": "Video", "metadata": {"sync": {"guid": "v1"}},
    \\     "children": [
    \\       {"OTIO_SCHEMA": "Clip.2", "name": "A", "metadata": {"sync": {"guid": "a"}}},
    \\       {"OTIO_SCHEMA": "Clip.2", "name": "B", "metadata": {"sync": {"guid": "b"}}}]}]}}}
;

const after =
    \\{"sequenceId": "main", "generation": 2, "otioJson": {
    \\  "OTIO_SCHEMA": "Timeline.1", "name": "cut", "metadata": {"sync": {"guid": "cut"}},
    \\  "tracks": {"OTIO_SCHEMA": "Stack.1", "metadata": {"sync": {"guid": "tracks"}}, "children": [
    \\    {"OTIO_SCHEMA": "Track.1", "name": "V1", "kind": "Video", "metadata": {"sync": {"guid": "v1"}},
    \\     "children": [
    \\       {"OTIO_SCHEMA": "Clip.2", "name": "B", "metadata": {"sync": {"guid": "b"}}},
    \\       {"OTIO_SCHEMA": "Clip.2", "name": "A", "metadata": {"sync": {"guid": "a"}}}]}]}}}
;

pub fn main() !void {
    var arena = std.heap.ArenaAllocator.init(std.heap.page_allocator);
    defer arena.deinit();
    const allocator = arena.allocator();

    try sync.enable();

    const patches = try sync.diff(allocator, before, after, true);
    const stats = try std.json.parseFromSliceLeaky(
        struct { fellBack: bool },
        allocator,
        try sync.diffStats(allocator, before, after),
        .{ .ignore_unknown_fields = true },
    );

    // One move, of A to after B: a patch names objects by GUID.
    std.debug.print("{s}\n", .{patches});
    std.debug.print("fell back to the whole timeline: {}\n", .{stats.fellBack});
}
