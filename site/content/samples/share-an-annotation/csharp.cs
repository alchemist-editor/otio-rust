using System;
using System.Text;
using System.Text.Json;
using OpenTimelineIO;
using OpenTimelineIO.Ext.Ori;
using OpenTimelineIO.Ext.Sync;

// The cut both sides have: one shot, which sync knows by the GUID
// main/shot.
const string Cut = """{"OTIO_SCHEMA":"Timeline.1","name":"review","metadata":{"sync":{"guid":"main"}},"tracks":{"OTIO_SCHEMA":"Stack.1","children":[{"OTIO_SCHEMA":"Track.1","name":"V1","kind":"Video","children":[{"OTIO_SCHEMA":"Clip.2","name":"shot","metadata":{"sync":{"guid":"main/shot"}},"source_range":{"OTIO_SCHEMA":"TimeRange.1","start_time":{"OTIO_SCHEMA":"RationalTime.1","value":0,"rate":24},"duration":{"OTIO_SCHEMA":"RationalTime.1","value":48,"rate":24}}}]}]}}""";

// What Furnace tells a reviewer who joins: who they are, and that they may
// comment.
const string Welcome = """{"v":2,"type":"welcome","sessionId":"s_1","seq":0,"ts":0,"order":1,"payload":{"selfId":"p_2","selfUserId":"u_bea","role":"viewer","roster":[],"session":{"id":"s_1","createdAt":0}}}""";

// What Furnace tells the host when it opens the session.
const string HostWelcome = """{"v":2,"type":"welcome","sessionId":"s_1","seq":0,"ts":0,"order":1,"payload":{"selfId":"p_1","selfUserId":"u_ari","role":"host","roster":[],"session":{"id":"s_1","createdAt":0}}}""";

// Where the reviewer drew: frame 10 of the shot, as the program shows it.
const string Anchor = """{"sequenceId":"main","capturedGeneration":1,"clipGuid":"main/shot","sourceTime":{"value":10,"rate":24},"capturedSequenceTime":{"value":10,"rate":24},"capturedAt":1787742000000}""";

// One stroke: a PaintStart, its points and a PaintEnd sharing a uuid.
const string Stroke = """[{"OTIO_SCHEMA":"PaintStart.1","uuid":"s1","rgba":[1,0.25,0.25,1]},{"OTIO_SCHEMA":"PaintPoint.1","uuid":"s1","points":{"OTIO_SCHEMA":"PaintVertices.1","x":[-0.5,0,0.5],"y":[0.2,0,-0.2],"size":[0.01,0.01,0.01],"alpha":[1,1,1]}},{"OTIO_SCHEMA":"PaintEnd.1","uuid":"s1"}]""";

// The comment as Furnace relays it once it has stored it, to everyone in
// the session. A real host reads this from its own socket.
const string Relay = """{"v":2,"type":"review_commit","sessionId":"s_1","senderId":"p_2","seq":1,"ts":0,"order":2,"payload":{"clientMutationId":"m1","commentId":"c1","authorUserId":"u_bea","authorPresenceId":"p_2","serverOrder":1,"revision":1,"createdAt":0,"anchor":{"sequenceId":"main","capturedGeneration":1,"clipGuid":"main/shot","sourceTime":{"value":10,"rate":24},"capturedSequenceTime":{"value":10,"rate":24},"capturedAt":1787742000000},"bodyMarkdown":"Too dark here.","annotationCommands":[{"OTIO_SCHEMA":"PaintStart.1","uuid":"s1","rgba":[1,0.25,0.25,1]},{"OTIO_SCHEMA":"PaintPoint.1","uuid":"s1","points":{"OTIO_SCHEMA":"PaintVertices.1","x":[-0.5,0,0.5],"y":[0.2,0,-0.2],"size":[0.01,0.01,0.01],"alpha":[1,1,1]}},{"OTIO_SCHEMA":"PaintEnd.1","uuid":"s1"}]}}""";

// ORI Annotations is an extension; enabling it enables sync too.
OriExtension.Enable();

// The reviewer joins with the right to comment.
var reviewer = SyncExtension.SessionNew(
    """{"token":"join-code","identity":{"capabilities":["review.comment.create","review.annotation.create"]}}""");
SyncExtension.SessionConnect(reviewer);
SendAll(reviewer);
SyncExtension.SessionReceive(reviewer, Encoding.UTF8.GetBytes(Welcome), false);
while (SyncExtension.SessionPollEvent(reviewer) is not null)
{
}

// Commit the stroke with a note. The session sends it again after every
// reconnect until Furnace stores it, so it is not lost.
var seen = Otio.FromJson(Cut);
Console.WriteLine($"committed {OriExtension.Commit(reviewer, seen, Anchor, "Too dark here.", Stroke, "")}");
SendAll(reviewer);

// The host hears the stored comment and folds it into its timeline.
var host = SyncExtension.SessionNew("""{"token":"host-token","identity":{"capabilities":["review.commit.receive"]}}""");
SyncExtension.SessionConnect(host);
SendAll(host);
SyncExtension.SessionReceive(host, Encoding.UTF8.GetBytes(HostWelcome), false);
SyncExtension.SessionReceive(host, Encoding.UTF8.GetBytes(Relay), false);
var timeline = Otio.FromJson(Cut);
while (SyncExtension.SessionPollEvent(host) is string occurrence)
{
    // A stroke still being drawn: draw it over the picture, then drop it.
    if (OriExtension.ReadPartial(occurrence) is string partial)
    {
        Console.WriteLine($"drawing {partial}");
        continue;
    }
    using var parsed = JsonDocument.Parse(occurrence);
    if (parsed.RootElement.GetProperty("type").GetString() != "review_relay")
    {
        continue;
    }
    var (clip, already) = OriExtension.Fold(timeline, occurrence);
    Console.WriteLine($"folded {OriExtension.Describe(clip)} already there: {already}");
    Console.WriteLine($"the stroke belongs to {OriExtension.OwnerOf(clip, "s1") ?? "nobody"}");
}
SyncExtension.SessionClose(reviewer);
SyncExtension.SessionClose(host);

// Stands in for the network: a real program writes every frame the session
// has waiting to its WebSocket to Furnace.
static void SendAll(ulong session)
{
    var now = DateTimeOffset.UtcNow.ToUnixTimeMilliseconds();
    while (SyncExtension.SessionPollTransmit(session, now) is var (bytes, _))
    {
        Console.WriteLine($"send {bytes.Length} bytes");
    }
}
