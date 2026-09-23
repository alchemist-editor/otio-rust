//! Tests for the generated Zig SDK.
//!
//! These are written by hand, not generated. A generator that also wrote its
//! own tests would only prove it is self-consistent; what needs proving is
//! that the Zig it writes does what a Zig programmer reading it would
//! expect, against the same library and the same fixtures the Rust tests
//! use.

const std = @import("std");
const otio = @import("otio");

const allocator = std.testing.allocator;

/// The EDL the Rust adapter's own tests read, so that the two agree about
/// what is in it.
const screening_edl = "../../crates/otio-cmx3600/tests/data/screening_example.edl";

/// An AAF the Rust adapter's tests read, whose five clips each carry the
/// MobID of the media they were cut from.
const colored_clips_aaf = "../../crates/otio-aaf/tests/data/colored_clips.aaf";

/// References every declaration in the package, and in the types it holds.
///
/// Zig only analyses what is reached, so without this a generated call that
/// does not compile would go unnoticed until somebody called it. The
/// standard library's own `refAllDecls` stops at one level; this walks down.
fn referenceEverything(comptime T: type) void {
    inline for (comptime std.meta.declarations(T)) |decl| {
        const held = @field(T, decl.name);
        if (@TypeOf(held) == type) {
            switch (@typeInfo(held)) {
                .@"struct", .@"enum", .@"union", .@"opaque" => referenceEverything(held),
                else => {},
            }
        }
        _ = &@field(T, decl.name);
    }
}

// The conformance scenarios, which the generator renders from the data in
// crates/otio-sdk-model into a file of their own. Importing that file from a
// test reaches every test in it, so a scenario added there runs here without
// being listed.
test {
    _ = @import("conformance.zig");
}

test "every generated declaration compiles" {
    referenceEverything(otio);
}

test "the version is reported" {
    try std.testing.expect(otio.version().len > 0);
}

test "reading an EDL finds its clips" {
    const document = try otio.Document.readFromFile(.cmx3600, screening_edl, null);
    defer document.deinit();

    const root = (try document.root()) orelse return error.TestUnexpectedResult;
    const clips = try root.findClips(allocator);
    defer allocator.free(clips);
    try std.testing.expectEqual(@as(usize, 9), clips.len);

    // Every one of them really is a clip, and says so.
    for (clips) |node| {
        try std.testing.expect(node.asClip() != null);
    }
}

test "open works out the format from the name" {
    const document = try otio.open(screening_edl);
    defer document.deinit();

    const root = (try document.root()) orelse return error.TestUnexpectedResult;
    const name = try root.name(allocator);
    defer allocator.free(name);
    try std.testing.expect(std.mem.indexOf(u8, name, "Example_Screening") != null);
}

test "open declines a suffix no format claims" {
    try std.testing.expectError(error.NoValue, otio.open("somewhere/cut.wav"));
}

test "building a timeline from nothing" {
    const document = try otio.Document.init();
    defer document.deinit();

    const timeline = try otio.Timeline.init(document, "Cut");
    const stack = try otio.Stack.init(document, "tracks");
    const track = try otio.Track.init(document, "V1", "Video");
    try timeline.setTracks(stack.asNode());
    try stack.appendChild(track.asNode());

    const rate = 24.0;
    for ([_][:0]const u8{ "A", "B", "C" }) |name| {
        const clip = try otio.Clip.init(document, name);
        try clip.setSourceRange(.{
            .start_time = .{ .value = 0, .rate = rate },
            .duration = .{ .value = 12, .rate = rate },
        });
        try track.appendChild(clip.asNode());
    }

    try std.testing.expectEqual(@as(usize, 3), try track.childCount());

    const duration = try track.duration();
    try std.testing.expectEqual(@as(f64, 36), duration.value);
    try std.testing.expectEqual(rate, duration.rate);

    // The kind a track carries is its own, not the schema's.
    const kind = try track.kind(allocator);
    defer allocator.free(kind);
    try std.testing.expectEqualStrings("Video", kind);
    try std.testing.expectEqual(otio.NodeKind.track, try track.schemaKind());
}

test "asking an object for something it is not fails loudly" {
    const document = try otio.Document.init();
    defer document.deinit();

    const clip = try otio.Clip.init(document, "A");

    // A clip is not a track, and the library says so rather than handing
    // back an empty string.
    const pretend = otio.Track{ .node = clip.asNode() };
    try std.testing.expectError(error.CoreError, pretend.kind(allocator));
    try std.testing.expect(std.mem.indexOf(u8, otio.lastErrorMessage(), "not a track") != null);

    // And the checked conversion declines rather than building one.
    try std.testing.expect(clip.asNode().asTrack() == null);
}

/// What each thread of the test below counts, shared between all of them.
const Tally = struct {
    /// Failures that came back with the wrong error or someone else's
    /// message.
    wrong: std.atomic.Value(u32) = .init(0),
    /// Failures that came back as they should, so the test can tell that
    /// every thread really did run.
    right: std.atomic.Value(u32) = .init(0),
};

/// How many threads fail at once, and how many times each.
const failing_threads = 16;
const failures_per_thread = 200;

/// One thread's share of the test below: fail two different ways, in turn,
/// yielding between each call and the check, and count whether every
/// failure came back with its own message.
fn failInTurn(track: otio.Track, tally: *Tally) void {
    for (0..failures_per_thread) |round| {
        const right = if (round % 2 == 0) badTimecode() else notATrack(track);
        _ = (if (right) &tally.right else &tally.wrong).fetchAdd(1, .monotonic);
    }
}

/// A timecode that is not one fails as a time error, with a sentence that
/// is about the timecode and not about a track.
fn badTimecode() bool {
    const answer = otio.RationalTime.fromTimecode("nonsense", 24);
    std.Thread.yield() catch {};
    if (answer != error.TimeError) return false;
    const message = otio.lastErrorMessage();
    return message.len > 0 and std.mem.indexOf(u8, message, "not a track") == null;
}

/// A clip asked for a track's kind fails as a core error, with a sentence
/// that says it is not a track.
fn notATrack(track: otio.Track) bool {
    const answer = track.kind(std.heap.smp_allocator);
    std.Thread.yield() catch {};
    if (answer) |kind| {
        std.heap.smp_allocator.free(kind);
        return false;
    } else |failure| if (failure != error.CoreError) return false;
    return std.mem.indexOf(u8, otio.lastErrorMessage(), "not a track") != null;
}

test "every failure carries its own message whatever thread it ran on" {
    // Each call hands its message back beside its status, and the package
    // keeps it for the thread that made the call. Many threads failing in
    // two different ways at once, and yielding between the call and the
    // check, must each still read the sentence their own call wrote. The
    // document is shared, which the library allows for threads that only
    // read, as all of these do.
    const document = try otio.Document.init();
    defer document.deinit();
    const clip = try otio.Clip.init(document, "A");
    const pretend = otio.Track{ .node = clip.asNode() };

    var tally: Tally = .{};
    var threads: [failing_threads]std.Thread = undefined;
    for (&threads) |*thread| {
        thread.* = try std.Thread.spawn(.{}, failInTurn, .{ pretend, &tally });
    }
    for (threads) |thread| thread.join();

    try std.testing.expectEqual(@as(u32, 0), tally.wrong.load(.monotonic));
    try std.testing.expectEqual(
        @as(u32, failing_threads * failures_per_thread),
        tally.right.load(.monotonic),
    );
}

test "an object of no document fails rather than crashing" {
    const orphan = otio.Node{ .doc = null, .handle = .{ .index = 0, .generation = 0 } };
    try std.testing.expectError(error.NullPointer, orphan.name(allocator));
    const clip = otio.Clip{ .node = orphan };
    try std.testing.expectError(error.NullPointer, clip.sourceRange());
}

test "a stale handle is refused" {
    const document = try otio.Document.init();
    defer document.deinit();

    const clip = try otio.Clip.init(document, "doomed");
    try document.removeNode(clip.asNode());
    try std.testing.expectError(error.StaleHandle, clip.name(allocator));
}

test "no value is an answer and not a failure" {
    const document = try otio.Document.init();
    defer document.deinit();

    const clip = try otio.Clip.init(document, "untrimmed");
    try std.testing.expect((try clip.sourceRange()) == null);

    const span = otio.TimeRange{
        .start_time = .{ .value = 5, .rate = 24 },
        .duration = .{ .value = 10, .rate = 24 },
    };
    try clip.setSourceRange(span);
    const got = (try clip.sourceRange()) orelse return error.TestUnexpectedResult;
    try std.testing.expectEqual(span, got);
}

test "time values compute without a document" {
    const first = otio.RationalTime.fromFrames(24, 24);
    try std.testing.expectEqual(@as(f64, 1), first.toSeconds());

    const timecode = try first.toTimecode(allocator);
    defer allocator.free(timecode);
    try std.testing.expectEqualStrings("00:00:01:00", timecode);

    const again = try otio.RationalTime.fromTimecode("00:00:01:00", 24);
    try std.testing.expect(again.equals(first));

    const sum = first.add(otio.RationalTime.fromFrames(12, 24));
    try std.testing.expectEqual(@as(f64, 36), sum.value);

    const span = otio.TimeRange.fromStartEndTime(first, sum);
    try std.testing.expectEqual(@as(f64, 12), span.duration.value);
    try std.testing.expect(span.containsTime(otio.RationalTime.fromFrames(30, 24)));

    // 29.97 written out is not the rate; 30000/1001 is, and the library says
    // which of the SMPTE rates a written-out one meant.
    const nearest = otio.nearestSmpteTimecodeRate(29.97);
    try std.testing.expect(otio.isDropFrameRate(nearest));
    try std.testing.expect(otio.isSmpteTimecodeRate(24));
    try std.testing.expect(!otio.isDropFrameRate(24));
}

test "metadata goes in and comes back" {
    const document = try otio.Document.init();
    defer document.deinit();

    const clip = try otio.Clip.init(document, "A");
    const metadata = clip.metadata();
    // A path separates its steps with dots, so this writes a reel inside a
    // cmx_3600 dictionary rather than one key with a funny name. The path is
    // followed rather than created, so the dictionary has to exist first.
    try metadata.setDictionary("cmx_3600");
    try metadata.setString("cmx_3600.reel", "ZZ100");
    try metadata.setInt("take", 3);

    const reel = try metadata.getString(allocator, "cmx_3600.reel");
    defer allocator.free(reel);
    try std.testing.expectEqualStrings("ZZ100", reel);

    // Reading it back by its steps is not enough on its own: a single key
    // literally named "cmx_3600.reel" would answer the same. What proves the
    // dictionary is really nested is that cmx_3600 is a dictionary of one.
    try std.testing.expectEqual(otio.ValueKind.dictionary, (try metadata.kind("cmx_3600")).?);
    try std.testing.expectEqual(@as(usize, 1), (try metadata.len("cmx_3600")).?);

    const key = (try metadata.keyAt(allocator, "cmx_3600", 0)) orelse
        return error.TestUnexpectedResult;
    defer allocator.free(key);
    try std.testing.expectEqualStrings("reel", key);

    try std.testing.expectEqual(@as(i64, 3), try metadata.getInt("take"));
    try std.testing.expectEqual(otio.ValueKind.int, (try metadata.kind("take")).?);
    try std.testing.expect(!try metadata.contains("nothing"));

    // Removing something that was never there is an answer, not a failure.
    try std.testing.expect(!try metadata.remove("nothing"));
    try std.testing.expect(try metadata.remove("take"));
}

test "clearing children hands them all back" {
    const document = try otio.Document.init();
    defer document.deinit();

    const track = try otio.Track.init(document, "V1", "Video");
    const names = [_][:0]const u8{ "A", "B", "C", "D" };
    for (names) |name| {
        const clip = try otio.Clip.init(document, name);
        try track.appendChild(clip.asNode());
    }

    // This one empties as it answers, so the generated call cannot ask
    // twice. If it did, it would come back with nothing.
    const removed = try track.clearChildren(allocator);
    defer allocator.free(removed);
    try std.testing.expectEqual(names.len, removed.len);
    for (removed, names) |node, expected| {
        const name = try node.name(allocator);
        defer allocator.free(name);
        try std.testing.expectEqualStrings(expected, name);
    }
    try std.testing.expectEqual(@as(usize, 0), try track.childCount());
}

test "every child and its range come back together" {
    const document = try otio.Document.init();
    defer document.deinit();

    const track = try otio.Track.init(document, "V1", "Video");
    for (0..3) |_| {
        const clip = try otio.Clip.init(document, null);
        try clip.setSourceRange(.{
            .start_time = .{ .value = 0, .rate = 24 },
            .duration = .{ .value = 10, .rate = 24 },
        });
        try track.appendChild(clip.asNode());
    }

    // Two lists filled in step, which is one call and not two.
    const both = try track.rangesOfChildren(allocator);
    defer both.deinit(allocator);
    try std.testing.expectEqual(@as(usize, 3), both.nodes.len);
    try std.testing.expectEqual(@as(usize, 3), both.ranges.len);
    for (both.ranges, 0..) |span, index| {
        try std.testing.expectEqual(@as(f64, @floatFromInt(index * 10)), span.start_time.value);
    }
}

test "an edit operation changes the timeline" {
    const document = try otio.Document.init();
    defer document.deinit();

    const track = try otio.Track.init(document, "V1", "Video");
    for (0..3) |_| {
        const clip = try otio.Clip.init(document, null);
        try clip.setSourceRange(.{
            .start_time = .{ .value = 0, .rate = 24 },
            .duration = .{ .value = 24, .rate = 24 },
        });
        try track.appendChild(clip.asNode());
    }
    const before = try track.childCount();

    // Cut the second clip in two, which makes one more child than there was.
    try document.slice(track.asNode(), .{ .value = 36, .rate = 24 }, true);
    try std.testing.expectEqual(before + 1, try track.childCount());

    // The cut lands where it was asked to.
    const span = try track.rangeOfChildAtIndex(1);
    try std.testing.expectEqual(@as(f64, 24), span.start_time.value);
    try std.testing.expectEqual(@as(f64, 12), span.duration.value);
}

test "a freshly built object is enabled" {
    const document = try otio.Document.init();
    defer document.deinit();

    try std.testing.expect(try (try otio.Clip.init(document, "clip")).enabled());
    try std.testing.expect(try (try otio.Stack.init(document, "stack")).enabled());
    try std.testing.expect(try (try otio.Track.init(document, "track", "Video")).enabled());
}

test "a document survives a round trip through JSON" {
    const document = try otio.open(screening_edl);
    defer document.deinit();

    const text = try document.toJson(allocator, 2);
    defer allocator.free(text);
    try std.testing.expect(std.mem.indexOf(u8, text, "\"OTIO_SCHEMA\"") != null);

    const terminated = try allocator.dupeZ(u8, text);
    defer allocator.free(terminated);
    const again = try otio.Document.fromJson(terminated);
    defer again.deinit();

    const round = try again.toJson(allocator, 2);
    defer allocator.free(round);
    try std.testing.expectEqualStrings(text, round);
}

test "saving and opening again keeps the clips" {
    const document = try otio.open(screening_edl);
    defer document.deinit();

    var temporary = std.testing.tmpDir(.{});
    defer temporary.cleanup();
    const path = try std.fmt.allocPrintSentinel(
        allocator,
        ".zig-cache/tmp/{s}/round-trip.otio",
        .{temporary.sub_path},
        0,
    );
    defer allocator.free(path);

    try document.save(path);
    const again = try otio.open(path);
    defer again.deinit();

    const root = (try again.root()) orelse return error.TestUnexpectedResult;
    const clips = try root.findClips(allocator);
    defer allocator.free(clips);
    try std.testing.expectEqual(@as(usize, 9), clips.len);
}

test "an AAF reads and writes back out" {
    const document = try otio.Document.readFromFile(.aaf, colored_clips_aaf, null);
    defer document.deinit();
    try std.testing.expectEqual(@as(usize, 5), try clipCount(document));

    // A cut read from an AAF keeps each clip's MobID, so it writes back out
    // with no leave to make any up.
    const written = try document.writeToBytes(allocator, .aaf, null);
    defer allocator.free(written);
    const again = try otio.Document.readFromBytes(.aaf, written, null);
    defer again.deinit();
    try std.testing.expectEqual(@as(usize, 5), try clipCount(again));

    // A fixed time and seed write the same file twice.
    const fixed = otio.WriteOptions{ .aaf_time = 1714979289, .aaf_id_seed = 59 };
    const first = try document.writeToBytes(allocator, .aaf, fixed);
    defer allocator.free(first);
    const second = try document.writeToBytes(allocator, .aaf, fixed);
    defer allocator.free(second);
    try std.testing.expectEqualSlices(u8, first, second);

    const nested = try otio.Document.readFromFile(.aaf, colored_clips_aaf, .{ .aaf_keep_nesting = true });
    defer nested.deinit();
    try std.testing.expectEqual(@as(usize, 5), try clipCount(nested));
    try std.testing.expectEqualStrings("AAF", otio.Format.aaf.name());
}

test "a bundle carries its media with it" {
    var temporary = std.testing.tmpDir(.{});
    defer temporary.cleanup();
    const dir = try std.fmt.allocPrintSentinel(allocator, ".zig-cache/tmp/{s}", .{temporary.sub_path}, 0);
    defer allocator.free(dir);
    try temporary.dir.writeFile(std.testing.io, .{ .sub_path = "shot.mov", .data = "not really a movie" });

    // A cut of two clips: one whose media is a file, named relative to the
    // directory it is in, and one whose media is on the web.
    const document = try otio.Document.init();
    defer document.deinit();
    const timeline = try otio.Timeline.init(document, "bundled");
    try document.setRoot(timeline.asNode());
    const stack = ((try timeline.tracks()) orelse return error.TestUnexpectedResult).asStack().?;
    const track = try otio.Track.init(document, "V1", "Video");
    try stack.appendChild(track.asNode());
    const sources = [_][2][:0]const u8{
        .{ "local", "shot.mov" },
        .{ "remote", "https://example.com/remote.mov" },
    };
    for (sources) |source| {
        const clip = try otio.Clip.init(document, source[0]);
        const reference = try otio.ExternalReference.init(document, source[0], source[1]);
        try clip.setMediaReference("DEFAULT_MEDIA", reference.asNode());
        try clip.setActiveMediaReferenceKey("DEFAULT_MEDIA");
        try track.appendChild(clip.asNode());
    }

    // Upstream's default refuses media that is not a file.
    var options = otio.WriteOptions{ .bundle_media_base_dir = dir };
    const refused = try std.fmt.allocPrintSentinel(allocator, "{s}/refused.otioz", .{dir}, 0);
    defer allocator.free(refused);
    try std.testing.expectError(error.IoError, document.writeToFile(.otioz, refused, options));

    options.bundle_media_policy = .missing_if_not_file;
    for ([_]otio.Format{ .otioz, .otiod }) |format| {
        const path = try std.fmt.allocPrintSentinel(allocator, "{s}/cut.{s}", .{ dir, format.name() }, 0);
        defer allocator.free(path);
        try document.writeToFile(format, path, options);

        // Read as it is, the file's reference points into the bundle and
        // the web one is missing.
        const plain = try otio.open(path);
        defer plain.deinit();
        const media = try activeMedia(plain);
        const url = try media[0].asExternalReference().?.targetUrl(allocator);
        defer allocator.free(url);
        try std.testing.expectEqualStrings("media/shot.mov", url);
        try std.testing.expect(media[1].asMissingReference() != null);

        // With absolute paths, it points at a real copy of the media, which
        // an .otioz has to be unpacked to have.
        const unpacked = if (format == .otioz)
            try std.fmt.allocPrintSentinel(allocator, "{s}/unpacked", .{dir}, 0)
        else
            try allocator.dupeZ(u8, path);
        defer allocator.free(unpacked);
        const read = otio.ReadOptions{
            .bundle_extract_path = if (format == .otioz) unpacked else null,
            .bundle_absolute_media_paths = true,
        };
        const absolute = try otio.Document.readFromFile(format, path, read);
        defer absolute.deinit();
        const copied = try (try activeMedia(absolute))[0].asExternalReference().?.targetUrl(allocator);
        defer allocator.free(copied);
        const expected = try std.fmt.allocPrint(allocator, "{s}/media/shot.mov", .{unpacked});
        defer allocator.free(expected);
        // A path on this system rather than a URL, so it is written in the
        // system's own separator: backslashes on Windows, all of them,
        // since the library normalizes the whole path.
        if (@import("builtin").os.tag == .windows) std.mem.replaceScalar(u8, expected, '/', '\\');
        try std.testing.expectEqualStrings(expected, copied);
        const held = try std.Io.Dir.cwd().readFileAlloc(std.testing.io, copied, allocator, .limited(1024));
        defer allocator.free(held);
        try std.testing.expectEqualStrings("not really a movie", held);

        // A bundle is never written over.
        try std.testing.expectError(error.IoError, document.writeToFile(format, path, options));
    }

    // Leaving every reference missing bundles no media at all.
    const empty = try std.fmt.allocPrintSentinel(allocator, "{s}/no-media.otioz", .{dir}, 0);
    defer allocator.free(empty);
    options.bundle_media_policy = .all_missing;
    try document.writeToFile(.otioz, empty, options);
    const again = try otio.open(empty);
    defer again.deinit();
    for (try activeMedia(again)) |reference| {
        try std.testing.expect(reference.asMissingReference() != null);
    }

    // A bundle lives on disk, so it is not written as bytes.
    try std.testing.expectError(error.Unsupported, document.writeToBytes(allocator, .otioz, null));
    try std.testing.expectEqualStrings("otiod", otio.Format.otiod.name());
}

/// The active media reference of each of the two clips a document holds.
fn activeMedia(document: *otio.Document) ![2]otio.Node {
    const root = (try document.root()) orelse return error.TestUnexpectedResult;
    const clips = try root.findClips(allocator);
    defer allocator.free(clips);
    if (clips.len != 2) return error.TestUnexpectedResult;
    var references: [2]otio.Node = undefined;
    for (clips, &references) |clip, *reference| {
        reference.* = (try clip.asClip().?.mediaReference(null)) orelse return error.TestUnexpectedResult;
    }
    return references;
}

/// How many clips a document's root holds.
fn clipCount(document: *otio.Document) !usize {
    const root = (try document.root()) orelse return error.TestUnexpectedResult;
    const clips = try root.findClips(allocator);
    defer allocator.free(clips);
    return clips.len;
}

test "writing bytes in every format the library knows" {
    const document = try otio.open(screening_edl);
    defer document.deinit();

    for ([_]otio.Format{ .otio_json, .cmx3600, .fcp7_xml }) |format| {
        const written = try document.writeToBytes(allocator, format, null);
        defer allocator.free(written);
        try std.testing.expect(written.len > 0);
    }
}

test "an enum says what the C interface calls it" {
    try std.testing.expectEqualStrings("OTIO_STATUS_NO_VALUE", otio.Status.no_value.cName());
    try std.testing.expectEqualStrings("OTIO_NODE_KIND_CLIP", otio.NodeKind.clip.cName());
    try std.testing.expectEqualStrings("cmx_3600", otio.Format.cmx3600.name());
}

test "an object knows which schemas it is" {
    const document = try otio.Document.init();
    defer document.deinit();

    const clip = try otio.Clip.init(document, "A");
    for ([_]otio.NodeKind{
        .clip,
        .item,
        .composable,
        .serializable_object_with_metadata,
        .serializable_object,
    }) |kind| {
        try std.testing.expect(clip.isA(kind));
    }
    for ([_]otio.NodeKind{ .track, .gap }) |kind| {
        try std.testing.expect(!clip.isA(kind));
    }
}

// Absorb is the call that lets an object be built on its own and put into a
// timeline afterwards. It is written by hand in the generator rather than
// emitted, so it needs a test of its own more than the mechanical calls do.
test "an object built on its own can join a timeline" {
    const timeline = try otio.Document.init();
    defer timeline.deinit();
    const track = try otio.Track.init(timeline, "V1", "Video");

    // A clip built somewhere else entirely, knowing nothing about the
    // timeline it is going to end up in.
    var aside: ?*otio.Document = try otio.Document.init();
    defer if (aside) |doomed| doomed.deinit();

    const clip = try otio.Clip.init(aside.?, "Insert");
    try clip.setSourceRange(.{
        .start_time = .{ .value = 0, .rate = 24 },
        .duration = .{ .value = 48, .rate = 24 },
    });

    const moved = try timeline.absorb(allocator, &aside);
    defer allocator.free(moved);

    // The source was consumed, so the deferred deinit above has nothing left
    // to do and every handle into it is now only good for looking up here.
    try std.testing.expect(aside == null);

    var arrived: ?otio.Node = null;
    for (moved) |entry| {
        if (std.meta.eql(entry.from, clip.asNode())) arrived = entry.to;
    }
    const insert = arrived orelse return error.TestUnexpectedResult;
    try std.testing.expectEqual(timeline, insert.doc.?);

    try track.appendChild(insert);
    const name = try insert.name(allocator);
    defer allocator.free(name);
    try std.testing.expectEqualStrings("Insert", name);

    const duration = try track.duration();
    try std.testing.expectEqual(@as(f64, 48), duration.value);
    try std.testing.expectEqual(@as(f64, 24), duration.rate);
}

// A handle is an index into one document's arena, and two fresh documents
// issue the same indices, so an object from one would resolve to an
// unrelated object in the other. Nothing in the handle itself says where it
// came from, so the Zig value has to carry it and the generated calls have
// to check.
test "an object from another document is refused" {
    const here = try otio.Document.init();
    defer here.deinit();
    const elsewhere = try otio.Document.init();
    defer elsewhere.deinit();

    // Built first in each document, so that the two really are handed the
    // same handle, which is what makes the check necessary rather than
    // merely tidy.
    const mine = try otio.Clip.init(here, "Mine");
    const theirs = try otio.Clip.init(elsewhere, "Theirs");
    const track = try otio.Track.init(here, "V1", "Video");

    try std.testing.expectEqual(mine.node.handle, theirs.node.handle);
    try std.testing.expect(!here.contains(theirs.asNode()));

    try std.testing.expectError(error.ForeignObject, track.appendChild(theirs.asNode()));
    try std.testing.expect(!mine.equals(theirs.asNode()));
    try std.testing.expectError(error.ForeignObject, here.deepClone(theirs.asNode()));

    // The object from this document still goes in, so the check refuses only
    // what it should.
    try track.appendChild(mine.asNode());

    // No object at all belongs to no document, so it is allowed wherever an
    // object is optional.
    const timeline = try otio.Timeline.init(here, "Cut");
    try timeline.setTracks(null);
}

// Upstream's Timeline() builds an empty stack named "tracks" in its
// constructor, so a caller can append to a fresh timeline's tracks without
// making one first. A timeline from here arrives the same way.
test "a new timeline arrives with its tracks" {
    const document = try otio.Document.init();
    defer document.deinit();

    const timeline = try otio.Timeline.init(document, "Cut");

    const held = (try timeline.tracks()) orelse return error.TestUnexpectedResult;
    const stack = held.asStack() orelse {
        std.debug.print("the tracks are held as a {}\n", .{try held.schemaKind()});
        return error.TestUnexpectedResult;
    };

    const name = try stack.name(allocator);
    defer allocator.free(name);
    try std.testing.expectEqualStrings("tracks", name);

    const owner = (try stack.parent()) orelse return error.TestUnexpectedResult;
    try std.testing.expect(owner.equals(timeline.asNode()));

    // Appending straight to it works, which is the point of building it.
    const track = try otio.Track.init(document, "V1", "Video");
    try stack.appendChild(track.asNode());
    try std.testing.expectEqual(@as(usize, 1), try stack.childCount());
}

// The temporary array a node list is marshalled into is freed on the way
// out of a failing call as well as a succeeding one. The testing allocator
// reports a leak, so this test fails rather than merely wasting memory if
// the `defer` ever moves back below the status check.
test "a rejected list call frees what it allocated" {
    const document = try otio.Document.init();
    defer document.deinit();

    // Of this document, so the foreign-object guard lets it through, but
    // not a track, so the library itself refuses it.
    const clip = try otio.Clip.init(document, "A");

    try std.testing.expectError(
        error.CoreError,
        document.flattenTracks(allocator, &.{clip.asNode()}),
    );
}

// absorb consumes the source, so everything it needs is allocated before
// the call rather than after it: an allocation that failed afterwards would
// leave the objects moved and the caller with no table saying where to.
test "a failed allocation in absorb leaves the source alone" {
    const held = try otio.Document.init();
    defer held.deinit();
    const track = try otio.Track.init(held, "V1", "Video");

    var aside: ?*otio.Document = try otio.Document.init();
    defer if (aside) |left| left.deinit();
    const clip = try otio.Clip.init(aside.?, "Insert");

    // absorb asks for three allocations, so the third one fails here.
    var failing = std.testing.FailingAllocator.init(allocator, .{ .fail_index = 2 });
    try std.testing.expectError(
        error.OutOfMemory,
        held.absorb(failing.allocator(), &aside),
    );

    // Nothing moved: the source is still there, and still holds its clip.
    try std.testing.expect(aside != null);
    try std.testing.expect(aside.?.contains(clip.asNode()));
    try std.testing.expectEqual(@as(usize, 0), try track.childCount());

    // And it still absorbs once the allocator is willing.
    const moved = try held.absorb(allocator, &aside);
    defer allocator.free(moved);
    try std.testing.expect(aside == null);
    try std.testing.expectEqual(@as(usize, 1), moved.len);
}
