// Tests for the otio-rust extensions, which are not part of OpenTimelineIO.
//
// Enabling one is process-wide and cannot be undone, and every test runs in
// this one process, in the order listed. So the one test that wants sync off
// runs first, and only asserts what it can when nothing turned sync on
// before this program started.

using System;
using System.IO;
using System.Linq;
using System.Text.Json;
using OpenTimelineIO;
using OpenTimelineIO.Ext.Sync;

namespace OpenTimelineIO.Tests;

internal static partial class Program
{
    /// The cut the apply-patches sample edits: timeline "cut", track V1,
    /// clips A and B, with no GUIDs yet.
    private const string Cut = """
        {
          "OTIO_SCHEMA": "Timeline.1",
          "name": "cut",
          "tracks": {"OTIO_SCHEMA": "Stack.1", "children": [
            {"OTIO_SCHEMA": "Track.1", "name": "V1", "kind": "Video", "children": [
              {"OTIO_SCHEMA": "Clip.2", "name": "A"},
              {"OTIO_SCHEMA": "Clip.2", "name": "B"}]}]}
        }
        """;

    private static void SyncRefusesToWorkBeforeItIsEnabled()
    {
        if (SyncExtension.IsEnabled())
        {
            Console.WriteLine("  (sync was already on; nothing to check)");
            return;
        }
        CheckEq(Threw(() => SyncExtension.NewGuid()), Status.PluginError, "a new GUID before enabling sync");
        CheckEq(
            Threw(() => SyncExtension.Diff("{}", "{}", false)),
            Status.PluginError,
            "a diff before enabling sync");
    }

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

    /// Whether two pieces of JSON say the same thing: objects by key whatever
    /// their order, numbers by value.
    private static bool JsonEquals(JsonElement left, JsonElement right)
    {
        if (left.ValueKind != right.ValueKind)
        {
            return false;
        }
        switch (left.ValueKind)
        {
            case JsonValueKind.Object:
                var leftProperties = left.EnumerateObject().ToList();
                var rightProperties = right.EnumerateObject().ToDictionary(property => property.Name, property => property.Value);
                return leftProperties.Count == rightProperties.Count
                    && leftProperties.All(property =>
                        rightProperties.TryGetValue(property.Name, out var value) && JsonEquals(property.Value, value));
            case JsonValueKind.Array:
                var leftItems = left.EnumerateArray().ToList();
                var rightItems = right.EnumerateArray().ToList();
                return leftItems.Count == rightItems.Count
                    && leftItems.Zip(rightItems).All(pair => JsonEquals(pair.First, pair.Second));
            case JsonValueKind.Number:
                return left.GetDouble() == right.GetDouble();
            case JsonValueKind.String:
                return left.GetString() == right.GetString();
            default:
                return true;
        }
    }

    private static void SyncMatchesFurnacesGoldenDiffs()
    {
        SyncExtension.Enable();
        var fixtures = Path.Combine(
            Repository(), "extensions", "sync", "otio-ext-sync", "tests", "fixtures", "otiodiff");
        string[] fallsBack = ["generation_regression", "guidless_transition", "metadata_sync_threshold_fallback", "sequence_changed"];
        var ran = 0;
        foreach (var directory in Directory.GetDirectories(fixtures).OrderBy(path => path, StringComparer.Ordinal))
        {
            var name = Path.GetFileName(directory);
            var old = File.ReadAllText(Path.Combine(directory, "old.json"));
            var @new = File.ReadAllText(Path.Combine(directory, "new.json"));
            var golden = File.ReadAllText(Path.Combine(directory, "patches.json"));

            CheckEq(SyncExtension.Diff(old, @new, true) + "\n", golden, $"{name}: the pretty patches");

            var stats = SyncExtension.DiffStats(old, @new);
            var fellBack = fallsBack.Contains(name);
            Check(
                stats.Contains($"\"fellBack\":{(fellBack ? "true" : "false")}"),
                $"{name}: the stats should say fellBack {fellBack}: {stats}");

            var compact = SyncExtension.Diff(old, @new, false);
            using var oldDocument = JsonDocument.Parse(old);
            using var newDocument = JsonDocument.Parse(@new);
            var applied = SyncExtension.Apply(oldDocument.RootElement.GetProperty("otioJson").GetRawText(), compact);
            using var appliedDocument = JsonDocument.Parse(applied);
            Check(
                JsonEquals(appliedDocument.RootElement, newDocument.RootElement.GetProperty("otioJson")),
                $"{name}: applying the patches to the old timeline does not give the new one");
            ran += 1;
        }
        CheckEq(ran, 11, "the golden cases that ran");
    }

    private static void ASyncTransactionCarriesToAnotherCopy()
    {
        SyncExtension.Enable();
        var editor = Otio.FromJson(Cut);
        var before = SyncExtension.Begin(editor, "main", 1);
        var viewer = Otio.FromJson(editor.ToJson(0));

        editor.SetName("fine cut");
        var patches = SyncExtension.Commit(editor, before);
        Check(patches.Contains("fine cut"), $"the patches do not carry the new name: {patches}");

        SyncExtension.ApplyTo(viewer, patches);
        CheckEq(viewer.Name(), "fine cut", "the name the viewer's own timeline reports");

        var guid = SyncExtension.Guid(editor);
        Check(!string.IsNullOrEmpty(guid), "the editor's timeline has no GUID after beginning");
        var found = SyncExtension.Find(viewer, guid ?? "");
        Check(found is not null && found.Equals(viewer), "finding the timeline's GUID in the viewer");
        CheckEq(SyncExtension.Stamp(viewer), 0, "the GUIDs stamping the stamped copy gives");
        Check(SyncExtension.Find(viewer, "nobody") is null, "finding a GUID nothing carries");

        var fresh = SyncExtension.NewGuid();
        CheckEq(fresh.Length, 36, "a new GUID's length");
        var clip = Otio.FromJson("""{"OTIO_SCHEMA": "Clip.2", "name": "C"}""");
        Check(SyncExtension.Guid(clip) is null, "a clip that was never stamped has a GUID");
        SyncExtension.SetGuid(clip, fresh);
        CheckEq(SyncExtension.Guid(clip), fresh, "the GUID a clip was given");
    }

    private static void ASyncTransactionRollsBackAndTheJournalRecords()
    {
        SyncExtension.Enable();
        var timeline = Otio.FromJson(Cut);
        var before = SyncExtension.Begin(timeline, "main", 1);
        timeline.SetName("fine cut");
        SyncExtension.Rollback(timeline, before);
        CheckEq(timeline.Name(), "cut", "the name after rolling back");

        SyncExtension.StartJournal(timeline);
        Check(!SyncExtension.CommitJournal(timeline), "the journal recorded a change before any edit");
        timeline.SetName("fine cut");
        Check(SyncExtension.CommitJournal(timeline), "the journal missed a rename");
        Check(!SyncExtension.CommitJournal(timeline), "the journal kept a change it had committed");
        SyncExtension.StopJournal(timeline);
    }

    private static readonly (string Name, Action Body)[] ExtensionTests =
    {
        ("sync refuses to work before it is enabled", SyncRefusesToWorkBeforeItIsEnabled),
        ("sync is enabled through its own module", SyncIsEnabledThroughItsOwnModule),
        ("sync matches Furnace's golden diffs", SyncMatchesFurnacesGoldenDiffs),
        ("a sync transaction carries to another copy", ASyncTransactionCarriesToAnotherCopy),
        ("a sync transaction rolls back and the journal records", ASyncTransactionRollsBackAndTheJournalRecords),
    };
}
