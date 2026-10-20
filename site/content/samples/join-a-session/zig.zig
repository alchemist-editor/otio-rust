const std = @import("std");
const otio = @import("otio");
const sync = otio.ext.sync;

// The frame Furnace sends back to a viewer joining a session whose host
// shared one timeline.
const welcome =
    \\{"v":2,"type":"welcome","sessionId":"s_1","seq":0,"ts":0,"order":1,"payload":{"selfId":"p_2","role":"viewer","roster":[{"id":"p_1","displayName":"Host","role":"host","joinedAt":0,"connectionState":"connected","muted":false}],"session":{"id":"s_1","createdAt":0},"snapshot":{"sequenceId":"main","generation":1,"otioJson":{"OTIO_SCHEMA":"Timeline.1","name":"Coastline","metadata":{"sync":{"guid":"t-1"}},"tracks":{"OTIO_SCHEMA":"Stack.1","metadata":{"sync":{"guid":"s-1"}},"children":[{"OTIO_SCHEMA":"Track.1","name":"V1","kind":"Video","metadata":{"sync":{"guid":"v-1"}},"children":[{"OTIO_SCHEMA":"Clip.2","name":"Waves","metadata":{"sync":{"guid":"c-1"}}}]}]}}}}}
;

// Sends every frame the session has. A real program writes each one to its
// WebSocket to Furnace; this one only says what it would send.
fn send(allocator: std.mem.Allocator, io: std.Io, session: u64) !void {
    const now = std.Io.Clock.real.now(io).toMilliseconds();
    while (try sync.sessionPollTransmit(allocator, session, now)) |frame| {
        const kind = if (frame.binary) "binary" else "text";
        std.debug.print("send a {s} frame of {d} bytes\n", .{ kind, frame.bytes.len });
    }
}

pub fn main(init: std.process.Init) !void {
    const allocator = init.arena.allocator();
    const io = init.io;

    try sync.enable();

    // A session owns no socket. Connect once the socket is open, and the
    // hello is the first frame to send.
    const session = try sync.sessionNew(
        \\{"token":"join-code","identity":{"displayName":"Ada"}}
    );
    try sync.sessionConnect(session);
    try send(allocator, io, session);

    // Hand the session each frame that arrives.
    try sync.sessionReceive(session, welcome, false);

    // The program's own copy of the timeline starts empty, and each event
    // goes to handle with it.
    const document = try otio.Document.fromJson(
        \\{"OTIO_SCHEMA":"Timeline.1"}
    );
    defer document.deinit();
    const timeline = (try document.root()) orelse return error.Empty;
    while (try sync.sessionPollEvent(allocator, session)) |event| {
        const parsed = try std.json.parseFromSlice(std.json.Value, allocator, event, .{});
        const kind = parsed.value.object.get("type").?.string;
        const handled = try sync.handle(document, allocator, timeline, try allocator.dupeZ(u8, event));
        std.debug.print("{s}: {s}\n", .{ kind, handled });
    }
    std.debug.print("the timeline is {s}\n", .{try timeline.name(allocator)});

    // Move the playhead, and send what that queued.
    try sync.sessionSetPlayhead(session, "main", 24, true, 1);
    try send(allocator, io, session);

    // Leaving queues the goodbye. Close the session once it is sent.
    try sync.sessionLeave(session);
    try send(allocator, io, session);
    try sync.sessionClose(session);
}
