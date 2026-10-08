const std = @import("std");
const otio = @import("otio");

// A cut of two clips, as an .otio file would hold it.
const cut =
    \\{
    \\  "OTIO_SCHEMA": "Track.1",
    \\  "name": "V1",
    \\  "kind": "Video",
    \\  "children": [
    \\    {"OTIO_SCHEMA": "Clip.2", "name": "A"},
    \\    {"OTIO_SCHEMA": "Clip.2", "name": "B"}
    \\  ]
    \\}
;

// Zig has no closures, so a plugin is a function and a context it is handed
// on every call. Here the context carries the allocator.
const Plugins = struct {
    allocator: std.mem.Allocator,

    // A media linker is handed each clip as it is read, with the arguments
    // the read was given, and answers with the media the clip should use,
    // built in the clip's own document. Null leaves the clip as it was.
    fn proxies(self: *const Plugins, clip: otio.Clip, arguments: otio.Metadata) !?otio.Node {
        const name = try clip.name(self.allocator);
        defer self.allocator.free(name);
        const root = try arguments.getString(self.allocator, "root");
        defer self.allocator.free(root);
        const url = try std.fmt.allocPrintSentinel(self.allocator, "{s}/{s}.mov", .{ root, name }, 0);
        defer self.allocator.free(url);
        const proxy = try otio.ExternalReference.init(clip.node.doc.?, null, url);
        return proxy.node;
    }

    // A hook script is handed the whole result, and answers with what the
    // read goes on with: here the same object, stamped.
    fn stamp(self: *const Plugins, target: otio.Node, arguments: otio.Metadata) !otio.Node {
        const who = try arguments.getString(self.allocator, "who");
        defer self.allocator.free(who);
        const held = try self.allocator.dupeZ(u8, who);
        defer self.allocator.free(held);
        try target.metadata().setString("read_by", held);
        return target;
    }
};

pub fn main() !void {
    const allocator = std.heap.smp_allocator;
    const plugins: Plugins = .{ .allocator = allocator };

    try otio.registerMediaLinker("proxies", &plugins, Plugins.proxies);
    defer _ = otio.unregisterMediaLinker("proxies");
    try otio.registerHookScript("stamp", &plugins, Plugins.stamp);
    defer _ = otio.unregisterHookScript("stamp");
    try otio.attachHookScript("post_adapter_read", "stamp");
    defer _ = otio.detachHookScript("post_adapter_read", "stamp");

    // The read names the linker, and carries both sets of arguments as JSON.
    var options = otio.readOptionsDefault();
    options.media_linker = "proxies";
    options.media_linker_arguments = "{\"root\": \"/proxies\"}";
    options.hook_arguments = "{\"who\": \"the conform\"}";
    const document = try otio.Document.readFromBytes(.otio_json, cut, options);
    defer document.deinit();

    const track = (try document.root()) orelse return error.Empty;
    const who = try track.metadata().getString(allocator, "read_by");
    defer allocator.free(who);
    std.debug.print("read by {s}\n", .{who});

    const clips = try track.findClips(allocator);
    defer allocator.free(clips);
    for (clips) |node| {
        const clip = node.asClip() orelse continue;
        const media = (try clip.mediaReference(null)) orelse continue;
        const external = media.asExternalReference() orelse continue;
        const url = try external.targetUrl(allocator);
        defer allocator.free(url);
        const name = try clip.name(allocator);
        defer allocator.free(name);
        std.debug.print("{s} -> {s}\n", .{ name, url });
    }
}
