import Foundation
import OTIOExtSync
import OpenTimelineIO

// What Furnace sends a viewer joining a session whose host shared one
// timeline.
let welcome = """
    {"v":2,"type":"welcome","sessionId":"s_1","seq":0,"ts":0,"order":1,"payload":{"selfId":"p_2","role":"viewer","roster":[{"id":"p_1","displayName":"Host","role":"host","joinedAt":0,"connectionState":"connected","muted":false}],"session":{"id":"s_1","createdAt":0},"snapshot":{"sequenceId":"main","generation":1,"otioJson":{"OTIO_SCHEMA":"Timeline.1","name":"Coastline","metadata":{"sync":{"guid":"t-1"}},"tracks":{"OTIO_SCHEMA":"Stack.1","metadata":{"sync":{"guid":"s-1"}},"children":[{"OTIO_SCHEMA":"Track.1","name":"V1","kind":"Video","metadata":{"sync":{"guid":"v-1"}},"children":[{"OTIO_SCHEMA":"Clip.2","name":"Waves","metadata":{"sync":{"guid":"c-1"}}}]}]}}}}}
    """

// A real program writes each frame to its WebSocket to Furnace. This one
// only says what it would send.
func sendFrames(_ session: UInt64) throws {
    while let frame = try Sync.sessionPollTransmit(session, now: 0) {
        print("send \(frame.bytes.count) bytes, \(frame.binary ? "binary" : "text")")
    }
}

try Sync.enable()

// A session owns no socket: it says what to send, and is told what arrived.
let session = try Sync.sessionNew(
    #"{"token":"join-code","identity":{"displayName":"Ada"},"encodings":[]}"#)
try Sync.sessionConnect(session)
try sendFrames(session)

// The frame Furnace sends back.
try Sync.sessionReceive(session, data: Array(welcome.utf8), binary: false)

// The program's copy of the timeline starts empty; handling the events
// fills it in.
let timeline = try OTIO.fromJSON(#"{"OTIO_SCHEMA":"Timeline.1"}"#)
while let event = try Sync.sessionPollEvent(session) {
    let fields = try JSONSerialization.jsonObject(with: Data(event.utf8)) as? [String: Any]
    print("event: \(fields?["type"] ?? "?")")
    print("handled: \(try Sync.handle(timeline, event: event))")
}
print("the timeline is \(try timeline.name())")

// Moving the playhead is one more frame to send.
try Sync.sessionSetPlayhead(session, sequenceID: "main", frame: 24, playing: true, rate: 1)
try sendFrames(session)

// Leaving sends a goodbye. Closing the session then frees it.
try Sync.sessionLeave(session)
try sendFrames(session)
try Sync.sessionClose(session)
