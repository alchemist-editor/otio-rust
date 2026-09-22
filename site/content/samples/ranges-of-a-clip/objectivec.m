#import <Foundation/Foundation.h>

#import <OpenTimelineIO/OpenTimelineIO.h>

static NSString *Frames(OTIOTimeRange span) {
    return [NSString stringWithFormat:@"%g for %g", span.startTime.value, span.duration.value];
}

int main(void) {
    @autoreleasepool {
        NSError *error = nil;

        // Ten seconds of rushes on disk. The available range belongs to the
        // media, not to the clip: it is what the file offers, whoever uses
        // it.
        OTIOExternalReference *media = [OTIOExternalReference externalReferenceWithName:@"A001"
                                                                              targetURL:@"file:///A001.mov"
                                                                                  error:&error];
        [media setAvailableRange:OTIOTimeRangeMake(OTIORationalTimeMake(0, 24),
                                                   OTIORationalTimeMake(240, 24))
                           error:&error];

        // Three seconds of it, starting two seconds in. A source range is in
        // the media's clock, which is why it starts at 48 rather than at 0.
        OTIOClip *clip = [OTIOClip clipWithName:@"shot" error:&error];
        [clip setMediaReference:@"DEFAULT_MEDIA" reference:media error:&error];
        [clip setSourceRange:OTIOTimeRangeMake(OTIORationalTimeMake(48, 24),
                                               OTIORationalTimeMake(72, 24))
                       error:&error];

        // A second of black in front of it, so the clip does not start the
        // track.
        OTIOGap *head = [OTIOGap gapWithName:nil error:&error];
        [head setSourceRange:OTIOTimeRangeMake(OTIORationalTimeMake(0, 24),
                                               OTIORationalTimeMake(24, 24))
                       error:&error];

        OTIOTrack *track = [OTIOTrack trackWithName:@"V1" kind:@"Video" error:&error];
        [track appendChild:head error:&error];
        [track appendChild:clip error:&error];

        // The same clip, asked four questions. The first three answer in the
        // media's clock; the last answers in the track's.
        OTIOTimeRange range;
        if ([clip getAvailableRange:&range error:&error]) NSLog(@"available: %@", Frames(range));
        if ([clip getTrimmedRange:&range error:&error]) NSLog(@"trimmed:   %@", Frames(range));
        if ([clip getVisibleRange:&range error:&error]) NSLog(@"visible:   %@", Frames(range));
        if ([clip getRangeInParent:&range error:&error]) NSLog(@"in parent: %@", Frames(range));
    }
    return 0;
}
