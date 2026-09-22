// Tests for the generated Swift SDK.
//
// These are written by hand, not generated. A generator that also wrote its
// own tests would only prove it is self-consistent; what needs proving is
// that the Swift it writes does what a Swift programmer reading it would
// expect, against the same library and the same fixtures the Rust tests use.

import Foundation
import XCTest

import OpenTimelineIO

/// The directory one above a path.
private func parent(_ path: String) -> String {
    guard let slash = path.lastIndex(of: "/") else { return path }
    return String(path[path.startIndex..<slash])
}

/// The repository, found from where this file sits in it:
/// `<root>/sdk/swift/Tests/OpenTimelineIOTests/OpenTimelineIOTests.swift`.
private let repository = parent(parent(parent(parent(parent(#filePath)))))

/// The EDL the Rust adapter's own tests read, so that the two agree about
/// what is in it.
private let screeningEDL = repository + "/crates/otio-cmx3600/tests/data/screening_example.edl"

/// A path in a directory of this run's own, so two tests cannot collide.
private func temporary(_ name: String) throws -> String {
    let directory = FileManager.default.temporaryDirectory
        .appendingPathComponent("otio-swift-\(UUID().uuidString)")
    try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
    return directory.appendingPathComponent(name).path
}

/// The status a call failed with, for a test that wants to name it.
private func status(of error: Error) -> Status? {
    (error as? OTIOError)?.status
}

final class LibraryTests: XCTestCase {
    func testTheLibraryReportsAVersion() {
        XCTAssertFalse(OTIO.version().isEmpty)
    }

    func testAnEnumSaysWhatTheCInterfaceCallsIt() {
        XCTAssertEqual(Format.cmx3600.description, "OTIO_FORMAT_CMX_3600")
        XCTAssertEqual(Status.noValue.description, "OTIO_STATUS_NO_VALUE")
        XCTAssertEqual(Format.cmx3600.name, "cmx_3600")
    }

    func testRatesAreClassified() {
        XCTAssertTrue(OTIO.isDropFrameRate(29.97))
        XCTAssertFalse(OTIO.isDropFrameRate(24))
        XCTAssertTrue(OTIO.isSMPTETimecodeRate(24))
    }
}

final class ReadingTests: XCTestCase {
    func testReadingAnEDLFindsItsClips() throws {
        let document = try Document.readFromFile(.cmx3600, path: screeningEDL)
        defer { document.close() }

        let root = try XCTUnwrap(document.root())
        let clips = try root.findClips()
        XCTAssertEqual(clips.count, 9)

        // Every one of them really is a clip, and the class says so without
        // being asked to check.
        for node in clips {
            let kind = try node.schemaKind()
            XCTAssertNotNil(node as? Clip, "findClips answered with a \(kind)")
        }
        XCTAssertEqual(clips.compactMap { $0 as? Clip }.count, 9)
    }

    func testOpenWorksOutTheFormatFromTheName() throws {
        let document = try Document.open(screeningEDL)
        defer { document.close() }

        let root = try XCTUnwrap(document.root())
        XCTAssertTrue(try root.name().contains("Example_Screening"))
    }

    func testOpenDeclinesASuffixNoFormatClaims() throws {
        XCTAssertThrowsError(try Document.open("/tmp/nothing.wav")) { error in
            XCTAssertEqual(status(of: error), .noValue)
        }
    }

    func testADocumentSurvivesARoundTripThroughJSON() throws {
        let document = try Document.open(screeningEDL)
        defer { document.close() }

        let text = try document.toJSON(2)
        XCTAssertTrue(text.contains("Timeline"))

        let again = try Document.fromJSON(text)
        defer { again.close() }
        let root = try XCTUnwrap(again.root())
        XCTAssertEqual(try root.findClips().count, 9)
    }

    func testSavingAndOpeningAgainKeepsTheClips() throws {
        let document = try Document.open(screeningEDL)
        defer { document.close() }

        let path = try temporary("round-trip.otio")
        try document.save(path)

        let again = try Document.open(path)
        defer { again.close() }
        let root = try XCTUnwrap(again.root())
        XCTAssertEqual(try root.findClips().count, 9)
    }

    func testWritingBytesInEveryFormatTheLibraryKnows() throws {
        let document = try Document.open(screeningEDL)
        defer { document.close() }

        for format in [Format.otioJSON, .cmx3600] {
            let bytes = try document.writeToBytes(format)
            XCTAssertFalse(bytes.isEmpty, "\(format) wrote nothing")
        }
    }
}

final class BuildingTests: XCTestCase {
    /// Builds a timeline with one video track holding two clips.
    private func makeTimeline(in document: Document) throws -> (Timeline, Track, [Clip]) {
        let timeline = try document.newTimeline("Assembly")
        let stack = try document.newStack("tracks")
        try timeline.setTracks(stack)
        let track = try document.newTrack("V1", kind: "Video")
        try stack.appendChild(track)

        var clips: [Clip] = []
        for (index, name) in ["A", "B"].enumerated() {
            let clip = try document.newClip(name)
            let start = RationalTime(value: Double(index * 24), rate: 24)
            try clip.setSourceRange(TimeRange(startTime: start, duration: RationalTime(value: 24, rate: 24)))
            try track.appendChild(clip)
            clips.append(clip)
        }
        try document.setRoot(timeline)
        return (timeline, track, clips)
    }

    func testBuildingATimelineFromNothing() throws {
        let document = Document.new()
        defer { document.close() }

        let (timeline, track, clips) = try makeTimeline(in: document)
        XCTAssertEqual(try track.childCount(), 2)
        XCTAssertEqual(try timeline.findClips().count, 2)
        XCTAssertEqual(try clips[0].name(), "A")
        XCTAssertEqual(try track.kind(), "Video")

        // The whole track is as long as the two clips together.
        XCTAssertEqual(try track.duration().toSeconds, 2, accuracy: 1e-9)
    }

    func testAFreshlyBuiltObjectIsEnabled() throws {
        let document = Document.new()
        defer { document.close() }
        let clip = try document.newClip("A")
        XCTAssertTrue(try clip.enabled())
        try clip.setEnabled(false)
        XCTAssertFalse(try clip.enabled())
    }

    func testNoValueIsAnAnswerAndNotAFailure() throws {
        let document = Document.new()
        defer { document.close() }

        let clip = try document.newClip("untrimmed")
        // An item that uses all of its media has no source range, and that
        // is an answer rather than a failure.
        XCTAssertNil(try clip.sourceRange())

        let span = TimeRange(
            startTime: RationalTime(value: 0, rate: 24),
            duration: RationalTime(value: 12, rate: 24))
        try clip.setSourceRange(span)
        XCTAssertEqual(try clip.sourceRange(), span)

        try clip.clearSourceRange()
        XCTAssertNil(try clip.sourceRange())
    }

    func testAnObjectKnowsWhichSchemasItIs() throws {
        let document = Document.new()
        defer { document.close() }

        let clip = try document.newClip("A")
        XCTAssertTrue(clip.isA(.clip))
        XCTAssertTrue(clip.isA(.item))
        XCTAssertTrue(clip.isA(.composable))
        XCTAssertTrue(clip.isA(.serializableObject))
        XCTAssertFalse(clip.isA(.track))
        XCTAssertEqual(try clip.schemaKind(), .clip)
        XCTAssertEqual(try clip.schemaName(), "Clip")
    }

    func testClearingChildrenHandsThemAllBack() throws {
        let document = Document.new()
        defer { document.close() }

        let (_, track, clips) = try makeTimeline(in: document)
        let taken = try track.clearChildren()
        XCTAssertEqual(taken.count, clips.count)
        XCTAssertEqual(try track.childCount(), 0)
        XCTAssertEqual(try taken.map { try $0.name() }, ["A", "B"])
    }

    func testEveryChildAndItsRangeComeBackTogether() throws {
        let document = Document.new()
        defer { document.close() }

        let (_, track, _) = try makeTimeline(in: document)
        let (nodes, ranges) = try track.rangesOfChildren()
        XCTAssertEqual(nodes.count, 2)
        XCTAssertEqual(ranges.count, 2)
        XCTAssertEqual(ranges[0].startTime.toSeconds, 0, accuracy: 1e-9)
        XCTAssertEqual(ranges[1].startTime.toSeconds, 1, accuracy: 1e-9)
    }
}

final class FailureTests: XCTestCase {
    func testAStaleHandleIsRefused() throws {
        let document = Document.new()
        defer { document.close() }

        let clip = try document.newClip("A")
        try document.removeNode(clip)
        XCTAssertThrowsError(try clip.name()) { error in
            XCTAssertEqual(status(of: error), .staleHandle)
        }
    }

    func testAnObjectOfNoDocumentFailsRatherThanCrashing() throws {
        let orphan = SerializableObject.none()
        XCTAssertTrue(orphan.isNone)
        XCTAssertNil(orphan.document)
        XCTAssertThrowsError(try orphan.name())
    }

    func testAnObjectFromAnotherDocumentIsRefused() throws {
        let one = Document.new()
        defer { one.close() }
        let other = Document.new()
        defer { other.close() }

        let track = try one.newTrack("V1", kind: "Video")
        let stranger = try other.newClip("elsewhere")

        XCTAssertThrowsError(try track.appendChild(stranger)) { error in
            XCTAssertEqual(status(of: error), .invalidArgument)
        }
        // A call that cannot fail answers rather than throwing, and the
        // answer is no.
        XCTAssertFalse(one.contains(stranger))
    }
}

final class MetadataTests: XCTestCase {
    func testMetadataGoesInAndComesBack() throws {
        let document = Document.new()
        defer { document.close() }

        let clip = try document.newClip("A")
        try clip.metadata.setString("reel", value: "ZZ100")
        try clip.metadata.setInt("take", value: 3)
        try clip.metadata.setBool("circled", value: true)
        try clip.metadata.setDouble("gain", value: 0.5)

        XCTAssertEqual(try clip.metadata.getString("reel"), "ZZ100")
        XCTAssertEqual(try clip.metadata.getInt("take"), 3)
        XCTAssertEqual(try clip.metadata.getBool("circled"), true)
        XCTAssertEqual(try clip.metadata.getDouble("gain"), 0.5)
        XCTAssertTrue(try clip.metadata.contains("reel"))
        XCTAssertFalse(try clip.metadata.contains("nothing"))

        // A path is followed, not created: the dictionary has to exist
        // before anything can be written inside it.
        try clip.metadata.setDictionary("cmx_3600")
        try clip.metadata.setString("cmx_3600.reel", value: "AX")
        XCTAssertEqual(try clip.metadata.getString("cmx_3600.reel"), "AX")

        try clip.metadata.clear()
        XCTAssertFalse(try clip.metadata.contains("reel"))
    }
}

final class TimeTests: XCTestCase {
    func testTimeValuesComputeWithoutADocument() throws {
        let time = RationalTime(value: 48, rate: 24)
        XCTAssertEqual(time.toSeconds, 2)
        XCTAssertEqual(time.toFrames, 48)
        XCTAssertEqual(time.rescaledTo(48), RationalTime(value: 96, rate: 48))
        XCTAssertEqual(
            RationalTime.durationFromStartEndTime(
                RationalTime(value: 0, rate: 24), endTimeExclusive: time),
            time)
        XCTAssertTrue(time.isValid)
        XCTAssertEqual(try time.toTimecode(), "00:00:02:00")
        XCTAssertEqual(try RationalTime.fromTimecode("00:00:02:00", rate: 24), time)
    }

    func testAnUnreadableTimecodeIsAFailure() {
        XCTAssertThrowsError(try RationalTime.fromTimecode("not a timecode", rate: 24)) { error in
            XCTAssertEqual(status(of: error), .timeError)
        }
    }

    func testARangeAnswersAboutWhatItCovers() {
        let span = TimeRange(
            startTime: RationalTime(value: 0, rate: 24),
            duration: RationalTime(value: 24, rate: 24))
        XCTAssertEqual(span.endTimeExclusive, RationalTime(value: 24, rate: 24))
        XCTAssertTrue(span.containsTime(RationalTime(value: 12, rate: 24)))
        XCTAssertFalse(span.containsTime(RationalTime(value: 24, rate: 24)))
    }
}

final class AbsorbTests: XCTestCase {
    func testAnObjectBuiltOnItsOwnCanJoinATimeline() throws {
        let document = Document.new()
        defer { document.close() }
        let track = try document.newTrack("V1", kind: "Video")

        // A clip built in a document of its own, as a binding that hides the
        // document would build one.
        let workshop = Document.new()
        let clip = try workshop.newClip("guest")

        let translated = try document.absorb(workshop)
        let arrived = try XCTUnwrap(translated[clip])
        XCTAssertTrue(arrived is Clip)
        XCTAssertTrue(arrived.document === document)

        try track.appendChild(arrived)
        XCTAssertEqual(try track.childCount(), 1)
        XCTAssertEqual(try arrived.name(), "guest")
    }
}
