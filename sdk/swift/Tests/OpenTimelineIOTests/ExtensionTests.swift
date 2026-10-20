// Tests for otio-rust's extensions, which are not part of OpenTimelineIO.
//
// Enabling one is process-wide and cannot be undone, and every test in the
// target shares the process, so nothing here asserts that one starts off:
// XCTest promises neither the order the tests run in nor that a filtered or
// parallel run leaves this file's tests alone, so a check that a call is
// refused before sync is enabled cannot be made reliably here. The C ABI's
// own Rust tests make it.

import Foundation
import XCTest

import OTIOExtSync
import OpenTimelineIO

/// The directory one above a path.
private func parent(_ path: String) -> String {
    guard let slash = path.lastIndex(of: "/") else { return path }
    return String(path[path.startIndex..<slash])
}

/// The repository, found from where this file sits in it:
/// `<root>/sdk/swift/Tests/OpenTimelineIOTests/ExtensionTests.swift`.
private let repository = parent(parent(parent(parent(parent(#filePath)))))

/// Furnace's golden corpus, which the Rust crate's own tests read too.
private let corpus = repository + "/extensions/sync/otio-ext-sync/tests/fixtures/otiodiff"

/// The cases Furnace answers with one `replace_timeline` patch.
private let fallsBack: Set<String> = [
    "generation_regression",
    "guidless_transition",
    "metadata_sync_threshold_fallback",
    "sequence_changed",
]

/// The cut the apply-patches sample edits. Nothing in it has a GUID yet.
private let cut = """
    {"OTIO_SCHEMA": "Timeline.1", "name": "cut",
     "tracks": {"OTIO_SCHEMA": "Stack.1", "children": [
       {"OTIO_SCHEMA": "Track.1", "name": "V1", "kind": "Video", "children": [
         {"OTIO_SCHEMA": "Clip.2", "name": "A"},
         {"OTIO_SCHEMA": "Clip.2", "name": "B"}]}]}}
    """

/// Parses JSON text into Foundation's values.
private func parsed(_ text: String) throws -> Any {
    try JSONSerialization.jsonObject(with: Data(text.utf8), options: [.fragmentsAllowed])
}

/// Whether two parsed JSON values say the same thing, numbers by value.
private func sameJSON(_ left: Any, _ right: Any) -> Bool {
    switch (left, right) {
    case let (left as [String: Any], right as [String: Any]):
        return left.count == right.count
            && left.allSatisfy { key, value in right[key].map { sameJSON(value, $0) } ?? false }
    case let (left as [Any], right as [Any]):
        return left.count == right.count && zip(left, right).allSatisfy { sameJSON($0, $1) }
    case let (left as String, right as String):
        return left == right
    case (is NSNull, is NSNull):
        return true
    case let (left as NSNumber, right as NSNumber):
        return left.doubleValue == right.doubleValue
    default:
        return false
    }
}

/// The timeline a snapshot carries, as JSON text of its own.
private func timelineOf(_ snapshot: String) throws -> String {
    guard let object = try parsed(snapshot) as? [String: Any], let timeline = object["otioJson"] else {
        throw OTIOError(status: .invalidArgument, message: "no otioJson in the snapshot")
    }
    return String(decoding: try JSONSerialization.data(withJSONObject: timeline), as: UTF8.self)
}

/// What Furnace sends a viewer joining a session whose host shared one
/// timeline.
private let viewerWelcome = """
    {"v":2,"type":"welcome","sessionId":"s_1","seq":0,"ts":0,"order":1,"payload":{"selfId":"p_2","role":"viewer","roster":[{"id":"p_1","displayName":"Host","role":"host","joinedAt":0,"connectionState":"connected","muted":false}],"session":{"id":"s_1","createdAt":0},"snapshot":{"sequenceId":"main","generation":1,"otioJson":{"OTIO_SCHEMA":"Timeline.1","name":"Coastline","metadata":{"sync":{"guid":"t-1"}},"tracks":{"OTIO_SCHEMA":"Stack.1","metadata":{"sync":{"guid":"s-1"}},"children":[{"OTIO_SCHEMA":"Track.1","name":"V1","kind":"Video","metadata":{"sync":{"guid":"v-1"}},"children":[{"OTIO_SCHEMA":"Clip.2","name":"Waves","metadata":{"sync":{"guid":"c-1"}}}]}]}}}}}
    """

/// What Furnace sends the host of a new session.
private let hostWelcome = """
    {"v":2,"type":"welcome","sessionId":"s_1","seq":0,"ts":0,"order":1,"payload":{"selfId":"p_1","role":"host","roster":[],"session":{"id":"s_1","createdAt":0}}}
    """

/// Every frame `session` has to send now, as text, and whether each is binary.
private func drained(_ session: UInt64) throws -> [(text: String, binary: Bool)] {
    var frames: [(text: String, binary: Bool)] = []
    while let frame = try Sync.sessionPollTransmit(session, now: 0) {
        frames.append((text: String(decoding: frame.bytes, as: UTF8.self), binary: frame.binary))
    }
    return frames
}

/// Every event `session` has for the program now.
private func events(_ session: UInt64) throws -> [String] {
    var events: [String] = []
    while let event = try Sync.sessionPollEvent(session) {
        events.append(event)
    }
    return events
}

/// Asserts `body` throws an `OTIOError` with `status`.
private func assertFails(
    _ status: Status, _ body: @autoclosure () throws -> some Any,
    file: StaticString = #filePath, line: UInt = #line
) {
    XCTAssertThrowsError(try body(), file: file, line: line) {
        XCTAssertEqual(($0 as? OTIOError)?.status, status, "\($0)", file: file, line: line)
    }
}

final class ExtensionTests: XCTestCase {
    func testSyncIsEnabledThroughItsOwnModuleAndSeenByTheCore() throws {
        try Sync.enable()
        XCTAssertTrue(try Sync.isEnabled())
        XCTAssertTrue(try OTIO.isExtensionEnabled("sync"))

        // An id the library was not built with fails as every other call
        // fails, with an `OTIOError` naming what it was asked for.
        XCTAssertThrowsError(try OTIO.enableExtension("no-such-extension")) {
            guard let failure = $0 as? OTIOError else {
                return XCTFail("not an OTIOError: \($0)")
            }
            XCTAssertEqual(failure.status, .invalidArgument)
            XCTAssertTrue(failure.message.contains("no-such-extension"), failure.message)
        }
    }

    /// Diffs and applies every case of Furnace's corpus exactly as Furnace
    /// does: the patches byte for byte, the fallbacks where it falls back,
    /// and applying the patches lands on the new timeline.
    func testSyncMatchesFurnacesGoldenCorpus() throws {
        try Sync.enable()
        let cases = try FileManager.default.contentsOfDirectory(atPath: corpus)
            .filter { name in
                var directory: ObjCBool = false
                return FileManager.default.fileExists(atPath: corpus + "/" + name, isDirectory: &directory)
                    && directory.boolValue
            }
            .sorted()
        for name in cases {
            let directory = corpus + "/" + name
            let old = try String(contentsOfFile: directory + "/old.json", encoding: .utf8)
            let new = try String(contentsOfFile: directory + "/new.json", encoding: .utf8)
            let want = try String(contentsOfFile: directory + "/patches.json", encoding: .utf8)

            XCTAssertEqual(try Sync.diff(old, newSnapshot: new, pretty: true) + "\n", want, "\(name): patches")

            let stats = try Sync.diffStats(old, newSnapshot: new)
            let fellBack = fallsBack.contains(name)
            XCTAssertTrue(stats.contains("\"fellBack\":\(fellBack)"), "\(name): \(stats)")

            let compact = try Sync.diff(old, newSnapshot: new, pretty: false)
            let applied = try Sync.apply(try timelineOf(old), patches: compact)
            XCTAssertTrue(
                sameJSON(try parsed(applied), try parsed(try timelineOf(new))), "\(name): apply")
        }
        XCTAssertEqual(cases.count, 11)
    }

    /// A transaction's patches carry an edit to another copy of the
    /// timeline, which applies them in place.
    func testSyncCarriesATransactionToAnotherCopy() throws {
        try Sync.enable()
        let editor = try OTIO.fromJSON(cut)
        let before = try Sync.begin(editor, sequenceID: "main", generation: 1)
        XCTAssertTrue(before.contains("\"sequenceId\""), before)

        // A second copy, read from the stamped timeline: the same objects,
        // under the same GUIDs.
        let viewer = try OTIO.fromJSON(try editor.toJSON(0))
        XCTAssertTrue(viewer is Timeline)

        try editor.setName("fine cut")
        let patches = try Sync.commit(editor, before: before)
        XCTAssertTrue(patches.contains("\"path\":\"/name\""), patches)
        try Sync.applyTo(viewer, patches: patches)
        XCTAssertEqual(try viewer.name(), "fine cut", "the object held before applying is the one renamed")

        let guid = try XCTUnwrap(try Sync.guid(editor))
        XCTAssertFalse(guid.isEmpty)
        let found = try XCTUnwrap(try Sync.find(viewer, guid: guid))
        XCTAssertEqual(found, viewer)
        XCTAssertTrue(found is Timeline)
        XCTAssertEqual(try Sync.stamp(viewer), 0)
        XCTAssertNil(try Sync.find(viewer, guid: "nobody"))

        // A GUID can be read, given and taken from a fresh one.
        let clip = try Clip(name: "C")
        XCTAssertNil(try Sync.guid(clip))
        let fresh = try Sync.newGuid()
        XCTAssertFalse(fresh.isEmpty)
        try Sync.setGuid(clip, guid: fresh)
        XCTAssertEqual(try Sync.guid(clip), fresh)
    }

    /// Rolling back puts the timeline back as it was, and the journal says
    /// whether anything changed since it was last asked.
    func testSyncRollsBackAndJournals() throws {
        try Sync.enable()
        let timeline = try OTIO.fromJSON(cut)
        let before = try Sync.begin(timeline, sequenceID: "main", generation: 1)
        try timeline.setName("scrapped")
        try Sync.rollback(timeline, before: before)
        XCTAssertEqual(try timeline.name(), "cut")

        try Sync.startJournal(timeline)
        XCTAssertFalse(try Sync.commitJournal(timeline))
        try timeline.setName("journalled")
        XCTAssertTrue(try Sync.commitJournal(timeline))
        XCTAssertFalse(try Sync.commitJournal(timeline), "a commit starts a fresh record")
        try Sync.stopJournal(timeline)
    }

    /// A viewer joins: it says hello, takes the host's timeline from the
    /// welcome, and sends its playhead once it has joined.
    func testSyncSessionJoinsAsAViewer() throws {
        try Sync.enable()
        let session = try Sync.sessionNew(
            #"{"token":"join-code","identity":{"displayName":"Ada"},"encodings":[]}"#)
        XCTAssertNil(try Sync.sessionPollTransmit(session, now: 0), "nothing to send before connecting")

        try Sync.sessionConnect(session)
        let hello = try XCTUnwrap(try Sync.sessionPollTransmit(session, now: 0))
        XCTAssertFalse(hello.binary)
        let text = String(decoding: hello.bytes, as: UTF8.self)
        XCTAssertTrue(text.contains("\"type\":\"hello\""), text)
        XCTAssertTrue(text.contains("Ada"), text)
        XCTAssertNil(try Sync.sessionPollTransmit(session, now: 0))

        try Sync.sessionReceive(session, data: Array(viewerWelcome.utf8), binary: false)
        let arrived = try events(session)
        XCTAssertEqual(arrived.count, 2, "\(arrived)")
        XCTAssertTrue(arrived[0].contains("\"type\":\"timeline\""), arrived[0])
        XCTAssertTrue(arrived[0].contains("\"joining\":true"), arrived[0])
        XCTAssertTrue(arrived[1].contains("\"type\":\"joined\""), arrived[1])

        let timeline = try OTIO.fromJSON(#"{"OTIO_SCHEMA":"Timeline.1"}"#)
        XCTAssertEqual(try Sync.handle(timeline, event: arrived[0]), #"{"handled":"applied"}"#)
        XCTAssertEqual(try timeline.name(), "Coastline")
        let tracks = try XCTUnwrap(try (timeline as? Timeline)?.tracks())
        let children = try tracks.children()
        XCTAssertEqual(children.count, 1)
        let track = try XCTUnwrap(children.first as? Track)
        let clips = try track.children()
        XCTAssertEqual(clips.count, 1)
        XCTAssertEqual(try (clips.first as? Clip)?.name(), "Waves")
        XCTAssertEqual(try Sync.handle(timeline, event: arrived[1]), #"{"handled":"joined"}"#)

        let state = try Sync.sessionState(session)
        XCTAssertTrue(state.contains("\"role\":\"viewer\""), state)
        XCTAssertTrue(state.contains("\"joined\":true"), state)
        let snapshot = try XCTUnwrap(try Sync.sessionTimeline(session, sequenceID: "main"))
        XCTAssertTrue(snapshot.contains("\"sequenceId\":\"main\""), snapshot)
        XCTAssertNil(try Sync.sessionTimeline(session, sequenceID: "other"))

        XCTAssertFalse(try Sync.sessionMaySend(session, channel: "timeline"))
        assertFails(.invalidArgument, try Sync.sessionPublish(session, timeline: timeline, sequenceID: "main"))

        try Sync.sessionSetPlayhead(session, sequenceID: "main", frame: 12, playing: true, rate: 1)
        let sent = try drained(session)
        XCTAssertTrue(sent.contains { $0.text.contains("\"type\":\"presence\"") }, "\(sent)")

        try Sync.sessionClose(session)
        try Sync.sessionClose(session)  // closing one already closed does nothing
        XCTAssertThrowsError(try Sync.sessionState(session), "a closed session is gone")
    }

    /// The host publishes its timeline: the whole of it first, then only
    /// what changed, and nothing when nothing did.
    func testSyncSessionPublishesAsTheHost() throws {
        try Sync.enable()
        let session = try Sync.sessionNew(#"{"token":"host-token","encodings":[]}"#)
        try Sync.sessionConnect(session)
        _ = try drained(session)
        try Sync.sessionReceive(session, data: Array(hostWelcome.utf8), binary: false)
        let first = try XCTUnwrap(try Sync.sessionPollEvent(session))
        XCTAssertTrue(first.contains("\"type\":\"joined\""), first)
        XCTAssertTrue(first.contains("\"role\":\"host\""), first)

        let timeline = try OTIO.fromJSON(cut)
        let patches = try Sync.sessionPublish(session, timeline: timeline, sequenceID: "main")
        XCTAssertTrue(patches.contains("replace_timeline"), patches)
        let sent = try drained(session)
        XCTAssertTrue(sent.contains { $0.text.contains("\"type\":\"snapshot\"") }, "\(sent)")

        XCTAssertEqual(try Sync.sessionPublish(session, timeline: timeline, sequenceID: "main"), "[]")
        try timeline.setName("fine cut")
        let renamed = try Sync.sessionPublish(session, timeline: timeline, sequenceID: "main")
        XCTAssertTrue(renamed.contains("set_property"), renamed)
        try Sync.sessionClose(session)
    }

    func testSyncSessionRefusesBadSettings() throws {
        try Sync.enable()
        XCTAssertThrowsError(try Sync.sessionNew(#"{"encodings":["zstd"]}"#))
    }
}
