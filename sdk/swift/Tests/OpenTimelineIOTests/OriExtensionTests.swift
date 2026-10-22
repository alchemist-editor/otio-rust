// Tests for ORI Annotations, an otio-rust extension and not part of
// OpenTimelineIO. Enabling is process-wide, so nothing here asserts that it
// starts off; the C ABI's own Rust tests check that.

import Foundation
import XCTest

import OTIOExtOri
import OTIOExtSync
import OpenTimelineIO

/// The directory one above a path.
private func parent(_ path: String) -> String {
    guard let slash = path.lastIndex(of: "/") else { return path }
    return String(path[path.startIndex..<slash])
}

/// A file ORI's own Python wrote: a shot annotated at frames 10 and 4.
private let oriBuilt = parent(parent(parent(parent(parent(#filePath)))))
    + "/extensions/ori/otio-ext-ori-annotations/tests/fixtures/ori_built.otio"

private func openOriBuilt() throws -> SerializableObject {
    try OTIO.fromJSON(try String(contentsOfFile: oriBuilt, encoding: .utf8))
}

/// A timeline whose one clip has the sync GUID main/clip-1.
private let reviewTimeline = #"{"OTIO_SCHEMA":"Timeline.1","name":"Program","metadata":{"sync":{"guid":"main"}},"tracks":{"OTIO_SCHEMA":"Stack.1","metadata":{"sync":{"guid":"main/stack"}},"children":[{"OTIO_SCHEMA":"Track.1","name":"V1","kind":"Video","metadata":{"sync":{"guid":"main/v1"}},"children":[{"OTIO_SCHEMA":"Clip.2","name":"Beach Run","metadata":{"sync":{"guid":"main/clip-1"}},"source_range":{"OTIO_SCHEMA":"TimeRange.1","start_time":{"OTIO_SCHEMA":"RationalTime.1","value":0,"rate":24},"duration":{"OTIO_SCHEMA":"RationalTime.1","value":48,"rate":24}}}]}]}}"#

/// One stroke, s1.
private let reviewStroke = #"[{"OTIO_SCHEMA":"PaintStart.1","uuid":"s1","rgba":[1,0,0,1]},{"OTIO_SCHEMA":"PaintPoint.1","uuid":"s1","points":{"OTIO_SCHEMA":"PaintVertices.1","x":[0.1],"y":[0.2],"size":[0.01],"alpha":[1]}},{"OTIO_SCHEMA":"PaintEnd.1","uuid":"s1"}]"#

/// Where a reviewer drew it: frame 10 of main/clip-1.
private let reviewAnchor = #"{"sequenceId":"main","capturedGeneration":1,"clipGuid":"main/clip-1","sourceTime":{"value":10,"rate":24},"capturedSequenceTime":{"value":10,"rate":24},"capturedAt":1787742000000}"#

/// What a viewer who may comment hears on joining.
private let reviewWelcome = #"{"v":2,"type":"welcome","sessionId":"s_1","seq":0,"ts":0,"order":1,"payload":{"selfId":"p_2","selfUserId":"u_bea","role":"viewer","roster":[],"session":{"id":"s_1","createdAt":0}}}"#

/// The comment, as the host's session hears it from Furnace.
private let reviewRelay = #"{"type":"review_relay","commit":{"clientMutationId":"m1","anchor":"#
    + reviewAnchor + #","bodyMarkdown":"Too dark.","annotationCommands":"# + reviewStroke
    + #"},"ack":{"clientMutationId":"m1","commentId":"c1","serverOrder":1,"revision":1},"authorUserId":"u_bea","authorPresenceId":"p_2","createdAt":0}"#

/// A stroke in progress, as ORI's RV plugin sends one.
private let reviewPartial = #"{"type":"message","kind":"Annotation.1/PARTIAL","sender":"p_3","payload":{"clip_guid":"main/clip-1","frame":10.0,"fps":24.0,"events":[{"OTIO_SCHEMA":"PaintStart.1","uuid":"s2"}]}}"#

/// Every frame `session` has to send now, as text.
private func drained(_ session: UInt64) throws -> [String] {
    var frames: [String] = []
    while let frame = try Sync.sessionPollTransmit(session, now: 0) {
        frames.append(String(decoding: frame.bytes, as: UTF8.self))
    }
    return frames
}

final class OriExtensionTests: XCTestCase {
    override func setUpWithError() throws {
        try Ori.enable()
    }

    func testTurnsOnWithSync() throws {
        XCTAssertTrue(try Ori.isEnabled())
        XCTAssertTrue(try OTIO.isExtensionEnabled("ori-annotations"))
        XCTAssertTrue(try OTIO.isExtensionEnabled("sync"))
    }

    func testReadsWhatOriWrote() throws {
        let timeline = try openOriBuilt()
        let holders = try Ori.list(timeline)
        XCTAssertEqual(holders.count, 2)
        XCTAssertEqual(
            try Ori.describe(holders[0]),
            #"{"form":"track","clip_guid":"shot","clip_name":null,"frame":{"value":10.0,"rate":24.0},"note":null,"status":null,"command_count":4}"#
        )
        let track = try XCTUnwrap(try Ori.findTrack(timeline))
        XCTAssertEqual(try track.name(), "Annotations")
        XCTAssertNil(try Ori.findClip(track, clipGuid: "shot", frame: 99))
        XCTAssertEqual(try Ori.listAtFrame(timeline, clipGuid: "shot", frame: 4).count, 1)
        XCTAssertEqual(try Ori.listForClip(timeline, clipGuid: "shot").count, 2)
    }

    func testAnnotatesAFrameAndEditsByUUID() throws {
        let commands = try Ori.commands(try Ori.list(try openOriBuilt())[0], pretty: false)

        let timeline = try OTIO.fromJSON(
            #"{"OTIO_SCHEMA": "Timeline.1", "tracks": {"OTIO_SCHEMA": "Stack.1", "children": []}}"#)
        let track = try Ori.ensureTrack(timeline)
        let clip = try Ori.ensureClip(track, clipGuid: "shot", value: 10, rate: 24)
        try Ori.addCommands(clip, commandsJSON: commands)
        _ = try Ori.ensureClip(track, clipGuid: "shot", value: 10, rate: 24)
        XCTAssertEqual(try Ori.list(timeline).count, 1, "asking again adds no second clip")
        XCTAssertEqual(try Ori.commands(clip, pretty: false), commands)

        XCTAssertTrue(try Ori.replaceCommands(
            clip, uuid: "stroke-1",
            commandsJSON: #"[{"OTIO_SCHEMA": "TextAnnotation.1", "uuid": "stroke-1", "text": "was a stroke"}]"#))
        XCTAssertEqual(try Ori.removeCommands(clip, uuidsJSON: #"["text-1"]"#), 1)
        let left = try Ori.commands(clip, pretty: false)
        XCTAssertTrue(left.contains(#""text":"was a stroke""#), left)
        XCTAssertFalse(left.contains("text-1"), left)
        try Ori.setCommands(clip, commandsJSON: "[]")
        XCTAssertThrowsError(try Ori.addCommands(clip, commandsJSON: "not json"))

        let corner = try Ori.pixelsToOri(1920, height: 1080, px: 0, py: 0)
        XCTAssertEqual(corner.x, -8.0 / 9.0, accuracy: 1e-12)
        XCTAssertEqual(corner.y, 0.5, accuracy: 1e-12)
        let mapped = try Ori.oriToBounds(0.5, y: 0.5, minX: -16, minY: -9, maxX: 16, maxY: 9)
        XCTAssertEqual(mapped.x, 9)
        XCTAssertEqual(mapped.y, 9)
    }

    func testMergeRules() throws {
        let timeline = try OTIO.fromJSON(reviewTimeline)
        let track = try Ori.ensureTrack(timeline)
        let clip = try Ori.ensureClip(track, clipGuid: "main/clip-1", value: 10, rate: 24)
        try Ori.setCommands(
            clip, commandsJSON: #"[{"OTIO_SCHEMA":"TextAnnotation.1","uuid":"t"},{"OTIO_SCHEMA":"PaintStart.1","uuid":"a"}]"#)
        let merged = try Ori.mergeCommands(
            clip, commandsJSON: #"[{"OTIO_SCHEMA":"PaintStart.1","uuid":"a","brush":"gauss"},{"OTIO_SCHEMA":"PaintStart.1","uuid":"b"}]"#)
        XCTAssertEqual(merged.replaced, 1)
        XCTAssertEqual(merged.added, 1)
        XCTAssertEqual(try Ori.replaceByKind(clip, commandsJSON: #"[{"OTIO_SCHEMA":"PaintStart.1","uuid":"c"}]"#), 2)
        let left = try Ori.commands(clip, pretty: false)
        XCTAssertTrue(left.contains(#""uuid":"t""#) && left.contains(#""uuid":"c""#), left)
        XCTAssertEqual(try Ori.mergeDuplicates(timeline), 0)
        XCTAssertNil(try Ori.ownerOf(clip, uuid: "t"))
    }

    func testLiveReview() throws {
        // A stroke in progress from another peer.
        let partial = try XCTUnwrap(try Ori.readPartial(reviewPartial))
        XCTAssertTrue(partial.contains(#""clip_guid":"main/clip-1""#) && partial.contains(#""brush":"circle""#), partial)
        XCTAssertNil(try Ori.readPartial(#"{"type":"leader","leader":null}"#))

        // A reviewer commits a stroke with a note.
        let reviewer = try Sync.sessionNew(
            #"{"token":"join-code","identity":{"capabilities":["review.comment.create","review.annotation.create"]},"encodings":[]}"#)
        defer { try? Sync.sessionClose(reviewer) }
        try Sync.sessionConnect(reviewer)
        _ = try drained(reviewer)
        try Sync.sessionReceive(reviewer, data: Array(reviewWelcome.utf8), binary: false)
        while try Sync.sessionPollEvent(reviewer) != nil {}
        let id = try Ori.commit(
            reviewer, timeline: try OTIO.fromJSON(reviewTimeline), anchorJSON: reviewAnchor,
            bodyMarkdown: "Too dark.", commandsJSON: reviewStroke, clientMutationID: "")
        XCTAssertTrue(id.hasPrefix("mutation_"), id)
        let frames = try drained(reviewer).joined(separator: "\n")
        XCTAssertTrue(frames.contains(#""type":"review_commit""#) && frames.contains(#""uuid":"s1""#), frames)
        try Ori.sendPartial(reviewer, partialJSON: #"{"clip_guid":"main/clip-1","frame":10,"fps":24,"events":[]}"#)
        XCTAssertThrowsError(try Sync.sessionPollTransmit(reviewer, now: 0)) {
            XCTAssertEqual(($0 as? OTIOError)?.status, .unsupported, "Furnace has no stroke in progress: \($0)")
        }

        // The host folds the comment the server relays into its timeline.
        let timeline = try OTIO.fromJSON(reviewTimeline)
        let folded = try Ori.fold(timeline, eventJSON: reviewRelay)
        XCTAssertFalse(folded.already)
        XCTAssertTrue(try Ori.fold(timeline, eventJSON: reviewRelay).already)
        let commands = try Ori.commands(folded.clip, pretty: false)
        XCTAssertEqual(commands.components(separatedBy: #""uuid":"s1""#).count - 1, 3, commands)
        XCTAssertEqual(try Ori.ownerOf(folded.clip, uuid: "s1"), "u_bea")
    }
}
