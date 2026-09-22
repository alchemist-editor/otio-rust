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
        var root = Otio.ReadFromFile(Format.Cmx3600, ScreeningEdl(), null);

        var clips = root.FindClips();
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
        var timeline = Otio.Open(ScreeningEdl());

        var named = 0;
        foreach (var child in timeline.FindClips())
        {
            var clip = (Clip)child;
            Check(clip.Name().Length > 0, "a clip has no name");
            Check(clip.Duration().ToSeconds() > 0, "a clip has no duration");
            named += 1;
        }
        CheckEq(named, 9, "the number of named clips");
    }

    private static void OpenWorksOutTheFormatFromTheName()
    {
        var root = Otio.Open(ScreeningEdl());
        Check(root.Name().Contains("Example_Screening"), "the root's name");
    }

    private static void OpenDeclinesASuffixNoFormatClaims()
    {
        CheckEq(
            Threw(() => Otio.Open("/tmp/nothing.wav")),
            Status.NoValue,
            "opening a .wav");
    }

    private static void ATimelineSurvivesARoundTripThroughJson()
    {
        var root = Otio.Open(ScreeningEdl());
        var text = root.ToJson(2);
        Check(text.Contains("Timeline"), "the JSON has no timeline in it");

        var again = Otio.FromJson(text);
        CheckEq(again.FindClips().Length, 9, "the clips after a round trip");
    }

    private static void SavingAndOpeningAgainKeepsTheClips()
    {
        var root = Otio.Open(ScreeningEdl());
        var path = Temporary("round-trip.otio");
        Otio.Save(root, path);

        var again = Otio.Open(path);
        CheckEq(again.FindClips().Length, 9, "the clips after saving and opening");
    }

    private static void WritingBytesInEveryFormatTheLibraryKnows()
    {
        var root = Otio.Open(ScreeningEdl());
        foreach (var format in new[] { Format.OtioJson, Format.Cmx3600 })
        {
            Check(Otio.WriteToBytes(format, root, null).Length > 0, $"{format} wrote nothing");
        }
    }

    /// A timeline with one video track holding two clips.
    private sealed record Built(Timeline Timeline, Track Track, List<Clip> Clips);

    /// Builds one the way a caller does now: every object on its own, joined
    /// up afterwards. Nothing has to exist before the thing it goes into.
    private static Built MakeTimeline()
    {
        var timeline = new Timeline("Assembly");
        var stack = new Stack("tracks");
        timeline.SetTracks(stack);
        var track = new Track("V1", "Video");
        stack.AppendChild(track);

        var clips = new List<Clip>();
        var names = new[] { "A", "B" };
        for (var index = 0; index < names.Length; index++)
        {
            var clip = new Clip(names[index]);
            var start = new RationalTime(index * 24, 24);
            clip.SetSourceRange(new TimeRange(start, new RationalTime(24, 24)));
            track.AppendChild(clip);
            clips.Add(clip);
        }
        return new Built(timeline, track, clips);
    }

    private static void BuildingATimelineFromNothing()
    {
        var built = MakeTimeline();

        CheckEq(built.Track.ChildCount(), 2, "the track's children");
        CheckEq(built.Timeline.FindClips().Length, 2, "the timeline's clips");
        CheckEq(built.Clips[0].Name(), "A", "the first clip's name");
        CheckEq(built.Track.Kind(), "Video", "the track's kind");

        // The whole track is as long as the two clips together.
        CheckNear(built.Track.Duration().ToSeconds(), 2, "the track's duration");
    }

    /// A clip built on its own is a whole timeline of one object until it
    /// joins another, which is what lets it exist before its track does.
    private static void AnObjectBuiltOnItsOwnStandsAlone()
    {
        var clip = new Clip("alone");
        CheckEq(clip.Name(), "alone", "the name of an object with no timeline");
        Check(clip.IsLive(), "an object built on its own is not live");
        Check(clip.Parent() is null, "an object built on its own has a parent");
    }

    private static void AFreshlyBuiltObjectIsEnabled()
    {
        var clip = new Clip("A");
        Check(clip.Enabled(), "a new clip is not enabled");
        clip.SetEnabled(false);
        Check(!clip.Enabled(), "a disabled clip says it is enabled");
    }

    private static void ABuiltObjectMayBeLeftUnnamed()
    {
        var clip = new Clip();
        CheckEq(clip.Name(), string.Empty, "an unnamed clip has a name");
    }

    private static void NoValueIsAnAnswerAndNotAFailure()
    {
        var clip = new Clip("untrimmed");

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
        var clip = new Clip("A");

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
        var built = MakeTimeline();

        var taken = built.Track.ClearChildren();
        CheckEq(taken.Length, built.Clips.Count, "the number handed back");
        CheckEq(built.Track.ChildCount(), 0, "the track is not empty");
        CheckEq(taken[0].Name(), "A", "the first one handed back");
        CheckEq(taken[1].Name(), "B", "the second one handed back");
    }

    private static void EveryChildAndItsRangeComeBackTogether()
    {
        var built = MakeTimeline();

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
        var clip = new Clip("A");
        clip.RemoveFromTimeline();

        Check(!clip.IsLive(), "a removed clip is still live");
        CheckEq(Threw(() => clip.Name()), Status.StaleHandle, "a removed clip's name");
    }

    private static void AnObjectOfNoTimelineFailsRatherThanCrashing()
    {
        var orphan = SerializableObject.None();
        Check(orphan.IsNone(), "the none object is not none");
        Check(Threw(() => orphan.Name()) is not null, "asking an orphan its name worked");
    }

    private static void AnObjectFromAnotherTimelineIsRefused()
    {
        var track = new Track("V1", "Video");
        var elsewhere = new Track("V2", "Video");
        var stranger = new Clip("elsewhere");
        elsewhere.AppendChild(stranger);

        CheckEq(
            Threw(() => track.HasChild(stranger)),
            Status.InvalidArgument,
            "asking about a foreign clip");

        // Even the question that cannot fail answers rather than throwing, and
        // the answer is no: two arenas issue the same handles, so the honest
        // answer about an object of another timeline is that it is not this one.
        Check(!track.Equals(stranger), "a track claims to be a foreign clip");

        // The timeline that refused it is still whole, which is what tells
        // refusing apart from absorbing and then failing.
        CheckEq(elsewhere.ChildCount(), 1, "the other timeline lost its clip");
        CheckEq(stranger.Name(), "elsewhere", "the foreign clip stopped answering");
    }

    private static void AnEditPutsANewlyBuiltItemIntoATrack()
    {
        var built = MakeTimeline();
        var arriving = new Clip("C");
        arriving.SetSourceRange(
            new TimeRange(new RationalTime(0, 24), new RationalTime(24, 24)));

        // The item is built in a timeline of its own and moves into the
        // track's as the edit places it.
        Otio.Insert(arriving, built.Track, new RationalTime(24, 24), false, null);

        CheckEq(built.Track.ChildCount(), 3, "the track's children after an insert");
        CheckEq(arriving.Name(), "C", "the inserted clip stopped answering");
        Check(arriving.Parent() is not null, "the inserted clip has no parent");
    }

    private static void AnObjectOfAnAbsorbedTimelineFollowsIt()
    {
        var track = new Track("V1", "Video");
        var clip = new Clip("guest");

        // The handle held from before the move is translated on the way, so it
        // still names the same object afterwards.
        track.AppendChild(clip);

        CheckEq(track.ChildCount(), 1, "the track did not take it");
        CheckEq(clip.Name(), "guest", "the name did not travel");
        Check(track.HasChild(clip), "the track does not know its own child");
        Check(clip.Parent() is not null, "the clip has no parent");
    }

    private static void MetadataGoesInAndComesBack()
    {
        var clip = new Clip("A");

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

    private static void TimeValuesComputeWithoutATimeline()
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

    /// An object holds the arena rather than the raw pointer, so that closing
    /// the timeline leaves the object naming nothing instead of dangling.
    private static void AnObjectOutlivingItsTimelineFailsRatherThanCrashing()
    {
        SerializableObject survivor;
        SerializableObject sibling;
        {
            var track = new Track("V1", "Video");
            survivor = new Clip("A");
            sibling = new Clip("B");
            track.AppendChild(survivor);
            track.AppendChild(sibling);
            track.Close();
        }
        CheckEq(Threw(() => survivor.Name()), Status.NullPointer, "reading a closed timeline");
        CheckEq(
            Threw(() => survivor.SetName("B")),
            Status.NullPointer,
            "writing to a closed timeline");
        CheckEq(
            Threw(() => survivor.FindClips()),
            Status.NullPointer,
            "searching a closed timeline");
        // Asking what schema it is answers "none" rather than reading anything.
        Check(!survivor.IsA(NodeKind.Clip), "a closed timeline's object still has a schema");
        // Two objects of the same closed timeline still compare as themselves.
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
        ("a timeline survives a round trip through JSON", ATimelineSurvivesARoundTripThroughJson),
        ("saving and opening again keeps the clips", SavingAndOpeningAgainKeepsTheClips),
        ("writing bytes in every format the library knows", WritingBytesInEveryFormatTheLibraryKnows),
        ("building a timeline from nothing", BuildingATimelineFromNothing),
        ("an object built on its own stands alone", AnObjectBuiltOnItsOwnStandsAlone),
        ("a freshly built object is enabled", AFreshlyBuiltObjectIsEnabled),
        ("a built object may be left unnamed", ABuiltObjectMayBeLeftUnnamed),
        ("no value is an answer and not a failure", NoValueIsAnAnswerAndNotAFailure),
        ("an object knows which schemas it is", AnObjectKnowsWhichSchemasItIs),
        ("clearing children hands them all back", ClearingChildrenHandsThemAllBack),
        ("every child and its range come back together", EveryChildAndItsRangeComeBackTogether),
        ("a stale handle is refused", AStaleHandleIsRefused),
        ("an object of no timeline fails rather than crashing", AnObjectOfNoTimelineFailsRatherThanCrashing),
        ("an object from another timeline is refused", AnObjectFromAnotherTimelineIsRefused),
        ("an edit puts a newly built item into a track", AnEditPutsANewlyBuiltItemIntoATrack),
        ("an object of an absorbed timeline follows it", AnObjectOfAnAbsorbedTimelineFollowsIt),
        ("metadata goes in and comes back", MetadataGoesInAndComesBack),
        ("time values compute without a timeline", TimeValuesComputeWithoutATimeline),
        ("an unreadable timecode is a failure", AnUnreadableTimecodeIsAFailure),
        ("a range answers about what it covers", ARangeAnswersAboutWhatItCovers),
        ("an object outliving its timeline fails rather than crashing", AnObjectOutlivingItsTimelineFailsRatherThanCrashing),
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
