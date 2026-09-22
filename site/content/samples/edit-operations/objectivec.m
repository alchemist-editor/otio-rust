#import <Foundation/Foundation.h>

#import <OpenTimelineIO/OpenTimelineIO.h>

/// One second of picture, named.
static OTIOClip *Second(NSString *name) {
    NSError *error = nil;
    OTIOClip *clip = [OTIOClip clipWithName:name error:&error];
    [clip setSourceRange:OTIOTimeRangeMake(OTIORationalTimeMake(0, 24),
                                           OTIORationalTimeMake(24, 24))
                   error:&error];
    return clip;
}

static void Show(OTIOTrack *track) {
    NSError *error = nil;
    NSMutableArray<NSString *> *names = [NSMutableArray array];
    for (OTIOSerializableObject *child in [track children:&error]) {
        [names addObject:[child name:&error]];
    }

    OTIORationalTime duration;
    [track getDuration:&duration error:&error];
    NSLog(@"%@ - %g frames", [names componentsJoinedByString:@" "], duration.value);
}

int main(void) {
    @autoreleasepool {
        NSError *error = nil;

        OTIOTrack *track = [OTIOTrack trackWithName:@"V1" kind:@"Video" error:&error];
        for (NSString *name in @[ @"A", @"B", @"C" ]) {
            [track appendChild:Second(name) error:&error];
        }
        Show(track);

        // Insert makes room: everything from the insertion point onwards
        // moves later, and the track gets longer.
        OTIOInsert(Second(@"D"), track, OTIORationalTimeMake(24, 24), NO, nil, &error);
        Show(track);

        // Overwrite does not: it lays an item over a span and whatever was
        // in that span gives way. The track is the same length afterwards.
        OTIOOverwrite(Second(@"E"), track,
                      OTIOTimeRangeMake(OTIORationalTimeMake(48, 24),
                                        OTIORationalTimeMake(24, 24)),
                      NO, nil, &error);
        Show(track);
    }
    return 0;
}
