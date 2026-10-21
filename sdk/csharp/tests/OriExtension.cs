// Tests for ORI Annotations, an otio-rust extension and not part of
// OpenTimelineIO. They run after the sync tests, since enabling ORI
// Annotations enables sync and one sync test wants it off.

using System;
using System.IO;
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
    };
}
