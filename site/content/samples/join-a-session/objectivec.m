#import <Foundation/Foundation.h>

#import <OpenTimelineIO/OTIOExtSync.h>

// The frame Furnace sends back once the hello arrives: the viewer is in, and
// the host's timeline comes with it.
static NSString *const WELCOME =
    @"{\"v\":2,\"type\":\"welcome\",\"sessionId\":\"s_1\",\"seq\":0,\"ts\":0,\"order\":1,"
     "\"payload\":{\"selfId\":\"p_2\",\"role\":\"viewer\",\"roster\":[{\"id\":\"p_1\","
     "\"displayName\":\"Host\",\"role\":\"host\",\"joinedAt\":0,\"connectionState\":\"connected\","
     "\"muted\":false}],\"session\":{\"id\":\"s_1\",\"createdAt\":0},\"snapshot\":{"
     "\"sequenceId\":\"main\",\"generation\":1,\"otioJson\":{\"OTIO_SCHEMA\":\"Timeline.1\","
     "\"name\":\"Coastline\",\"metadata\":{\"sync\":{\"guid\":\"t-1\"}},\"tracks\":{"
     "\"OTIO_SCHEMA\":\"Stack.1\",\"metadata\":{\"sync\":{\"guid\":\"s-1\"}},\"children\":[{"
     "\"OTIO_SCHEMA\":\"Track.1\",\"name\":\"V1\",\"kind\":\"Video\",\"metadata\":{\"sync\":{"
     "\"guid\":\"v-1\"}},\"children\":[{\"OTIO_SCHEMA\":\"Clip.2\",\"name\":\"Waves\","
     "\"metadata\":{\"sync\":{\"guid\":\"c-1\"}}}]}]}}}}}";

// A real program writes the frame to its WebSocket to Furnace.
static void Send(NSData *bytes, BOOL binary) {
    printf("send %lu bytes (%s)\n", (unsigned long)bytes.length, binary ? "binary" : "text");
}

// Sends every frame the session has waiting.
static BOOL SendAll(uint64_t session, NSError **error) {
    for (;;) {
        NSData *bytes = nil;
        BOOL binary = NO;
        if (!OTIOExtSyncSessionPollTransmit(session, 0, &bytes, &binary, error)) {
            return OTIOIsNoValue(*error);
        }
        Send(bytes, binary);
    }
}

int main(void) {
    @autoreleasepool {
        NSError *error = nil;
        uint64_t session = 0;
        if (!OTIOExtSyncEnable(&error)
            || !OTIOExtSyncSessionNew(
                @"{\"token\":\"join-code\",\"identity\":{\"displayName\":\"Ada\"}}", &session,
                &error)) {
            NSLog(@"%@", error);
            return 1;
        }

        // Connect once the socket is open, and send the hello.
        if (!OTIOExtSyncSessionConnect(session, &error) || !SendAll(session, &error)) {
            NSLog(@"%@", error);
            return 1;
        }

        // Hand the session each frame that arrives.
        NSData *welcome = [WELCOME dataUsingEncoding:NSUTF8StringEncoding];
        if (!OTIOExtSyncSessionReceive(session, welcome, NO, &error)) {
            NSLog(@"%@", error);
            return 1;
        }

        // The program's copy of the timeline starts empty; handle fills it in.
        OTIOSerializableObject *timeline = OTIOFromJSON(@"{\"OTIO_SCHEMA\": \"Timeline.1\"}", &error);
        if (timeline == nil) {
            NSLog(@"%@", error);
            return 1;
        }
        for (;;) {
            NSString *event = OTIOExtSyncSessionPollEvent(session, &error);
            if (event == nil) {
                break;
            }
            NSDictionary *parsed = [NSJSONSerialization
                JSONObjectWithData:[event dataUsingEncoding:NSUTF8StringEncoding]
                           options:0
                             error:NULL];
            NSString *handled = OTIOExtSyncHandle(timeline, event, &error);
            if (handled == nil) {
                NSLog(@"%@", error);
                return 1;
            }
            printf("%s: %s\n", [[parsed objectForKey:@"type"] UTF8String], handled.UTF8String);
        }
        if (!OTIOIsNoValue(error)) {
            NSLog(@"%@", error);
            return 1;
        }
        error = nil;
        NSString *name = [timeline name:&error];
        if (name == nil) {
            NSLog(@"%@", error);
            return 1;
        }
        printf("the host is showing %s\n", name.UTF8String);

        // Move the playhead; the others see it from the frames this sends.
        if (!OTIOExtSyncSessionSetPlayhead(session, @"main", 24, YES, 1, &error)
            || !SendAll(session, &error)) {
            NSLog(@"%@", error);
            return 1;
        }

        // Say goodbye before closing the socket.
        if (!OTIOExtSyncSessionLeave(session, &error) || !SendAll(session, &error)) {
            NSLog(@"%@", error);
            return 1;
        }

        // Then let the session go.
        if (!OTIOExtSyncSessionClose(session, &error)) {
            NSLog(@"%@", error);
            return 1;
        }
    }
    return 0;
}
