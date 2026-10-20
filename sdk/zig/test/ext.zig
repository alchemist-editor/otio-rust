//! Tests for the extensions under `otio.ext`, which are otio-rust's own and
//! not part of OpenTimelineIO.
//!
//! Enabling an extension is process-wide and cannot be undone, and every
//! test here shares the one process, so nothing assumes one starts off.

const std = @import("std");
const otio = @import("otio");

test "enabling sync through its module turns it on for the core too" {
    try otio.ext.sync.enable();
    try std.testing.expect(try otio.ext.sync.isEnabled());
    try std.testing.expect(try otio.isExtensionEnabled("sync"));

    try std.testing.expectError(
        otio.Error.InvalidArgument,
        otio.enableExtension("no-such-extension"),
    );
    try std.testing.expect(
        std.mem.indexOf(u8, otio.lastErrorMessage(), "no-such-extension") != null,
    );
}

const allocator = std.testing.allocator;
const sync = otio.ext.sync;

/// Furnace's golden corpus, which the Rust extension's own tests read: one
/// directory per case, each holding two snapshots and the patches between
/// them.
const otiodiff_fixtures = "../../extensions/sync/otio-ext-sync/tests/fixtures/otiodiff";

/// The cases in which Furnace gives up on patches and replaces the timeline.
const falls_back = [_][]const u8{
    "generation_regression",
    "guidless_transition",
    "metadata_sync_threshold_fallback",
    "sequence_changed",
};

/// The cut the apply-patches sample opens. Nothing in it has a GUID yet.
const cut =
    \\{"OTIO_SCHEMA": "Timeline.1", "name": "cut",
    \\ "tracks": {"OTIO_SCHEMA": "Stack.1", "children": [
    \\  {"OTIO_SCHEMA": "Track.1", "name": "V1", "kind": "Video", "children": [
    \\   {"OTIO_SCHEMA": "Clip.2", "name": "A"},
    \\   {"OTIO_SCHEMA": "Clip.2", "name": "B"}]}]}}
;

fn readFixture(dir: std.Io.Dir, path: []const u8) ![:0]u8 {
    return dir.readFileAllocOptions(std.testing.io, path, allocator, .limited(1 << 20), .of(u8), 0);
}

/// The timeline a snapshot carries as `otioJson`, written back out as JSON.
fn timelineOf(snapshot: []const u8) ![:0]u8 {
    const parsed = try std.json.parseFromSlice(std.json.Value, allocator, snapshot, .{});
    defer parsed.deinit();
    const timeline = parsed.value.object.get("otioJson") orelse return error.NoTimeline;
    const text = try std.json.Stringify.valueAlloc(allocator, timeline, .{});
    defer allocator.free(text);
    return allocator.dupeZ(u8, text);
}

fn number(value: std.json.Value) ?f64 {
    return switch (value) {
        .integer => |integer| @floatFromInt(integer),
        .float => |float| float,
        .number_string => |text| std.fmt.parseFloat(f64, text) catch null,
        else => null,
    };
}

/// Whether two JSON values say the same thing: objects regardless of key
/// order, and numbers by value, so that `24` and `24.0` agree.
fn jsonEqual(a: std.json.Value, b: std.json.Value) bool {
    if (number(a)) |x| {
        const y = number(b) orelse return false;
        return x == y;
    }
    return switch (a) {
        .null => b == .null,
        .bool => |x| b == .bool and b.bool == x,
        .string => |x| b == .string and std.mem.eql(u8, x, b.string),
        .array => |x| {
            if (b != .array or b.array.items.len != x.items.len) return false;
            for (x.items, b.array.items) |left, right| {
                if (!jsonEqual(left, right)) return false;
            }
            return true;
        },
        .object => |x| {
            if (b != .object or b.object.count() != x.count()) return false;
            var entries = x.iterator();
            while (entries.next()) |entry| {
                const other = b.object.get(entry.key_ptr.*) orelse return false;
                if (!jsonEqual(entry.value_ptr.*, other)) return false;
            }
            return true;
        },
        else => false,
    };
}

fn sameJson(a: []const u8, b: []const u8) !bool {
    const left = try std.json.parseFromSlice(std.json.Value, allocator, a, .{});
    defer left.deinit();
    const right = try std.json.parseFromSlice(std.json.Value, allocator, b, .{});
    defer right.deinit();
    return jsonEqual(left.value, right.value);
}

test "sync diffs, counts and applies Furnace's golden corpus" {
    try sync.enable();
    const io = std.testing.io;
    var fixtures = try std.Io.Dir.cwd().openDir(io, otiodiff_fixtures, .{ .iterate = true });
    defer fixtures.close(io);

    var cases: usize = 0;
    var entries = fixtures.iterate();
    while (try entries.next(io)) |entry| {
        if (entry.kind != .directory) continue;
        cases += 1;
        var case = try fixtures.openDir(io, entry.name, .{});
        defer case.close(io);
        errdefer std.debug.print("in the case {s}\n", .{entry.name});

        const old = try readFixture(case, "old.json");
        defer allocator.free(old);
        const new = try readFixture(case, "new.json");
        defer allocator.free(new);
        const want = try readFixture(case, "patches.json");
        defer allocator.free(want);

        const patches = try sync.diff(allocator, old, new, true);
        defer allocator.free(patches);
        const written = try std.mem.concat(allocator, u8, &.{ patches, "\n" });
        defer allocator.free(written);
        try std.testing.expectEqualStrings(want, written);

        const stats = try sync.diffStats(allocator, old, new);
        defer allocator.free(stats);
        var fell_back = false;
        for (falls_back) |name| {
            if (std.mem.eql(u8, name, entry.name)) fell_back = true;
        }
        const expected = if (fell_back) "\"fellBack\":true" else "\"fellBack\":false";
        try std.testing.expect(std.mem.indexOf(u8, stats, expected) != null);

        const compact = try sync.diff(allocator, old, new, false);
        defer allocator.free(compact);
        const compact_z = try allocator.dupeZ(u8, compact);
        defer allocator.free(compact_z);
        const old_timeline = try timelineOf(old);
        defer allocator.free(old_timeline);
        const new_timeline = try timelineOf(new);
        defer allocator.free(new_timeline);
        const applied = try sync.apply(allocator, old_timeline, compact_z);
        defer allocator.free(applied);
        try std.testing.expect(try sameJson(applied, new_timeline));
    }
    try std.testing.expectEqual(@as(usize, 11), cases);
}

fn nameOf(node: otio.Node) ![]u8 {
    return node.name(allocator);
}

test "a sync transaction carries an edit to another copy of the timeline" {
    try sync.enable();

    const editor = try otio.Document.fromJson(cut);
    defer editor.deinit();
    const timeline = (try editor.root()).?;
    const before = try sync.begin(editor, allocator, timeline, "main", 1);
    defer allocator.free(before);
    const before_z = try allocator.dupeZ(u8, before);
    defer allocator.free(before_z);

    // The second copy starts from the stamped timeline.
    const json = try editor.toJson(allocator, 0);
    defer allocator.free(json);
    const json_z = try allocator.dupeZ(u8, json);
    defer allocator.free(json_z);
    const viewer = try otio.Document.fromJson(json_z);
    defer viewer.deinit();
    const shown = (try viewer.root()).?;

    try timeline.setName("fine cut");
    const patches = try sync.commit(editor, allocator, timeline, before_z);
    defer allocator.free(patches);
    try std.testing.expect(std.mem.indexOf(u8, patches, "\"path\":\"/name\"") != null);
    const patches_z = try allocator.dupeZ(u8, patches);
    defer allocator.free(patches_z);

    try sync.applyTo(viewer, shown, patches_z);
    const name = try nameOf(shown);
    defer allocator.free(name);
    try std.testing.expectEqualStrings("fine cut", name);

    // Both copies name the timeline by the same GUID, and stamping finds
    // nothing left to do.
    const guid = (try sync.guid(editor, allocator, timeline)).?;
    defer allocator.free(guid);
    try std.testing.expect(guid.len > 0);
    const guid_z = try allocator.dupeZ(u8, guid);
    defer allocator.free(guid_z);
    const found = (try sync.find(viewer, shown, guid_z)).?;
    try std.testing.expectEqual(shown.handle, found.handle);
    try std.testing.expect(found.belongsTo(viewer));
    try std.testing.expectEqual(@as(usize, 0), try sync.stamp(viewer, shown));
    try std.testing.expect((try sync.find(viewer, shown, "nobody")) == null);

    // An object from the other copy is turned away rather than read as
    // whatever shares its handle here.
    try std.testing.expectError(otio.Error.ForeignObject, sync.stamp(viewer, timeline));

    const fresh = try sync.newGuid(allocator);
    defer allocator.free(fresh);
    try std.testing.expect(fresh.len > 0);
    try sync.setGuid(viewer, shown, "renamed");
    const renamed = (try sync.guid(viewer, allocator, shown)).?;
    defer allocator.free(renamed);
    try std.testing.expectEqualStrings("renamed", renamed);
}

test "a sync rollback restores the timeline, and the journal records edits" {
    try sync.enable();

    const document = try otio.Document.fromJson(cut);
    defer document.deinit();
    const timeline = (try document.root()).?;
    const before = try sync.begin(document, allocator, timeline, "main", 1);
    defer allocator.free(before);
    const before_z = try allocator.dupeZ(u8, before);
    defer allocator.free(before_z);
    try timeline.setName("scrapped");
    try sync.rollback(document, timeline, before_z);
    const name = try nameOf(timeline);
    defer allocator.free(name);
    try std.testing.expectEqualStrings("cut", name);

    try sync.startJournal(document, timeline);
    try std.testing.expect(!try sync.commitJournal(document, timeline));
    try timeline.setName("journalled");
    try std.testing.expect(try sync.commitJournal(document, timeline));
    // A commit starts a fresh record.
    try std.testing.expect(!try sync.commitJournal(document, timeline));
    try sync.stopJournal(document, timeline);
}
