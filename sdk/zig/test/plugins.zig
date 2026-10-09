//! Tests for media linkers and hook scripts written in Zig.
//!
//! The registry is the library's, shared by the whole process, so every test
//! here registers under names of its own and unregisters them when it ends.

const std = @import("std");
const otio = @import("otio");

const allocator = std.testing.allocator;

/// Writes a two-clip timeline as OTIO JSON, each clip pointing at media
/// under file:///media. The caller frees it.
fn pluginCut() ![]u8 {
    const document = try otio.Document.init();
    defer document.deinit();
    const timeline = try otio.Timeline.init(document, "Cut");
    const stack = try otio.Stack.init(document, "tracks");
    const track = try otio.Track.init(document, "V1", "Video");
    try timeline.setTracks(stack.asNode());
    try stack.appendChild(track.asNode());
    try document.setRoot(timeline.asNode());
    inline for (.{ "first", "second" }) |name| {
        const clip = try otio.Clip.init(document, name);
        const reference = try otio.ExternalReference.init(document, name, "file:///media/" ++ name ++ ".mov");
        try clip.setMediaReference("DEFAULT_MEDIA", reference.asNode());
        try clip.setActiveMediaReferenceKey("DEFAULT_MEDIA");
        try track.appendChild(clip.asNode());
    }
    return document.writeToBytes(allocator, .otio_json, null);
}

/// Reads `written` as OTIO JSON with `options`.
fn readCut(written: []const u8, options: otio.ReadOptions) !*otio.Document {
    return otio.Document.readFromBytes(.otio_json, written, options);
}

/// Whether the first clip of a document's root links to `expected`.
fn expectFirstUrl(document: *otio.Document, expected: []const u8) !void {
    const root = (try document.root()) orelse return error.TestUnexpectedResult;
    const clips = try root.findClips(allocator);
    defer allocator.free(clips);
    try std.testing.expect(clips.len > 0);
    const clip = clips[0].asClip() orelse return error.TestUnexpectedResult;
    const media = (try clip.mediaReference(null)) orelse return error.TestUnexpectedResult;
    const external = media.asExternalReference() orelse return error.TestUnexpectedResult;
    const url = try external.targetUrl(allocator);
    defer allocator.free(url);
    try std.testing.expectEqualStrings(expected, url);
}

/// Whether the last failure on this thread mentions `needle`.
fn expectMessage(needle: []const u8) !void {
    const message = otio.lastErrorMessage();
    if (std.mem.indexOf(u8, message, needle) == null) {
        std.debug.print("expected {s} in: {s}\n", .{ needle, message });
        return error.TestUnexpectedResult;
    }
}

/// A linker that points each clip at a proxy under the `root` argument.
fn proxies(_: void, clip: otio.Clip, arguments: otio.Metadata) !?otio.Node {
    const name = try clip.name(allocator);
    defer allocator.free(name);
    const root = arguments.getString(allocator, "root") catch return error.NoRootToLinkUnder;
    defer allocator.free(root);
    const url = try std.fmt.allocPrintSentinel(allocator, "{s}/{s}.mov", .{ root, name }, 0);
    defer allocator.free(url);
    const reference = try otio.ExternalReference.init(clip.node.doc.?, "proxy", url);
    return reference.node;
}

test "a media linker written in Zig links every clip" {
    const written = try pluginCut();
    defer allocator.free(written);
    try otio.registerMediaLinker("zig_proxies", {}, proxies);
    defer _ = otio.unregisterMediaLinker("zig_proxies");

    var options = otio.readOptionsDefault();
    options.media_linker = "zig_proxies";
    options.media_linker_arguments = "{\"root\": \"/proxies\"}";
    const linked = try readCut(written, options);
    defer linked.deinit();
    try expectFirstUrl(linked, "/proxies/first.mov");

    // Asked not to link, it does not.
    options.do_not_link_media = true;
    const unlinked = try readCut(written, options);
    defer unlinked.deinit();
    try expectFirstUrl(unlinked, "file:///media/first.mov");
}

/// A linker that counts the clips it is shown and leaves them alone.
const Watcher = struct {
    seen: usize = 0,

    fn link(self: *Watcher, clip: otio.Clip, arguments: otio.Metadata) !?otio.Node {
        _ = clip;
        _ = arguments;
        self.seen += 1;
        return null;
    }
};

test "a linker that leaves a clip alone keeps its media" {
    const written = try pluginCut();
    defer allocator.free(written);
    var watcher: Watcher = .{};
    try otio.registerMediaLinker("zig_watcher", &watcher, Watcher.link);
    defer _ = otio.unregisterMediaLinker("zig_watcher");

    var options = otio.readOptionsDefault();
    options.media_linker = "zig_watcher";
    const document = try readCut(written, options);
    defer document.deinit();
    try std.testing.expectEqual(@as(usize, 2), watcher.seen);
    try expectFirstUrl(document, "file:///media/first.mov");
}

fn offline(_: void, _: otio.Clip, _: otio.Metadata) !?otio.Node {
    return error.TheProxiesAreOffline;
}

/// A linker built in a document of its own, which the trampoline absorbs.
fn elsewhere(_: void, clip: otio.Clip, _: otio.Metadata) !?otio.Node {
    _ = clip;
    const own = try otio.Document.init();
    const reference = try otio.ExternalReference.init(own, "elsewhere", "/elsewhere.mov");
    return reference.node;
}

/// A linker that tries to free the document it was lent.
fn freeing(_: void, clip: otio.Clip, _: otio.Metadata) !?otio.Node {
    var lent: ?*otio.Document = clip.node.doc;
    const into = try otio.Document.init();
    defer into.deinit();
    const moved = try into.absorb(allocator, &lent);
    allocator.free(moved);
    return null;
}

test "a linker that fails stops the read in its own words" {
    const written = try pluginCut();
    defer allocator.free(written);
    try otio.registerMediaLinker("zig_offline", {}, offline);
    defer _ = otio.unregisterMediaLinker("zig_offline");

    var options = otio.readOptionsDefault();
    options.media_linker = "zig_offline";
    try std.testing.expectError(error.PluginError, readCut(written, options));
    try expectMessage("TheProxiesAreOffline");

    // Absorbing the document it was lent is refused, not a free.
    try otio.registerMediaLinker("zig_offline", {}, freeing);
    try std.testing.expectError(error.PluginError, readCut(written, options));
    try expectMessage("InvalidArgument");

    // An answer from a document of its own is moved into the read's.
    try otio.registerMediaLinker("zig_offline", {}, elsewhere);
    const document = try readCut(written, options);
    defer document.deinit();
    try expectFirstUrl(document, "/elsewhere.mov");

    // And a linker nobody registered is refused, as upstream refuses one.
    options.media_linker = "zig_nowhere";
    try std.testing.expectError(error.PluginError, readCut(written, options));
    try expectMessage("zig_nowhere");
}

/// A hook script that writes who ran it into the metadata of what it is
/// handed, under `key`.
fn Stamp(comptime key: [:0]const u8) type {
    return struct {
        fn run(_: void, target: otio.Node, arguments: otio.Metadata) !otio.Node {
            const who = arguments.getString(allocator, "who") catch try allocator.dupe(u8, "nobody");
            defer allocator.free(who);
            const held = try allocator.dupeZ(u8, who);
            defer allocator.free(held);
            try target.metadata().setString(key, held);
            return target;
        }
    };
}

test "hook scripts written in Zig run around reads and writes" {
    const written = try pluginCut();
    defer allocator.free(written);
    try otio.registerHookScript("zig_stamp_read", {}, Stamp("read_by").run);
    defer _ = otio.unregisterHookScript("zig_stamp_read");
    try otio.attachHookScript("post_adapter_read", "zig_stamp_read");
    defer _ = otio.detachHookScript("post_adapter_read", "zig_stamp_read");

    var options = otio.readOptionsDefault();
    options.hook_arguments = "{\"who\": \"the Zig test\"}";
    const document = try readCut(written, options);
    defer document.deinit();
    const root = (try document.root()) orelse return error.TestUnexpectedResult;
    const who = try root.metadata().getString(allocator, "read_by");
    defer allocator.free(who);
    try std.testing.expectEqualStrings("the Zig test", who);

    // A write runs its hooks on a copy, so the timeline is left alone.
    try otio.registerHookScript("zig_stamp_write", {}, Stamp("written_by").run);
    defer _ = otio.unregisterHookScript("zig_stamp_write");
    try otio.attachHookScript("pre_adapter_write", "zig_stamp_write");
    defer _ = otio.detachHookScript("pre_adapter_write", "zig_stamp_write");
    var write_options = otio.writeOptionsDefault();
    write_options.hook_arguments = "{\"who\": \"the writer\"}";
    const out = try document.writeToBytes(allocator, .otio_json, write_options);
    defer allocator.free(out);
    try std.testing.expect(std.mem.indexOf(u8, out, "\"written_by\": \"the writer\"") != null);
    try std.testing.expectError(error.NoValue, root.metadata().getString(allocator, "written_by"));
}

fn replace(_: void, target: otio.Node, _: otio.Metadata) !otio.Node {
    const replacement = try otio.Clip.init(target.doc.?, "replacement");
    return replacement.node;
}

fn nothing(_: void, _: otio.Node, _: otio.Metadata) !otio.Node {
    return otio.Node.none();
}

test "a hook of your own runs when asked" {
    const document = try otio.Document.init();
    defer document.deinit();
    const clip = try otio.Clip.init(document, "A");

    try otio.registerHookScript("zig_replace", {}, replace);
    defer _ = otio.unregisterHookScript("zig_replace");
    try otio.registerHookScript("zig_stamp", {}, Stamp("stamped_by").run);
    defer _ = otio.unregisterHookScript("zig_stamp");

    try otio.attachHookScript("zig_mine", "zig_stamp");
    defer _ = otio.detachHookScript("zig_mine", "zig_stamp");
    const result = try clip.runHook("zig_mine", "{\"who\": \"me\"}");
    try std.testing.expect(result.equals(clip.asNode()));
    const who = try clip.metadata().getString(allocator, "stamped_by");
    defer allocator.free(who);
    try std.testing.expectEqualStrings("me", who);

    // A script may hand back a different object to go on with.
    try otio.attachHookScript("zig_swap", "zig_replace");
    defer _ = otio.detachHookScript("zig_swap", "zig_replace");
    const swapped = try clip.runHook("zig_swap", null);
    const name = try swapped.name(allocator);
    defer allocator.free(name);
    try std.testing.expectEqualStrings("replacement", name);

    // A script that answers with nothing fails, since a hook needs an
    // object to go on with.
    try otio.registerHookScript("zig_nothing", {}, nothing);
    defer _ = otio.unregisterHookScript("zig_nothing");
    try otio.attachHookScript("zig_empty", "zig_nothing");
    defer _ = otio.detachHookScript("zig_empty", "zig_nothing");
    try std.testing.expectError(error.PluginError, clip.runHook("zig_empty", null));
    try expectMessage("no object to go on with");

    try std.testing.expectError(error.PluginError, clip.runHook("zig_undeclared", null));
}

test "only a plugin's own call sees its document as lent" {
    const document = try otio.Document.init();
    defer document.deinit();
    const clip = try otio.Clip.init(document, "A");

    // The caller's own document is what runHook lends, so inside the
    // script it is lent, and outside it is not.
    const Probe = struct {
        var lent_inside: bool = false;
        fn run(_: void, target: otio.Node, _: otio.Metadata) !otio.Node {
            var source: ?*otio.Document = target.doc;
            const into = try otio.Document.init();
            defer into.deinit();
            lent_inside = if (into.absorb(allocator, &source)) |moved| blk: {
                allocator.free(moved);
                break :blk false;
            } else |err| err == error.InvalidArgument;
            return target;
        }
    };
    try otio.registerHookScript("zig_probe", {}, Probe.run);
    defer _ = otio.unregisterHookScript("zig_probe");
    try otio.attachHookScript("zig_probing", "zig_probe");
    defer _ = otio.detachHookScript("zig_probing", "zig_probe");
    _ = try clip.runHook("zig_probing", null);
    try std.testing.expect(Probe.lent_inside);
    try std.testing.expect(document.contains(clip.asNode()));
}

test "unregistering says whether there was anything" {
    try otio.registerHookScript("zig_brief", {}, Stamp("x").run);
    try std.testing.expect(otio.unregisterHookScript("zig_brief"));
    try std.testing.expect(!otio.unregisterHookScript("zig_brief"));

    try otio.registerMediaLinker("zig_brief_linker", {}, offline);
    try std.testing.expect(otio.unregisterMediaLinker("zig_brief_linker"));
    try std.testing.expect(!otio.unregisterMediaLinker("zig_brief_linker"));

    try std.testing.expect(std.meta.isError(otio.registerMediaLinker("", {}, offline)));
}
