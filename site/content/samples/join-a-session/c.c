#include <stdint.h>
#include <stdio.h>
#include <string.h>

#include "otio_ext_sync.h"

/* What Furnace sends a viewer joining a session whose host shared one
 * timeline. */
static const char WELCOME[] =
    "{\"v\":2,\"type\":\"welcome\",\"sessionId\":\"s_1\",\"seq\":0,\"ts\":0,\"order\":1,"
    "\"payload\":{\"selfId\":\"p_2\",\"role\":\"viewer\","
    "\"roster\":[{\"id\":\"p_1\",\"displayName\":\"Host\",\"role\":\"host\",\"joinedAt\":0,"
    "\"connectionState\":\"connected\",\"muted\":false}],"
    "\"session\":{\"id\":\"s_1\",\"createdAt\":0},"
    "\"snapshot\":{\"sequenceId\":\"main\",\"generation\":1,"
    "\"otioJson\":{\"OTIO_SCHEMA\":\"Timeline.1\",\"name\":\"Coastline\","
    "\"metadata\":{\"sync\":{\"guid\":\"t-1\"}},"
    "\"tracks\":{\"OTIO_SCHEMA\":\"Stack.1\",\"metadata\":{\"sync\":{\"guid\":\"s-1\"}},"
    "\"children\":[{\"OTIO_SCHEMA\":\"Track.1\",\"name\":\"V1\",\"kind\":\"Video\","
    "\"metadata\":{\"sync\":{\"guid\":\"v-1\"}},"
    "\"children\":[{\"OTIO_SCHEMA\":\"Clip.2\",\"name\":\"Waves\","
    "\"metadata\":{\"sync\":{\"guid\":\"c-1\"}}}]}]}}}}}";

/* Sends every frame the session has waiting. A real program writes each one
 * to its WebSocket to Furnace; this one only says what it would send. */
static OtioStatus send_frames(uint64_t session, OtioBuffer *error) {
    for (;;) {
        OtioBuffer frame = {0};
        bool binary = false;
        OtioStatus status = otio_ext_sync_session_poll_transmit(session, 0, &frame, &binary, error);
        if (status == OTIO_STATUS_NO_VALUE) {
            return OTIO_STATUS_OK;
        }
        if (status != OTIO_STATUS_OK) {
            return status;
        }
        printf("send %zu bytes as %s\n", frame.len, binary ? "binary" : "text");
        otio_buffer_free(frame);
    }
}

/* Prints an event's type. A real program reads the event with its JSON
 * library. */
static void print_type(const char *event) {
    const char *type = strstr(event, "\"type\":\"");
    if (type != NULL) {
        type += strlen("\"type\":\"");
        printf("event %.*s: ", (int)strcspn(type, "\""), type);
    }
}

int main(void) {
    OtioBuffer error = {0}, name = {0};
    OtioDocument *copy = NULL;
    OtioNode timeline = otio_node_none();
    uint64_t session = 0;
    int failed = 1;

    /* A session owns no socket. Connect once yours is open: the hello is
     * the first frame to send. */
    if (otio_ext_sync_enable(&error) != OTIO_STATUS_OK ||
        otio_ext_sync_session_new("{\"token\": \"join-code\", \"identity\": {\"displayName\": \"Ada\"}}",
                                  &session, &error) != OTIO_STATUS_OK ||
        otio_ext_sync_session_connect(session, &error) != OTIO_STATUS_OK ||
        send_frames(session, &error) != OTIO_STATUS_OK) {
        goto done;
    }

    /* The frame Furnace sends back. */
    if (otio_ext_sync_session_receive(session, (const uint8_t *)WELCOME, strlen(WELCOME), false,
                                      &error) != OTIO_STATUS_OK) {
        goto done;
    }

    /* The program's own copy of the timeline starts empty. Each event goes
     * to otio_ext_sync_handle, which applies what it can and says what it
     * did. */
    if (otio_document_from_json("{\"OTIO_SCHEMA\": \"Timeline.1\"}", &copy, &error) != OTIO_STATUS_OK ||
        otio_document_root(copy, &timeline, &error) != OTIO_STATUS_OK) {
        goto done;
    }
    for (;;) {
        OtioBuffer event = {0}, handled = {0};
        OtioStatus status = otio_ext_sync_session_poll_event(session, &event, &error);
        if (status == OTIO_STATUS_NO_VALUE) {
            break;
        }
        if (status == OTIO_STATUS_OK) {
            print_type(event.data);
            status = otio_ext_sync_handle(copy, timeline, event.data, &handled, &error);
        }
        if (status == OTIO_STATUS_OK) {
            printf("%s\n", handled.data);
        }
        otio_buffer_free(event);
        otio_buffer_free(handled);
        if (status != OTIO_STATUS_OK) {
            goto done;
        }
    }
    if (otio_node_name(copy, timeline, &name, &error) != OTIO_STATUS_OK) {
        goto done;
    }
    printf("joined, showing %s\n", name.data);

    /* Move the playhead, and send what that says to everyone else. */
    if (otio_ext_sync_session_set_playhead(session, "main", 24, true, 1, &error) != OTIO_STATUS_OK ||
        send_frames(session, &error) != OTIO_STATUS_OK) {
        goto done;
    }

    /* Leaving queues a goodbye to send before the socket closes. Closing
     * the session at the end releases it. */
    if (otio_ext_sync_session_leave(session, &error) != OTIO_STATUS_OK ||
        send_frames(session, &error) != OTIO_STATUS_OK) {
        goto done;
    }
    failed = 0;

done:
    if (failed) {
        fprintf(stderr, "%s\n", error.data);
    }
    otio_ext_sync_session_close(session, NULL);
    otio_buffer_free(error);
    otio_buffer_free(name);
    otio_document_free(copy);
    return failed;
}
