#import <Foundation/Foundation.h>

#import <OpenTimelineIO/OpenTimelineIO.h>

int main(void) {
    @autoreleasepool {
        NSError *error = nil;

        OTIOTimeline *timeline = [OTIOTimeline timelineWithName:@"Cut" error:&error];
        OTIOStack *stack = [OTIOStack stackWithName:@"tracks" error:&error];
        OTIOTrack *track = [OTIOTrack trackWithName:@"V1" kind:@"Video" error:&error];

        [timeline setTracks:stack error:&error];
        [stack appendChild:track error:&error];

        OTIOTimeRange oneSecond =
            OTIOTimeRangeMake(OTIORationalTimeMake(0, 24), OTIORationalTimeMake(24, 24));

        // An AAF clip is cut from media of a known length, so each clip's
        // media says how much of it there is. A new clip has no media at
        // all, so its reference goes in under upstream's key and is made
        // the active one.
        for (NSString *name in @[ @"A001C003", @"A001C004" ]) {
            NSString *url = [NSString stringWithFormat:@"file:///media/%@.mov", name];
            OTIOExternalReference *media =
                [OTIOExternalReference externalReferenceWithName:nil targetURL:url error:&error];
            [media setAvailableRange:oneSecond error:&error];

            OTIOClip *clip = [OTIOClip clipWithName:name error:&error];
            [clip setMediaReference:@"DEFAULT_MEDIA" reference:media error:&error];
            [clip setActiveMediaReferenceKey:@"DEFAULT_MEDIA" error:&error];
            [clip setSourceRange:oneSecond error:&error];
            [track appendChild:clip error:&error];
        }

        // Every clip needs a MobID, from its metadata, its media's metadata
        // or the AAF its media names. A cut built from scratch has none, so
        // let the writer make them up rather than refuse the clip.
        OTIOWriteOptions options = OTIOWriteOptionsDefault();
        options.aafUseEmptyMobIds = YES;
        if (!OTIOWriteToFile(OTIOFormatAAF, timeline, @"cut.aaf", &options, &error)) {
            NSLog(@"%@", error.localizedDescription);
            return 1;
        }

        OTIOSerializableObject *root = OTIOReadFromFile(OTIOFormatAAF, @"cut.aaf", NULL, &error);
        for (OTIOSerializableObject *node in [root findClips:&error]) {
            if ([node isKindOfClass:[OTIOClip class]]) {
                NSLog(@"%@", [(OTIOClip *)node name:&error]);
            }
        }
    }
    return 0;
}
