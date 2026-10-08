// Tests for media linkers and hook scripts written in Swift.
//
// The registry is the library's, shared by the whole process, so every test
// here registers under names of its own and unregisters them when it ends.

import XCTest

import OpenTimelineIO

/// A failure a plugin raises, in words of its own.
private struct Offline: Error, CustomStringConvertible {
    var description: String { "the proxies are offline" }
}

/// A failure with no words but its name.
private enum Disk: Error {
    case noDisk
}

/// Something a closure holds, to see when the library lets the closure go.
private final class Token {}

/// Whether a call failed because a plugin, or the registry, said so.
private func isPluginError(_ error: Error, saying words: String = "") -> Bool {
    guard let failure = error as? OTIOError else { return false }
    // Foundation's `contains` answers false for an empty string.
    return failure.status == .pluginError && (words.isEmpty || failure.message.contains(words))
}

/// A two-clip timeline as OTIO JSON, each clip pointing at media under
/// file:///media.
private func pluginCut() throws -> [UInt8] {
    let timeline = try Timeline(name: "Cut")
    let stack = try Stack(name: "tracks")
    let track = try Track(name: "V1", kind: "Video")
    try timeline.setTracks(stack)
    try stack.appendChild(track)
    for name in ["first", "second"] {
        let clip = try Clip(name: name)
        let reference = try ExternalReference(name: name, targetURL: "file:///media/\(name).mov")
        try clip.setMediaReference("DEFAULT_MEDIA", reference: reference)
        try clip.setActiveMediaReferenceKey("DEFAULT_MEDIA")
        try track.appendChild(clip)
    }
    return try OTIO.writeToBytes(.otioJSON, root: timeline)
}

/// The target URL of the first clip's active media.
private func firstURL(_ root: SerializableObject) throws -> String {
    guard let clip = try root.findClips().first as? Clip else {
        XCTFail("the timeline has no clips")
        return ""
    }
    guard let media = try clip.mediaReference() as? ExternalReference else {
        XCTFail("the clip's media is not an external reference")
        return ""
    }
    return try media.targetURL()
}

/// A hook script that writes who ran it into the metadata of what it is
/// handed, under `key`.
private func stamp(_ key: String) -> HookScript {
    return { target, arguments in
        let who = (try? arguments.getString("who")) ?? "nobody"
        guard let named = target as? SerializableObjectWithMetadata else {
            throw OTIOError(status: .invalidArgument, message: "it has no metadata")
        }
        try named.metadata.setString(key, value: who)
        return target
    }
}

final class PluginTests: XCTestCase {
    func testAMediaLinkerWrittenInSwiftLinksEveryClip() throws {
        let written = try pluginCut()
        try OTIO.registerMediaLinker("swift_proxies") { clip, arguments in
            let root = try arguments.getString("root")
            return try ExternalReference(name: "proxy", targetURL: root + "/" + clip.name() + ".mov")
        }
        defer { _ = OTIO.unregisterMediaLinker("swift_proxies") }

        var options = ReadOptions(
            mediaLinker: "swift_proxies", mediaLinkerArguments: "{\"root\": \"/proxies\"}")
        let root = try OTIO.readFromBytes(.otioJSON, data: written, options: options)
        XCTAssertEqual(try firstURL(root), "/proxies/first.mov")

        // Asked not to link, it does not.
        options.doNotLinkMedia = true
        let unlinked = try OTIO.readFromBytes(.otioJSON, data: written, options: options)
        XCTAssertEqual(try firstURL(unlinked), "file:///media/first.mov")
    }

    func testALinkerThatLeavesAClipAloneKeepsItsMedia() throws {
        let written = try pluginCut()
        var seen = 0
        try OTIO.registerMediaLinker("swift_watcher") { _, _ in
            seen += 1
            return nil
        }
        defer { _ = OTIO.unregisterMediaLinker("swift_watcher") }

        let root = try OTIO.readFromBytes(
            .otioJSON, data: written, options: ReadOptions(mediaLinker: "swift_watcher"))
        XCTAssertEqual(seen, 2)
        XCTAssertEqual(try firstURL(root), "file:///media/first.mov")
    }

    func testALinkerThatThrowsStopsTheReadInItsOwnWords() throws {
        let written = try pluginCut()
        try OTIO.registerMediaLinker("swift_offline") { _, _ in throw Offline() }
        defer { _ = OTIO.unregisterMediaLinker("swift_offline") }
        let options = ReadOptions(mediaLinker: "swift_offline")

        XCTAssertThrowsError(try OTIO.readFromBytes(.otioJSON, data: written, options: options)) {
            XCTAssertTrue(isPluginError($0, saying: "the proxies are offline"), "\($0)")
        }

        // Any error at all is caught where the library called in, and told
        // back by its description, rather than crossing into the library.
        try OTIO.registerMediaLinker("swift_offline") { _, _ in throw Disk.noDisk }
        XCTAssertThrowsError(try OTIO.readFromBytes(.otioJSON, data: written, options: options)) {
            XCTAssertTrue(isPluginError($0, saying: "noDisk"), "\($0)")
        }

        // And a linker nobody registered is refused, as upstream refuses one.
        XCTAssertThrowsError(
            try OTIO.readFromBytes(
                .otioJSON, data: written, options: ReadOptions(mediaLinker: "swift_nowhere"))
        ) {
            XCTAssertTrue(isPluginError($0, saying: "swift_nowhere"), "\($0)")
        }
    }

    func testHookScriptsWrittenInSwiftRunAroundReadsAndWrites() throws {
        let written = try pluginCut()
        try OTIO.registerHookScript("swift_stamp_read", script: stamp("read_by"))
        defer { _ = OTIO.unregisterHookScript("swift_stamp_read") }
        try OTIO.attachHookScript("post_adapter_read", script: "swift_stamp_read")
        defer { _ = OTIO.detachHookScript("post_adapter_read", script: "swift_stamp_read") }

        let root = try OTIO.readFromBytes(
            .otioJSON, data: written,
            options: ReadOptions(hookArguments: "{\"who\": \"the Swift test\"}"))
        let timeline = try XCTUnwrap(root as? Timeline)
        XCTAssertEqual(try timeline.metadata.getString("read_by"), "the Swift test")

        // A write runs its hooks on a copy, so the timeline is left alone.
        try OTIO.registerHookScript("swift_stamp_write", script: stamp("written_by"))
        defer { _ = OTIO.unregisterHookScript("swift_stamp_write") }
        try OTIO.attachHookScript("pre_adapter_write", script: "swift_stamp_write")
        defer { _ = OTIO.detachHookScript("pre_adapter_write", script: "swift_stamp_write") }
        let out = try OTIO.writeToBytes(
            .otioJSON, root: root, options: WriteOptions(hookArguments: "{\"who\": \"the writer\"}"))
        XCTAssertTrue(
            String(decoding: out, as: UTF8.self).contains("\"written_by\": \"the writer\""),
            "the write hook did not reach what was written")
        XCTAssertFalse(try timeline.metadata.contains("written_by"))
    }

    func testAHookOfYourOwnRunsWhenAsked() throws {
        let clip = try Clip(name: "A")

        try OTIO.registerHookScript("swift_stamp", script: stamp("stamped_by"))
        defer { _ = OTIO.unregisterHookScript("swift_stamp") }
        try OTIO.attachHookScript("swift_mine", script: "swift_stamp")
        defer { _ = OTIO.detachHookScript("swift_mine", script: "swift_stamp") }
        let result = try clip.runHook("swift_mine", arguments: "{\"who\": \"me\"}")
        XCTAssertEqual(result, clip)
        XCTAssertEqual(try clip.metadata.getString("stamped_by"), "me")

        // A script may hand back a different object to go on with, built on
        // its own.
        try OTIO.registerHookScript("swift_replace") { _, _ in try Clip(name: "replacement") }
        defer { _ = OTIO.unregisterHookScript("swift_replace") }
        try OTIO.attachHookScript("swift_swap", script: "swift_replace")
        defer { _ = OTIO.detachHookScript("swift_swap", script: "swift_replace") }
        let swapped = try clip.runHook("swift_swap")
        XCTAssertEqual(try swapped.name(), "replacement")

        // A script cannot answer with nothing, since its type says it
        // answers an object; the nearest thing, an object of a timeline
        // already released, fails rather than reaching the library.
        try OTIO.registerHookScript("swift_closed") { _, _ in
            let gone = try Clip(name: "gone")
            gone.close()
            return gone
        }
        defer { _ = OTIO.unregisterHookScript("swift_closed") }
        try OTIO.attachHookScript("swift_empty", script: "swift_closed")
        defer { _ = OTIO.detachHookScript("swift_empty", script: "swift_closed") }
        XCTAssertThrowsError(try clip.runHook("swift_empty")) {
            XCTAssertTrue(isPluginError($0), "\($0)")
        }

        XCTAssertThrowsError(try clip.runHook("swift_undeclared")) {
            XCTAssertTrue(isPluginError($0), "\($0)")
        }
    }

    func testAScriptCannotFreeTheTimelineItRunsOn() throws {
        // Under runHook the timeline lent to the script is the caller's own,
        // which the script can reach both as what it is handed and as the
        // object it holds from outside.
        let clip = try Clip(name: "A")
        var refusals: [String] = []
        try OTIO.registerHookScript("swift_vandal") { target, _ in
            let elsewhere = try Track(name: "elsewhere")
            for victim in [target, clip] {
                do {
                    try elsewhere.appendChild(victim)
                    refusals.append("moved")
                } catch let error as OTIOError {
                    refusals.append(error.message)
                }
            }
            target.close()
            clip.close()
            return target
        }
        defer { _ = OTIO.unregisterHookScript("swift_vandal") }
        try OTIO.attachHookScript("swift_vandalise", script: "swift_vandal")
        defer { _ = OTIO.detachHookScript("swift_vandalise", script: "swift_vandal") }

        let result = try clip.runHook("swift_vandalise")
        XCTAssertEqual(refusals.count, 2)
        for refusal in refusals {
            XCTAssertTrue(refusal.contains("a plugin is running on"), refusal)
        }
        // Closing it from inside was let go of, so it is still here.
        XCTAssertEqual(try clip.name(), "A")
        XCTAssertEqual(result, clip)
    }

    func testWhatAScriptIsHandedIsNoUseAfterwards() throws {
        let clip = try Clip(name: "A")
        var kept: SerializableObject?
        try OTIO.registerHookScript("swift_keeper") { target, _ in
            kept = target
            return target
        }
        defer { _ = OTIO.unregisterHookScript("swift_keeper") }
        try OTIO.attachHookScript("swift_keep", script: "swift_keeper")
        defer { _ = OTIO.detachHookScript("swift_keep", script: "swift_keeper") }

        _ = try clip.runHook("swift_keep")
        let held = try XCTUnwrap(kept)
        XCTAssertThrowsError(try held.name())
        XCTAssertEqual(try clip.name(), "A")
    }

    func testUnregisteringSaysWhetherThereWasAnything() throws {
        try OTIO.registerHookScript("swift_brief", script: stamp("x"))
        XCTAssertTrue(OTIO.unregisterHookScript("swift_brief"))
        XCTAssertFalse(OTIO.unregisterHookScript("swift_brief"))

        XCTAssertThrowsError(try OTIO.registerMediaLinker("") { _, _ in nil })
    }

    func testTheLibraryLetsGoOfAClosureWhenItIsDoneWithIt() throws {
        weak var watched: Token?
        do {
            let token = Token()
            watched = token
            try OTIO.registerMediaLinker("swift_kept") { _, _ in
                _ = token
                return nil
            }
        }
        XCTAssertNotNil(watched, "the library let go of a closure it still has")
        XCTAssertTrue(OTIO.unregisterMediaLinker("swift_kept"))
        XCTAssertNil(watched, "unregistering did not release the closure")

        // A registration that fails keeps nothing, and leaks nothing either.
        do {
            let token = Token()
            watched = token
            XCTAssertThrowsError(
                try OTIO.registerHookScript("") { target, _ in
                    _ = token
                    return target
                })
        }
        XCTAssertNil(watched, "a failed registration kept its closure")
    }
}
