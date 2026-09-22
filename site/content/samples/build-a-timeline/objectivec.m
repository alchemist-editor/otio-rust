#import <Foundation/Foundation.h>

#import <OpenTimelineIO/OpenTimelineIO.h>

int main(void) {
    @autoreleasepool {
        NSError *error = nil;

        // The document owns every object in it. Closing it releases the whole
        // timeline at once, at a moment you chose.
        OTIODocument *document = [[OTIODocument alloc] init];

        OTIOTimeline *timeline = [document makeTimeline:@"Cut" error:&error];
        OTIOStack *stack = [document makeStack:@"tracks" error:&error];
        [timeline setTracks:stack error:&error];
        OTIOTrack *track = [document makeTrack:@"V1" kind:@"Video" error:&error];
        [stack appendChild:track error:&error];

        NSArray<NSString *> *names = @[ @"A", @"B", @"C" ];
        for (NSUInteger index = 0; index < names.count; index++) {
            OTIOClip *clip = [document makeClip:[names objectAtIndex:index] error:&error];
            OTIORationalTime start = OTIORationalTimeMake((double)index * 24, 24);
            OTIOTimeRange span = OTIOTimeRangeMake(start, OTIORationalTimeMake(24, 24));
            [clip setSourceRange:span error:&error];
            [track appendChild:clip error:&error];
        }

        [document setRoot:timeline error:&error];

        // Three seconds of picture, written as canonical OpenTimelineIO JSON.
        OTIORationalTime duration;
        if ([track getDuration:&duration error:&error]) {
            NSLog(@"%f", OTIORationalTimeToSeconds(duration));
        }
        [document save:@"cut.otio" error:&error];
        [document close];
    }
    return 0;
}
