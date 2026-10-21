#import <Foundation/Foundation.h>

#import <OpenTimelineIO/OTIOExtOri.h>
#import <OpenTimelineIO/OTIOExtSync.h>

// The cut both sides have: one shot, which sync knows by the GUID
// main/shot.
static NSString *const CUT =
    @"{\"OTIO_SCHEMA\":\"Timeline.1\",\"name\":\"review\",\"metadata\":{\"sync\":{\"guid\""
     ":\"main\"}},\"tracks\":{\"OTIO_SCHEMA\":\"Stack.1\",\"children\":[{\"OTIO_SCHEMA\""
     ":\"Track.1\",\"name\":\"V1\",\"kind\":\"Video\",\"children\":[{\"OTIO_SCHEMA\":\"Clip"
     ".2\",\"name\":\"shot\",\"metadata\":{\"sync\":{\"guid\":\"main/shot\"}},\"source_ran"
     "ge\":{\"OTIO_SCHEMA\":\"TimeRange.1\",\"start_time\":{\"OTIO_SCHEMA\":\"Rational"
     "Time.1\",\"value\":0,\"rate\":24},\"duration\":{\"OTIO_SCHEMA\":\"RationalTime.1"
     "\",\"value\":48,\"rate\":24}}}]}]}}";

// What Furnace tells a reviewer who joins: who they are, and that they may
// comment.
static NSString *const WELCOME =
    @"{\"v\":2,\"type\":\"welcome\",\"sessionId\":\"s_1\",\"seq\":0,\"ts\":0,\"order\":1,\"pa"
     "yload\":{\"selfId\":\"p_2\",\"selfUserId\":\"u_bea\",\"role\":\"viewer\",\"roster\":["
     "],\"session\":{\"id\":\"s_1\",\"createdAt\":0}}}";

// What Furnace tells the host when it opens the session.
static NSString *const HOST_WELCOME =
    @"{\"v\":2,\"type\":\"welcome\",\"sessionId\":\"s_1\",\"seq\":0,\"ts\":0,\"order\":1,\"pa"
     "yload\":{\"selfId\":\"p_1\",\"selfUserId\":\"u_ari\",\"role\":\"host\",\"roster\":[],"
     "\"session\":{\"id\":\"s_1\",\"createdAt\":0}}}";

// Where the reviewer drew: frame 10 of the shot, as the program shows it.
static NSString *const ANCHOR =
    @"{\"sequenceId\":\"main\",\"capturedGeneration\":1,\"clipGuid\":\"main/shot\",\"so"
     "urceTime\":{\"value\":10,\"rate\":24},\"capturedSequenceTime\":{\"value\":10,\"r"
     "ate\":24},\"capturedAt\":1787742000000}";

// One stroke: a PaintStart, its points and a PaintEnd sharing a uuid.
static NSString *const STROKE =
    @"[{\"OTIO_SCHEMA\":\"PaintStart.1\",\"uuid\":\"s1\",\"rgba\":[1,0.25,0.25,1]},{\"O"
     "TIO_SCHEMA\":\"PaintPoint.1\",\"uuid\":\"s1\",\"points\":{\"OTIO_SCHEMA\":\"PaintV"
     "ertices.1\",\"x\":[-0.5,0,0.5],\"y\":[0.2,0,-0.2],\"size\":[0.01,0.01,0.01],\""
     "alpha\":[1,1,1]}},{\"OTIO_SCHEMA\":\"PaintEnd.1\",\"uuid\":\"s1\"}]";

// The comment as Furnace relays it once it has stored it, to everyone in
// the session. A real host reads this from its own socket.
static NSString *const RELAY =
    @"{\"v\":2,\"type\":\"review_commit\",\"sessionId\":\"s_1\",\"senderId\":\"p_2\",\"seq\""
     ":1,\"ts\":0,\"order\":2,\"payload\":{\"clientMutationId\":\"m1\",\"commentId\":\"c1"
     "\",\"authorUserId\":\"u_bea\",\"authorPresenceId\":\"p_2\",\"serverOrder\":1,\"rev"
     "ision\":1,\"createdAt\":0,\"anchor\":{\"sequenceId\":\"main\",\"capturedGenerati"
     "on\":1,\"clipGuid\":\"main/shot\",\"sourceTime\":{\"value\":10,\"rate\":24},\"capt"
     "uredSequenceTime\":{\"value\":10,\"rate\":24},\"capturedAt\":1787742000000},\""
     "bodyMarkdown\":\"Too dark here.\",\"annotationCommands\":[{\"OTIO_SCHEMA\":\"P"
     "aintStart.1\",\"uuid\":\"s1\",\"rgba\":[1,0.25,0.25,1]},{\"OTIO_SCHEMA\":\"Paint"
     "Point.1\",\"uuid\":\"s1\",\"points\":{\"OTIO_SCHEMA\":\"PaintVertices.1\",\"x\":[-0"
     ".5,0,0.5],\"y\":[0.2,0,-0.2],\"size\":[0.01,0.01,0.01],\"alpha\":[1,1,1]}},{"
     "\"OTIO_SCHEMA\":\"PaintEnd.1\",\"uuid\":\"s1\"}]}}";

// Stops at the first failure, saying why.
#define TRY(ok)                      \
    if (!(ok)) {                     \
        NSLog(@"%@", error);         \
        return 1;                    \
    }

// Stands in for the network: a real program writes every frame the session
// has waiting to its WebSocket to Furnace.
static BOOL SendAll(uint64_t session, NSError **error) {
    for (;;) {
        NSData *bytes = nil;
        BOOL binary = NO;
        if (!OTIOExtSyncSessionPollTransmit(session, 0, &bytes, &binary, error)) {
            return OTIOIsNoValue(*error);
        }
        printf("send %lu bytes\n", (unsigned long)bytes.length);
    }
}

static BOOL Receive(uint64_t session, NSString *frame, NSError **error) {
    return OTIOExtSyncSessionReceive(session, [frame dataUsingEncoding:NSUTF8StringEncoding], NO,
                                     error);
}

int main(void) {
    @autoreleasepool {
        NSError *error = nil;

        // ORI Annotations is an extension; enabling it enables sync too.
        TRY(OTIOExtOriEnable(&error));

        // The reviewer joins with the right to comment.
        uint64_t reviewer = 0;
        TRY(OTIOExtSyncSessionNew(@"{\"token\":\"join-code\",\"identity\":{\"capabilities\":[\"review.comment.create\",\"review.annotation.create\"]}}",
                                  &reviewer, &error));
        TRY(OTIOExtSyncSessionConnect(reviewer, &error) && SendAll(reviewer, &error));
        TRY(Receive(reviewer, WELCOME, &error));
        while (OTIOExtSyncSessionPollEvent(reviewer, &error) != nil) {
        }

        // Commit the stroke with a note. The session sends it again after
        // every reconnect until Furnace stores it, so it is not lost.
        OTIOSerializableObject *seen = OTIOFromJSON(CUT, &error);
        TRY(seen != nil);
        NSString *mutation =
            OTIOExtOriCommit(reviewer, seen, ANCHOR, @"Too dark here.", STROKE, @"", &error);
        TRY(mutation != nil);
        printf("committed %s\n", mutation.UTF8String);
        TRY(SendAll(reviewer, &error));

        // The host hears the stored comment and folds it into its timeline.
        uint64_t host = 0;
        TRY(OTIOExtSyncSessionNew(@"{\"token\":\"host-token\",\"identity\":{\"capabilities\":[\"review.commit.receive\"]}}", &host,
                                  &error));
        TRY(OTIOExtSyncSessionConnect(host, &error) && SendAll(host, &error));
        TRY(Receive(host, HOST_WELCOME, &error) && Receive(host, RELAY, &error));
        OTIOSerializableObject *timeline = OTIOFromJSON(CUT, &error);
        TRY(timeline != nil);
        for (;;) {
            NSString *event = OTIOExtSyncSessionPollEvent(host, &error);
            if (event == nil) {
                break;
            }
            // A stroke still being drawn: draw it over the picture, then drop it.
            NSString *partial = OTIOExtOriReadPartial(event, &error);
            if (partial != nil) {
                printf("drawing %s\n", partial.UTF8String);
                continue;
            }
            NSDictionary *parsed = [NSJSONSerialization
                JSONObjectWithData:[event dataUsingEncoding:NSUTF8StringEncoding]
                           options:0
                             error:NULL];
            if (![[parsed objectForKey:@"type"] isEqual:@"review_relay"]) {
                continue;
            }
            OTIOSerializableObject *clip = nil;
            BOOL already = NO;
            TRY(OTIOExtOriFold(timeline, event, &clip, &already, &error));
            NSString *about = OTIOExtOriDescribe(clip, &error);
            NSString *owner = OTIOExtOriOwnerOf(clip, @"s1", &error);
            TRY(about != nil && owner != nil);
            printf("folded %s already there: %s\n", about.UTF8String, already ? "yes" : "no");
            printf("the stroke belongs to %s\n", owner.UTF8String);
        }
        TRY(OTIOExtSyncSessionClose(reviewer, &error) && OTIOExtSyncSessionClose(host, &error));
    }
    return 0;
}
