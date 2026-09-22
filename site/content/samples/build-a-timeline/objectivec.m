#import <Foundation/Foundation.h>

#import <OpenTimelineIO/OpenTimelineIO.h>

int main(void) {
    @autoreleasepool {
        NSError *error = nil;

        // Each object is made on its own and joins a timeline when you put
        // it into one. Nothing has to exist before the thing it goes into,
        // so these are ordinary Cocoa class factories.
        OTIOTimeline *timeline = [OTIOTimeline timelineWithName:@"Cut" error:&error];
        OTIOStack *stack = [OTIOStack stackWithName:@"tracks" error:&error];
        OTIOTrack *track = [OTIOTrack trackWithName:@"V1" kind:@"Video" error:&error];

        [timeline setTracks:stack error:&error];
        [stack appendChild:track error:&error];

        NSArray<NSString *> *names = @[ @"A", @"B", @"C" ];
        for (NSUInteger index = 0; index < names.count; index++) {
            OTIOClip *clip = [OTIOClip clipWithName:[names objectAtIndex:index] error:&error];
            OTIORationalTime start = OTIORationalTimeMake((double)index * 24, 24);
            OTIOTimeRange span = OTIOTimeRangeMake(start, OTIORationalTimeMake(24, 24));
            [clip setSourceRange:span error:&error];
            [track appendChild:clip error:&error];
        }

        // Three seconds of picture, written as canonical OpenTimelineIO JSON.
        // The objects keep their timeline alive between them, so there is
        // nothing to close.
        OTIORationalTime duration;
        if ([track getDuration:&duration error:&error]) {
            NSLog(@"%f", OTIORationalTimeToSeconds(duration));
        }
        OTIOSave(timeline, @"cut.otio", &error);
    }
    return 0;
}
