// Tests for the otio-rust extensions, which are not part of OpenTimelineIO.
//
// Enabling one is process-wide and cannot be undone, and every test runs in
// this one process, so nothing here asserts that an extension starts off.

using System;
using OpenTimelineIO;
using OpenTimelineIO.Ext.Sync;

namespace OpenTimelineIO.Tests;

internal static partial class Program
{
    private static void SyncIsEnabledThroughItsOwnModule()
    {
        SyncExtension.Enable();
        Check(SyncExtension.IsEnabled(), "sync says it is off after enabling it");
        Check(Otio.IsExtensionEnabled("sync"), "the core says sync is off after enabling it");

        string? message = null;
        try
        {
            Otio.EnableExtension("no-such-extension");
        }
        catch (OtioException error)
        {
            CheckEq(error.Status, Status.InvalidArgument, "the unknown extension's status");
            message = error.Message;
        }
        Check(message is not null, "an unknown extension was enabled");
        Check(
            message?.Contains("no-such-extension") ?? false,
            $"the failure does not name the extension: {message}");
    }

    private static readonly (string Name, Action Body)[] ExtensionTests =
    {
        ("sync is enabled through its own module", SyncIsEnabledThroughItsOwnModule),
    };
}
