using System;
using System.Text;
using System.Text.Json;
using OpenTimelineIO;
using OpenTimelineIO.Ext.Sync;

// The frame Furnace sends back to a viewer joining a session whose host
// shared one timeline.
const string Welcome = """{"v":2,"type":"welcome","sessionId":"s_1","seq":0,"ts":0,"order":1,"payload":{"selfId":"p_2","role":"viewer","roster":[{"id":"p_1","displayName":"Host","role":"host","joinedAt":0,"connectionState":"connected","muted":false}],"session":{"id":"s_1","createdAt":0},"snapshot":{"sequenceId":"main","generation":1,"otioJson":{"OTIO_SCHEMA":"Timeline.1","name":"Coastline","metadata":{"sync":{"guid":"t-1"}},"tracks":{"OTIO_SCHEMA":"Stack.1","metadata":{"sync":{"guid":"s-1"}},"children":[{"OTIO_SCHEMA":"Track.1","name":"V1","kind":"Video","metadata":{"sync":{"guid":"v-1"}},"children":[{"OTIO_SCHEMA":"Clip.2","name":"Waves","metadata":{"sync":{"guid":"c-1"}}}]}]}}}}}""";

SyncExtension.Enable();

// A session owns no socket. Make one with the join code, then connect it
// once the socket is open.
var session = SyncExtension.SessionNew("""{"token":"join-code","identity":{"displayName":"Ada"}}""");
SyncExtension.SessionConnect(session);
SendAll(session);

// Hand the session every frame that arrives. This is the one Furnace sends
// back.
SyncExtension.SessionReceive(session, Encoding.UTF8.GetBytes(Welcome), false);

// The program's copy of the timeline starts empty. Handling the events puts
// the host's timeline in it, and keeps it current from then on.
var timeline = Otio.FromJson("""{"OTIO_SCHEMA": "Timeline.1"}""");
while (SyncExtension.SessionPollEvent(session) is string occurrence)
{
    using var parsed = JsonDocument.Parse(occurrence);
    var handled = SyncExtension.Handle(timeline, occurrence);
    Console.WriteLine($"{parsed.RootElement.GetProperty("type")} {handled}");
}
Console.WriteLine($"the viewer now shows {timeline.Name()}");

// Moving the playhead tells everyone else where it is.
SyncExtension.SessionSetPlayhead(session, "main", 24, true, 1);
SendAll(session);

// Leaving queues a goodbye; send it, then the socket can close.
SyncExtension.SessionLeave(session);
SendAll(session);
SyncExtension.SessionClose(session);

// Sends every frame the session has. A real program writes each one to its
// WebSocket to Furnace; this one only says what it would send.
static void SendAll(ulong session)
{
    var now = DateTimeOffset.UtcNow.ToUnixTimeMilliseconds();
    while (SyncExtension.SessionPollTransmit(session, now) is var (bytes, binary))
    {
        Console.WriteLine($"send {bytes.Length} bytes as {(binary ? "binary" : "text")}");
    }
}
