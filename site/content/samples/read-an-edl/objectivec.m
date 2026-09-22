#import <Foundation/Foundation.h>

#import <OpenTimelineIO/OpenTimelineIO.h>

int main(void) {
    @autoreleasepool {
        NSError *error = nil;

        // An EDL never says what rate its timecode is at, so this has to be
        // right: a file read at the wrong rate puts every event in the wrong
        // place rather than failing.
        OTIOReadOptions options = OTIOReadOptionsDefault();
        options.rate = 24;

        // Reading hands back the object the file is about, which for an EDL
        // is the timeline it describes.
        OTIOSerializableObject *timeline = OTIOReadFromFile(OTIOFormatCMX3600,
                                                            @"cut.edl",
                                                            &options,
                                                            &error);
        if (timeline == nil) {
            NSLog(@"%@", error);
            return 1;
        }

        for (OTIOSerializableObject *node in [timeline findClips:&error]) {
            // Every object arrives as the class its schema names, so this is
            // an ordinary Cocoa class test rather than a cast.
            if ([node isKindOfClass:[OTIOClip class]]) {
                NSLog(@"%@", [node name:&error]);
            }
        }
    }
    return 0;
}
