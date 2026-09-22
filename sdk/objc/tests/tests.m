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

#import <OpenTimelineIO/OpenTimelineIO.h>

// The tests hold on to a couple of objects past the pool that made them, to
// prove an object outlives its document. That is the tests' own business, not
// the SDK's, so the macro lives here.
#if __has_feature(objc_arc)
#define OTIO_KEEP(object) (object)
#else
#define OTIO_KEEP(object) [(object) retain]
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

static void ReadingAnEdlFindsItsClips(void) {
    NSError *error = nil;
    OTIODocument *document = [OTIODocument readFromFile:OTIOFormatCMX3600
                                                   path:ScreeningEdl()
                                                options:NULL
                                                  error:&error];
    Check(document != nil, @"the EDL did not open");
    if (document == nil) {
        return;
    }

    OTIOSerializableObject *root = [document root:&error];
    Check(root != nil, @"the document has no root");
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
    [document close];
}

/// The quickstart in `sdk/objc/README.md` is generated, so nothing compiles
/// it. This is that example, so that it cannot go stale.
static void TheQuickstartFromTheReadmeRuns(void) {
    NSError *error = nil;
    OTIODocument *document = [OTIODocument open:ScreeningEdl() error:&error];
    Check(document != nil, @"the document did not open");
    if (document == nil) {
        return;
    }

    NSInteger named = 0;
    OTIOSerializableObject *root = [document root:&error];
    Check([root isKindOfClass:[OTIOTimeline class]], @"the root is not a timeline");
    for (OTIOSerializableObject *clip in [root findClips:&error]) {
        Check([clip name:&error].length > 0, @"a clip has no name");
        named += 1;
    }
    CheckEqual(named, 9, @"the number of named clips");
    [document close];
}

static void OpenWorksOutTheFormatFromTheName(void) {
    NSError *error = nil;
    OTIODocument *document = [OTIODocument open:ScreeningEdl() error:&error];
    Check(document != nil, @"the document did not open");
    OTIOSerializableObject *root = [document root:&error];
    Check(
        [[root name:&error] rangeOfString:@"Example_Screening"].location != NSNotFound,
        @"the root's name");
    [document close];
}

static void OpenDeclinesASuffixNoFormatClaims(void) {
    NSError *error = nil;
    OTIODocument *document = [OTIODocument open:@"/tmp/nothing.wav" error:&error];
    Check(document == nil, @"a .wav opened");
    CheckEqual(StatusOf(error), OTIOStatusNoValue, @"opening a .wav");
    Check(OTIOIsNoValue(error), @"OTIOIsNoValue did not recognise it");
}

static void ADocumentSurvivesARoundTripThroughJson(void) {
    NSError *error = nil;
    OTIODocument *document = [OTIODocument open:ScreeningEdl() error:&error];
    NSString *text = [document toJSON:2 error:&error];
    Check([text rangeOfString:@"Timeline"].location != NSNotFound, @"the JSON has no timeline");

    OTIODocument *again = [OTIODocument fromJSON:text error:&error];
    Check(again != nil, @"the JSON did not read back");
    OTIOSerializableObject *root = [again root:&error];
    CheckEqual((NSInteger)[root findClips:&error].count, 9, @"the clips after a round trip");
    [document close];
    [again close];
}

static void SavingAndOpeningAgainKeepsTheClips(void) {
    NSError *error = nil;
    OTIODocument *document = [OTIODocument open:ScreeningEdl() error:&error];
    NSString *path = Temporary(@"round-trip.otio");
    Check([document save:path error:&error], @"the document did not save");

    OTIODocument *again = [OTIODocument open:path error:&error];
    Check(again != nil, @"the saved document did not open");
    OTIOSerializableObject *root = [again root:&error];
    CheckEqual((NSInteger)[root findClips:&error].count, 9, @"the clips after saving");
    [document close];
    [again close];
}

static void WritingBytesInEveryFormatTheLibraryKnows(void) {
    NSError *error = nil;
    OTIODocument *document = [OTIODocument open:ScreeningEdl() error:&error];
    OTIOFormat formats[] = {OTIOFormatOTIOJSON, OTIOFormatCMX3600};
    for (size_t slot = 0; slot < sizeof(formats) / sizeof(formats[0]); slot++) {
        NSData *written = [document writeToBytes:formats[slot] options:NULL error:&error];
        Check(written.length > 0, @"a format wrote nothing");
    }
    [document close];
}

/// A timeline with one video track holding two clips.
static OTIOTrack *MakeTimeline(OTIODocument *document, NSMutableArray *clips) {
    NSError *error = nil;
    OTIOTimeline *timeline = [document makeTimeline:@"Assembly" error:&error];
    OTIOStack *stack = [document makeStack:@"tracks" error:&error];
    [timeline setTracks:stack error:&error];
    OTIOTrack *track = [document makeTrack:@"V1" kind:@"Video" error:&error];
    [stack appendChild:track error:&error];

    NSArray<NSString *> *names = @[@"A", @"B"];
    for (NSUInteger index = 0; index < names.count; index++) {
        OTIOClip *clip = [document makeClip:[names objectAtIndex:index] error:&error];
        OTIORationalTime start = OTIORationalTimeMake((double)index * 24, 24);
        OTIOTimeRange span = OTIOTimeRangeMake(start, OTIORationalTimeMake(24, 24));
        [clip setSourceRange:span error:&error];
        [track appendChild:clip error:&error];
        [clips addObject:clip];
    }
    [document setRoot:timeline error:&error];
    Check(error == nil, @"building the timeline reported a failure");
    return track;
}

static void BuildingATimelineFromNothing(void) {
    NSError *error = nil;
    OTIODocument *document = [[OTIODocument alloc] init];
    NSMutableArray *clips = [NSMutableArray array];
    OTIOTrack *track = MakeTimeline(document, clips);

    NSUInteger children = 0;
    Check([track getChildCount:&children error:&error], @"the child count failed");
    CheckEqual((NSInteger)children, 2, @"the track's children");
    CheckText([[clips objectAtIndex:0] name:&error], @"A", @"the first clip's name");
    CheckText([track kind:&error], @"Video", @"the track's kind");

    // The whole track is as long as the two clips together.
    OTIORationalTime duration;
    Check([track getDuration:&duration error:&error], @"the duration failed");
    CheckNear(OTIORationalTimeToSeconds(duration), 2, @"the track's duration");
    [document close];
}

static void AFreshlyBuiltObjectIsEnabled(void) {
    NSError *error = nil;
    OTIODocument *document = [[OTIODocument alloc] init];
    OTIOClip *clip = [document makeClip:@"A" error:&error];

    BOOL enabled = NO;
    Check([clip getEnabled:&enabled error:&error], @"reading enabled failed");
    Check(enabled, @"a new clip is not enabled");
    Check([clip setEnabled:NO error:&error], @"disabling failed");
    Check([clip getEnabled:&enabled error:&error], @"reading enabled failed");
    Check(!enabled, @"a disabled clip says it is enabled");
    [document close];
}

static void NoValueIsAnAnswerAndNotAFailure(void) {
    NSError *error = nil;
    OTIODocument *document = [[OTIODocument alloc] init];
    OTIOClip *clip = [document makeClip:@"untrimmed" error:&error];

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
    [document close];
}

static void AnObjectKnowsWhichSchemasItIs(void) {
    NSError *error = nil;
    OTIODocument *document = [[OTIODocument alloc] init];
    OTIOClip *clip = [document makeClip:@"A" error:&error];

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
    [document close];
}

static void ClearingChildrenHandsThemAllBack(void) {
    NSError *error = nil;
    OTIODocument *document = [[OTIODocument alloc] init];
    NSMutableArray *clips = [NSMutableArray array];
    OTIOTrack *track = MakeTimeline(document, clips);

    NSArray<OTIOSerializableObject *> *taken = [track clearChildren:&error];
    CheckEqual((NSInteger)taken.count, (NSInteger)clips.count, @"the number handed back");
    NSUInteger children = 1;
    Check([track getChildCount:&children error:&error], @"the child count failed");
    CheckEqual((NSInteger)children, 0, @"the track is not empty");
    CheckText([[taken objectAtIndex:0] name:&error], @"A", @"the first one handed back");
    CheckText([[taken objectAtIndex:1] name:&error], @"B", @"the second one handed back");
    [document close];
}

static void EveryChildAndItsRangeComeBackTogether(void) {
    NSError *error = nil;
    OTIODocument *document = [[OTIODocument alloc] init];
    NSMutableArray *clips = [NSMutableArray array];
    OTIOTrack *track = MakeTimeline(document, clips);

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
    [document close];
}

static void AStaleHandleIsRefused(void) {
    NSError *error = nil;
    OTIODocument *document = [[OTIODocument alloc] init];
    OTIOClip *clip = [document makeClip:@"A" error:&error];
    Check([document removeNode:clip error:&error], @"removing the clip failed");

    error = nil;
    Check([clip name:&error] == nil, @"a removed clip still has a name");
    CheckEqual(StatusOf(error), OTIOStatusStaleHandle, @"a removed clip's name");
    [document close];
}

static void AnObjectOfNoDocumentFailsRatherThanCrashing(void) {
    OTIOSerializableObject *orphan = [OTIOSerializableObject none];
    Check([orphan isNone], @"the none object is not none");
    Check(orphan.document == nil, @"the none object has a document");

    NSError *error = nil;
    Check([orphan name:&error] == nil, @"asking an orphan its name worked");
    Check(error != nil, @"asking an orphan its name reported no failure");
}

static void AnObjectFromAnotherDocumentIsRefused(void) {
    NSError *error = nil;
    OTIODocument *one = [[OTIODocument alloc] init];
    OTIODocument *other = [[OTIODocument alloc] init];

    OTIOTrack *track = [one makeTrack:@"V1" kind:@"Video" error:&error];
    OTIOClip *stranger = [other makeClip:@"elsewhere" error:&error];

    error = nil;
    Check(![track appendChild:stranger error:&error], @"a foreign clip was appended");
    CheckEqual(StatusOf(error), OTIOStatusInvalidArgument, @"appending a foreign clip");

    // A call that cannot fail answers rather than reporting, and the answer is
    // no.
    Check(![one contains:stranger], @"a document claims to contain a foreign object");

    // The document that refused it is still whole, which is what tells
    // refusing apart from absorbing and then failing.
    CheckEqual((NSInteger)[other nodeCount], 1, @"the other document lost its clip");
    error = nil;
    CheckText([stranger name:&error], @"elsewhere", @"the foreign clip stopped answering");
    [one close];
    [other close];
}

static void MetadataGoesInAndComesBack(void) {
    NSError *error = nil;
    OTIODocument *document = [[OTIODocument alloc] init];
    OTIOClip *clip = [document makeClip:@"A" error:&error];

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
    [document close];
}

static void TimeValuesComputeWithoutADocument(void) {
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

static void AnObjectBuiltOnItsOwnCanJoinATimeline(void) {
    NSError *error = nil;
    OTIODocument *document = [[OTIODocument alloc] init];
    OTIOTrack *track = [document makeTrack:@"V1" kind:@"Video" error:&error];

    // A clip built in a document of its own, as a binding that hides the
    // document would build one.
    OTIODocument *workshop = [[OTIODocument alloc] init];
    OTIOClip *clip = [workshop makeClip:@"guest" error:&error];

    NSDictionary<OTIOSerializableObject *, OTIOSerializableObject *> *translated =
        [document absorb:workshop error:&error];
    Check(translated != nil, @"absorbing failed");

    OTIOSerializableObject *arrived = [translated objectForKey:clip];
    Check(arrived != nil, @"the clip did not move");
    if (arrived == nil) {
        return;
    }
    Check([arrived isKindOfClass:[OTIOClip class]], @"what arrived is not a clip");
    Check(arrived.document == document, @"it arrived in the wrong document");

    Check([track appendChild:arrived error:&error], @"the track did not take it");
    NSUInteger children = 0;
    Check([track getChildCount:&children error:&error], @"the child count failed");
    CheckEqual((NSInteger)children, 1, @"the track did not take it");
    CheckText([arrived name:&error], @"guest", @"the name did not travel");
    [document close];
}

/// An object holds the OTIODocument rather than the raw pointer, so that
/// closing the document leaves the object naming nothing instead of leaving it
/// dangling.
static void AnObjectOutlivingItsDocumentFailsRatherThanCrashing(void) {
    NSError *error = nil;
    OTIOSerializableObject *survivor = nil;
    OTIOSerializableObject *sibling = nil;
    @autoreleasepool {
        OTIODocument *document = [[OTIODocument alloc] init];
        survivor = OTIO_KEEP([document makeClip:@"A" error:&error]);
        sibling = OTIO_KEEP([document makeClip:@"B" error:&error]);
        [document close];
    }

    error = nil;
    Check([survivor name:&error] == nil, @"a closed document's object still has a name");
    CheckEqual(StatusOf(error), OTIOStatusNullPointer, @"reading a closed document");
    error = nil;
    Check(![survivor setName:@"B" error:&error], @"writing to a closed document worked");
    CheckEqual(StatusOf(error), OTIOStatusNullPointer, @"writing to a closed document");
    error = nil;
    Check([survivor findClips:&error] == nil, @"searching a closed document worked");
    CheckEqual(StatusOf(error), OTIOStatusNullPointer, @"searching a closed document");

    // Asking what schema it is answers "none" rather than reading anything.
    Check(![survivor isA:OTIONodeKindClip], @"a closed document's object has a schema");
    // Two objects of the same closed document still compare as themselves.
    Check([survivor isEqual:survivor], @"an object is not itself");
    Check(![survivor isEqual:sibling], @"two objects compare equal");
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
    {"the quickstart from the README runs", TheQuickstartFromTheReadmeRuns},
    {"open works out the format from the name", OpenWorksOutTheFormatFromTheName},
    {"open declines a suffix no format claims", OpenDeclinesASuffixNoFormatClaims},
    {"a document survives a round trip through JSON", ADocumentSurvivesARoundTripThroughJson},
    {"saving and opening again keeps the clips", SavingAndOpeningAgainKeepsTheClips},
    {"writing bytes in every format the library knows", WritingBytesInEveryFormatTheLibraryKnows},
    {"building a timeline from nothing", BuildingATimelineFromNothing},
    {"a freshly built object is enabled", AFreshlyBuiltObjectIsEnabled},
    {"no value is an answer and not a failure", NoValueIsAnAnswerAndNotAFailure},
    {"an object knows which schemas it is", AnObjectKnowsWhichSchemasItIs},
    {"clearing children hands them all back", ClearingChildrenHandsThemAllBack},
    {"every child and its range come back together", EveryChildAndItsRangeComeBackTogether},
    {"a stale handle is refused", AStaleHandleIsRefused},
    {"an object of no document fails rather than crashing",
     AnObjectOfNoDocumentFailsRatherThanCrashing},
    {"an object from another document is refused", AnObjectFromAnotherDocumentIsRefused},
    {"metadata goes in and comes back", MetadataGoesInAndComesBack},
    {"time values compute without a document", TimeValuesComputeWithoutADocument},
    {"an unreadable timecode is a failure", AnUnreadableTimecodeIsAFailure},
    {"a range answers about what it covers", ARangeAnswersAboutWhatItCovers},
    {"an object built on its own can join a timeline", AnObjectBuiltOnItsOwnCanJoinATimeline},
    {"an object outliving its document fails rather than crashing",
     AnObjectOutlivingItsDocumentFailsRatherThanCrashing},
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
