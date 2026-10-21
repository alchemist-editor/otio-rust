import Foundation
import OTIOExtOri
import OTIOExtSync
import OpenTimelineIO

// The cut both sides have: one shot, which sync knows by the GUID
// main/shot.
let cut = #"{"OTIO_SCHEMA":"Timeline.1","name":"review","metadata":{"sync":{"guid":"main"}},"tracks":{"OTIO_SCHEMA":"Stack.1","children":[{"OTIO_SCHEMA":"Track.1","name":"V1","kind":"Video","children":[{"OTIO_SCHEMA":"Clip.2","name":"shot","metadata":{"sync":{"guid":"main/shot"}},"source_range":{"OTIO_SCHEMA":"TimeRange.1","start_time":{"OTIO_SCHEMA":"RationalTime.1","value":0,"rate":24},"duration":{"OTIO_SCHEMA":"RationalTime.1","value":48,"rate":24}}}]}]}}"#

// What Furnace tells a reviewer who joins: who they are, and that they may
// comment.
let welcome = #"{"v":2,"type":"welcome","sessionId":"s_1","seq":0,"ts":0,"order":1,"payload":{"selfId":"p_2","selfUserId":"u_bea","role":"viewer","roster":[],"session":{"id":"s_1","createdAt":0}}}"#

// What Furnace tells the host when it opens the session.
let hostWelcome = #"{"v":2,"type":"welcome","sessionId":"s_1","seq":0,"ts":0,"order":1,"payload":{"selfId":"p_1","selfUserId":"u_ari","role":"host","roster":[],"session":{"id":"s_1","createdAt":0}}}"#

// Where the reviewer drew: frame 10 of the shot, as the program shows it.
let anchor = #"{"sequenceId":"main","capturedGeneration":1,"clipGuid":"main/shot","sourceTime":{"value":10,"rate":24},"capturedSequenceTime":{"value":10,"rate":24},"capturedAt":1787742000000}"#

// One stroke: a PaintStart, its points and a PaintEnd sharing a uuid.
let stroke = #"[{"OTIO_SCHEMA":"PaintStart.1","uuid":"s1","rgba":[1,0.25,0.25,1]},{"OTIO_SCHEMA":"PaintPoint.1","uuid":"s1","points":{"OTIO_SCHEMA":"PaintVertices.1","x":[-0.5,0,0.5],"y":[0.2,0,-0.2],"size":[0.01,0.01,0.01],"alpha":[1,1,1]}},{"OTIO_SCHEMA":"PaintEnd.1","uuid":"s1"}]"#

// The comment as Furnace relays it once it has stored it, to everyone in
// the session. A real host reads this from its own socket.
let relay = #"{"v":2,"type":"review_commit","sessionId":"s_1","senderId":"p_2","seq":1,"ts":0,"order":2,"payload":{"clientMutationId":"m1","commentId":"c1","authorUserId":"u_bea","authorPresenceId":"p_2","serverOrder":1,"revision":1,"createdAt":0,"anchor":{"sequenceId":"main","capturedGeneration":1,"clipGuid":"main/shot","sourceTime":{"value":10,"rate":24},"capturedSequenceTime":{"value":10,"rate":24},"capturedAt":1787742000000},"bodyMarkdown":"Too dark here.","annotationCommands":[{"OTIO_SCHEMA":"PaintStart.1","uuid":"s1","rgba":[1,0.25,0.25,1]},{"OTIO_SCHEMA":"PaintPoint.1","uuid":"s1","points":{"OTIO_SCHEMA":"PaintVertices.1","x":[-0.5,0,0.5],"y":[0.2,0,-0.2],"size":[0.01,0.01,0.01],"alpha":[1,1,1]}},{"OTIO_SCHEMA":"PaintEnd.1","uuid":"s1"}]}}"#

// Stands in for the network: a real program writes every frame the session
// has waiting to its WebSocket to Furnace.
func sendFrames(_ session: UInt64) throws {
    while let frame = try Sync.sessionPollTransmit(session, now: 0) {
        print("send \(frame.bytes.count) bytes")
    }
}

// ORI Annotations is an extension; enabling it enables sync too.
try Ori.enable()

// The reviewer joins with the right to comment.
let reviewer = try Sync.sessionNew(
    #"{"token":"join-code","identity":{"capabilities":["review.comment.create","review.annotation.create"]}}"#)
try Sync.sessionConnect(reviewer)
try sendFrames(reviewer)
try Sync.sessionReceive(reviewer, data: Array(welcome.utf8), binary: false)
while try Sync.sessionPollEvent(reviewer) != nil {}

// Commit the stroke with a note. The session sends it again after every
// reconnect until Furnace stores it, so it is not lost.
let seen = try OTIO.fromJSON(cut)
let id = try Ori.commit(
    reviewer, timeline: seen, anchorJSON: anchor, bodyMarkdown: "Too dark here.",
    commandsJSON: stroke, clientMutationID: "")
print("committed \(id)")
try sendFrames(reviewer)

// The host hears the stored comment and folds it into its timeline.
let host = try Sync.sessionNew(#"{"token":"host-token","identity":{"capabilities":["review.commit.receive"]}}"#)
try Sync.sessionConnect(host)
try sendFrames(host)
for frame in [hostWelcome, relay] {
    try Sync.sessionReceive(host, data: Array(frame.utf8), binary: false)
}
let timeline = try OTIO.fromJSON(cut)
while let event = try Sync.sessionPollEvent(host) {
    // A stroke still being drawn: draw it over the picture, then drop it.
    if let partial = try Ori.readPartial(event) {
        print("drawing \(partial)")
        continue
    }
    let fields = try JSONSerialization.jsonObject(with: Data(event.utf8)) as? [String: Any]
    guard fields?["type"] as? String == "review_relay" else { continue }
    let folded = try Ori.fold(timeline, eventJSON: event)
    print("folded \(try Ori.describe(folded.clip)) already there: \(folded.already)")
    print("the stroke belongs to \(try Ori.ownerOf(folded.clip, uuid: "s1") ?? "nobody")")
}
try Sync.sessionClose(reviewer)
try Sync.sessionClose(host)
