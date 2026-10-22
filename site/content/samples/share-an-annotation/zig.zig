const std = @import("std");
const otio = @import("otio");
const ori = otio.ext.ori;
const sync = otio.ext.sync;

// The cut both sides have: one shot, which sync knows by the GUID
// main/shot.
const cut =
    \\{"OTIO_SCHEMA":"Timeline.1","name":"review","metadata":{"sync":{"guid":"main"}},"tracks":{"OTIO_SCHEMA":"Stack.1","children":[{"OTIO_SCHEMA":"Track.1","name":"V1","kind":"Video","children":[{"OTIO_SCHEMA":"Clip.2","name":"shot","metadata":{"sync":{"guid":"main/shot"}},"source_range":{"OTIO_SCHEMA":"TimeRange.1","start_time":{"OTIO_SCHEMA":"RationalTime.1","value":0,"rate":24},"duration":{"OTIO_SCHEMA":"RationalTime.1","value":48,"rate":24}}}]}]}}
;

// What Furnace tells a reviewer who joins: who they are, and that they may
// comment.
const welcome =
    \\{"v":2,"type":"welcome","sessionId":"s_1","seq":0,"ts":0,"order":1,"payload":{"selfId":"p_2","selfUserId":"u_bea","role":"viewer","roster":[],"session":{"id":"s_1","createdAt":0}}}
;

// What Furnace tells the host when it opens the session.
const hostWelcome =
    \\{"v":2,"type":"welcome","sessionId":"s_1","seq":0,"ts":0,"order":1,"payload":{"selfId":"p_1","selfUserId":"u_ari","role":"host","roster":[],"session":{"id":"s_1","createdAt":0}}}
;

// Where the reviewer drew: frame 10 of the shot, as the program shows it.
const anchor =
    \\{"sequenceId":"main","capturedGeneration":1,"clipGuid":"main/shot","sourceTime":{"value":10,"rate":24},"capturedSequenceTime":{"value":10,"rate":24},"capturedAt":1787742000000}
;

// One stroke: a PaintStart, its points and a PaintEnd sharing a uuid.
const stroke =
    \\[{"OTIO_SCHEMA":"PaintStart.1","uuid":"s1","rgba":[1,0.25,0.25,1]},{"OTIO_SCHEMA":"PaintPoint.1","uuid":"s1","points":{"OTIO_SCHEMA":"PaintVertices.1","x":[-0.5,0,0.5],"y":[0.2,0,-0.2],"size":[0.01,0.01,0.01],"alpha":[1,1,1]}},{"OTIO_SCHEMA":"PaintEnd.1","uuid":"s1"}]
;

// The comment as Furnace relays it once it has stored it, to everyone in
// the session. A real host reads this from its own socket.
const relay =
    \\{"v":2,"type":"review_commit","sessionId":"s_1","senderId":"p_2","seq":1,"ts":0,"order":2,"payload":{"clientMutationId":"m1","commentId":"c1","authorUserId":"u_bea","authorPresenceId":"p_2","serverOrder":1,"revision":1,"createdAt":0,"anchor":{"sequenceId":"main","capturedGeneration":1,"clipGuid":"main/shot","sourceTime":{"value":10,"rate":24},"capturedSequenceTime":{"value":10,"rate":24},"capturedAt":1787742000000},"bodyMarkdown":"Too dark here.","annotationCommands":[{"OTIO_SCHEMA":"PaintStart.1","uuid":"s1","rgba":[1,0.25,0.25,1]},{"OTIO_SCHEMA":"PaintPoint.1","uuid":"s1","points":{"OTIO_SCHEMA":"PaintVertices.1","x":[-0.5,0,0.5],"y":[0.2,0,-0.2],"size":[0.01,0.01,0.01],"alpha":[1,1,1]}},{"OTIO_SCHEMA":"PaintEnd.1","uuid":"s1"}]}}
;

// Stands in for the network: a real program writes every frame the session
// has waiting to its WebSocket to Furnace.
fn send(allocator: std.mem.Allocator, io: std.Io, session: u64) !void {
    const now = std.Io.Clock.real.now(io).toMilliseconds();
    while (try sync.sessionPollTransmit(allocator, session, now)) |frame| {
        std.debug.print("send {d} bytes\n", .{frame.bytes.len});
    }
}

pub fn main(init: std.process.Init) !void {
    const allocator = init.arena.allocator();
    const io = init.io;

    // ORI Annotations is an extension; enabling it enables sync too.
    try ori.enable();

    // The reviewer joins with the right to comment.
    const reviewer = try sync.sessionNew(
        \\{"token":"join-code","identity":{"capabilities":["review.comment.create","review.annotation.create"]}}
    );
    try sync.sessionConnect(reviewer);
    try send(allocator, io, reviewer);
    try sync.sessionReceive(reviewer, welcome, false);
    while (try sync.sessionPollEvent(allocator, reviewer)) |_| {}

    // Commit the stroke with a note. The session sends it again after every
    // reconnect until Furnace stores it, so it is not lost.
    const seen = try otio.Document.fromJson(cut);
    defer seen.deinit();
    const seen_root = (try seen.root()) orelse return error.Empty;
    const id = try ori.commit(seen, allocator, reviewer, seen_root, anchor, "Too dark here.", stroke, "");
    std.debug.print("committed {s}\n", .{id});
    try send(allocator, io, reviewer);

    // The host hears the stored comment and folds it into its timeline.
    const host = try sync.sessionNew(
        \\{"token":"host-token","identity":{"capabilities":["review.commit.receive"]}}
    );
    try sync.sessionConnect(host);
    try send(allocator, io, host);
    try sync.sessionReceive(host, hostWelcome, false);
    try sync.sessionReceive(host, relay, false);
    const document = try otio.Document.fromJson(cut);
    defer document.deinit();
    const timeline = (try document.root()) orelse return error.Empty;
    while (try sync.sessionPollEvent(allocator, host)) |text| {
        const event = try allocator.dupeZ(u8, text);
        // A stroke still being drawn: draw it over the picture, then drop it.
        if (try ori.readPartial(allocator, event)) |partial| {
            std.debug.print("drawing {s}\n", .{partial});
            continue;
        }
        const parsed = try std.json.parseFromSlice(std.json.Value, allocator, event, .{});
        if (!std.mem.eql(u8, parsed.value.object.get("type").?.string, "review_relay")) continue;
        const folded = try ori.fold(document, timeline, event);
        std.debug.print("folded {s} already there: {}\n", .{ try ori.describe(document, allocator, folded.clip), folded.already });
        const owner = try ori.ownerOf(document, allocator, folded.clip, "s1");
        std.debug.print("the stroke belongs to {s}\n", .{owner orelse "nobody"});
    }
    try sync.sessionClose(reviewer);
    try sync.sessionClose(host);
}
