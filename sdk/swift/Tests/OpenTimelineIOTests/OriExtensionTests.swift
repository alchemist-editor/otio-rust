// Tests for ORI Annotations, an otio-rust extension and not part of
// OpenTimelineIO. Enabling is process-wide, so nothing here asserts that it
// starts off; the C ABI's own Rust tests check that.

import Foundation
import XCTest

import OTIOExtOri
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
}
