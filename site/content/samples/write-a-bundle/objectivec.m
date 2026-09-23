#import <Foundation/Foundation.h>

#import <OpenTimelineIO/OpenTimelineIO.h>

int main(void) {
    @autoreleasepool {
        NSError *error = nil;

        OTIOTimeline *timeline = [OTIOTimeline timelineWithName:@"Cut" error:&error];
        OTIOTrack *track = [OTIOTrack trackWithName:@"V1" kind:@"Video" error:&error];
        [(OTIOStack *)[timeline tracks:&error] appendChild:track error:&error];

        // A cut of two clips: one whose media is a file beside the program,
        // and one whose media is on the web.
        NSArray<NSString *> *names = @[ @"A001C003", @"A001C004" ];
        NSArray<NSString *> *urls = @[ @"shot.mov", @"https://example.com/remote.mov" ];
        for (NSUInteger index = 0; index < names.count; index++) {
            OTIOExternalReference *media =
                [OTIOExternalReference externalReferenceWithName:nil
                                                       targetURL:[urls objectAtIndex:index]
                                                           error:&error];
            OTIOClip *clip = [OTIOClip clipWithName:[names objectAtIndex:index] error:&error];
            [clip setMediaReference:@"DEFAULT_MEDIA" reference:media error:&error];
            [clip setActiveMediaReferenceKey:@"DEFAULT_MEDIA" error:&error];
            [track appendChild:clip error:&error];
        }

        // Every clip whose media is a file has the file copied into the
        // bundle and its reference pointed at the copy. Media that is not a
        // file would stop the write, so it is made missing instead.
        // OTIOFormatOTIOD writes the same layout as a directory.
        OTIOWriteOptions options = OTIOWriteOptionsDefault();
        options.bundleMediaPolicy = OTIOBundleMediaPolicyMissingIfNotFile;
        if (!OTIOWriteToFile(OTIOFormatOTIOZ, timeline, @"cut.otioz", &options, &error)) {
            NSLog(@"%@", error.localizedDescription);
            return 1;
        }

        // Unpacked, with each reference made absolute, the media is ready to
        // use.
        OTIOReadOptions read = OTIOReadOptionsDefault();
        read.bundleExtractPath = @"cut";
        read.bundleAbsoluteMediaPaths = YES;
        OTIOSerializableObject *root =
            OTIOReadFromFile(OTIOFormatOTIOZ, @"cut.otioz", &read, &error);
        for (OTIOSerializableObject *node in [root findClips:&error]) {
            OTIOSerializableObject *media = [(OTIOClip *)node mediaReference:nil error:&error];
            if ([media isKindOfClass:[OTIOExternalReference class]]) {
                NSLog(@"%@", [(OTIOExternalReference *)media targetURL:&error]);
            } else {
                NSLog(@"missing");
            }
        }
    }
    return 0;
}
