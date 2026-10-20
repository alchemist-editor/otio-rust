#import <Foundation/Foundation.h>

#import <OpenTimelineIO/OTIOExtSync.h>

// Two versions of one sequence, as Furnace sends them: in the second, clip B
// has moved in front of clip A. Every object carries its GUID in
// metadata.sync.guid, which is how a patch names it.
#define CLIP(name, guid) \
    "{\"OTIO_SCHEMA\": \"Clip.2\", \"name\": \"" name "\"," \
    " \"metadata\": {\"sync\": {\"guid\": \"" guid "\"}}}"
#define SNAPSHOT(generation, first, second) \
    @"{\"sequenceId\": \"main\", \"generation\": " generation ", \"otioJson\": {" \
    "\"OTIO_SCHEMA\": \"Timeline.1\", \"name\": \"cut\"," \
    " \"metadata\": {\"sync\": {\"guid\": \"cut\"}}," \
    " \"tracks\": {\"OTIO_SCHEMA\": \"Stack.1\"," \
    " \"metadata\": {\"sync\": {\"guid\": \"tracks\"}}, \"children\": [" \
    "{\"OTIO_SCHEMA\": \"Track.1\", \"name\": \"V1\", \"kind\": \"Video\"," \
    " \"metadata\": {\"sync\": {\"guid\": \"v1\"}}," \
    " \"children\": [" first ", " second "]}]}}}"

int main(void) {
    @autoreleasepool {
        NSString *before = SNAPSHOT("1", CLIP("A", "a"), CLIP("B", "b"));
        NSString *after = SNAPSHOT("2", CLIP("B", "b"), CLIP("A", "a"));
        NSError *error = nil;

        if (!OTIOExtSyncEnable(&error)) {
            NSLog(@"%@", error);
            return 1;
        }
        NSString *patches = OTIOExtSyncDiff(before, after, YES, &error);
        NSString *stats = patches == nil ? nil : OTIOExtSyncDiffStats(before, after, &error);
        if (stats == nil) {
            NSLog(@"%@", error);
            return 1;
        }

        // One move, of A to after B: a patch names objects by GUID.
        printf("%s\n%s\n", patches.UTF8String, stats.UTF8String);
    }
    return 0;
}
