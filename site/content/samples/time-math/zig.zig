const std = @import("std");
const otio = @import("otio");

pub fn main() !void {
    var arena = std.heap.ArenaAllocator.init(std.heap.page_allocator);
    defer arena.deinit();
    const allocator = arena.allocator();

    // A time is a value and a rate, not a number of seconds. Four seconds at
    // 24 is 96 units; the rate travels with it so nothing has to guess later.
    const start = try otio.RationalTime.fromTimecode("01:00:00:00", 24);
    const duration = otio.RationalTime.fromFrames(96, 24);

    const end = start.add(duration);

    // A call that has to allocate takes the allocator first, which is the
    // only signal you need that something has to be freed.
    const timecode = try end.toTimecode(allocator);
    defer allocator.free(timecode);

    std.debug.print("{s} for {d} seconds\n", .{ timecode, duration.toSeconds() });

    // Comparison rescales first, so the same instant at two rates is equal.
    const a = otio.RationalTime{ .value = 24, .rate = 24 };
    const b = otio.RationalTime{ .value = 48, .rate = 48 };
    std.debug.assert(a.equals(b));
}
