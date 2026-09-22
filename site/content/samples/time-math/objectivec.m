#import <Foundation/Foundation.h>

#import <OpenTimelineIO/OpenTimelineIO.h>

int main(void) {
    @autoreleasepool {
        NSError *error = nil;

        // A time is a value and a rate, not a number of seconds. Four seconds
        // at 24 is 96 units; the rate travels with it so nothing has to guess
        // later.
        //
        // A call that answers a struct has no nil to fail with, so it answers
        // through an out-parameter and says NO. That is the one place this
        // SDK reads like C rather than like Cocoa, and why.
        OTIORationalTime start;
        if (!OTIORationalTimeFromTimecode(@"01:00:00:00", 24, &start, &error)) {
            NSLog(@"%@", error);
            return 1;
        }
        OTIORationalTime duration = OTIORationalTimeFromFrames(96, 24);

        OTIORationalTime end = OTIORationalTimeAdd(start, duration);
        NSLog(@"%@ for %f seconds",
              OTIORationalTimeToTimecode(end, &error),
              OTIORationalTimeToSeconds(duration));

        // Comparison rescales first, so the same instant at two rates is equal.
        return OTIORationalTimeEquals(OTIORationalTimeMake(24, 24),
                                      OTIORationalTimeMake(48, 48))
                   ? 0
                   : 1;
    }
}
