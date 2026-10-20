// Tests for the otio-rust extensions, which are not part of OpenTimelineIO.
//
// Enabling one is process-wide and cannot be undone, and every test runs in
// this one process, in the order listed. So the one test that wants sync off
// runs first, and only asserts what it can when nothing turned sync on
// before this program started.

using System;
using System.Collections.Generic;
using System.IO;
using System.Linq;
using System.Text;
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

    /// What the server sends a viewer joining a session whose host shared one
    /// timeline, "Coastline", as the sequence "main".
    private const string ViewerWelcome = """{"v":2,"type":"welcome","sessionId":"s_1","seq":0,"ts":0,"order":1,"payload":{"selfId":"p_2","role":"viewer","roster":[{"id":"p_1","displayName":"Host","role":"host","joinedAt":0,"connectionState":"connected","muted":false}],"session":{"id":"s_1","createdAt":0},"snapshot":{"sequenceId":"main","generation":1,"otioJson":{"OTIO_SCHEMA":"Timeline.1","name":"Coastline","metadata":{"sync":{"guid":"t-1"}},"tracks":{"OTIO_SCHEMA":"Stack.1","metadata":{"sync":{"guid":"s-1"}},"children":[{"OTIO_SCHEMA":"Track.1","name":"V1","kind":"Video","metadata":{"sync":{"guid":"v-1"}},"children":[{"OTIO_SCHEMA":"Clip.2","name":"Waves","metadata":{"sync":{"guid":"c-1"}}}]}]}}}}}""";

    /// What the server sends the host of a new session.
    private const string HostWelcome = """{"v":2,"type":"welcome","sessionId":"s_1","seq":0,"ts":0,"order":1,"payload":{"selfId":"p_1","role":"host","roster":[],"session":{"id":"s_1","createdAt":0}}}""";

    /// Every frame a session has to send now, as text.
    private static List<(string Text, bool Binary)> Transmit(ulong session)
    {
        var frames = new List<(string, bool)>();
        while (SyncExtension.SessionPollTransmit(session, 0) is var (bytes, binary))
        {
            frames.Add((Encoding.UTF8.GetString(bytes), binary));
        }
        return frames;
    }

    /// Every event a session has for the program now.
    private static List<string> Events(ulong session)
    {
        var events = new List<string>();
        while (SyncExtension.SessionPollEvent(session) is string occurrence)
        {
            events.Add(occurrence);
        }
        return events;
    }

    private static void AViewerJoinsASessionAndReceivesItsTimeline()
    {
        SyncExtension.Enable();
        var session = SyncExtension.SessionNew("""{"token":"join-code","identity":{"displayName":"Ada"},"encodings":[]}""");
        Check(SyncExtension.SessionPollTransmit(session, 0) is null, "a frame to send before connecting");

        SyncExtension.SessionConnect(session);
        var hello = SyncExtension.SessionPollTransmit(session, 0);
        Check(hello is not null, "no hello to send after connecting");
        if (hello is var (helloBytes, helloBinary))
        {
            var text = Encoding.UTF8.GetString(helloBytes);
            Check(!helloBinary, "the hello is a binary frame");
            Check(text.Contains("\"type\":\"hello\""), $"the first frame is not a hello: {text}");
            Check(text.Contains("Ada"), $"the hello does not name the viewer: {text}");
        }
        Check(SyncExtension.SessionPollTransmit(session, 0) is null, "a second frame after the hello");

        SyncExtension.SessionReceive(session, Encoding.UTF8.GetBytes(ViewerWelcome), false);
        var events = Events(session);
        CheckEq(events.Count, 2, "the events the welcome gives");
        if (events.Count == 2)
        {
            Check(events[0].Contains("\"type\":\"timeline\""), $"the first event is not a timeline: {events[0]}");
            Check(events[0].Contains("\"joining\":true"), $"the timeline event is not the joining one: {events[0]}");
            Check(events[1].Contains("\"type\":\"joined\""), $"the second event is not joined: {events[1]}");

            var timeline = Otio.FromJson("""{"OTIO_SCHEMA":"Timeline.1"}""");
            CheckEq(SyncExtension.Handle(timeline, events[0]), """{"handled":"applied"}""", "handling the timeline event");
            CheckEq(timeline.Name(), "Coastline", "the name of the timeline that arrived");
            var stack = (timeline as Timeline)?.Tracks();
            Check(stack is not null, "the timeline that arrived has no tracks");
            if (stack is not null)
            {
                CheckEq(stack.ChildCount(), 1, "the tracks that arrived");
                var track = stack.ChildAt(0);
                CheckEq(track.ChildCount(), 1, "the clips in the track that arrived");
                CheckEq(track.ChildAt(0).Name(), "Waves", "the clip that arrived");
            }
            CheckEq(SyncExtension.Handle(timeline, events[1]), """{"handled":"joined"}""", "handling the joined event");
        }

        var state = SyncExtension.SessionState(session);
        Check(state.Contains("\"role\":\"viewer\""), $"the state does not say viewer: {state}");
        Check(state.Contains("\"joined\":true"), $"the state does not say joined: {state}");
        var main = SyncExtension.SessionTimeline(session, "main");
        Check(main?.Contains("\"sequenceId\":\"main\"") ?? false, $"the session's copy of main: {main}");
        Check(SyncExtension.SessionTimeline(session, "other") is null, "the session holds a sequence nobody shared");

        Check(!SyncExtension.SessionMaySend(session, "timeline"), "a viewer may send on the timeline channel");
        var mine = Otio.FromJson(Cut);
        CheckEq(
            Threw(() => SyncExtension.SessionPublish(session, mine, "main")),
            Status.InvalidArgument,
            "a viewer publishing a timeline");

        SyncExtension.SessionSetPlayhead(session, "main", 12, true, 1);
        var frames = Transmit(session);
        Check(
            frames.Any(frame => frame.Text.Contains("\"type\":\"presence\"")),
            $"moving the playhead sends no presence: {string.Join(" | ", frames.Select(frame => frame.Text))}");

        SyncExtension.SessionClose(session);
        CheckEq(Threw(() => SyncExtension.SessionClose(session)), null, "closing a closed session again");
        Check(Threw(() => SyncExtension.SessionState(session)) is not null, "asking a closed session its state");
    }

    private static void AHostPublishesItsTimeline()
    {
        SyncExtension.Enable();
        var session = SyncExtension.SessionNew("""{"token":"host-token","encodings":[]}""");
        SyncExtension.SessionConnect(session);
        Transmit(session);
        SyncExtension.SessionReceive(session, Encoding.UTF8.GetBytes(HostWelcome), false);
        var joined = SyncExtension.SessionPollEvent(session);
        Check(joined?.Contains("\"type\":\"joined\"") ?? false, $"the first event is not joined: {joined}");
        Check(joined?.Contains("\"role\":\"host\"") ?? false, $"the joined event does not say host: {joined}");
        Events(session);

        var timeline = Otio.FromJson(Cut);
        var patches = SyncExtension.SessionPublish(session, timeline, "main");
        Check(patches.Contains("replace_timeline"), $"the first publish does not replace the timeline: {patches}");
        var frames = Transmit(session);
        Check(
            frames.Any(frame => frame.Text.Contains("\"type\":\"snapshot\"")),
            $"the first publish sends no snapshot: {string.Join(" | ", frames.Select(frame => frame.Text))}");

        CheckEq(SyncExtension.SessionPublish(session, timeline, "main"), "[]", "publishing an unchanged timeline");
        Transmit(session);

        timeline.SetName("fine cut");
        var renamed = SyncExtension.SessionPublish(session, timeline, "main");
        Check(renamed.Contains("set_property"), $"publishing a rename does not set a property: {renamed}");
        SyncExtension.SessionClose(session);
    }

    private static void ASessionRefusesSettingsItCannotUse()
    {
        SyncExtension.Enable();
        Check(
            Threw(() => SyncExtension.SessionNew("""{"encodings":["zstd"]}""")) is not null,
            "a session that accepts an encoding nobody knows");
    }

    private static readonly (string Name, Action Body)[] ExtensionTests =
    {
        ("sync refuses to work before it is enabled", SyncRefusesToWorkBeforeItIsEnabled),
        ("sync is enabled through its own module", SyncIsEnabledThroughItsOwnModule),
        ("sync matches Furnace's golden diffs", SyncMatchesFurnacesGoldenDiffs),
        ("a sync transaction carries to another copy", ASyncTransactionCarriesToAnotherCopy),
        ("a sync transaction rolls back and the journal records", ASyncTransactionRollsBackAndTheJournalRecords),
        ("a viewer joins a session and receives its timeline", AViewerJoinsASessionAndReceivesItsTimeline),
        ("a host publishes its timeline", AHostPublishesItsTimeline),
        ("a session refuses settings it cannot use", ASessionRefusesSettingsItCannotUse),
    };
}
