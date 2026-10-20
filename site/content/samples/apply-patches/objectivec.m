#import <Foundation/Foundation.h>

#import <OpenTimelineIO/OTIOExtSync.h>

// A cut, as an .otio file would hold it. Nothing in it has a GUID yet.
static NSString *const CUT =
    @"{\"OTIO_SCHEMA\": \"Timeline.1\", \"name\": \"cut\","
     " \"tracks\": {\"OTIO_SCHEMA\": \"Stack.1\", \"children\": ["
     "{\"OTIO_SCHEMA\": \"Track.1\", \"name\": \"V1\", \"kind\": \"Video\", \"children\": ["
     "{\"OTIO_SCHEMA\": \"Clip.2\", \"name\": \"A\"},"
     " {\"OTIO_SCHEMA\": \"Clip.2\", \"name\": \"B\"}]}]}}";

int main(void) {
    @autoreleasepool {
        NSError *error = nil;
        if (!OTIOExtSyncEnable(&error)) {
            NSLog(@"%@", error);
            return 1;
        }
        OTIOSerializableObject *timeline = OTIOFromJSON(CUT, &error);
        if (timeline == nil) {
            NSLog(@"%@", error);
            return 1;
        }

        // Beginning a transaction gives every object a GUID, and writes the
        // timeline as it was as a snapshot: that snapshot is the transaction.
        // A second program starts from the same stamped timeline.
        NSString *before = OTIOExtSyncBegin(timeline, @"main", 1, &error);
        NSString *json = before == nil ? nil : [timeline toJSON:0 error:&error];
        OTIOSerializableObject *shown = json == nil ? nil : OTIOFromJSON(json, &error);
        if (shown == nil) {
            NSLog(@"%@", error);
            return 1;
        }

        // Edit as usual, then commit for the patches that say what changed.
        NSString *patches = [timeline setName:@"fine cut" error:&error]
                                ? OTIOExtSyncCommit(timeline, before, &error)
                                : nil;
        if (patches == nil) {
            NSLog(@"%@", error);
            return 1;
        }
        printf("%s\n", patches.UTF8String);

        // The other copy applies them in place: what it held still names the
        // same objects.
        NSString *name = OTIOExtSyncApplyTo(shown, patches, &error) ? [shown name:&error] : nil;
        if (name == nil) {
            NSLog(@"%@", error);
            return 1;
        }
        printf("the viewer now shows %s\n", name.UTF8String);
    }
    return 0;
}
