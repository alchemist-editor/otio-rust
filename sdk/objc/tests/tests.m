// Tests for the generated Objective-C SDK.
//
// These are written by hand, not generated. A generator that also wrote its
// own tests would only prove it is self-consistent; what needs proving is
// that the Objective-C it writes does what someone reading it would expect,
// against the same library and the same fixtures the Rust tests read.
//
// GNUstep's Foundation predates object subscripting, so collections are read
// with objectAtIndex: and objectForKey: rather than with brackets.
//
// There is no test framework here, for the reason the C++ and C# suites give:
// the SDKs take no third-party dependencies, and there is no test runner both
// Apple's Foundation and GNUstep's have. This is a program. It prints what it
// ran and exits non-zero if anything failed.

#import <Foundation/Foundation.h>

#include <sched.h>

#import <OpenTimelineIO/OpenTimelineIO.h>

// The tests hold on to a couple of objects past the pool that made them, to
// prove an object outlives its document. That is the tests' own business, not
// the SDK's, so the macro lives here.
// The threads one test starts are released the same way, for the same reason.
#if __has_feature(objc_arc)
#define OTIO_KEEP(object) (object)
#define OTIO_LET_GO(object) ((void)(object))
#define OTIO_AUTORELEASE_TEST(object) (object)
#else
#define OTIO_KEEP(object) [(object) retain]
#define OTIO_LET_GO(object) [(object) release]
#define OTIO_AUTORELEASE_TEST(object) [(object) autorelease]
#endif

static int failures = 0;

static void Fail(NSString *what) {
    printf("  FAIL %s\n", what.UTF8String);
    failures += 1;
}

static void Check(BOOL condition, NSString *what) {
    if (!condition) {
        Fail(what);
    }
}

static void CheckEqual(NSInteger left, NSInteger right, NSString *what) {
    if (left != right) {
        Fail([NSString stringWithFormat:@"%@: %ld is not %ld", what, (long)left, (long)right]);
    }
}

static void CheckText(NSString *_Nullable left, NSString *right, NSString *what) {
    if (![right isEqualToString:left ?: @""]) {
        Fail([NSString stringWithFormat:@"%@: \"%@\" is not \"%@\"", what, left, right]);
    }
}

static void CheckNear(double left, double right, NSString *what) {
    if (fabs(left - right) > 1e-9) {
        Fail([NSString stringWithFormat:@"%@: %g is not %g", what, left, right]);
    }
}

/// The status a call failed with, for a test that wants to name it, or -1
/// where it did not fail at all.
static NSInteger StatusOf(NSError *_Nullable error) {
    return error == nil ? -1 : error.code;
}

/// The repository, found by walking up from wherever this was run.
static NSString *Repository(void) {
    NSFileManager *files = [NSFileManager defaultManager];
    NSString *directory = [files currentDirectoryPath];
    while (directory.length > 1) {
        NSString *marker = [directory stringByAppendingPathComponent:@"crates/otio-capi"];
        BOOL isDirectory = NO;
        if ([files fileExistsAtPath:marker isDirectory:&isDirectory] && isDirectory) {
            return directory;
        }
        directory = [directory stringByDeletingLastPathComponent];
    }
    printf("the repository is not above this program\n");
    exit(2);
}

/// The EDL the Rust adapter's own tests read, so that the two agree about
/// what is in it.
static NSString *ScreeningEdl(void) {
    return [Repository()
        stringByAppendingPathComponent:
            @"crates/otio-cmx3600/tests/data/screening_example.edl"];
}

/// An AAF the Rust adapter's tests read, whose five clips each carry the
/// MobID of the media they were cut from.
static NSString *ColoredClipsAaf(void) {
    return [Repository()
        stringByAppendingPathComponent:@"crates/otio-aaf/tests/data/colored_clips.aaf"];
}

/// A path in a directory of this run's own, so two tests cannot collide.
static NSString *Temporary(NSString *name) {
    static int counter = 0;
    counter += 1;
    NSString *directory = [NSTemporaryDirectory()
        stringByAppendingPathComponent:[NSString stringWithFormat:@"otio-objc-%d", counter]];
    [[NSFileManager defaultManager] createDirectoryAtPath:directory
                              withIntermediateDirectories:YES
                                               attributes:nil
                                                    error:NULL];
    return [directory stringByAppendingPathComponent:name];
}

#pragma mark - Tests

static void TheLibraryReportsAVersion(void) {
    Check(OTIOVersion().length > 0, @"the version is empty");
}

static void AnEnumSaysWhatTheCInterfaceCallsIt(void) {
    CheckText(OTIOFormatCName(OTIOFormatCMX3600), @"OTIO_FORMAT_CMX_3600", @"the format's C name");
    CheckText(OTIOStatusCName(OTIOStatusNoValue), @"OTIO_STATUS_NO_VALUE", @"the status's C name");
    CheckText(OTIOFormatName(OTIOFormatCMX3600), @"cmx_3600", @"the format's own name");
}

static void RatesAreClassified(void) {
    // The drop-frame rate is 30000/1001, which is not the 29.97 people write;
    // asking for the nearest SMPTE rate is what turns one into the other.
    Check(!OTIOIsDropFrameRate(29.97), @"29.97 is not the drop-frame rate");
    Check(
        OTIOIsDropFrameRate(OTIONearestSMPTETimecodeRate(29.97)),
        @"the nearest SMPTE rate to 29.97 is the drop-frame one");
    Check(!OTIOIsDropFrameRate(24), @"24 is not a drop-frame rate");
    Check(OTIOIsSMPTETimecodeRate(24), @"24 is an SMPTE rate");
}

static void AnAafReadsAndWritesBackOut(void) {
    NSError *error = nil;
    OTIOSerializableObject *root = OTIOReadFromFile(OTIOFormatAAF, ColoredClipsAaf(), NULL, &error);
    Check(root != nil, @"the AAF did not open");
    if (root == nil) {
        return;
    }
    CheckEqual((NSInteger)[root findClips:&error].count, 5, @"the number of clips");

    // A cut read from an AAF keeps each clip's MobID, so it writes back out
    // with no leave to make any up.
    NSData *written = OTIOWriteToBytes(OTIOFormatAAF, root, NULL, &error);
    Check(written != nil, @"the AAF did not write");
    OTIOSerializableObject *again = OTIOReadFromBytes(OTIOFormatAAF, written, NULL, &error);
    CheckEqual((NSInteger)[again findClips:&error].count, 5, @"the number of clips after writing");

    // A fixed time and seed write the same file twice.
    OTIOWriteOptions fixed = OTIOWriteOptionsDefault();
    fixed.aafTime = 1714979289;
    fixed.aafIDSeed = 59;
    NSData *first = OTIOWriteToBytes(OTIOFormatAAF, root, &fixed, &error);
    NSData *second = OTIOWriteToBytes(OTIOFormatAAF, root, &fixed, &error);
    Check(first != nil && [first isEqualToData:second],
          @"two writes with the same time and seed differ");

    OTIOReadOptions nested = OTIOReadOptionsDefault();
    nested.aafKeepNesting = YES;
    OTIOSerializableObject *kept = OTIOReadFromFile(OTIOFormatAAF, ColoredClipsAaf(), &nested, &error);
    CheckEqual((NSInteger)[kept findClips:&error].count, 5, @"the number of clips, nesting kept");
    CheckText(OTIOFormatName(OTIOFormatAAF), @"AAF", @"the format's own name");
}

/// The active media reference of each clip under a root, in order.
static NSArray *ActiveMedia(OTIOSerializableObject *root) {
    NSError *error = nil;
    NSMutableArray *references = [NSMutableArray array];
    for (OTIOSerializableObject *clip in [root findClips:&error]) {
        OTIOSerializableObject *reference = [(OTIOClip *)clip mediaReference:nil error:&error];
        if (reference != nil) {
            [references addObject:reference];
        }
    }
    return references;
}

static void ABundleCarriesItsMediaWithIt(void) {
    NSError *error = nil;
    NSFileManager *files = [NSFileManager defaultManager];

    // A cut of two clips: one whose media is a file, named relative to the
    // directory it is in, and one whose media is on the web. A bundle is
    // never written over, so the directory starts empty on every run.
    NSString *directory =
        [NSTemporaryDirectory() stringByAppendingPathComponent:@"otio-objc-bundles"];
    [files removeItemAtPath:directory error:NULL];
    [files createDirectoryAtPath:directory
        withIntermediateDirectories:YES
                         attributes:nil
                              error:NULL];
    NSString *movie = @"not really a movie";
    [[movie dataUsingEncoding:NSUTF8StringEncoding]
        writeToFile:[directory stringByAppendingPathComponent:@"shot.mov"]
         atomically:NO];

    OTIOTimeline *timeline = [OTIOTimeline timelineWithName:@"bundled" error:&error];
    OTIOStack *stack = (OTIOStack *)[timeline tracks:&error];
    OTIOTrack *track = [OTIOTrack trackWithName:@"V1" kind:@"Video" error:&error];
    [stack appendChild:track error:&error];
    NSArray<NSString *> *names = @[@"local", @"remote"];
    NSArray<NSString *> *urls = @[@"shot.mov", @"https://example.com/remote.mov"];
    for (NSUInteger index = 0; index < names.count; index++) {
        OTIOClip *clip = [OTIOClip clipWithName:[names objectAtIndex:index] error:&error];
        OTIOExternalReference *reference =
            [OTIOExternalReference externalReferenceWithName:[names objectAtIndex:index]
                                                   targetURL:[urls objectAtIndex:index]
                                                       error:&error];
        [clip setMediaReference:@"DEFAULT_MEDIA" reference:reference error:&error];
        [clip setActiveMediaReferenceKey:@"DEFAULT_MEDIA" error:&error];
        [track appendChild:clip error:&error];
    }
    Check(error == nil, @"building the cut reported a failure");

    // Upstream's default refuses media that is not a file.
    OTIOWriteOptions options = OTIOWriteOptionsDefault();
    options.bundleMediaBaseDir = directory;
    Check(!OTIOWriteToFile(OTIOFormatOTIOZ, timeline,
                           [directory stringByAppendingPathComponent:@"refused.otioz"],
                           &options, &error),
          @"a web reference was bundled under the default policy");
    CheckEqual(StatusOf(error), OTIOStatusIoError, @"the refusal's status");

    options.bundleMediaPolicy = OTIOBundleMediaPolicyMissingIfNotFile;
    OTIOFormat formats[] = {OTIOFormatOTIOZ, OTIOFormatOTIOD};
    for (size_t slot = 0; slot < 2; slot++) {
        OTIOFormat format = formats[slot];
        error = nil;
        NSString *path = [directory
            stringByAppendingPathComponent:[@"cut." stringByAppendingString:OTIOFormatName(format)]];
        Check(OTIOWriteToFile(format, timeline, path, &options, &error), @"the bundle did not write");

        // Read as it is, the file's reference points into the bundle and
        // the web one is missing.
        OTIOSerializableObject *plain = OTIOOpen(path, &error);
        NSArray *references = ActiveMedia(plain);
        CheckEqual((NSInteger)references.count, 2, @"the references read back");
        if (references.count != 2) {
            continue;
        }
        CheckText([(OTIOExternalReference *)[references objectAtIndex:0] targetURL:&error],
                  @"media/shot.mov", @"the bundled reference");
        Check([[references objectAtIndex:1] isKindOfClass:[OTIOMissingReference class]],
              @"the web reference was not made missing");

        // With absolute paths it points at a real copy of the media, which
        // an .otioz has to be unpacked to have.
        OTIOReadOptions read = OTIOReadOptionsDefault();
        read.bundleAbsoluteMediaPaths = YES;
        NSString *unpacked = path;
        if (format == OTIOFormatOTIOZ) {
            unpacked = [directory stringByAppendingPathComponent:@"unpacked"];
            read.bundleExtractPath = unpacked;
        }
        OTIOSerializableObject *absolute = OTIOReadFromFile(format, path, &read, &error);
        NSString *url =
            [(OTIOExternalReference *)[ActiveMedia(absolute) objectAtIndex:0] targetURL:&error];
        CheckText(url, [unpacked stringByAppendingPathComponent:@"media/shot.mov"],
                  @"the absolute reference");
        CheckText([NSString stringWithContentsOfFile:url encoding:NSUTF8StringEncoding error:NULL],
                  movie, @"the bundled media");

        // A bundle is never written over.
        Check(!OTIOWriteToFile(format, timeline, path, &options, &error),
              @"a bundle was written over");
        CheckEqual(StatusOf(error), OTIOStatusIoError, @"the overwrite's status");
    }

    // Leaving every reference missing bundles no media at all.
    error = nil;
    NSString *empty = [directory stringByAppendingPathComponent:@"no-media.otioz"];
    options.bundleMediaPolicy = OTIOBundleMediaPolicyAllMissing;
    Check(OTIOWriteToFile(OTIOFormatOTIOZ, timeline, empty, &options, &error),
          @"the media-less bundle did not write");
    for (OTIOSerializableObject *reference in ActiveMedia(OTIOOpen(empty, &error))) {
        Check([reference isKindOfClass:[OTIOMissingReference class]],
              @"a reference survived the all-missing policy");
    }

    // A bundle lives on disk, so it is not written as bytes.
    Check(OTIOWriteToBytes(OTIOFormatOTIOZ, timeline, NULL, &error) == nil,
          @"an .otioz was written as bytes");
    CheckEqual(StatusOf(error), OTIOStatusUnsupported, @"the bytes refusal's status");
    CheckText(OTIOFormatName(OTIOFormatOTIOD), @"otiod", @"the format's own name");
}

static void ReadingAnEdlFindsItsClips(void) {
    NSError *error = nil;
    OTIOSerializableObject *root =
        OTIOReadFromFile(OTIOFormatCMX3600, ScreeningEdl(), NULL, &error);
    Check(root != nil, @"the EDL did not open");
    if (root == nil) {
        return;
    }

    NSArray<OTIOSerializableObject *> *clips = [root findClips:&error];
    CheckEqual((NSInteger)clips.count, 9, @"the number of clips");

    // Every one of them really is a clip, and the library says so.
    NSInteger counted = 0;
    for (OTIOSerializableObject *node in clips) {
        Check([node isA:OTIONodeKindClip], @"a found clip is not a clip");
        if ([node isKindOfClass:[OTIOClip class]]) {
            counted += 1;
        }
    }
    CheckEqual(counted, 9, @"the number that are OTIOClip");
}

/// The quickstart in `sdk/objc/README.md` is generated, so nothing compiles
/// it. This is that example, so that it cannot go stale.
static void TheQuickstartFromTheReadmeRuns(void) {
    NSError *error = nil;
    OTIOSerializableObject *timeline = OTIOOpen(ScreeningEdl(), &error);
    Check(timeline != nil, @"the timeline did not open");
    if (timeline == nil) {
        return;
    }

    NSInteger named = 0;
    Check([timeline isKindOfClass:[OTIOTimeline class]], @"the root is not a timeline");
    for (OTIOSerializableObject *clip in [timeline findClips:&error]) {
        Check([clip name:&error].length > 0, @"a clip has no name");
        named += 1;
    }
    CheckEqual(named, 9, @"the number of named clips");
}

static void OpenWorksOutTheFormatFromTheName(void) {
    NSError *error = nil;
    OTIOSerializableObject *root = OTIOOpen(ScreeningEdl(), &error);
    Check(root != nil, @"the timeline did not open");
    Check(
        [[root name:&error] rangeOfString:@"Example_Screening"].location != NSNotFound,
        @"the root's name");
}

static void OpenDeclinesASuffixNoFormatClaims(void) {
    NSError *error = nil;
    OTIOSerializableObject *root = OTIOOpen(@"/tmp/nothing.wav", &error);
    Check(root == nil, @"a .wav opened");
    CheckEqual(StatusOf(error), OTIOStatusNoValue, @"opening a .wav");
    Check(OTIOIsNoValue(error), @"OTIOIsNoValue did not recognise it");
}

static void ATimelineSurvivesARoundTripThroughJson(void) {
    NSError *error = nil;
    OTIOSerializableObject *root = OTIOOpen(ScreeningEdl(), &error);
    NSString *text = [root toJSON:2 error:&error];
    Check([text rangeOfString:@"Timeline"].location != NSNotFound, @"the JSON has no timeline");

    OTIOSerializableObject *again = OTIOFromJSON(text, &error);
    Check(again != nil, @"the JSON did not read back");
    CheckEqual((NSInteger)[again findClips:&error].count, 9, @"the clips after a round trip");
}

static void SavingAndOpeningAgainKeepsTheClips(void) {
    NSError *error = nil;
    OTIOSerializableObject *root = OTIOOpen(ScreeningEdl(), &error);
    NSString *path = Temporary(@"round-trip.otio");
    Check(OTIOSave(root, path, &error), @"the timeline did not save");

    OTIOSerializableObject *again = OTIOOpen(path, &error);
    Check(again != nil, @"the saved timeline did not open");
    CheckEqual((NSInteger)[again findClips:&error].count, 9, @"the clips after saving");
}

static void WritingBytesInEveryFormatTheLibraryKnows(void) {
    NSError *error = nil;
    OTIOSerializableObject *root = OTIOOpen(ScreeningEdl(), &error);
    OTIOFormat formats[] = {OTIOFormatOTIOJSON, OTIOFormatCMX3600};
    for (size_t slot = 0; slot < sizeof(formats) / sizeof(formats[0]); slot++) {
        NSData *written = OTIOWriteToBytes(formats[slot], root, NULL, &error);
        Check(written.length > 0, @"a format wrote nothing");
    }
}

/// A timeline with one video track holding two clips.
///
/// Every object is made on its own and joins the timeline when it is put into
/// one, so nothing has to exist before the thing it goes into.
static OTIOTrack *MakeTimeline(NSMutableArray *clips) {
    NSError *error = nil;
    OTIOTimeline *timeline = [OTIOTimeline timelineWithName:@"Assembly" error:&error];
    OTIOStack *stack = [OTIOStack stackWithName:@"tracks" error:&error];
    [timeline setTracks:stack error:&error];
    OTIOTrack *track = [OTIOTrack trackWithName:@"V1" kind:@"Video" error:&error];
    [stack appendChild:track error:&error];

    NSArray<NSString *> *names = @[@"A", @"B"];
    for (NSUInteger index = 0; index < names.count; index++) {
        OTIOClip *clip = [OTIOClip clipWithName:[names objectAtIndex:index] error:&error];
        OTIORationalTime start = OTIORationalTimeMake((double)index * 24, 24);
        OTIOTimeRange span = OTIOTimeRangeMake(start, OTIORationalTimeMake(24, 24));
        [clip setSourceRange:span error:&error];
        [track appendChild:clip error:&error];
        [clips addObject:clip];
    }
    Check(error == nil, @"building the timeline reported a failure");
    return track;
}

static void BuildingATimelineFromNothing(void) {
    NSError *error = nil;
    NSMutableArray *clips = [NSMutableArray array];
    OTIOTrack *track = MakeTimeline(clips);

    NSUInteger children = 0;
    Check([track getChildCount:&children error:&error], @"the child count failed");
    CheckEqual((NSInteger)children, 2, @"the track's children");
    CheckText([[clips objectAtIndex:0] name:&error], @"A", @"the first clip's name");
    CheckText([track kind:&error], @"Video", @"the track's kind");

    // The whole track is as long as the two clips together.
    OTIORationalTime duration;
    Check([track getDuration:&duration error:&error], @"the duration failed");
    CheckNear(OTIORationalTimeToSeconds(duration), 2, @"the track's duration");
}

/// A clip built on its own is a whole timeline of one object until it joins
/// another, which is what lets it exist before its track does.
static void AnObjectBuiltOnItsOwnStandsAlone(void) {
    NSError *error = nil;
    OTIOClip *clip = [OTIOClip clipWithName:@"alone" error:&error];
    Check(clip != nil, @"the clip was not built");
    CheckText([clip name:&error], @"alone", @"the name of an object with no timeline");
    Check([clip isLive], @"an object built on its own is not live");

    error = nil;
    Check([clip parent:&error] == nil, @"an object built on its own has a parent");
    Check(OTIOIsNoValue(error), @"having no parent is not reported as no value");
}

static void AFreshlyBuiltObjectIsEnabled(void) {
    NSError *error = nil;
    OTIOClip *clip = [OTIOClip clipWithName:@"A" error:&error];

    BOOL enabled = NO;
    Check([clip getEnabled:&enabled error:&error], @"reading enabled failed");
    Check(enabled, @"a new clip is not enabled");
    Check([clip setEnabled:NO error:&error], @"disabling failed");
    Check([clip getEnabled:&enabled error:&error], @"reading enabled failed");
    Check(!enabled, @"a disabled clip says it is enabled");
}

static void ABuiltObjectMayBeLeftUnnamed(void) {
    NSError *error = nil;
    OTIOClip *clip = [OTIOClip clipWithName:nil error:&error];
    Check(clip != nil, @"an unnamed clip was not built");
    CheckText([clip name:&error], @"", @"an unnamed clip has a name");
}

static void NoValueIsAnAnswerAndNotAFailure(void) {
    NSError *error = nil;
    OTIOClip *clip = [OTIOClip clipWithName:@"untrimmed" error:&error];

    // An item that uses all of its media has no source range, and that is an
    // answer rather than a failure.
    OTIOTimeRange range;
    error = nil;
    Check(![clip getSourceRange:&range error:&error], @"an untrimmed clip reported a range");
    Check(OTIOIsNoValue(error), @"the absent range is not reported as no value");

    OTIOTimeRange span =
        OTIOTimeRangeMake(OTIORationalTimeMake(0, 24), OTIORationalTimeMake(12, 24));
    error = nil;
    Check([clip setSourceRange:span error:&error], @"setting the range failed");
    Check([clip getSourceRange:&range error:&error], @"a trimmed clip reports no range");
    CheckNear(OTIORationalTimeToSeconds(range.duration), 0.5, @"the range's duration");

    Check([clip clearSourceRange:&error], @"clearing the range failed");
    error = nil;
    Check(![clip getSourceRange:&range error:&error], @"a cleared range comes back");
    Check(OTIOIsNoValue(error), @"a cleared range is not reported as no value");
}

static void AnObjectKnowsWhichSchemasItIs(void) {
    NSError *error = nil;
    OTIOClip *clip = [OTIOClip clipWithName:@"A" error:&error];

    Check([clip isA:OTIONodeKindClip], @"a clip is not a clip");
    Check([clip isA:OTIONodeKindItem], @"a clip is not an item");
    Check([clip isA:OTIONodeKindComposable], @"a clip is not composable");
    Check([clip isA:OTIONodeKindSerializableObject], @"a clip is not serializable");
    Check(![clip isA:OTIONodeKindTrack], @"a clip says it is a track");

    OTIOSerializableObject *node = clip;
    Check([node isKindOfClass:[OTIOItem class]], @"a clip is not an OTIOItem");
    Check(![node isKindOfClass:[OTIOTrack class]], @"a clip is an OTIOTrack");

    OTIONodeKind kind;
    Check([clip getSchemaKind:&kind error:&error], @"reading the schema kind failed");
    CheckEqual(kind, OTIONodeKindClip, @"the clip's schema kind");
    CheckText([clip schemaName:&error], @"Clip", @"the clip's schema name");
}

static void ClearingChildrenHandsThemAllBack(void) {
    NSError *error = nil;
    NSMutableArray *clips = [NSMutableArray array];
    OTIOTrack *track = MakeTimeline(clips);

    NSArray<OTIOSerializableObject *> *taken = [track clearChildren:&error];
    CheckEqual((NSInteger)taken.count, (NSInteger)clips.count, @"the number handed back");
    NSUInteger children = 1;
    Check([track getChildCount:&children error:&error], @"the child count failed");
    CheckEqual((NSInteger)children, 0, @"the track is not empty");
    CheckText([[taken objectAtIndex:0] name:&error], @"A", @"the first one handed back");
    CheckText([[taken objectAtIndex:1] name:&error], @"B", @"the second one handed back");
}

static void EveryChildAndItsRangeComeBackTogether(void) {
    NSError *error = nil;
    NSMutableArray *clips = [NSMutableArray array];
    OTIOTrack *track = MakeTimeline(clips);

    NSArray<OTIOSerializableObject *> *nodes = nil;
    NSArray<NSValue *> *ranges = nil;
    Check(
        [track getRangesOfChildren:&nodes ranges:&ranges error:&error],
        @"reading the ranges failed");
    CheckEqual((NSInteger)nodes.count, 2, @"the children");
    CheckEqual((NSInteger)ranges.count, 2, @"the ranges");
    if (ranges.count == 2) {
        // A struct has no object identity, so a list of them is boxed; this is
        // the only place in the SDK where that shows.
        OTIOTimeRange first = OTIOTimeRangeUnboxed([ranges objectAtIndex:0]);
        OTIOTimeRange second = OTIOTimeRangeUnboxed([ranges objectAtIndex:1]);
        CheckNear(OTIORationalTimeToSeconds(first.startTime), 0, @"the first child's start");
        CheckNear(OTIORationalTimeToSeconds(second.startTime), 1, @"the second child's start");
    }
}

static void AStaleHandleIsRefused(void) {
    NSError *error = nil;
    OTIOClip *clip = [OTIOClip clipWithName:@"A" error:&error];
    Check([clip removeFromTimeline:&error], @"removing the clip failed");

    Check(![clip isLive], @"a removed clip is still live");
    error = nil;
    Check([clip name:&error] == nil, @"a removed clip still has a name");
    CheckEqual(StatusOf(error), OTIOStatusStaleHandle, @"a removed clip's name");
}

static void AnObjectOfNoTimelineFailsRatherThanCrashing(void) {
    OTIOSerializableObject *orphan = [OTIOSerializableObject none];
    Check([orphan isNone], @"the none object is not none");

    NSError *error = nil;
    Check([orphan name:&error] == nil, @"asking an orphan its name worked");
    Check(error != nil, @"asking an orphan its name reported no failure");
}

static void AnObjectFromAnotherTimelineIsRefused(void) {
    NSError *error = nil;
    OTIOTrack *track = [OTIOTrack trackWithName:@"V1" kind:@"Video" error:&error];
    OTIOTrack *elsewhere = [OTIOTrack trackWithName:@"V2" kind:@"Video" error:&error];
    OTIOClip *stranger = [OTIOClip clipWithName:@"elsewhere" error:&error];
    Check([elsewhere appendChild:stranger error:&error], @"the other track did not take it");

    // Even the question that looks harmless is refused. "Is this mine" has an
    // obvious answer for an object from elsewhere, but answering it would mean
    // resolving a handle of another arena against this one, where it names an
    // unrelated object. The refusal is the answer.
    BOOL has = YES;
    error = nil;
    Check(
        ![track getHasChild:&has child:stranger error:&error],
        @"a foreign clip was asked about");
    CheckEqual(StatusOf(error), OTIOStatusInvalidArgument, @"asking about a foreign clip");
    Check(OTIOIsOtherTimeline(error), @"the refusal does not say it was another timeline's");

    // A call that cannot fail answers rather than reporting, and the answer is
    // no.
    Check(![track equals:stranger], @"a track claims to be a foreign clip");

    // The timeline that refused it is still whole, which is what tells
    // refusing apart from absorbing and then failing.
    NSUInteger children = 0;
    error = nil;
    Check([elsewhere getChildCount:&children error:&error], @"the other child count failed");
    CheckEqual((NSInteger)children, 1, @"the other timeline lost its clip");
    CheckText([stranger name:&error], @"elsewhere", @"the foreign clip stopped answering");
}

/// The refusal of another timeline's object has the code any unusable
/// argument has, so the code alone cannot tell it apart; OTIOIsOtherTimeline
/// can, and says no to every other failure, including one with that code.
static void ARefusalOfAnotherTimelinesObjectIsToldApart(void) {
    NSError *error = nil;
    OTIOTrack *first = [OTIOTrack trackWithName:@"V1" kind:@"Video" error:&error];
    OTIOTrack *second = [OTIOTrack trackWithName:@"V2" kind:@"Video" error:&error];

    error = nil;
    Check(OTIOFlattenTracks(@[first, second], &error) == nil, @"two timelines were flattened");
    CheckEqual(StatusOf(error), OTIOStatusInvalidArgument, @"flattening two timelines");
    Check(OTIOIsOtherTimeline(error), @"a list from two timelines is not told apart");

    error = nil;
    Check(OTIOFlattenTracks(@[], &error) == nil, @"nothing was flattened");
    CheckEqual(StatusOf(error), OTIOStatusInvalidArgument, @"flattening nothing");
    Check(!OTIOIsOtherTimeline(error), @"an empty list is taken for another timeline's");

    OTIOClip *clip = [OTIOClip clipWithName:@"A" error:&error];
    Check([clip removeFromTimeline:&error], @"removing the clip failed");
    error = nil;
    Check([clip name:&error] == nil, @"a removed clip still has a name");
    Check(!OTIOIsOtherTimeline(error), @"a stale handle is taken for another timeline's");
    Check(!OTIOIsOtherTimeline(nil), @"no failure is taken for another timeline's");
}

static void AnEditPutsANewlyBuiltItemIntoATrack(void) {
    NSError *error = nil;
    NSMutableArray *clips = [NSMutableArray array];
    OTIOTrack *track = MakeTimeline(clips);

    OTIOClip *arriving = [OTIOClip clipWithName:@"C" error:&error];
    OTIOTimeRange span =
        OTIOTimeRangeMake(OTIORationalTimeMake(0, 24), OTIORationalTimeMake(24, 24));
    Check([arriving setSourceRange:span error:&error], @"setting the range failed");

    // The item is built in a timeline of its own and moves into the track's as
    // the edit places it.
    Check(
        OTIOInsert(arriving, track, OTIORationalTimeMake(24, 24), NO, nil, &error),
        @"the insert failed");

    NSUInteger children = 0;
    Check([track getChildCount:&children error:&error], @"the child count failed");
    CheckEqual((NSInteger)children, 3, @"the track's children after an insert");
    CheckText([arriving name:&error], @"C", @"the inserted clip stopped answering");
    Check([arriving parent:&error] != nil, @"the inserted clip has no parent");
}

static void AnObjectOfAnAbsorbedTimelineFollowsIt(void) {
    NSError *error = nil;
    OTIOTrack *track = [OTIOTrack trackWithName:@"V1" kind:@"Video" error:&error];
    OTIOClip *clip = [OTIOClip clipWithName:@"guest" error:&error];

    // The handle held from before the move is translated on the way, so it
    // still names the same object afterwards.
    Check([track appendChild:clip error:&error], @"the track did not take it");

    NSUInteger children = 0;
    Check([track getChildCount:&children error:&error], @"the child count failed");
    CheckEqual((NSInteger)children, 1, @"the track did not take it");
    CheckText([clip name:&error], @"guest", @"the name did not travel");

    BOOL has = NO;
    Check([track getHasChild:&has child:clip error:&error], @"asking about the child failed");
    Check(has, @"the track does not know its own child");
    Check([clip parent:&error] != nil, @"the clip has no parent");
}

static void MetadataGoesInAndComesBack(void) {
    NSError *error = nil;
    OTIOClip *clip = [OTIOClip clipWithName:@"A" error:&error];

    Check([clip.metadata setString:@"reel" value:@"ZZ100" error:&error], @"setting the reel");
    Check([clip.metadata setInt:@"take" value:3 error:&error], @"setting the take");
    Check([clip.metadata setBool:@"circled" value:YES error:&error], @"setting circled");
    Check([clip.metadata setDouble:@"gain" value:0.5 error:&error], @"setting the gain");

    CheckText([clip.metadata getString:@"reel" error:&error], @"ZZ100", @"the reel");
    int64_t take = 0;
    Check([clip.metadata getInt:&take path:@"take" error:&error], @"reading the take");
    CheckEqual((NSInteger)take, 3, @"the take");
    BOOL circled = NO;
    Check([clip.metadata getBool:&circled path:@"circled" error:&error], @"reading circled");
    Check(circled, @"the circled flag");
    double gain = 0;
    Check([clip.metadata getDouble:&gain path:@"gain" error:&error], @"reading the gain");
    CheckNear(gain, 0.5, @"the gain");

    BOOL has = NO;
    Check([clip.metadata getContains:&has path:@"reel" error:&error], @"asking for the reel");
    Check(has, @"the reel is missing");
    Check([clip.metadata getContains:&has path:@"nothing" error:&error], @"asking for nothing");
    Check(!has, @"a key that was never set is there");

    // A path is followed, not created: the dictionary has to exist before
    // anything can be written inside it.
    Check([clip.metadata setDictionary:@"cmx_3600" error:&error], @"making the dictionary");
    Check(
        [clip.metadata setString:@"cmx_3600.reel" value:@"AX" error:&error],
        @"setting the nested reel");
    CheckText([clip.metadata getString:@"cmx_3600.reel" error:&error], @"AX", @"the nested reel");

    Check([clip.metadata clear:&error], @"clearing the metadata");
    Check([clip.metadata getContains:&has path:@"reel" error:&error], @"asking after clearing");
    Check(!has, @"the metadata was not cleared");
}

static void TimeValuesComputeWithoutATimeline(void) {
    NSError *error = nil;
    OTIORationalTime time = OTIORationalTimeMake(48, 24);
    CheckNear(OTIORationalTimeToSeconds(time), 2, @"the time in seconds");
    CheckEqual((NSInteger)OTIORationalTimeToFrames(time), 48, @"the time in frames");
    Check(
        OTIORationalTimeEquals(OTIORationalTimeRescaledTo(time, 48), OTIORationalTimeMake(96, 48)),
        @"rescaling to 48");
    Check(
        OTIORationalTimeEquals(
            OTIORationalTimeDurationFromStartEndTime(OTIORationalTimeMake(0, 24), time), time),
        @"the duration from zero");
    Check(OTIORationalTimeIsValid(time), @"the time is not valid");

    NSString *timecode = OTIORationalTimeToTimecode(time, &error);
    CheckText(timecode, @"00:00:02:00", @"the timecode");

    OTIORationalTime read;
    Check(
        OTIORationalTimeFromTimecode(@"00:00:02:00", 24, &read, &error),
        @"reading a timecode failed");
    Check(OTIORationalTimeEquals(read, time), @"reading a timecode");
}

static void AnUnreadableTimecodeIsAFailure(void) {
    NSError *error = nil;
    OTIORationalTime read;
    Check(
        !OTIORationalTimeFromTimecode(@"not a timecode", 24, &read, &error),
        @"nonsense read as a timecode");
    CheckEqual(StatusOf(error), OTIOStatusTimeError, @"reading nonsense as a timecode");
}

static void ARangeAnswersAboutWhatItCovers(void) {
    OTIOTimeRange span =
        OTIOTimeRangeMake(OTIORationalTimeMake(0, 24), OTIORationalTimeMake(24, 24));
    Check(
        OTIORationalTimeEquals(OTIOTimeRangeEndTimeExclusive(span), OTIORationalTimeMake(24, 24)),
        @"the exclusive end");
    Check(
        OTIOTimeRangeContainsTime(span, OTIORationalTimeMake(12, 24)),
        @"the middle is not inside");
    Check(
        !OTIOTimeRangeContainsTime(span, OTIORationalTimeMake(24, 24)),
        @"the exclusive end is inside");
}

/// Whether a piece of text says something, spelled the way GNUstep's
/// Foundation can answer as well as Apple's.
static BOOL Says(NSString *_Nullable text, NSString *what) {
    return text != nil && [text rangeOfString:what].location != NSNotFound;
}

/// One thread's share of the test below. It fails in two different ways,
/// taking turns, and counts every failure that came back with a message other
/// than the one its own call should have written.
///
/// The instance variables and @synthesize are spelled out because GNUstep's
/// runtime, like the SDK's own classes, predates their being implied.
@interface OTIOFailingThread : NSThread {
    NSInteger _number;
    NSInteger _rounds;
    NSInteger _wrongTimecodes;
    NSInteger _wrongIndexes;
}
@property(nonatomic) NSInteger number;
@property(nonatomic) NSInteger rounds;
@property(nonatomic) NSInteger wrongTimecodes;
@property(nonatomic) NSInteger wrongIndexes;
@end

@implementation OTIOFailingThread

@synthesize number = _number;
@synthesize rounds = _rounds;
@synthesize wrongTimecodes = _wrongTimecodes;
@synthesize wrongIndexes = _wrongIndexes;

- (void)main {
    @autoreleasepool {
        NSError *error = nil;
        // A timeline of its own, because a timeline being read on one thread
        // may not be touched on another; what is shared is the library.
        OTIOTrack *track = [OTIOTrack trackWithName:@"V1" kind:@"Video" error:&error];
        if (track == nil) {
            self.wrongIndexes = self.rounds;
            return;
        }
        for (NSInteger round = 0; round < self.rounds; round++) {
            @autoreleasepool {
                // Every call is asked something only it asks, so the message
                // can be checked against the call that should have written
                // it rather than only for being there at all.
                NSInteger asked = self.number * 100000 + round;
                NSError *failure = nil;
                // Odd and even threads start on different kinds, so at any
                // moment some threads are failing one way and some the other.
                if ((round + self.number) % 2 == 0) {
                    NSString *nonsense = [NSString stringWithFormat:@"nonsense %ld", (long)asked];
                    OTIORationalTime read;
                    BOOL readIt = OTIORationalTimeFromTimecode(nonsense, 24, &read, &failure);
                    // Give another thread the chance to fail in between the
                    // call and the reading of what it said.
                    sched_yield();
                    if (readIt || failure.code != OTIOStatusTimeError
                        || !Says(failure.localizedDescription, nonsense)
                        || Says(failure.localizedDescription, @"out of range")) {
                        self.wrongTimecodes += 1;
                    }
                } else {
                    OTIOTimeRange range;
                    BOOL found = [track getRangeOfChildAtIndex:&range index:asked error:&failure];
                    sched_yield();
                    // Upstream's wording, which names no index; the
                    // timecodes above are what tell two threads apart.
                    if (found || failure.code != OTIOStatusCoreError
                        || !Says(failure.localizedDescription, @"illegal index")) {
                        self.wrongIndexes += 1;
                    }
                }
            }
        }
        [track close];
    }
}

@end

/// The library hands each call's message back beside its status, rather than
/// leaving it somewhere a later call could overwrite, so there is nothing to
/// keep a failure and its message on the same thread. Many threads failing in
/// two different ways at once must each still read the sentence their own
/// call wrote.
static void EveryFailureCarriesItsOwnMessageWhateverThreadItRanOn(void) {
    NSMutableArray<OTIOFailingThread *> *threads = [NSMutableArray array];
    for (NSInteger number = 0; number < 16; number++) {
        OTIOFailingThread *thread = [[OTIOFailingThread alloc] init];
        thread.number = number;
        thread.rounds = 200;
        [threads addObject:thread];
        OTIO_LET_GO(thread);
    }
    for (OTIOFailingThread *thread in threads) {
        [thread start];
    }
    // Neither Foundation has a join, so this waits for each to say it is done.
    for (OTIOFailingThread *thread in threads) {
        while (![thread isFinished]) {
            [NSThread sleepForTimeInterval:0.001];
        }
    }
    for (OTIOFailingThread *thread in threads) {
        CheckEqual(
            thread.wrongTimecodes, 0,
            [NSString stringWithFormat:@"thread %ld's unreadable timecodes with the wrong message",
                                       (long)thread.number]);
        CheckEqual(
            thread.wrongIndexes, 0,
            [NSString stringWithFormat:@"thread %ld's missing children with the wrong message",
                                       (long)thread.number]);
    }
}

/// An object holds its arena rather than the raw pointer, so that closing the
/// timeline leaves the object naming nothing instead of leaving it dangling.
static void AnObjectOutlivingItsTimelineFailsRatherThanCrashing(void) {
    NSError *error = nil;
    OTIOSerializableObject *survivor = nil;
    OTIOSerializableObject *sibling = nil;
    @autoreleasepool {
        OTIOTrack *track = [OTIOTrack trackWithName:@"V1" kind:@"Video" error:&error];
        survivor = OTIO_KEEP([OTIOClip clipWithName:@"A" error:&error]);
        sibling = OTIO_KEEP([OTIOClip clipWithName:@"B" error:&error]);
        [track appendChild:survivor error:&error];
        [track appendChild:sibling error:&error];
        [track close];
    }

    error = nil;
    Check([survivor name:&error] == nil, @"a closed timeline's object still has a name");
    CheckEqual(StatusOf(error), OTIOStatusNullPointer, @"reading a closed timeline");
    error = nil;
    Check(![survivor setName:@"B" error:&error], @"writing to a closed timeline worked");
    CheckEqual(StatusOf(error), OTIOStatusNullPointer, @"writing to a closed timeline");
    error = nil;
    Check([survivor findClips:&error] == nil, @"searching a closed timeline worked");
    CheckEqual(StatusOf(error), OTIOStatusNullPointer, @"searching a closed timeline");

    // Asking what schema it is answers "none" rather than reading anything.
    Check(![survivor isA:OTIONodeKindClip], @"a closed timeline's object has a schema");
    // Two objects of the same closed timeline still compare as themselves.
    Check([survivor isEqual:survivor], @"an object is not itself");
    Check(![survivor isEqual:sibling], @"two objects compare equal");
}

/// Every scenario in crates/otio-sdk-model/src/conformance.rs, rendered into
/// conformance.m with the list that runs them, so none is left out here.
#pragma mark - Media linkers and hook scripts

// The registry is the library's, shared by the whole process, so every test
// here registers under names of its own and unregisters them when it ends.

/// Whether a failure is one a plugin, or the registry, reported.
static BOOL IsPluginError(NSError *_Nullable error) {
    return error != nil && [error.domain isEqualToString:OTIOErrorDomain]
        && error.code == (NSInteger)OTIOStatusPluginError;
}

/// A failure of a test plugin's own, in its own words.
static NSError *Trouble(NSString *text) {
    return [NSError errorWithDomain:@"OTIOTests"
                               code:1
                           userInfo:@{NSLocalizedDescriptionKey: text}];
}

/// A two-clip timeline as OTIO JSON, each clip pointing at media under
/// file:///media.
static NSData *PluginCut(void) {
    NSError *error = nil;
    OTIOTimeline *timeline = [OTIOTimeline timelineWithName:@"Cut" error:&error];
    OTIOStack *stack = [OTIOStack stackWithName:@"tracks" error:&error];
    OTIOTrack *track = [OTIOTrack trackWithName:@"V1" kind:@"Video" error:&error];
    [timeline setTracks:stack error:&error];
    [stack appendChild:track error:&error];
    NSArray<NSString *> *names = @[@"first", @"second"];
    for (NSString *name in names) {
        OTIOClip *clip = [OTIOClip clipWithName:name error:&error];
        NSString *url = [NSString stringWithFormat:@"file:///media/%@.mov", name];
        OTIOExternalReference *reference =
            [OTIOExternalReference externalReferenceWithName:name targetURL:url error:&error];
        [clip setMediaReference:@"DEFAULT_MEDIA" reference:reference error:&error];
        [clip setActiveMediaReferenceKey:@"DEFAULT_MEDIA" error:&error];
        [track appendChild:clip error:&error];
    }
    NSData *written = OTIOWriteToBytes(OTIOFormatOTIOJSON, timeline, NULL, &error);
    Check(error == nil && written.length > 0, @"building the cut reported a failure");
    return written;
}

/// The target URL of the first clip's active media.
static NSString *FirstURL(OTIOSerializableObject *root) {
    NSError *error = nil;
    NSArray<OTIOSerializableObject *> *clips = [root findClips:&error];
    if (clips.count == 0) {
        Fail(@"there are no clips to ask");
        return @"";
    }
    OTIOSerializableObject *media = [(OTIOClip *)[clips objectAtIndex:0] mediaReference:nil
                                                                                  error:&error];
    if (![media isKindOfClass:[OTIOExternalReference class]]) {
        Fail(@"the clip's media is not an external reference");
        return @"";
    }
    return [(OTIOExternalReference *)media targetURL:&error];
}

/// Links each clip to a proxy under the root its arguments name.
@interface ProxyLinker : NSObject <OTIOMediaLinker>
@end

@implementation ProxyLinker
- (nullable OTIOMediaReference *)linkMediaReferenceForClip:(OTIOClip *)clip
                                                 arguments:(OTIOMetadata *)arguments
                                                     error:(NSError **)error {
    NSString *name = [clip name:error];
    if (name == nil) {
        return nil;
    }
    NSString *root = [arguments getString:@"root" error:NULL];
    if (root == nil) {
        if (error != NULL) {
            *error = Trouble(@"no root to link under");
        }
        return nil;
    }
    NSString *url = [NSString stringWithFormat:@"%@/%@.mov", root, name];
    return [OTIOExternalReference externalReferenceWithName:@"proxy" targetURL:url error:error];
}
@end

/// How many clips WatchingLinker has been handed.
static NSInteger watched = 0;

/// Counts the clips it is handed and leaves every one alone.
@interface WatchingLinker : NSObject <OTIOMediaLinker>
@end

@implementation WatchingLinker
- (nullable OTIOMediaReference *)linkMediaReferenceForClip:(OTIOClip *)clip
                                                 arguments:(OTIOMetadata *)arguments
                                                     error:(NSError **)error {
    watched += 1;
    return nil;
}
@end

/// Fails every clip, in its own words.
@interface OfflineLinker : NSObject <OTIOMediaLinker>
@end

@implementation OfflineLinker
- (nullable OTIOMediaReference *)linkMediaReferenceForClip:(OTIOClip *)clip
                                                 arguments:(OTIOMetadata *)arguments
                                                     error:(NSError **)error {
    if (error != NULL) {
        *error = Trouble(@"the proxies are offline");
    }
    return nil;
}
@end

/// Raises an exception rather than answering.
@interface RaisingLinker : NSObject <OTIOMediaLinker>
@end

@implementation RaisingLinker
- (nullable OTIOMediaReference *)linkMediaReferenceForClip:(OTIOClip *)clip
                                                 arguments:(OTIOMetadata *)arguments
                                                     error:(NSError **)error {
    [NSException raise:@"OTIODiskError" format:@"no disk"];
    return nil;
}
@end

/// Throws something that is not an NSException at all.
@interface ThrowingLinker : NSObject <OTIOMediaLinker>
@end

@implementation ThrowingLinker
- (nullable OTIOMediaReference *)linkMediaReferenceForClip:(OTIOClip *)clip
                                                 arguments:(OTIOMetadata *)arguments
                                                     error:(NSError **)error {
    @throw @"a bare string";
}
@end

/// Tries to take the lent clip into a track of its own.
@interface KidnappingLinker : NSObject <OTIOMediaLinker>
@end

@implementation KidnappingLinker
- (nullable OTIOMediaReference *)linkMediaReferenceForClip:(OTIOClip *)clip
                                                 arguments:(OTIOMetadata *)arguments
                                                     error:(NSError **)error {
    OTIOTrack *track = [OTIOTrack trackWithName:@"elsewhere" kind:@"Video" error:error];
    if (track == nil || ![track appendChild:clip error:error]) {
        return nil;
    }
    return nil;
}
@end

/// Writes who ran it into the metadata of what it is handed, under a key.
@interface StampScript : NSObject <OTIOHookScript> {
@private
    NSString *_key;
}
- (instancetype)initWithKey:(NSString *)key;
@end

@implementation StampScript
- (instancetype)initWithKey:(NSString *)key {
    self = [super init];
    if (self) {
        _key = [key copy];
    }
    return self;
}

- (void)dealloc {
#if !__has_feature(objc_arc)
    [_key release];
    [super dealloc];
#endif
}

- (nullable OTIOSerializableObject *)runHookOnObject:(OTIOSerializableObject *)target
                                           arguments:(OTIOMetadata *)arguments
                                               error:(NSError **)error {
    NSString *who = [arguments getString:@"who" error:NULL];
    if (who == nil) {
        who = @"nobody";
    }
    if (![target isKindOfClass:[OTIOSerializableObjectWithMetadata class]]) {
        if (error != NULL) {
            *error = Trouble(@"what the hook ran on carries no metadata");
        }
        return nil;
    }
    OTIOMetadata *metadata = [(OTIOSerializableObjectWithMetadata *)target metadata];
    if (![metadata setString:_key value:who error:error]) {
        return nil;
    }
    return target;
}
@end

static StampScript *Stamp(NSString *key) {
    return OTIO_AUTORELEASE_TEST([[StampScript alloc] initWithKey:key]);
}

/// Answers a clip of its own in place of what it is handed.
@interface ReplacingScript : NSObject <OTIOHookScript>
@end

@implementation ReplacingScript
- (nullable OTIOSerializableObject *)runHookOnObject:(OTIOSerializableObject *)target
                                           arguments:(OTIOMetadata *)arguments
                                               error:(NSError **)error {
    return [OTIOClip clipWithName:@"replacement" error:error];
}
@end

/// Answers nothing, and says nothing went wrong.
@interface SilentScript : NSObject <OTIOHookScript>
@end

@implementation SilentScript
- (nullable OTIOSerializableObject *)runHookOnObject:(OTIOSerializableObject *)target
                                           arguments:(OTIOMetadata *)arguments
                                               error:(NSError **)error {
    return nil;
}
@end

/// What KeepingScript held on to past its call.
static OTIOSerializableObject *kept = nil;

/// Keeps what it is handed, which it should not, and answers it.
@interface KeepingScript : NSObject <OTIOHookScript>
@end

@implementation KeepingScript
- (nullable OTIOSerializableObject *)runHookOnObject:(OTIOSerializableObject *)target
                                           arguments:(OTIOMetadata *)arguments
                                               error:(NSError **)error {
    kept = OTIO_KEEP(target);
    return target;
}
@end

/// The caller's own clip, which MeddlingScript is handed through a run of a
/// hook on it, and what it said when it tried to move it.
static OTIOClip *meddled = nil;
static NSError *meddlingRefused = nil;

/// Tries to close, and then to move, the caller's own object while the
/// caller's document is lent to it, and answers that object.
@interface MeddlingScript : NSObject <OTIOHookScript>
@end

@implementation MeddlingScript
- (nullable OTIOSerializableObject *)runHookOnObject:(OTIOSerializableObject *)target
                                           arguments:(OTIOMetadata *)arguments
                                               error:(NSError **)error {
    [meddled close];
    OTIOTrack *track = [OTIOTrack trackWithName:@"elsewhere" kind:@"Video" error:error];
    NSError *refused = nil;
    if ([track appendChild:meddled error:&refused]) {
        return nil;
    }
    meddlingRefused = OTIO_KEEP(refused);
    return meddled;
}
@end

#define NEW(class) OTIO_AUTORELEASE_TEST([[class alloc] init])

static void AMediaLinkerLinksEveryClip(void) {
    NSData *written = PluginCut();
    NSError *error = nil;
    Check(OTIORegisterMediaLinker(@"objc_proxies", NEW(ProxyLinker), &error),
          @"registering the linker");

    OTIOReadOptions options = OTIOReadOptionsDefault();
    options.mediaLinker = @"objc_proxies";
    options.mediaLinkerArguments = @"{\"root\": \"/proxies\"}";
    OTIOSerializableObject *root = OTIOReadFromBytes(OTIOFormatOTIOJSON, written, &options, &error);
    Check(root != nil, [NSString stringWithFormat:@"the linked read failed: %@", error]);
    if (root != nil) {
        CheckText(FirstURL(root), @"/proxies/first.mov", @"the first clip's media");
    }

    // Asked not to link, it does not.
    options.doNotLinkMedia = YES;
    OTIOSerializableObject *unlinked =
        OTIOReadFromBytes(OTIOFormatOTIOJSON, written, &options, &error);
    Check(unlinked != nil, @"the unlinked read failed");
    if (unlinked != nil) {
        CheckText(FirstURL(unlinked), @"file:///media/first.mov", @"the unlinked clip's media");
    }
    OTIOUnregisterMediaLinker(@"objc_proxies");
}

static void ALinkerThatLeavesAClipAloneKeepsItsMedia(void) {
    NSData *written = PluginCut();
    NSError *error = nil;
    watched = 0;
    Check(OTIORegisterMediaLinker(@"objc_watcher", NEW(WatchingLinker), &error),
          @"registering the linker");

    OTIOReadOptions options = OTIOReadOptionsDefault();
    options.mediaLinker = @"objc_watcher";
    OTIOSerializableObject *root = OTIOReadFromBytes(OTIOFormatOTIOJSON, written, &options, &error);
    Check(root != nil, @"the read failed");
    CheckEqual(watched, 2, @"the clips the linker saw");
    if (root != nil) {
        CheckText(FirstURL(root), @"file:///media/first.mov", @"the first clip's media");
    }
    OTIOUnregisterMediaLinker(@"objc_watcher");
}

static void ALinkerThatFailsStopsTheReadInItsOwnWords(void) {
    NSData *written = PluginCut();
    NSError *error = nil;
    OTIOReadOptions options = OTIOReadOptionsDefault();
    options.mediaLinker = @"objc_offline";

    Check(OTIORegisterMediaLinker(@"objc_offline", NEW(OfflineLinker), &error),
          @"registering the linker");
    error = nil;
    Check(OTIOReadFromBytes(OTIOFormatOTIOJSON, written, &options, &error) == nil,
          @"a failing linker's read succeeded");
    Check(IsPluginError(error) && Says(error.localizedDescription, @"the proxies are offline"),
          [NSString stringWithFormat:@"expected the linker's own failure, got %@", error]);

    // An exception is a failure too, not a crash.
    Check(OTIORegisterMediaLinker(@"objc_offline", NEW(RaisingLinker), &error),
          @"registering the raising linker");
    error = nil;
    Check(OTIOReadFromBytes(OTIOFormatOTIOJSON, written, &options, &error) == nil,
          @"a raising linker's read succeeded");
    Check(IsPluginError(error) && Says(error.localizedDescription, @"no disk") && Says(error.localizedDescription, @"OTIODiskError"),
          [NSString stringWithFormat:@"expected the exception as a failure, got %@", error]);

    // So is a thrown object that is not an exception.
    Check(OTIORegisterMediaLinker(@"objc_offline", NEW(ThrowingLinker), &error),
          @"registering the throwing linker");
    error = nil;
    Check(OTIOReadFromBytes(OTIOFormatOTIOJSON, written, &options, &error) == nil,
          @"a throwing linker's read succeeded");
    Check(IsPluginError(error) && Says(error.localizedDescription, @"a bare string"),
          [NSString stringWithFormat:@"expected the throw as a failure, got %@", error]);
    OTIOUnregisterMediaLinker(@"objc_offline");

    // And a linker nobody registered is refused, as upstream refuses one.
    options.mediaLinker = @"objc_nowhere";
    error = nil;
    Check(OTIOReadFromBytes(OTIOFormatOTIOJSON, written, &options, &error) == nil,
          @"an unknown linker's read succeeded");
    Check(IsPluginError(error) && Says(error.localizedDescription, @"objc_nowhere"),
          [NSString stringWithFormat:@"expected an unknown linker to be refused, got %@", error]);
}

static void TheLentTimelineStaysWhereItIs(void) {
    NSData *written = PluginCut();
    NSError *error = nil;
    Check(OTIORegisterMediaLinker(@"objc_kidnapper", NEW(KidnappingLinker), &error),
          @"registering the linker");
    OTIOReadOptions options = OTIOReadOptionsDefault();
    options.mediaLinker = @"objc_kidnapper";
    Check(OTIOReadFromBytes(OTIOFormatOTIOJSON, written, &options, &error) == nil,
          @"moving the lent clip elsewhere succeeded");
    Check(IsPluginError(error) && Says(error.localizedDescription, @"lent"),
          [NSString stringWithFormat:@"expected the move to be refused, got %@", error]);
    OTIOUnregisterMediaLinker(@"objc_kidnapper");

    // An object kept past the call names a document no longer lent, and
    // fails rather than reaching it.
    Check(OTIORegisterHookScript(@"objc_keeper", NEW(KeepingScript), &error),
          @"registering the script");
    Check(OTIOAttachHookScript(@"objc_keep", @"objc_keeper", &error), @"attaching the script");
    OTIOClip *clip = [OTIOClip clipWithName:@"kept" error:&error];
    Check([clip runHook:@"objc_keep" arguments:nil error:&error] != nil, @"running the hook");
    Check(kept != nil, @"the script kept nothing");
    if (kept != nil) {
        error = nil;
        Check([kept name:&error] == nil, @"an object kept past its call still answered");
        CheckEqual(StatusOf(error), OTIOStatusNullPointer, @"asking a kept object");
        OTIO_LET_GO(kept);
        kept = nil;
    }
    CheckText([clip name:&error], @"kept", @"the clip the hook ran on");
    OTIODetachHookScript(@"objc_keep", @"objc_keeper");
    OTIOUnregisterHookScript(@"objc_keeper");
}

static void AHookCannotFreeTheDocumentItsCallerLent(void) {
    NSError *error = nil;
    OTIOClip *clip = [OTIOClip clipWithName:@"mine" error:&error];
    meddled = clip;
    Check(OTIORegisterHookScript(@"objc_meddler", NEW(MeddlingScript), &error),
          @"registering the script");
    Check(OTIOAttachHookScript(@"objc_meddle", @"objc_meddler", &error), @"attaching the script");

    // runHook lends the clip's own document, so neither closing the clip nor
    // moving it frees that document under the library.
    OTIOSerializableObject *result = [clip runHook:@"objc_meddle" arguments:nil error:&error];
    Check(result != nil, [NSString stringWithFormat:@"the hook failed: %@", error]);
    Check([result isEqual:clip], @"the hook did not answer the caller's own clip");
    Check(meddlingRefused != nil && Says(meddlingRefused.localizedDescription, @"lent"),
          [NSString stringWithFormat:@"moving the caller's clip was not refused: %@",
                                     meddlingRefused]);
    OTIO_LET_GO(meddlingRefused);
    meddlingRefused = nil;
    meddled = nil;

    // The loan is over, so the clip is whole and can be closed after all.
    CheckText([clip name:&error], @"mine", @"the clip after the hook");
    [clip close];
    error = nil;
    Check([clip name:&error] == nil, @"a closed clip still answered");
    CheckEqual(StatusOf(error), OTIOStatusNullPointer, @"asking a closed clip");

    OTIODetachHookScript(@"objc_meddle", @"objc_meddler");
    OTIOUnregisterHookScript(@"objc_meddler");
}

static void HookScriptsRunAroundReadsAndWrites(void) {
    NSData *written = PluginCut();
    NSError *error = nil;
    Check(OTIORegisterHookScript(@"objc_stamp_read", Stamp(@"read_by"), &error),
          @"registering the read script");
    Check(OTIOAttachHookScript(@"post_adapter_read", @"objc_stamp_read", &error),
          @"attaching the read script");

    OTIOReadOptions readOptions = OTIOReadOptionsDefault();
    readOptions.hookArguments = @"{\"who\": \"the Objective-C test\"}";
    OTIOSerializableObject *root =
        OTIOReadFromBytes(OTIOFormatOTIOJSON, written, &readOptions, &error);
    Check([root isKindOfClass:[OTIOTimeline class]], @"the read did not answer a timeline");
    if (![root isKindOfClass:[OTIOTimeline class]]) {
        return;
    }
    OTIOMetadata *metadata = [(OTIOTimeline *)root metadata];
    CheckText([metadata getString:@"read_by" error:&error], @"the Objective-C test",
              @"what the read hook left");

    // A write runs its hooks on a copy, so the timeline is left alone.
    Check(OTIORegisterHookScript(@"objc_stamp_write", Stamp(@"written_by"), &error),
          @"registering the write script");
    Check(OTIOAttachHookScript(@"pre_adapter_write", @"objc_stamp_write", &error),
          @"attaching the write script");
    OTIOWriteOptions writeOptions = OTIOWriteOptionsDefault();
    writeOptions.hookArguments = @"{\"who\": \"the writer\"}";
    NSData *out = OTIOWriteToBytes(OTIOFormatOTIOJSON, root, &writeOptions, &error);
    NSString *text = OTIO_AUTORELEASE_TEST(
        [[NSString alloc] initWithData:out ?: [NSData data] encoding:NSUTF8StringEncoding]);
    Check([text rangeOfString:@"\"written_by\": \"the writer\""].location != NSNotFound,
          @"the write hook did not reach what was written");
    error = nil;
    Check([metadata getString:@"written_by" error:&error] == nil && OTIOIsNoValue(error),
          @"the write hook changed the timeline");

    OTIODetachHookScript(@"pre_adapter_write", @"objc_stamp_write");
    OTIOUnregisterHookScript(@"objc_stamp_write");
    OTIODetachHookScript(@"post_adapter_read", @"objc_stamp_read");
    OTIOUnregisterHookScript(@"objc_stamp_read");
}

static void AHookOfYourOwnRunsWhenAsked(void) {
    NSError *error = nil;
    OTIOClip *clip = [OTIOClip clipWithName:@"A" error:&error];

    Check(OTIORegisterHookScript(@"objc_stamp", Stamp(@"stamped_by"), &error),
          @"registering the stamp");
    Check(OTIOAttachHookScript(@"objc_mine", @"objc_stamp", &error), @"attaching the stamp");
    OTIOSerializableObject *result =
        [clip runHook:@"objc_mine" arguments:@"{\"who\": \"me\"}" error:&error];
    Check([result isEqual:clip], @"the hook answered with something other than the clip");
    CheckText([clip.metadata getString:@"stamped_by" error:&error], @"me", @"what the hook left");

    // A script may hand back a different object to go on with.
    Check(OTIORegisterHookScript(@"objc_replace", NEW(ReplacingScript), &error),
          @"registering the replacement");
    Check(OTIOAttachHookScript(@"objc_swap", @"objc_replace", &error),
          @"attaching the replacement");
    OTIOSerializableObject *swapped = [clip runHook:@"objc_swap" arguments:nil error:&error];
    Check([swapped isKindOfClass:[OTIOClip class]], @"the replacement is not a clip");
    CheckText([swapped name:&error], @"replacement", @"what the hook answered with");

    // A script that answers with nothing fails, since a hook needs an object
    // to go on with.
    Check(OTIORegisterHookScript(@"objc_nothing", NEW(SilentScript), &error),
          @"registering the silent script");
    Check(OTIOAttachHookScript(@"objc_empty", @"objc_nothing", &error),
          @"attaching the silent script");
    error = nil;
    Check([clip runHook:@"objc_empty" arguments:nil error:&error] == nil,
          @"a script with no answer succeeded");
    Check(IsPluginError(error) && Says(error.localizedDescription, @"no object to go on with"),
          [NSString stringWithFormat:@"expected a script with no answer to fail, got %@", error]);

    error = nil;
    Check([clip runHook:@"objc_undeclared" arguments:nil error:&error] == nil,
          @"an undeclared hook ran");
    Check(IsPluginError(error), @"an undeclared hook did not fail as a plugin error");

    OTIODetachHookScript(@"objc_mine", @"objc_stamp");
    OTIODetachHookScript(@"objc_swap", @"objc_replace");
    OTIODetachHookScript(@"objc_empty", @"objc_nothing");
    OTIOUnregisterHookScript(@"objc_stamp");
    OTIOUnregisterHookScript(@"objc_replace");
    OTIOUnregisterHookScript(@"objc_nothing");
}

/// Counts its own deallocations, to show the library lets a plugin go.
@interface CountedScript : SilentScript
@end

static NSInteger countedAlive = 0;

@implementation CountedScript
- (instancetype)init {
    self = [super init];
    if (self) {
        countedAlive += 1;
    }
    return self;
}

- (void)dealloc {
    countedAlive -= 1;
#if !__has_feature(objc_arc)
    [super dealloc];
#endif
}
@end

static void UnregisteringSaysWhetherThereWasAnything(void) {
    NSError *error = nil;
    @autoreleasepool {
        Check(OTIORegisterHookScript(@"objc_brief", NEW(CountedScript), &error),
              @"registering the script");
    }
    CheckEqual(countedAlive, 1, @"the scripts the library holds");
    Check(OTIOUnregisterHookScript(@"objc_brief"), @"unregistering did not answer yes");
    Check(!OTIOUnregisterHookScript(@"objc_brief"), @"unregistering twice answered yes");
    CheckEqual(countedAlive, 0, @"the scripts left once unregistered");

    // Registering again under a name releases the one it replaces.
    @autoreleasepool {
        OTIORegisterHookScript(@"objc_brief", NEW(CountedScript), &error);
        OTIORegisterHookScript(@"objc_brief", NEW(CountedScript), &error);
    }
    CheckEqual(countedAlive, 1, @"the scripts left once one replaced another");
    OTIOUnregisterHookScript(@"objc_brief");
    CheckEqual(countedAlive, 0, @"the scripts left at the end");

    // A refused registration keeps nothing.
    @autoreleasepool {
        error = nil;
        Check(!OTIORegisterMediaLinker(@"", NEW(WatchingLinker), &error),
              @"a linker with no name was accepted");
        CheckEqual(StatusOf(error), OTIOStatusInvalidArgument, @"a linker with no name");
        error = nil;
        Check(!OTIORegisterHookScript(@"", NEW(CountedScript), &error),
              @"a script with no name was accepted");
    }
    CheckEqual(countedAlive, 0, @"the scripts left after a refusal");

    id<OTIOMediaLinker> missing = nil;
    error = nil;
    Check(!OTIORegisterMediaLinker(@"objc_nil", missing, &error), @"a nil linker was accepted");
    CheckEqual(StatusOf(error), OTIOStatusNullPointer, @"a nil linker");
}

#if __has_feature(blocks)
static void BlocksServeAsLinkersAndScripts(void) {
    NSData *written = PluginCut();
    NSError *error = nil;
    __block NSInteger seen = 0;
    Check(OTIORegisterMediaLinkerUsingBlock(
              @"objc_block_proxies",
              ^OTIOMediaReference *_Nullable(
                  OTIOClip *clip, OTIOMetadata *arguments, NSError **failure) {
                  seen += 1;
                  NSString *root = [arguments getString:@"root" error:failure];
                  NSString *name = [clip name:failure];
                  if (root == nil || name == nil) {
                      return nil;
                  }
                  NSString *url = [NSString stringWithFormat:@"%@/%@.mov", root, name];
                  return [OTIOExternalReference externalReferenceWithName:name
                                                                targetURL:url
                                                                    error:failure];
              },
              &error),
          @"registering the block linker");
    Check(OTIORegisterHookScriptUsingBlock(
              @"objc_block_stamp",
              ^OTIOSerializableObject *_Nullable(
                  OTIOSerializableObject *target, OTIOMetadata *arguments, NSError **failure) {
                  NSString *who = [arguments getString:@"who" error:failure];
                  if (who == nil || ![[(OTIOTimeline *)target metadata] setString:@"read_by"
                                                                            value:who
                                                                            error:failure]) {
                      return nil;
                  }
                  return target;
              },
              &error),
          @"registering the block script");
    Check(OTIOAttachHookScript(@"post_adapter_read", @"objc_block_stamp", &error),
          @"attaching the block script");

    OTIOReadOptions options = OTIOReadOptionsDefault();
    options.mediaLinker = @"objc_block_proxies";
    options.mediaLinkerArguments = @"{\"root\": \"/blocks\"}";
    options.hookArguments = @"{\"who\": \"a block\"}";
    OTIOSerializableObject *root = OTIOReadFromBytes(OTIOFormatOTIOJSON, written, &options, &error);
    Check([root isKindOfClass:[OTIOTimeline class]], @"the read did not answer a timeline");
    CheckEqual(seen, 2, @"the clips the block saw");
    if ([root isKindOfClass:[OTIOTimeline class]]) {
        CheckText(FirstURL(root), @"/blocks/first.mov", @"the first clip's media");
        CheckText([[(OTIOTimeline *)root metadata] getString:@"read_by" error:&error],
                  @"a block", @"what the block script left");
    }

    // A block that throws fails the read rather than crashing it.
    OTIORegisterMediaLinkerUsingBlock(
        @"objc_block_proxies",
        ^OTIOMediaReference *_Nullable(
            OTIOClip *clip, OTIOMetadata *arguments, NSError **failure) {
            [NSException raise:@"OTIOBlockError" format:@"the block gave up"];
            return nil;
        },
        &error);
    error = nil;
    Check(OTIOReadFromBytes(OTIOFormatOTIOJSON, written, &options, &error) == nil,
          @"a raising block's read succeeded");
    Check(IsPluginError(error) && Says(error.localizedDescription, @"the block gave up"),
          [NSString stringWithFormat:@"expected the block's exception, got %@", error]);

    OTIODetachHookScript(@"post_adapter_read", @"objc_block_stamp");
    Check(OTIOUnregisterHookScript(@"objc_block_stamp"), @"unregistering the block script");
    Check(OTIOUnregisterMediaLinker(@"objc_block_proxies"), @"unregistering the block linker");
}
#endif

int RunConformanceScenarios(void);

static void TheConformanceScenariosAgree(void) {
    failures += RunConformanceScenarios();
}

#pragma mark - Running

typedef void (*Body)(void);

typedef struct {
    const char *name;
    Body body;
} Test;

static const Test tests[] = {
    {"the library reports a version", TheLibraryReportsAVersion},
    {"an enum says what the C interface calls it", AnEnumSaysWhatTheCInterfaceCallsIt},
    {"rates are classified", RatesAreClassified},
    {"reading an EDL finds its clips", ReadingAnEdlFindsItsClips},
    {"an AAF reads and writes back out", AnAafReadsAndWritesBackOut},
    {"a bundle carries its media with it", ABundleCarriesItsMediaWithIt},
    {"the quickstart from the README runs", TheQuickstartFromTheReadmeRuns},
    {"open works out the format from the name", OpenWorksOutTheFormatFromTheName},
    {"open declines a suffix no format claims", OpenDeclinesASuffixNoFormatClaims},
    {"a timeline survives a round trip through JSON", ATimelineSurvivesARoundTripThroughJson},
    {"saving and opening again keeps the clips", SavingAndOpeningAgainKeepsTheClips},
    {"writing bytes in every format the library knows", WritingBytesInEveryFormatTheLibraryKnows},
    {"building a timeline from nothing", BuildingATimelineFromNothing},
    {"an object built on its own stands alone", AnObjectBuiltOnItsOwnStandsAlone},
    {"a freshly built object is enabled", AFreshlyBuiltObjectIsEnabled},
    {"a built object may be left unnamed", ABuiltObjectMayBeLeftUnnamed},
    {"no value is an answer and not a failure", NoValueIsAnAnswerAndNotAFailure},
    {"an object knows which schemas it is", AnObjectKnowsWhichSchemasItIs},
    {"clearing children hands them all back", ClearingChildrenHandsThemAllBack},
    {"every child and its range come back together", EveryChildAndItsRangeComeBackTogether},
    {"a stale handle is refused", AStaleHandleIsRefused},
    {"an object of no timeline fails rather than crashing",
     AnObjectOfNoTimelineFailsRatherThanCrashing},
    {"an object from another timeline is refused", AnObjectFromAnotherTimelineIsRefused},
    {"a refusal of another timeline's object is told apart",
     ARefusalOfAnotherTimelinesObjectIsToldApart},
    {"an edit puts a newly built item into a track", AnEditPutsANewlyBuiltItemIntoATrack},
    {"an object of an absorbed timeline follows it", AnObjectOfAnAbsorbedTimelineFollowsIt},
    {"metadata goes in and comes back", MetadataGoesInAndComesBack},
    {"time values compute without a timeline", TimeValuesComputeWithoutATimeline},
    {"an unreadable timecode is a failure", AnUnreadableTimecodeIsAFailure},
    {"a range answers about what it covers", ARangeAnswersAboutWhatItCovers},
    {"an object outliving its timeline fails rather than crashing",
     AnObjectOutlivingItsTimelineFailsRatherThanCrashing},
    {"every failure carries its own message whatever thread it ran on",
     EveryFailureCarriesItsOwnMessageWhateverThreadItRanOn},
    {"a media linker links every clip", AMediaLinkerLinksEveryClip},
    {"a linker that leaves a clip alone keeps its media", ALinkerThatLeavesAClipAloneKeepsItsMedia},
    {"a linker that fails stops the read in its own words",
     ALinkerThatFailsStopsTheReadInItsOwnWords},
    {"the lent timeline stays where it is", TheLentTimelineStaysWhereItIs},
    {"a hook cannot free the document its caller lent", AHookCannotFreeTheDocumentItsCallerLent},
    {"hook scripts run around reads and writes", HookScriptsRunAroundReadsAndWrites},
    {"a hook of your own runs when asked", AHookOfYourOwnRunsWhenAsked},
    {"unregistering says whether there was anything", UnregisteringSaysWhetherThereWasAnything},
#if __has_feature(blocks)
    {"blocks serve as linkers and scripts", BlocksServeAsLinkersAndScripts},
#endif
    {"the conformance scenarios agree", TheConformanceScenariosAgree},
};

int main(void) {
    @autoreleasepool {
        size_t howMany = sizeof(tests) / sizeof(tests[0]);
        for (size_t slot = 0; slot < howMany; slot++) {
            printf("%s\n", tests[slot].name);
            @autoreleasepool {
                tests[slot].body();
            }
        }
        if (failures > 0) {
            printf("\n%d failed\n", failures);
            return 1;
        }
        printf("\nall %zu passed\n", howMany);
    }
    return 0;
}
