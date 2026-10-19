// Tests for otio-rust's extensions, which are not part of OpenTimelineIO.
//
// Enabling one is process-wide and cannot be undone, and every test in the
// target shares the process, so nothing here asserts that one starts off.

import Foundation
import XCTest

import OTIOExtSync
import OpenTimelineIO

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
}
