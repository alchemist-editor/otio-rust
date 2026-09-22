// Tests for the generated Swift SDK.
//
// These are written by hand, not generated. A generator that also wrote its
// own tests would only prove it is self-consistent; what needs proving is
// that the Swift it writes does what a Swift programmer reading it would
// expect, against the same library and the same fixtures the Rust tests use.

import Dispatch
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

/// Why a call did not fail the way a test expected, or nil where it did.
///
/// A test running the call on many threads cannot assert from all of them,
/// so it collects these instead and reports them once everything is done.
private func wrongFailure(
    _ what: String, status expected: Status, saying words: String, _ body: () throws -> Void
) -> String? {
    do {
        try body()
        return "\(what): did not fail"
    } catch let error as OTIOError {
        if error.status == expected && error.message.contains(words) {
            return nil
        }
        return "\(what): \(error.status) \"\(error.message)\""
    } catch {
        return "\(what): \(error)"
    }
}

/// Things several threads report at once, kept behind a lock.
private final class Reports: @unchecked Sendable {
    private let lock = NSLock()
    private var items: [String] = []

    func add(_ item: String) {
        lock.lock()
        defer { lock.unlock() }
        items.append(item)
    }

    var all: [String] {
        lock.lock()
        defer { lock.unlock() }
        return items
    }
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
        // The drop-frame rate is 30000/1001, which is not the 29.97 people
        // write; asking for the nearest SMPTE rate is what turns one into
        // the other.
        XCTAssertFalse(OTIO.isDropFrameRate(29.97))
        XCTAssertTrue(OTIO.isDropFrameRate(OTIO.nearestSMPTETimecodeRate(29.97)))
        XCTAssertFalse(OTIO.isDropFrameRate(24))
        XCTAssertTrue(OTIO.isSMPTETimecodeRate(24))
    }
}

final class ReadingTests: XCTestCase {
    func testReadingAnEDLFindsItsClips() throws {
        let root = try OTIO.readFromFile(.cmx3600, path: screeningEDL)
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

    /// The quickstart in `sdk/swift/README.md` is generated, so nothing
    /// compiles it. This is that example, so that it cannot go stale.
    func testTheQuickstartFromTheReadmeRuns() throws {
        let root = try OTIO.open(screeningEDL)

        var named = 0
        for case let clip as Clip in try root.findClips() {
            _ = try clip.name()
            _ = try clip.duration()
            named += 1
        }
        XCTAssertEqual(named, 9)
    }

    func testOpenWorksOutTheFormatFromTheName() throws {
        let root = try OTIO.open(screeningEDL)
        XCTAssertTrue(try root.name().contains("Example_Screening"))
    }

    func testOpenDeclinesASuffixNoFormatClaims() throws {
        XCTAssertThrowsError(try OTIO.open("/tmp/nothing.wav")) { error in
            XCTAssertEqual(status(of: error), .noValue)
        }
    }

    func testATimelineSurvivesARoundTripThroughJSON() throws {
        let root = try OTIO.open(screeningEDL)
        let text = try root.toJSON(2)
        XCTAssertTrue(text.contains("Timeline"))

        let again = try OTIO.fromJSON(text)
        XCTAssertEqual(try again.findClips().count, 9)
    }

    func testSavingAndOpeningAgainKeepsTheClips() throws {
        let root = try OTIO.open(screeningEDL)
        let path = try temporary("round-trip.otio")
        try OTIO.save(root, to: path)

        let again = try OTIO.open(path)
        XCTAssertEqual(try again.findClips().count, 9)
    }

    func testWritingBytesInEveryFormatTheLibraryKnows() throws {
        let root = try OTIO.open(screeningEDL)
        for format in [Format.otioJSON, .cmx3600] {
            let bytes = try OTIO.writeToBytes(format, root: root)
            XCTAssertFalse(bytes.isEmpty, "\(format) wrote nothing")
        }
    }
}

final class BuildingTests: XCTestCase {
    /// Builds a timeline with one video track holding two clips.
    ///
    /// Every one of these is built on its own, in an arena of its own, and
    /// joins the timeline only when it is appended: five arenas become one,
    /// and the objects held here keep working across every move.
    private func makeTimeline() throws -> (Timeline, Track, [Clip]) {
        let timeline = try Timeline(name: "Assembly")
        let stack = try Stack(name: "tracks")
        try timeline.setTracks(stack)
        let track = try Track(name: "V1", kind: "Video")
        try stack.appendChild(track)

        var clips: [Clip] = []
        for (index, name) in ["A", "B"].enumerated() {
            let clip = try Clip(name: name)
            let start = RationalTime(value: Double(index * 24), rate: 24)
            try clip.setSourceRange(TimeRange(startTime: start, duration: RationalTime(value: 24, rate: 24)))
            try track.appendChild(clip)
            clips.append(clip)
        }
        return (timeline, track, clips)
    }

    func testBuildingATimelineFromNothing() throws {
        let (timeline, track, clips) = try makeTimeline()
        XCTAssertEqual(try track.childCount(), 2)
        XCTAssertEqual(try timeline.findClips().count, 2)
        XCTAssertEqual(try clips[0].name(), "A")
        XCTAssertEqual(try track.kind(), "Video")

        // The whole track is as long as the two clips together.
        XCTAssertEqual(try track.duration().toSeconds, 2, accuracy: 1e-9)
    }

    func testAFreshlyBuiltObjectIsEnabled() throws {
        let clip = try Clip(name: "A")
        XCTAssertTrue(try clip.enabled())
        try clip.setEnabled(false)
        XCTAssertFalse(try clip.enabled())
    }

    func testNoValueIsAnAnswerAndNotAFailure() throws {
        let clip = try Clip(name: "untrimmed")
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
        let clip = try Clip(name: "A")
        XCTAssertTrue(clip.isA(.clip))
        XCTAssertTrue(clip.isA(.item))
        XCTAssertTrue(clip.isA(.composable))
        XCTAssertTrue(clip.isA(.serializableObject))
        XCTAssertFalse(clip.isA(.track))
        XCTAssertEqual(try clip.schemaKind(), .clip)
        XCTAssertEqual(try clip.schemaName(), "Clip")
    }

    func testClearingChildrenHandsThemAllBack() throws {
        let (_, track, clips) = try makeTimeline()
        let taken = try track.clearChildren()
        XCTAssertEqual(taken.count, clips.count)
        XCTAssertEqual(try track.childCount(), 0)
        XCTAssertEqual(try taken.map { try $0.name() }, ["A", "B"])
    }

    func testEveryChildAndItsRangeComeBackTogether() throws {
        let (_, track, _) = try makeTimeline()
        let (nodes, ranges) = try track.rangesOfChildren()
        XCTAssertEqual(nodes.count, 2)
        XCTAssertEqual(ranges.count, 2)
        XCTAssertEqual(ranges[0].startTime.toSeconds, 0, accuracy: 1e-9)
        XCTAssertEqual(ranges[1].startTime.toSeconds, 1, accuracy: 1e-9)
    }
}

/// An object is built in an arena of its own and moves into a timeline's when
/// it is put in one. These are the tests of that move.
final class JoiningTests: XCTestCase {
    func testAnObjectBuiltOnItsOwnCanJoinATimeline() throws {
        let track = try Track(name: "V1", kind: "Video")
        let clip = try Clip(name: "guest")

        // Two arenas until this line, one after it.
        try track.appendChild(clip)

        XCTAssertEqual(try track.childCount(), 1)
        XCTAssertEqual(try clip.name(), "guest")
        XCTAssertTrue(clip.isLive())
        // The object the caller is still holding resolves to the one that
        // moved, not to whatever took its old index.
        XCTAssertEqual(try track.indexOfChild(clip), 0)
        XCTAssertEqual(try track.findClips().first, clip as SerializableObject)
    }

    func testAnEditPutsANewlyBuiltItemIntoATrack() throws {
        let track = try Track(name: "V1", kind: "Video")
        let existing = try Clip(name: "on the timeline")
        try existing.setSourceRange(
            TimeRange(
                startTime: RationalTime(value: 0, rate: 24),
                duration: RationalTime(value: 48, rate: 24)))
        try track.appendChild(existing)

        // The edit is anchored on the composition, not on the item it
        // places: anchoring it the other way round would make the call in
        // the new clip's own arena and then refuse the track for being
        // somewhere else.
        let arriving = try Clip(name: "arriving")
        try arriving.setSourceRange(
            TimeRange(
                startTime: RationalTime(value: 0, rate: 24),
                duration: RationalTime(value: 24, rate: 24)))
        try OTIO.insert(
            arriving, composition: track, time: RationalTime(value: 24, rate: 24),
            removeTransitions: false)

        XCTAssertEqual(try track.findClips().count, 3)
        XCTAssertEqual(try arriving.name(), "arriving")
    }

    /// An arena an absorb consumed is freed by the C interface itself, so
    /// nothing here may free it again, and an object still naming it has to
    /// be followed to where its object went rather than left dangling.
    func testAnObjectOfAnAbsorbedTimelineFollowsIt() throws {
        let timeline = try Timeline(name: "cut")
        let stack = try Stack(name: "tracks")
        try timeline.setTracks(stack)
        let track = try Track(name: "V1", kind: "Video")
        let clip = try Clip(name: "guest")

        // Four arenas, joined in an order that leaves a chain: the clip's
        // went into the track's, and the track's into the timeline's.
        try track.appendChild(clip)
        try stack.appendChild(track)

        XCTAssertEqual(try clip.name(), "guest")
        XCTAssertEqual(try timeline.findClips().count, 1)
        XCTAssertEqual(try timeline.findClips().first, clip as SerializableObject)
        XCTAssertEqual(try timeline.name(), "cut")
    }
}

final class FailureTests: XCTestCase {
    func testAStaleHandleIsRefused() throws {
        let track = try Track(name: "V1", kind: "Video")
        let clip = try Clip(name: "A")
        try track.appendChild(clip)

        try clip.removeFromTimeline()
        XCTAssertThrowsError(try clip.name()) { error in
            XCTAssertEqual(status(of: error), .staleHandle)
        }
    }

    func testAnObjectOfNoTimelineFailsRatherThanCrashing() throws {
        let orphan = SerializableObject.none()
        XCTAssertTrue(orphan.isNone)
        XCTAssertThrowsError(try orphan.name())
    }

    /// A handle is an index into one arena, and two arenas issue the same
    /// indices, so an object from one would resolve to an unrelated object
    /// in the other rather than failing. A call that only names an object
    /// therefore has to refuse one from elsewhere — and refuse it before
    /// asking the library, because absorbing first and failing afterwards
    /// would already have merged the two timelines.
    func testAnObjectFromAnotherTimelineIsRefused() throws {
        let track = try Track(name: "V1", kind: "Video")
        let mine = try Clip(name: "mine")
        try track.appendChild(mine)

        let elsewhere = try Track(name: "V2", kind: "Video")
        let stranger = try Clip(name: "elsewhere")
        try elsewhere.appendChild(stranger)

        XCTAssertThrowsError(try track.detachChild(stranger)) { error in
            XCTAssertEqual(status(of: error), .invalidArgument)
        }
        XCTAssertThrowsError(try track.indexOfChild(stranger)) { error in
            XCTAssertEqual(status(of: error), .invalidArgument)
        }
        XCTAssertThrowsError(try track.hasChild(stranger)) { error in
            XCTAssertEqual(status(of: error), .invalidArgument)
        }
        XCTAssertThrowsError(try OTIO.flattenTracks([track, elsewhere])) { error in
            XCTAssertEqual(status(of: error), .invalidArgument)
        }

        // A call that cannot fail answers rather than throwing, and the
        // answer is no.
        XCTAssertFalse(track.equals(stranger))

        // What the refusal is protecting, and the only assertion that tells
        // a refusal apart from an absorb that failed afterwards: the two
        // timelines are still independent, so releasing this one leaves the
        // other whole.
        track.close()
        XCTAssertEqual(try elsewhere.childCount(), 1)
        XCTAssertEqual(try stranger.name(), "elsewhere")
    }

    /// The refusal of another timeline's object carries the same status the
    /// library answers a bad argument with, so the error says which one it
    /// is: a caller, or a test, can tell the SDK's refusal from the
    /// library's failure without reading the sentence.
    func testARefusalOfAnotherTimelineSaysSo() throws {
        let track = try Track(name: "V1", kind: "Video")
        let elsewhere = try Track(name: "V2", kind: "Video")
        let stranger = try Clip(name: "elsewhere")
        try elsewhere.appendChild(stranger)

        // One object named, and a list drawn from two timelines.
        XCTAssertThrowsError(try track.detachChild(stranger)) { error in
            XCTAssertEqual((error as? OTIOError)?.isOtherTimeline, true)
            XCTAssertEqual(status(of: error), .invalidArgument)
        }
        XCTAssertThrowsError(try OTIO.flattenTracks([track, elsewhere])) { error in
            XCTAssertEqual((error as? OTIOError)?.isOtherTimeline, true)
        }

        // A failure the library reported is not one.
        let clip = try Clip(name: "A")
        try track.appendChild(clip)
        try clip.removeFromTimeline()
        XCTAssertThrowsError(try clip.name()) { error in
            XCTAssertEqual((error as? OTIOError)?.isOtherTimeline, false)
            XCTAssertEqual(status(of: error), .staleHandle)
        }
        // Nor is one a caller makes for itself.
        XCTAssertFalse(OTIOError(status: .invalidArgument, message: "mine").isOtherTimeline)
    }

    /// Closing a timeline nulls the document the C interface knows, and the
    /// C interface refuses a null one, so every object that lived there
    /// fails rather than reading freed memory.
    func testAnObjectOutlivingItsTimelineFailsRatherThanCrashing() throws {
        let track = try Track(name: "V1", kind: "Video")
        let clip = try Clip(name: "A")
        try track.appendChild(clip)

        track.close()
        XCTAssertThrowsError(try clip.name()) { error in
            XCTAssertEqual(status(of: error), .nullPointer)
        }
        // Closing twice is harmless.
        clip.close()
        XCTAssertFalse(clip.isLive())
    }

    /// The library hands each call's message back beside the status it
    /// returns, so a failure carries the sentence its own call wrote and not
    /// one some other call left behind. Two different failures are made over
    /// and over on many threads at once, and every one of them has to come
    /// back with its own status and its own message.
    func testEveryFailureCarriesItsOwnMessageWhateverThreadItRanOn() throws {
        let track = try Track(name: "V1", kind: "Video")
        let clip = try Clip(name: "A")
        try track.appendChild(clip)
        try clip.removeFromTimeline()

        let reports = Reports()
        DispatchQueue.concurrentPerform(iterations: 400) { index in
            let wrong: String?
            if index % 2 == 0 {
                wrong = wrongFailure("timecode", status: .timeError, saying: "invalid timecode") {
                    _ = try RationalTime.fromTimecode("not a timecode", rate: 24)
                }
            } else {
                wrong = wrongFailure(
                    "removed clip", status: .staleHandle, saying: "no longer exists"
                ) {
                    _ = try clip.name()
                }
            }
            if let wrong {
                reports.add(wrong)
            }
        }
        XCTAssertEqual(reports.all, [])
    }
}

final class MetadataTests: XCTestCase {
    func testMetadataGoesInAndComesBack() throws {
        let clip = try Clip(name: "A")
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
    func testTimeValuesComputeWithoutATimeline() throws {
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
