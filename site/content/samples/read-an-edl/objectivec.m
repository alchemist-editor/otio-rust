#import <Foundation/Foundation.h>

#import <OpenTimelineIO/OpenTimelineIO.h>

int main(void) {
    @autoreleasepool {
        NSError *error = nil;

        // An EDL never says what rate its timecode is at, so this has to be
        // right: a file read at the wrong rate puts every event in the wrong
        // place rather than failing.
        OTIOReadOptions options = [OTIODocument readOptionsDefault];
        options.rate = 24;

        OTIODocument *document = [OTIODocument readFromFile:OTIOFormatCMX3600
                                                       path:@"cut.edl"
                                                    options:&options
                                                      error:&error];
        if (document == nil) {
            NSLog(@"%@", error);
            return 1;
        }

        OTIOSerializableObject *root = [document root:&error];
        for (OTIOSerializableObject *node in [root findClips:&error]) {
            // Every object arrives as the class its schema names, so this is
            // an ordinary Cocoa class test rather than a cast.
            if ([node isKindOfClass:[OTIOClip class]]) {
                NSLog(@"%@", [node name:&error]);
            }
        }
        [document close];
    }
    return 0;
}
