// Tests for ORI Annotations, an otio-rust extension and not part of
// OpenTimelineIO. They run after the sync tests, since enabling ORI
// Annotations enables sync and one sync test wants it off.

using System;
using System.IO;
using System.Text;
using OpenTimelineIO.Ext.Sync;
using OpenTimelineIO;
using OpenTimelineIO.Ext.Ori;

namespace OpenTimelineIO.Tests;

internal static partial class Program
{
    /// A file ORI's own Python wrote: a shot annotated at frames 10 and 4.
    private static SerializableObject OpenOriBuilt() =>
        Otio.FromJson(File.ReadAllText(Path.Combine(
            Repository(), "extensions", "ori", "otio-ext-ori-annotations", "tests", "fixtures",
            "ori_built.otio")));

    private static void OriAnnotationsTurnOnWithSync()
    {
        OriExtension.Enable();
        Check(OriExtension.IsEnabled(), "ORI Annotations is off after Enable");
        Check(Otio.IsExtensionEnabled("ori-annotations"), "the core does not see ori-annotations on");
        Check(Otio.IsExtensionEnabled("sync"), "sync is off after enabling ORI Annotations");
    }

    private static void OriAnnotationsReadWhatOriWrote()
    {
        OriExtension.Enable();
        var timeline = OpenOriBuilt();
        var holders = OriExtension.List(timeline);
        CheckEq(holders.Length, 2, "the annotations");
        CheckEq(
            OriExtension.Describe(holders[0]),
            """{"form":"track","clip_guid":"shot","clip_name":null,"frame":{"value":10.0,"rate":24.0},"note":null,"status":null,"command_count":4}""",
            "the first annotation");
        var track = OriExtension.FindTrack(timeline);
        CheckEq(track?.Name(), "Annotations", "the annotation track");
        Check(OriExtension.FindClip(track!, "shot", 99) is null, "a clip at a frame nobody annotated");
        CheckEq(OriExtension.ListAtFrame(timeline, "shot", 4).Length, 1, "the annotations at frame 4");
        CheckEq(OriExtension.ListForClip(timeline, "shot").Length, 2, "the shot's annotations");
    }

    private static void OriAnnotationsAnnotateAFrame()
    {
        OriExtension.Enable();
        var commands = OriExtension.Commands(OriExtension.List(OpenOriBuilt())[0], false);

        var timeline = Otio.FromJson(
            """{"OTIO_SCHEMA": "Timeline.1", "tracks": {"OTIO_SCHEMA": "Stack.1", "children": []}}""");
        var track = OriExtension.EnsureTrack(timeline);
        var clip = OriExtension.EnsureClip(track, "shot", 10, 24);
        OriExtension.AddCommands(clip, commands);
        OriExtension.EnsureClip(track, "shot", 10, 24);
        CheckEq(OriExtension.List(timeline).Length, 1, "annotations after asking twice");
        CheckEq(OriExtension.Commands(clip, false), commands, "the commands");

        Check(OriExtension.ReplaceCommands(
            clip, "stroke-1",
            """[{"OTIO_SCHEMA": "TextAnnotation.1", "uuid": "stroke-1", "text": "was a stroke"}]"""),
            "replacing the stroke");
        CheckEq(OriExtension.RemoveCommands(clip, """["text-1"]"""), 1, "removed");
        var left = OriExtension.Commands(clip, false);
        Check(left.Contains("\"text\":\"was a stroke\"") && !left.Contains("text-1"), left);
        OriExtension.SetCommands(clip, "[]");
        CheckEq(Threw(() => OriExtension.AddCommands(clip, "not json")), (Status?)Status.ParseError, "bad JSON");

        var corner = OriExtension.PixelsToOri(1920, 1080, 0, 0);
        CheckNear(corner.x, -8.0 / 9.0, "the top left corner's x");
        CheckNear(corner.y, 0.5, "the top left corner's y");
        var mapped = OriExtension.OriToBounds(0.5, 0.5, -16, -9, 16, 9);
        CheckEq(mapped, (9.0, 9.0), "a point in the bounds");
    }

    private static readonly (string Name, Action Body)[] OriExtensionTests =
    {
        ("ORI Annotations turn on with sync", OriAnnotationsTurnOnWithSync),
        ("ORI Annotations read what ORI wrote", OriAnnotationsReadWhatOriWrote),
        ("ORI Annotations annotate a frame", OriAnnotationsAnnotateAFrame),
        ("ORI Annotations merge by uuid and kind", OriAnnotationsMergeByUuidAndKind),
        ("ORI Annotations live review", OriAnnotationsLiveReview),
    };

    private const string OriTimeline =
        """{"OTIO_SCHEMA":"Timeline.1","name":"Program","metadata":{"sync":{"guid":"main"}},"tracks":{"OTIO_SCHEMA":"Stack.1","metadata":{"sync":{"guid":"main/stack"}},"children":[{"OTIO_SCHEMA":"Track.1","name":"V1","kind":"Video","metadata":{"sync":{"guid":"main/v1"}},"children":[{"OTIO_SCHEMA":"Clip.2","name":"Beach Run","metadata":{"sync":{"guid":"main/clip-1"}},"source_range":{"OTIO_SCHEMA":"TimeRange.1","start_time":{"OTIO_SCHEMA":"RationalTime.1","value":0,"rate":24},"duration":{"OTIO_SCHEMA":"RationalTime.1","value":48,"rate":24}}}]}]}}""";
    private const string OriStroke =
        """[{"OTIO_SCHEMA":"PaintStart.1","uuid":"s1","rgba":[1,0,0,1]},{"OTIO_SCHEMA":"PaintPoint.1","uuid":"s1","points":{"OTIO_SCHEMA":"PaintVertices.1","x":[0.1],"y":[0.2],"size":[0.01],"alpha":[1]}},{"OTIO_SCHEMA":"PaintEnd.1","uuid":"s1"}]""";
    private const string OriAnchor =
        """{"sequenceId":"main","capturedGeneration":1,"clipGuid":"main/clip-1","sourceTime":{"value":10,"rate":24},"capturedSequenceTime":{"value":10,"rate":24},"capturedAt":1787742000000}""";
    private const string OriWelcome =
        """{"v":2,"type":"welcome","sessionId":"s_1","seq":0,"ts":0,"order":1,"payload":{"selfId":"p_2","selfUserId":"u_bea","role":"viewer","roster":[],"session":{"id":"s_1","createdAt":0}}}""";
    private const string OriRelay =
        """{"type":"review_relay","commit":{"clientMutationId":"m1","anchor":""" + OriAnchor +
        ""","bodyMarkdown":"Too dark.","annotationCommands":""" + OriStroke +
        """},"ack":{"clientMutationId":"m1","commentId":"c1","serverOrder":1,"revision":1},"authorUserId":"u_bea","authorPresenceId":"p_2","createdAt":0}""";
    private const string OriPartial =
        """{"type":"message","kind":"Annotation.1/PARTIAL","sender":"p_3","payload":{"clip_guid":"main/clip-1","frame":10.0,"fps":24.0,"events":[{"OTIO_SCHEMA":"PaintStart.1","uuid":"s2"}]}}""";

    private static void OriAnnotationsMergeByUuidAndKind()
    {
        OriExtension.Enable();
        var timeline = Otio.FromJson(OriTimeline);
        var clip = OriExtension.EnsureClip(OriExtension.EnsureTrack(timeline), "main/clip-1", 10, 24);
        OriExtension.SetCommands(clip, """[{"OTIO_SCHEMA":"TextAnnotation.1","uuid":"t"},{"OTIO_SCHEMA":"PaintStart.1","uuid":"a"}]""");
        CheckEq(
            OriExtension.MergeCommands(clip, """[{"OTIO_SCHEMA":"PaintStart.1","uuid":"a","brush":"gauss"},{"OTIO_SCHEMA":"PaintStart.1","uuid":"b"}]"""),
            (1, 1),
            "what merging replaced and added");
        CheckEq(OriExtension.ReplaceByKind(clip, """[{"OTIO_SCHEMA":"PaintStart.1","uuid":"c"}]"""), 2, "the strokes replaced");
        var left = OriExtension.Commands(clip, false);
        Check(left.Contains("\"uuid\":\"t\"") && left.Contains("\"uuid\":\"c\""), $"the text kept beside the new stroke: {left}");
        CheckEq(OriExtension.MergeDuplicates(timeline), 0, "duplicate clips");
        Check(OriExtension.OwnerOf(clip, "t") is null, "an owner for a stroke no comment added");
    }

    private static void OriAnnotationsLiveReview()
    {
        OriExtension.Enable();
        var partial = OriExtension.ReadPartial(OriPartial);
        Check(partial is not null && partial.Contains("\"brush\":\"circle\""), $"the partial with ORI's defaults: {partial}");
        Check(OriExtension.ReadPartial("""{"type":"leader","leader":null}""") is null, "a partial read from another event");

        var reviewer = SyncExtension.SessionNew(
            """{"token":"join-code","identity":{"capabilities":["review.comment.create","review.annotation.create"]},"encodings":[]}""");
        SyncExtension.SessionConnect(reviewer);
        while (SyncExtension.SessionPollTransmit(reviewer, 0) is not null)
        {
        }
        SyncExtension.SessionReceive(reviewer, Encoding.UTF8.GetBytes(OriWelcome), false);
        while (SyncExtension.SessionPollEvent(reviewer) is not null)
        {
        }
        var id = OriExtension.Commit(reviewer, Otio.FromJson(OriTimeline), OriAnchor, "Too dark.", OriStroke, "");
        Check(id.StartsWith("mutation_", StringComparison.Ordinal), $"the commit's id {id}");
        var sent = SyncExtension.SessionPollTransmit(reviewer, 0);
        Check(sent is var (bytes, _) && Encoding.UTF8.GetString(bytes).Contains("\"type\":\"review_commit\""), "the commit was not sent");
        OriExtension.SendPartial(reviewer, """{"clip_guid":"main/clip-1","frame":10,"fps":24,"events":[]}""");
        CheckEq(Threw(() => SyncExtension.SessionPollTransmit(reviewer, 0)), Status.Unsupported, "Furnace sending a stroke in progress");
        SyncExtension.SessionClose(reviewer);

        var timeline = Otio.FromJson(OriTimeline);
        var (clip, already) = OriExtension.Fold(timeline, OriRelay);
        Check(!already, "the first fold found the comment there already");
        Check(OriExtension.Fold(timeline, OriRelay).already, "folding the comment again changed something");
        var commands = OriExtension.Commands(clip, false);
        CheckEq(commands.Split("\"uuid\":\"s1\"").Length - 1, 3, "the stroke's commands");
        CheckEq(OriExtension.OwnerOf(clip, "s1"), "u_bea", "the stroke's owner");
    }
}
