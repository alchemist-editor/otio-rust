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

        OTIOSerializableObject *timeline = OTIOReadFromFile(OTIOFormatCMX3600,
                                                            @"cut.edl",
                                                            &options,
                                                            &error);
        if (timeline == nil) {
            NSLog(@"%@", error);
            return 1;
        }

        // Nothing happens in between. The timeline an EDL parses to is the
        // same timeline FCP X writes out, so converting is a read and a
        // write: the object model is the interchange, and the file formats
        // are two ways of spelling it.
        if (!OTIOWriteToFile(OTIOFormatFcpxXML, timeline, @"cut.fcpxml", NULL, &error)) {
            NSLog(@"%@", error);
            return 1;
        }
    }
    return 0;
}
