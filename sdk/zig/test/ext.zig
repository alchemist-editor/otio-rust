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

/// What the server sends a viewer joining a session whose host shared one
/// timeline.
const viewer_welcome =
    \\{"v":2,"type":"welcome","sessionId":"s_1","seq":0,"ts":0,"order":1,"payload":{"selfId":"p_2","role":"viewer","roster":[{"id":"p_1","displayName":"Host","role":"host","joinedAt":0,"connectionState":"connected","muted":false}],"session":{"id":"s_1","createdAt":0},"snapshot":{"sequenceId":"main","generation":1,"otioJson":{"OTIO_SCHEMA":"Timeline.1","name":"Coastline","metadata":{"sync":{"guid":"t-1"}},"tracks":{"OTIO_SCHEMA":"Stack.1","metadata":{"sync":{"guid":"s-1"}},"children":[{"OTIO_SCHEMA":"Track.1","name":"V1","kind":"Video","metadata":{"sync":{"guid":"v-1"}},"children":[{"OTIO_SCHEMA":"Clip.2","name":"Waves","metadata":{"sync":{"guid":"c-1"}}}]}]}}}}}
;

/// What the server sends the host of a new session.
const host_welcome =
    \\{"v":2,"type":"welcome","sessionId":"s_1","seq":0,"ts":0,"order":1,"payload":{"selfId":"p_1","role":"host","roster":[],"session":{"id":"s_1","createdAt":0}}}
;

fn contains(haystack: []const u8, needle: []const u8) bool {
    return std.mem.indexOf(u8, haystack, needle) != null;
}

/// Polls `session` for frames to send until there are none, and hands back
/// how many there were.
fn drain(session: u64) !usize {
    var frames: usize = 0;
    while (try sync.sessionPollTransmit(allocator, session, 0)) |frame| {
        frame.deinit(allocator);
        frames += 1;
    }
    return frames;
}

test "a sync session joins as a viewer and hands its timeline to handle" {
    try sync.enable();
    const session = try sync.sessionNew(
        \\{"token":"join-code","identity":{"displayName":"Ada"},"encodings":[]}
    );

    // Nothing goes out until the program says the socket is open.
    try std.testing.expect((try sync.sessionPollTransmit(allocator, session, 0)) == null);
    try sync.sessionConnect(session);
    const hello = (try sync.sessionPollTransmit(allocator, session, 0)).?;
    defer hello.deinit(allocator);
    try std.testing.expect(!hello.binary);
    try std.testing.expect(contains(hello.bytes, "\"type\":\"hello\""));
    try std.testing.expect(contains(hello.bytes, "Ada"));
    try std.testing.expect((try sync.sessionPollTransmit(allocator, session, 0)) == null);

    try sync.sessionReceive(session, viewer_welcome, false);
    var events: std.ArrayList([:0]u8) = .empty;
    defer {
        for (events.items) |event| allocator.free(event);
        events.deinit(allocator);
    }
    while (try sync.sessionPollEvent(allocator, session)) |event| {
        defer allocator.free(event);
        try events.append(allocator, try allocator.dupeZ(u8, event));
    }
    try std.testing.expectEqual(@as(usize, 2), events.items.len);
    try std.testing.expect(contains(events.items[0], "\"type\":\"timeline\""));
    try std.testing.expect(contains(events.items[0], "\"joining\":true"));
    try std.testing.expect(contains(events.items[1], "\"type\":\"joined\""));

    // The program's own copy starts empty and takes the shared timeline.
    const document = try otio.Document.fromJson(
        \\{"OTIO_SCHEMA":"Timeline.1"}
    );
    defer document.deinit();
    const timeline = (try document.root()).?;
    const applied = try sync.handle(document, allocator, timeline, events.items[0]);
    defer allocator.free(applied);
    try std.testing.expect(try sameJson(applied, "{\"handled\":\"applied\"}"));
    const name = try nameOf(timeline);
    defer allocator.free(name);
    try std.testing.expectEqualStrings("Coastline", name);
    const stack = (try timeline.asTimeline().?.tracks()).?;
    try std.testing.expectEqual(@as(usize, 1), try stack.childCount());
    const track = try stack.childAt(0);
    try std.testing.expectEqual(@as(usize, 1), try track.childCount());
    const clip_name = try nameOf(try track.childAt(0));
    defer allocator.free(clip_name);
    try std.testing.expectEqualStrings("Waves", clip_name);

    const joined = try sync.handle(document, allocator, timeline, events.items[1]);
    defer allocator.free(joined);
    try std.testing.expect(try sameJson(joined, "{\"handled\":\"joined\"}"));

    const state = try sync.sessionState(allocator, session);
    defer allocator.free(state);
    try std.testing.expect(contains(state, "\"role\":\"viewer\""));
    try std.testing.expect(contains(state, "\"joined\":true"));
    const snapshot = (try sync.sessionTimeline(allocator, session, "main")).?;
    defer allocator.free(snapshot);
    try std.testing.expect(contains(snapshot, "\"sequenceId\":\"main\""));
    try std.testing.expect((try sync.sessionTimeline(allocator, session, "other")) == null);

    // A viewer may not send timelines.
    try std.testing.expect(!try sync.sessionMaySend(session, "timeline"));
    try std.testing.expectError(
        otio.Error.InvalidArgument,
        sync.sessionPublish(document, allocator, session, timeline, "main"),
    );

    try sync.sessionSetPlayhead(session, "main", 12, true, 1);
    const presence = (try sync.sessionPollTransmit(allocator, session, 0)).?;
    defer presence.deinit(allocator);
    try std.testing.expect(contains(presence.bytes, "\"type\":\"presence\""));
    _ = try drain(session);

    // Closing twice is fine, and a closed session's number names nothing.
    try sync.sessionClose(session);
    try sync.sessionClose(session);
    try std.testing.expectError(otio.Error.InvalidArgument, sync.sessionState(allocator, session));
}

test "a sync session publishes as the host" {
    try sync.enable();
    const session = try sync.sessionNew(
        \\{"token":"host-token","encodings":[]}
    );
    try sync.sessionConnect(session);
    _ = try drain(session);
    try sync.sessionReceive(session, host_welcome, false);
    const joined = (try sync.sessionPollEvent(allocator, session)).?;
    defer allocator.free(joined);
    try std.testing.expect(contains(joined, "\"type\":\"joined\""));
    try std.testing.expect(contains(joined, "\"role\":\"host\""));

    const document = try otio.Document.fromJson(cut);
    defer document.deinit();
    const timeline = (try document.root()).?;
    const first = try sync.sessionPublish(document, allocator, session, timeline, "main");
    defer allocator.free(first);
    try std.testing.expect(contains(first, "replace_timeline"));
    var snapshot_sent = false;
    while (try sync.sessionPollTransmit(allocator, session, 0)) |frame| {
        defer frame.deinit(allocator);
        if (contains(frame.bytes, "\"type\":\"snapshot\"")) snapshot_sent = true;
    }
    try std.testing.expect(snapshot_sent);

    // An unchanged timeline sends nothing.
    const again = try sync.sessionPublish(document, allocator, session, timeline, "main");
    defer allocator.free(again);
    try std.testing.expectEqualStrings("[]", again);

    try timeline.setName("fine cut");
    const renamed = try sync.sessionPublish(document, allocator, session, timeline, "main");
    defer allocator.free(renamed);
    try std.testing.expect(contains(renamed, "set_property"));
    try sync.sessionClose(session);
}

test "a sync session refuses settings it cannot honour" {
    try sync.enable();
    try std.testing.expectError(otio.Error.ParseError, sync.sessionNew(
        \\{"encodings":["zstd"]}
    ));
}

const ori = otio.ext.ori;

/// A file ORI's own Python wrote: a shot annotated at frames 10 and 4.
const ori_built = "../../extensions/ori/otio-ext-ori-annotations/tests/fixtures/ori_built.otio";

fn openOriBuilt() !*otio.Document {
    const text = try readFixture(std.Io.Dir.cwd(), ori_built);
    defer allocator.free(text);
    return otio.Document.fromJson(text);
}

test "ORI Annotations turn on with sync, and read what ORI wrote" {
    try ori.enable();
    try std.testing.expect(try ori.isEnabled());
    try std.testing.expect(try otio.isExtensionEnabled("ori-annotations"));
    try std.testing.expect(try otio.isExtensionEnabled("sync"));

    const document = try openOriBuilt();
    defer document.deinit();
    const timeline = (try document.root()).?;

    const holders = try ori.list(document, allocator, timeline);
    defer allocator.free(holders);
    try std.testing.expectEqual(@as(usize, 2), holders.len);
    const described = try ori.describe(document, allocator, holders[0]);
    defer allocator.free(described);
    try std.testing.expectEqualStrings(
        \\{"form":"track","clip_guid":"shot","clip_name":null,"frame":{"value":10.0,"rate":24.0},"note":null,"status":null,"command_count":4}
    , described);

    const track = (try ori.findTrack(document, timeline)).?;
    const name = try nameOf(track);
    defer allocator.free(name);
    try std.testing.expectEqualStrings("Annotations", name);
    try std.testing.expect(std.meta.eql((try ori.findClip(document, track, "shot", 10)).?, holders[0]));
    try std.testing.expectEqual(@as(?otio.Node, null), try ori.findClip(document, track, "shot", 99));
    const at_four = try ori.listAtFrame(document, allocator, timeline, "shot", 4);
    defer allocator.free(at_four);
    try std.testing.expectEqual(@as(usize, 1), at_four.len);
}

test "ORI Annotations annotate a frame and edit it by uuid" {
    try ori.enable();
    const built = try openOriBuilt();
    defer built.deinit();
    const built_holders = try ori.list(built, allocator, (try built.root()).?);
    defer allocator.free(built_holders);
    const commands = try ori.commands(built, allocator, built_holders[0], false);
    defer allocator.free(commands);
    const commands_z = try allocator.dupeZ(u8, commands);
    defer allocator.free(commands_z);

    const document = try otio.Document.fromJson(
        \\{"OTIO_SCHEMA": "Timeline.1", "tracks": {"OTIO_SCHEMA": "Stack.1", "children": []}}
    );
    defer document.deinit();
    const timeline = (try document.root()).?;
    const track = try ori.ensureTrack(document, timeline);
    const clip = try ori.ensureClip(document, track, "shot", 10, 24);
    try ori.addCommands(document, clip, commands_z);
    try std.testing.expect(std.meta.eql(try ori.ensureClip(document, track, "shot", 10, 24), clip));
    const back = try ori.commands(document, allocator, clip, false);
    defer allocator.free(back);
    try std.testing.expectEqualStrings(commands, back);

    try std.testing.expect(try ori.replaceCommands(
        document,
        clip,
        "stroke-1",
        \\[{"OTIO_SCHEMA": "TextAnnotation.1", "uuid": "stroke-1", "text": "was a stroke"}]
        ,
    ));
    try std.testing.expectEqual(@as(usize, 1), try ori.removeCommands(document, clip, "[\"text-1\"]"));
    const left = try ori.commands(document, allocator, clip, false);
    defer allocator.free(left);
    try std.testing.expect(std.mem.indexOf(u8, left, "\"text\":\"was a stroke\"") != null);
    try std.testing.expect(std.mem.indexOf(u8, left, "text-1") == null);
    try ori.setCommands(document, clip, "[]");
    try std.testing.expectError(otio.Error.ParseError, ori.addCommands(document, clip, "not json"));

    const corner = try ori.pixelsToOri(1920, 1080, 0, 0);
    try std.testing.expectApproxEqAbs(-8.0 / 9.0, corner.x, 1e-12);
    try std.testing.expectApproxEqAbs(0.5, corner.y, 1e-12);
    const mapped = try ori.oriToBounds(0.5, 0.5, -16, -9, 16, 9);
    try std.testing.expectEqual(@as(f64, 9), mapped.x);
    try std.testing.expectEqual(@as(f64, 9), mapped.y);
}

const ori_timeline =
    \\{"OTIO_SCHEMA":"Timeline.1","name":"Program","metadata":{"sync":{"guid":"main"}},"tracks":{"OTIO_SCHEMA":"Stack.1","metadata":{"sync":{"guid":"main/stack"}},"children":[{"OTIO_SCHEMA":"Track.1","name":"V1","kind":"Video","metadata":{"sync":{"guid":"main/v1"}},"children":[{"OTIO_SCHEMA":"Clip.2","name":"Beach Run","metadata":{"sync":{"guid":"main/clip-1"}},"source_range":{"OTIO_SCHEMA":"TimeRange.1","start_time":{"OTIO_SCHEMA":"RationalTime.1","value":0,"rate":24},"duration":{"OTIO_SCHEMA":"RationalTime.1","value":48,"rate":24}}}]}]}}
;
const ori_stroke =
    \\[{"OTIO_SCHEMA":"PaintStart.1","uuid":"s1","rgba":[1,0,0,1]},{"OTIO_SCHEMA":"PaintPoint.1","uuid":"s1","points":{"OTIO_SCHEMA":"PaintVertices.1","x":[0.1],"y":[0.2],"size":[0.01],"alpha":[1]}},{"OTIO_SCHEMA":"PaintEnd.1","uuid":"s1"}]
;
const ori_anchor =
    \\{"sequenceId":"main","capturedGeneration":1,"clipGuid":"main/clip-1","sourceTime":{"value":10,"rate":24},"capturedSequenceTime":{"value":10,"rate":24},"capturedAt":1787742000000}
;
const ori_welcome =
    \\{"v":2,"type":"welcome","sessionId":"s_1","seq":0,"ts":0,"order":1,"payload":{"selfId":"p_2","selfUserId":"u_bea","role":"viewer","roster":[],"session":{"id":"s_1","createdAt":0}}}
;
const ori_relay = "{\"type\":\"review_relay\",\"commit\":{\"clientMutationId\":\"m1\",\"anchor\":" ++ ori_anchor ++
    ",\"bodyMarkdown\":\"Too dark.\",\"annotationCommands\":" ++ ori_stroke ++
    "},\"ack\":{\"clientMutationId\":\"m1\",\"commentId\":\"c1\",\"serverOrder\":1,\"revision\":1},\"authorUserId\":\"u_bea\",\"authorPresenceId\":\"p_2\",\"createdAt\":0}";
const ori_partial =
    \\{"type":"message","kind":"Annotation.1/PARTIAL","sender":"p_3","payload":{"clip_guid":"main/clip-1","frame":10.0,"fps":24.0,"events":[{"OTIO_SCHEMA":"PaintStart.1","uuid":"s2"}]}}
;

test "ORI Annotations merge by uuid, and kind by kind" {
    try ori.enable();
    const document = try otio.Document.fromJson(ori_timeline);
    defer document.deinit();
    const timeline = (try document.root()).?;
    const clip = try ori.ensureClip(document, try ori.ensureTrack(document, timeline), "main/clip-1", 10, 24);
    try ori.setCommands(document, clip,
        \\[{"OTIO_SCHEMA":"TextAnnotation.1","uuid":"t"},{"OTIO_SCHEMA":"PaintStart.1","uuid":"a"}]
    );
    const merged = try ori.mergeCommands(document, clip,
        \\[{"OTIO_SCHEMA":"PaintStart.1","uuid":"a","brush":"gauss"},{"OTIO_SCHEMA":"PaintStart.1","uuid":"b"}]
    );
    try std.testing.expectEqual(@as(usize, 1), merged.replaced);
    try std.testing.expectEqual(@as(usize, 1), merged.added);
    try std.testing.expectEqual(@as(usize, 2), try ori.replaceByKind(document, clip,
        \\[{"OTIO_SCHEMA":"PaintStart.1","uuid":"c"}]
    ));
    const left = try ori.commands(document, allocator, clip, false);
    defer allocator.free(left);
    try std.testing.expect(contains(left, "\"uuid\":\"t\"") and contains(left, "\"uuid\":\"c\""));
    try std.testing.expectEqual(@as(usize, 0), try ori.mergeDuplicates(document, timeline));
    try std.testing.expect((try ori.ownerOf(document, allocator, clip, "t")) == null);
}

test "ORI Annotations: a reviewer commits a stroke and the host folds it in" {
    try ori.enable();
    const partial = (try ori.readPartial(allocator, ori_partial)).?;
    defer allocator.free(partial);
    try std.testing.expect(contains(partial, "\"brush\":\"circle\""));
    try std.testing.expect((try ori.readPartial(allocator, "{\"type\":\"leader\",\"leader\":null}")) == null);

    const reviewer = try sync.sessionNew(
        \\{"token":"join-code","identity":{"capabilities":["review.comment.create","review.annotation.create"]},"encodings":[]}
    );
    try sync.sessionConnect(reviewer);
    _ = try drain(reviewer);
    try sync.sessionReceive(reviewer, ori_welcome, false);
    while (try sync.sessionPollEvent(allocator, reviewer)) |event| allocator.free(event);
    const copy = try otio.Document.fromJson(ori_timeline);
    defer copy.deinit();
    const id = try ori.commit(copy, allocator, reviewer, (try copy.root()).?, ori_anchor, "Too dark.", ori_stroke, "");
    defer allocator.free(id);
    try std.testing.expect(std.mem.startsWith(u8, id, "mutation_"));
    const sent = (try sync.sessionPollTransmit(allocator, reviewer, 0)).?;
    defer sent.deinit(allocator);
    try std.testing.expect(contains(sent.bytes, "\"type\":\"review_commit\""));
    try ori.sendPartial(reviewer, "{\"clip_guid\":\"main/clip-1\",\"frame\":10,\"fps\":24,\"events\":[]}");
    try std.testing.expectError(otio.Error.Unsupported, sync.sessionPollTransmit(allocator, reviewer, 0));
    try sync.sessionClose(reviewer);

    const document = try otio.Document.fromJson(ori_timeline);
    defer document.deinit();
    const timeline = (try document.root()).?;
    const folded = try ori.fold(document, timeline, ori_relay);
    try std.testing.expect(!folded.already);
    try std.testing.expect((try ori.fold(document, timeline, ori_relay)).already);
    const commands = try ori.commands(document, allocator, folded.clip, false);
    defer allocator.free(commands);
    try std.testing.expectEqual(@as(usize, 3), std.mem.count(u8, commands, "\"uuid\":\"s1\""));
    const owner = (try ori.ownerOf(document, allocator, folded.clip, "s1")).?;
    defer allocator.free(owner);
    try std.testing.expectEqualStrings("u_bea", owner);
}
