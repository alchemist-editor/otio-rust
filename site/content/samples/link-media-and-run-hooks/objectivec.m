#import <Foundation/Foundation.h>

#import <OpenTimelineIO/OpenTimelineIO.h>

// A cut of two clips, as an .otio file would hold it.
static NSString *const cut = @"{"
    @"\"OTIO_SCHEMA\": \"Track.1\", \"name\": \"V1\", \"kind\": \"Video\","
    @"\"children\": ["
    @"{\"OTIO_SCHEMA\": \"Clip.2\", \"name\": \"A\"},"
    @"{\"OTIO_SCHEMA\": \"Clip.2\", \"name\": \"B\"}"
    @"]}";

// A media linker is handed each clip as it is read, with the arguments the
// read was given, and answers with the media the clip should use. nil, with
// no error, leaves the clip as it was.
@interface ProxyLinker : NSObject <OTIOMediaLinker>
@end

@implementation ProxyLinker
- (nullable OTIOMediaReference *)linkMediaReferenceForClip:(OTIOClip *)clip
                                                 arguments:(OTIOMetadata *)arguments
                                                     error:(NSError **)error {
    NSString *name = [clip name:error];
    NSString *root = [arguments getString:@"root" error:error];
    if (name == nil || root == nil) {
        return nil;
    }
    NSString *url = [NSString stringWithFormat:@"%@/%@.mov", root, name];
    return [OTIOExternalReference externalReferenceWithName:name targetURL:url error:error];
}
@end

// A hook script is handed the whole result, and answers with what the read
// goes on with: here the same object, stamped.
@interface StampScript : NSObject <OTIOHookScript>
@end

@implementation StampScript
- (nullable OTIOSerializableObject *)runHookOnObject:(OTIOSerializableObject *)target
                                           arguments:(OTIOMetadata *)arguments
                                               error:(NSError **)error {
    NSString *who = [arguments getString:@"who" error:error];
    OTIOMetadata *metadata = [(OTIOSerializableObjectWithMetadata *)target metadata];
    if (who == nil || ![metadata setString:@"read_by" value:who error:error]) {
        return nil;
    }
    return target;
}
@end

int main(void) {
    @autoreleasepool {
        NSError *error = nil;
        // The library keeps each plugin until it is unregistered. On Apple's
        // platforms OTIORegisterMediaLinkerUsingBlock takes a block instead.
        if (!OTIORegisterMediaLinker(@"proxies", [ProxyLinker new], &error)
            || !OTIORegisterHookScript(@"stamp", [StampScript new], &error)
            || !OTIOAttachHookScript(@"post_adapter_read", @"stamp", &error)) {
            NSLog(@"%@", error.localizedDescription);
            return 1;
        }

        // The read names the linker, and carries both sets of arguments as
        // JSON.
        OTIOReadOptions options = OTIOReadOptionsDefault();
        options.mediaLinker = @"proxies";
        options.mediaLinkerArguments = @"{\"root\": \"/proxies\"}";
        options.hookArguments = @"{\"who\": \"the conform\"}";
        NSData *data = [cut dataUsingEncoding:NSUTF8StringEncoding];
        OTIOSerializableObject *track =
            OTIOReadFromBytes(OTIOFormatOTIOJSON, data, &options, &error);
        if (track == nil) {
            NSLog(@"%@", error.localizedDescription);
            return 1;
        }

        OTIOMetadata *metadata = [(OTIOTrack *)track metadata];
        printf("read by %s\n", [metadata getString:@"read_by" error:&error].UTF8String);
        for (OTIOSerializableObject *clip in [track findClips:&error]) {
            OTIOSerializableObject *media = [(OTIOClip *)clip mediaReference:nil error:&error];
            NSString *url = [(OTIOExternalReference *)media targetURL:&error];
            printf("%s -> %s\n", [clip name:&error].UTF8String, url.UTF8String);
        }

        OTIODetachHookScript(@"post_adapter_read", @"stamp");
        OTIOUnregisterHookScript(@"stamp");
        OTIOUnregisterMediaLinker(@"proxies");
    }
    return 0;
}
