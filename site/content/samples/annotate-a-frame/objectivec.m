#import <Foundation/Foundation.h>

#import <OpenTimelineIO/OTIOExtOri.h>

// A cut with one shot, which sync knows by the GUID `shot`.
static NSString *const CUT =
    @"{\"OTIO_SCHEMA\": \"Timeline.1\", \"name\": \"review\","
     " \"tracks\": {\"OTIO_SCHEMA\": \"Stack.1\", \"children\": ["
     "  {\"OTIO_SCHEMA\": \"Track.1\", \"name\": \"V1\", \"kind\": \"Video\", \"children\": ["
     "   {\"OTIO_SCHEMA\": \"Clip.2\", \"name\": \"shot\","
     "    \"metadata\": {\"sync\": {\"guid\": \"shot\"}}}]}]}}";

static int Fail(NSError *error) {
    fprintf(stderr, "%s\n", error.localizedDescription.UTF8String);
    return 1;
}

int main(void) {
    @autoreleasepool {
        NSError *error = nil;

        // ORI Annotations is an extension; enabling it enables sync too.
        if (!OTIOExtOriEnable(&error)) return Fail(error);
        OTIOSerializableObject *timeline = OTIOFromJSON(CUT, &error);
        if (timeline == nil) return Fail(error);

        // The Annotations track, and the clip on it for frame 10 of the shot.
        OTIOSerializableObject *track = OTIOExtOriEnsureTrack(timeline, &error);
        if (track == nil) return Fail(error);
        OTIOSerializableObject *frame = OTIOExtOriEnsureClip(track, @"shot", 10, 24, &error);
        if (frame == nil) return Fail(error);

        // ORI's coordinates are height-normalised with the origin at the
        // centre, so convert from the pixels of the 1920x1080 picture.
        const double pixels[3][2] = {{200, 200}, {960, 540}, {1720, 880}};
        NSMutableArray *xs = [NSMutableArray array];
        NSMutableArray *ys = [NSMutableArray array];
        for (int i = 0; i < 3; i++) {
            double x = 0, y = 0;
            if (!OTIOExtOriPixelsToOri(1920, 1080, pixels[i][0], pixels[i][1], &x, &y, &error)) {
                return Fail(error);
            }
            [xs addObject:@(x)];
            [ys addObject:@(y)];
        }

        // A stroke is a PaintStart, its points and a PaintEnd sharing one
        // uuid. Fields left out take the defaults ORI's classes give them.
        NSString *uuid = OTIOExtOriNewUUID(&error);
        if (uuid == nil) return Fail(error);
        NSArray *stroke = @[
            @{@"OTIO_SCHEMA" : @"PaintStart.1", @"uuid" : uuid, @"rgba" : @[ @1, @0.25, @0.25, @1 ],
              @"participant_hash" : @"reviewer-1"},
            @{@"OTIO_SCHEMA" : @"PaintPoint.1", @"uuid" : uuid,
              @"points" : @{@"OTIO_SCHEMA" : @"PaintVertices.1", @"x" : xs, @"y" : ys,
                            @"size" : @[ @0.01, @0.01, @0.01 ], @"alpha" : @[ @1, @1, @1 ]}},
            @{@"OTIO_SCHEMA" : @"PaintEnd.1", @"uuid" : uuid},
        ];
        NSData *data = [NSJSONSerialization dataWithJSONObject:stroke options:0 error:&error];
        if (data == nil) return Fail(error);
        NSString *json = [[NSString alloc] initWithData:data encoding:NSUTF8StringEncoding];
        if (!OTIOExtOriAddCommands(frame, json, &error)) return Fail(error);

        // Read them back: every annotation on the shot.
        NSArray<OTIOSerializableObject *> *holders = OTIOExtOriListForClip(timeline, @"shot", &error);
        if (holders == nil) return Fail(error);
        for (OTIOSerializableObject *holder in holders) {
            printf("%s\n", OTIOExtOriDescribe(holder, &error).UTF8String);
        }

        // The file is plain OpenTimelineIO with ORI's schemas in it.
        printf("%s\n", [timeline toJSON:4 error:&error].UTF8String);
    }
    return 0;
}
