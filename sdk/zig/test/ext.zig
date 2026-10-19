//! Tests for the extensions under `otio.ext`, which are otio-rust's own and
//! not part of OpenTimelineIO.
//!
//! Enabling an extension is process-wide and cannot be undone, and every
//! test here shares the one process, so nothing assumes one starts off.

const std = @import("std");
const otio = @import("otio");

test "enabling sync through its module turns it on for the core too" {
    try otio.ext.sync.enable();
    try std.testing.expect(try otio.ext.sync.isEnabled());
    try std.testing.expect(try otio.isExtensionEnabled("sync"));

    try std.testing.expectError(
        otio.Error.InvalidArgument,
        otio.enableExtension("no-such-extension"),
    );
    try std.testing.expect(
        std.mem.indexOf(u8, otio.lastErrorMessage(), "no-such-extension") != null,
    );
}
