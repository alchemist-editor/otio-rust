// Tests for the generated C# SDK.
//
// These are written by hand, not generated. A generator that also wrote its
// own tests would only prove it is self-consistent; what needs proving is
// that the C# it writes does what someone reading it would expect, against
// the same library and the same fixtures the Rust tests read.
//
// There is no test framework here, for the reason the C++ suite gives: the
// SDKs take no third-party dependencies, and .NET has no test runner in the
// box. This is a program. It prints what it ran and exits non-zero if
// anything failed.

using System;
using System.Collections.Generic;
using System.IO;
using OpenTimelineIO;

namespace OpenTimelineIO.Tests;

internal static class Program
{
    private static int failures;

    private static void Fail(string what)
    {
        Console.WriteLine($"  FAIL {what}");
        failures += 1;
    }

    private static void Check(bool condition, string what)
    {
        if (!condition)
        {
            Fail(what);
        }
    }

    private static void CheckEq<T>(T left, T right, string what)
    {
        if (!EqualityComparer<T>.Default.Equals(left, right))
        {
            Fail($"{what}: {left} is not {right}");
        }
    }

    private static void CheckNear(double left, double right, string what)
    {
        if (Math.Abs(left - right) > 1e-9)
        {
            Fail($"{what}: {left} is not {right}");
        }
    }

    /// The status a call failed with, for a test that wants to name it.
    private static Status? Threw(Action body)
    {
        try
        {
            body();
        }
        catch (OtioException error)
        {
            return error.Status;
        }
        return null;
    }

    /// The repository, found by walking up from wherever this was built to.
    private static string Repository()
    {
        var directory = new DirectoryInfo(AppContext.BaseDirectory);
        while (directory is not null)
        {
            if (Directory.Exists(Path.Combine(directory.FullName, "crates", "otio-capi")))
            {
                return directory.FullName;
            }
            directory = directory.Parent;
        }
        throw new InvalidOperationException("the repository is not above this program");
    }

    /// The EDL the Rust adapter's own tests read, so that the two agree about
    /// what is in it.
    private static string ScreeningEdl() =>
        Path.Combine(
            Repository(), "crates", "otio-cmx3600", "tests", "data", "screening_example.edl");

    /// A path in a directory of this run's own, so two tests cannot collide.
    private static int counter;

    private static string Temporary(string name)
    {
        counter += 1;
        var directory = Path.Combine(Path.GetTempPath(), $"otio-csharp-{counter}");
        Directory.CreateDirectory(directory);
        return Path.Combine(directory, name);
    }

    private static void TheLibraryReportsAVersion()
    {
        Check(Otio.Version().Length > 0, "the version is empty");
    }

    private static void AnEnumSaysWhatTheCInterfaceCallsIt()
    {
        CheckEq(Format.Cmx3600.CName(), "OTIO_FORMAT_CMX_3600", "the format's C name");
        CheckEq(Status.NoValue.CName(), "OTIO_STATUS_NO_VALUE", "the status's C name");
        CheckEq(Format.Cmx3600.Name(), "cmx_3600", "the format's own name");
    }

    private static void RatesAreClassified()
    {
        // The drop-frame rate is 30000/1001, which is not the 29.97 people
        // write; asking for the nearest SMPTE rate is what turns one into the
        // other.
        Check(!Otio.IsDropFrameRate(29.97), "29.97 is not the drop-frame rate");
        Check(
            Otio.IsDropFrameRate(Otio.NearestSmpteTimecodeRate(29.97)),
            "the nearest SMPTE rate to 29.97 is the drop-frame one");
        Check(!Otio.IsDropFrameRate(24), "24 is not a drop-frame rate");
        Check(Otio.IsSmpteTimecodeRate(24), "24 is an SMPTE rate");
    }

    private static void ReadingAnEdlFindsItsClips()
    {
        using var document = Document.ReadFromFile(Format.Cmx3600, ScreeningEdl(), null);

        var root = document.Root();
        Check(root is not null, "the document has no root");
        var clips = root!.FindClips();
        CheckEq(clips.Length, 9, "the number of clips");

        // Every one of them really is a clip, and the library says so.
        var counted = 0;
        foreach (var node in clips)
        {
            Check(node.IsA(NodeKind.Clip), "a found clip is not a clip");
            if (node is Clip)
            {
                counted += 1;
            }
        }
        CheckEq(counted, 9, "the number that cast to Clip");
    }

    /// The quickstart in `sdk/csharp/README.md` is generated, so nothing
    /// compiles it. This is that example, so that it cannot go stale.
    private static void TheQuickstartFromTheReadmeRuns()
    {
        using var document = Document.Open(ScreeningEdl());

        var named = 0;
        var root = document.Root();
        if (root is not null)
        {
            foreach (var child in root.FindClips())
            {
                var clip = (Clip)child;
                Check(clip.Name().Length > 0, "a clip has no name");
                Check(clip.Duration().ToSeconds() > 0, "a clip has no duration");
                named += 1;
            }
        }
        CheckEq(named, 9, "the number of named clips");
    }

    private static void OpenWorksOutTheFormatFromTheName()
    {
        using var document = Document.Open(ScreeningEdl());
        var root = document.Root();
        Check(root is not null, "the document has no root");
        Check(root!.Name().Contains("Example_Screening"), "the root's name");
    }

    private static void OpenDeclinesASuffixNoFormatClaims()
    {
        CheckEq(
            Threw(() => Document.Open("/tmp/nothing.wav")),
            Status.NoValue,
            "opening a .wav");
    }

    private static void ADocumentSurvivesARoundTripThroughJson()
    {
        using var document = Document.Open(ScreeningEdl());
        var text = document.ToJson(2);
        Check(text.Contains("Timeline"), "the JSON has no timeline in it");

        using var again = Document.FromJson(text);
        var root = again.Root();
        Check(root is not null, "the rebuilt document has no root");
        CheckEq(root!.FindClips().Length, 9, "the clips after a round trip");
    }

    private static void SavingAndOpeningAgainKeepsTheClips()
    {
        using var document = Document.Open(ScreeningEdl());
        var path = Temporary("round-trip.otio");
        document.Save(path);

        using var again = Document.Open(path);
        var root = again.Root();
        Check(root is not null, "the reopened document has no root");
        CheckEq(root!.FindClips().Length, 9, "the clips after saving and opening");
    }

    private static void WritingBytesInEveryFormatTheLibraryKnows()
    {
        using var document = Document.Open(ScreeningEdl());
        foreach (var format in new[] { Format.OtioJson, Format.Cmx3600 })
        {
            Check(document.WriteToBytes(format, null).Length > 0, $"{format} wrote nothing");
        }
    }

    /// A timeline with one video track holding two clips.
    private sealed record Built(Timeline Timeline, Track Track, List<Clip> Clips);

    private static Built MakeTimeline(Document document)
    {
        var timeline = document.NewTimeline("Assembly");
        var stack = document.NewStack("tracks");
        timeline.SetTracks(stack);
        var track = document.NewTrack("V1", "Video");
        stack.AppendChild(track);

        var clips = new List<Clip>();
        var names = new[] { "A", "B" };
        for (var index = 0; index < names.Length; index++)
        {
            var clip = document.NewClip(names[index]);
            var start = new RationalTime(index * 24, 24);
            clip.SetSourceRange(new TimeRange(start, new RationalTime(24, 24)));
            track.AppendChild(clip);
            clips.Add(clip);
        }
        document.SetRoot(timeline);
        return new Built(timeline, track, clips);
    }

    private static void BuildingATimelineFromNothing()
    {
        using var document = Document.New();
        var built = MakeTimeline(document);

        CheckEq(built.Track.ChildCount(), 2, "the track's children");
        CheckEq(built.Timeline.FindClips().Length, 2, "the timeline's clips");
        CheckEq(built.Clips[0].Name(), "A", "the first clip's name");
        CheckEq(built.Track.Kind(), "Video", "the track's kind");

        // The whole track is as long as the two clips together.
        CheckNear(built.Track.Duration().ToSeconds(), 2, "the track's duration");
    }

    private static void AFreshlyBuiltObjectIsEnabled()
    {
        using var document = Document.New();
        var clip = document.NewClip("A");
        Check(clip.Enabled(), "a new clip is not enabled");
        clip.SetEnabled(false);
        Check(!clip.Enabled(), "a disabled clip says it is enabled");
    }

    private static void NoValueIsAnAnswerAndNotAFailure()
    {
        using var document = Document.New();
        var clip = document.NewClip("untrimmed");

        // An item that uses all of its media has no source range, and that is
        // an answer rather than a failure.
        Check(clip.SourceRange() is null, "an untrimmed clip reports a source range");

        var span = new TimeRange(new RationalTime(0, 24), new RationalTime(12, 24));
        clip.SetSourceRange(span);
        var read = clip.SourceRange();
        Check(read is not null, "a trimmed clip reports no source range");
        CheckNear(read!.Value.Duration.ToSeconds(), 0.5, "the source range's duration");

        clip.ClearSourceRange();
        Check(clip.SourceRange() is null, "a cleared source range comes back");
    }

    private static void AnObjectKnowsWhichSchemasItIs()
    {
        using var document = Document.New();
        var clip = document.NewClip("A");

        Check(clip.IsA(NodeKind.Clip), "a clip is not a clip");
        Check(clip.IsA(NodeKind.Item), "a clip is not an item");
        Check(clip.IsA(NodeKind.Composable), "a clip is not composable");
        Check(clip.IsA(NodeKind.SerializableObject), "a clip is not serializable");
        Check(!clip.IsA(NodeKind.Track), "a clip says it is a track");
        SerializableObject node = clip;
        Check(node is Item, "a clip does not cast to Item");
        Check(node is not Track, "a clip casts to Track");
        CheckEq(clip.SchemaKind(), NodeKind.Clip, "the clip's schema kind");
        CheckEq(clip.SchemaName(), "Clip", "the clip's schema name");
    }

    private static void ClearingChildrenHandsThemAllBack()
    {
        using var document = Document.New();
        var built = MakeTimeline(document);

        var taken = built.Track.ClearChildren();
        CheckEq(taken.Length, built.Clips.Count, "the number handed back");
        CheckEq(built.Track.ChildCount(), 0, "the track is not empty");
        CheckEq(taken[0].Name(), "A", "the first one handed back");
        CheckEq(taken[1].Name(), "B", "the second one handed back");
    }

    private static void EveryChildAndItsRangeComeBackTogether()
    {
        using var document = Document.New();
        var built = MakeTimeline(document);

        var (nodes, ranges) = built.Track.RangesOfChildren();
        CheckEq(nodes.Length, 2, "the children");
        CheckEq(ranges.Length, 2, "the ranges");
        if (ranges.Length == 2)
        {
            CheckNear(ranges[0].StartTime.ToSeconds(), 0, "the first child's start");
            CheckNear(ranges[1].StartTime.ToSeconds(), 1, "the second child's start");
        }
    }

    private static void AStaleHandleIsRefused()
    {
        using var document = Document.New();
        var clip = document.NewClip("A");
        document.RemoveNode(clip);

        CheckEq(Threw(() => clip.Name()), Status.StaleHandle, "a removed clip's name");
    }

    private static void AnObjectOfNoDocumentFailsRatherThanCrashing()
    {
        var orphan = SerializableObject.None();
        Check(orphan.IsNone(), "the none object is not none");
        Check(orphan.Document is null, "the none object has a document");
        Check(Threw(() => orphan.Name()) is not null, "asking an orphan its name worked");
    }

    private static void AnObjectFromAnotherDocumentIsRefused()
    {
        using var one = Document.New();
        using var other = Document.New();

        var track = one.NewTrack("V1", "Video");
        var stranger = other.NewClip("elsewhere");

        CheckEq(
            Threw(() => track.AppendChild(stranger)),
            Status.InvalidArgument,
            "appending a foreign clip");

        // A call that cannot fail answers rather than throwing, and the answer
        // is no.
        Check(!one.Contains(stranger), "a document claims to contain a foreign object");

        // The document that refused it is still whole, which is what tells
        // refusing apart from absorbing and then failing.
        CheckEq(other.NodeCount(), 1, "the other document lost its clip");
        CheckEq(stranger.Name(), "elsewhere", "the foreign clip stopped answering");
    }

    private static void MetadataGoesInAndComesBack()
    {
        using var document = Document.New();
        var clip = document.NewClip("A");

        clip.Metadata.SetString("reel", "ZZ100");
        clip.Metadata.SetInt("take", 3);
        clip.Metadata.SetBool("circled", true);
        clip.Metadata.SetDouble("gain", 0.5);

        CheckEq(clip.Metadata.GetString("reel"), "ZZ100", "the reel");
        CheckEq(clip.Metadata.GetInt("take"), 3L, "the take");
        Check(clip.Metadata.GetBool("circled"), "the circled flag");
        CheckNear(clip.Metadata.GetDouble("gain"), 0.5, "the gain");
        Check(clip.Metadata.Contains("reel"), "the reel is missing");
        Check(!clip.Metadata.Contains("nothing"), "a key that was never set is there");

        // A path is followed, not created: the dictionary has to exist before
        // anything can be written inside it.
        clip.Metadata.SetDictionary("cmx_3600");
        clip.Metadata.SetString("cmx_3600.reel", "AX");
        CheckEq(clip.Metadata.GetString("cmx_3600.reel"), "AX", "the nested reel");

        clip.Metadata.Clear();
        Check(!clip.Metadata.Contains("reel"), "the metadata was not cleared");
    }

    private static void TimeValuesComputeWithoutADocument()
    {
        var time = new RationalTime(48, 24);
        CheckNear(time.ToSeconds(), 2, "the time in seconds");
        CheckEq(time.ToFrames(), 48, "the time in frames");
        Check(time.RescaledTo(48).Equals(new RationalTime(96, 48)), "rescaling to 48");
        Check(
            RationalTime.DurationFromStartEndTime(new RationalTime(0, 24), time).Equals(time),
            "the duration from zero");
        Check(time.IsValid(), "the time is not valid");
        CheckEq(time.ToTimecode(), "00:00:02:00", "the timecode");
        Check(RationalTime.FromTimecode("00:00:02:00", 24).Equals(time), "reading a timecode");
    }

    private static void AnUnreadableTimecodeIsAFailure()
    {
        CheckEq(
            Threw(() => RationalTime.FromTimecode("not a timecode", 24)),
            Status.TimeError,
            "reading nonsense as a timecode");
    }

    private static void ARangeAnswersAboutWhatItCovers()
    {
        var span = new TimeRange(new RationalTime(0, 24), new RationalTime(24, 24));
        Check(span.EndTimeExclusive().Equals(new RationalTime(24, 24)), "the exclusive end");
        Check(span.ContainsTime(new RationalTime(12, 24)), "the middle is not inside");
        Check(!span.ContainsTime(new RationalTime(24, 24)), "the exclusive end is inside");
    }

    private static void AnObjectBuiltOnItsOwnCanJoinATimeline()
    {
        using var document = Document.New();
        var track = document.NewTrack("V1", "Video");

        // A clip built in a document of its own, as a binding that hides the
        // document would build one.
        var workshop = Document.New();
        var clip = workshop.NewClip("guest");

        var translated = document.Absorb(workshop);

        Check(translated.TryGetValue(clip, out var arrived), "the clip did not move");
        if (arrived is null)
        {
            return;
        }
        Check(arrived is Clip, "what arrived is not a clip");
        Check(ReferenceEquals(arrived.Document, document), "it arrived in the wrong document");

        track.AppendChild(arrived);
        CheckEq(track.ChildCount(), 1, "the track did not take it");
        CheckEq(arrived.Name(), "guest", "the name did not travel");
    }

    /// An object holds the Document rather than the raw pointer, so that
    /// disposing of the document leaves the object naming nothing instead of
    /// leaving it dangling.
    private static void AnObjectOutlivingItsDocumentFailsRatherThanCrashing()
    {
        SerializableObject survivor;
        SerializableObject sibling;
        {
            var document = Document.New();
            survivor = document.NewClip("A");
            sibling = document.NewClip("B");
            document.Close();
        }
        CheckEq(Threw(() => survivor.Name()), Status.NullPointer, "reading a closed document");
        CheckEq(
            Threw(() => survivor.SetName("B")),
            Status.NullPointer,
            "writing to a closed document");
        CheckEq(
            Threw(() => survivor.FindClips()),
            Status.NullPointer,
            "searching a closed document");
        // Asking what schema it is answers "none" rather than reading anything.
        Check(!survivor.IsA(NodeKind.Clip), "a closed document's object still has a schema");
        // Two objects of the same closed document still compare as themselves.
        Check(survivor.Equals((object)survivor), "an object is not itself");
        Check(!survivor.Equals((object)sibling), "two objects compare equal");
    }

    private static readonly (string Name, Action Body)[] Tests =
    {
        ("the library reports a version", TheLibraryReportsAVersion),
        ("an enum says what the C interface calls it", AnEnumSaysWhatTheCInterfaceCallsIt),
        ("rates are classified", RatesAreClassified),
        ("reading an EDL finds its clips", ReadingAnEdlFindsItsClips),
        ("the quickstart from the README runs", TheQuickstartFromTheReadmeRuns),
        ("open works out the format from the name", OpenWorksOutTheFormatFromTheName),
        ("open declines a suffix no format claims", OpenDeclinesASuffixNoFormatClaims),
        ("a document survives a round trip through JSON", ADocumentSurvivesARoundTripThroughJson),
        ("saving and opening again keeps the clips", SavingAndOpeningAgainKeepsTheClips),
        ("writing bytes in every format the library knows", WritingBytesInEveryFormatTheLibraryKnows),
        ("building a timeline from nothing", BuildingATimelineFromNothing),
        ("a freshly built object is enabled", AFreshlyBuiltObjectIsEnabled),
        ("no value is an answer and not a failure", NoValueIsAnAnswerAndNotAFailure),
        ("an object knows which schemas it is", AnObjectKnowsWhichSchemasItIs),
        ("clearing children hands them all back", ClearingChildrenHandsThemAllBack),
        ("every child and its range come back together", EveryChildAndItsRangeComeBackTogether),
        ("a stale handle is refused", AStaleHandleIsRefused),
        ("an object of no document fails rather than crashing", AnObjectOfNoDocumentFailsRatherThanCrashing),
        ("an object from another document is refused", AnObjectFromAnotherDocumentIsRefused),
        ("metadata goes in and comes back", MetadataGoesInAndComesBack),
        ("time values compute without a document", TimeValuesComputeWithoutADocument),
        ("an unreadable timecode is a failure", AnUnreadableTimecodeIsAFailure),
        ("a range answers about what it covers", ARangeAnswersAboutWhatItCovers),
        ("an object built on its own can join a timeline", AnObjectBuiltOnItsOwnCanJoinATimeline),
        ("an object outliving its document fails rather than crashing", AnObjectOutlivingItsDocumentFailsRatherThanCrashing),
    };

    private static int Main()
    {
        foreach (var test in Tests)
        {
            Console.WriteLine(test.Name);
            var before = failures;
            try
            {
                test.Body();
            }
            catch (OtioException error)
            {
                Fail($"threw {error.Status.CName()}: {error.Message}");
            }
            if (failures == before)
            {
                Console.WriteLine("  ok");
            }
        }
        if (failures != 0)
        {
            Console.WriteLine($"{failures} failed");
            return 1;
        }
        Console.WriteLine($"all {Tests.Length} passed");
        return 0;
    }
}
