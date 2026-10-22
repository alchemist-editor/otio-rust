#include <stdint.h>
#include <stdio.h>
#include <string.h>

#include "otio_ext_ori.h"
#include "otio_ext_sync.h"

/* The cut both sides have: one shot, which sync knows by the GUID
 * main/shot. */
static const char CUT[] =
    "{\"OTIO_SCHEMA\":\"Timeline.1\",\"name\":\"review\",\"metadata\":{\"sync\":{\"guid\""
    ":\"main\"}},\"tracks\":{\"OTIO_SCHEMA\":\"Stack.1\",\"children\":[{\"OTIO_SCHEMA\""
    ":\"Track.1\",\"name\":\"V1\",\"kind\":\"Video\",\"children\":[{\"OTIO_SCHEMA\":\"Clip"
    ".2\",\"name\":\"shot\",\"metadata\":{\"sync\":{\"guid\":\"main/shot\"}},\"source_ran"
    "ge\":{\"OTIO_SCHEMA\":\"TimeRange.1\",\"start_time\":{\"OTIO_SCHEMA\":\"Rational"
    "Time.1\",\"value\":0,\"rate\":24},\"duration\":{\"OTIO_SCHEMA\":\"RationalTime.1"
    "\",\"value\":48,\"rate\":24}}}]}]}}";

/* What Furnace tells a reviewer who joins: who they are, and that they may
 * comment. */
static const char WELCOME[] =
    "{\"v\":2,\"type\":\"welcome\",\"sessionId\":\"s_1\",\"seq\":0,\"ts\":0,\"order\":1,\"pa"
    "yload\":{\"selfId\":\"p_2\",\"selfUserId\":\"u_bea\",\"role\":\"viewer\",\"roster\":["
    "],\"session\":{\"id\":\"s_1\",\"createdAt\":0}}}";

/* What Furnace tells the host when it opens the session. */
static const char HOST_WELCOME[] =
    "{\"v\":2,\"type\":\"welcome\",\"sessionId\":\"s_1\",\"seq\":0,\"ts\":0,\"order\":1,\"pa"
    "yload\":{\"selfId\":\"p_1\",\"selfUserId\":\"u_ari\",\"role\":\"host\",\"roster\":[],"
    "\"session\":{\"id\":\"s_1\",\"createdAt\":0}}}";

/* Where the reviewer drew: frame 10 of the shot, as the program shows it. */
static const char ANCHOR[] =
    "{\"sequenceId\":\"main\",\"capturedGeneration\":1,\"clipGuid\":\"main/shot\",\"so"
    "urceTime\":{\"value\":10,\"rate\":24},\"capturedSequenceTime\":{\"value\":10,\"r"
    "ate\":24},\"capturedAt\":1787742000000}";

/* One stroke: a PaintStart, its points and a PaintEnd sharing a uuid. */
static const char STROKE[] =
    "[{\"OTIO_SCHEMA\":\"PaintStart.1\",\"uuid\":\"s1\",\"rgba\":[1,0.25,0.25,1]},{\"O"
    "TIO_SCHEMA\":\"PaintPoint.1\",\"uuid\":\"s1\",\"points\":{\"OTIO_SCHEMA\":\"PaintV"
    "ertices.1\",\"x\":[-0.5,0,0.5],\"y\":[0.2,0,-0.2],\"size\":[0.01,0.01,0.01],\""
    "alpha\":[1,1,1]}},{\"OTIO_SCHEMA\":\"PaintEnd.1\",\"uuid\":\"s1\"}]";

/* The comment as Furnace relays it once it has stored it, to everyone in
 * the session. A real host reads this from its own socket. */
static const char RELAY[] =
    "{\"v\":2,\"type\":\"review_commit\",\"sessionId\":\"s_1\",\"senderId\":\"p_2\",\"seq\""
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

/* Stops at the first failure, saying why. */
#define CHECK(call)                                                       \
    do {                                                                  \
        if ((call) != OTIO_STATUS_OK) {                                   \
            fprintf(stderr, "%s\n", error.data ? error.data : #call);     \
            return 1;                                                     \
        }                                                                 \
    } while (0)

/* OTIO_STATUS_NO_VALUE is an answer, not a failure: there is nothing more.
 * The library still wrote a message, which the program owns. */
static int nothing_more(OtioStatus status, OtioBuffer *error) {
    if (status != OTIO_STATUS_NO_VALUE) {
        return 0;
    }
    otio_buffer_free(*error);
    *error = (OtioBuffer){0};
    return 1;
}

/* Stands in for the network: a real program writes every frame the session
 * has waiting to its WebSocket to Furnace. */
static OtioStatus send_frames(uint64_t session, OtioBuffer *error) {
    for (;;) {
        OtioBuffer frame = {0};
        bool binary = false;
        OtioStatus status = otio_ext_sync_session_poll_transmit(session, 0, &frame, &binary, error);
        if (nothing_more(status, error)) {
            return OTIO_STATUS_OK;
        }
        if (status != OTIO_STATUS_OK) {
            return status;
        }
        printf("send %zu bytes\n", frame.len);
        otio_buffer_free(frame);
    }
}

static OtioStatus receive(uint64_t session, const char *frame, OtioBuffer *error) {
    return otio_ext_sync_session_receive(session, (const uint8_t *)frame, strlen(frame), false,
                                         error);
}

int main(void) {
    OtioBuffer error = {0};

    /* ORI Annotations is an extension; enabling it enables sync too. */
    CHECK(otio_ext_ori_enable(&error));

    /* The reviewer joins with the right to comment. */
    uint64_t reviewer = 0;
    CHECK(otio_ext_sync_session_new("{\"token\":\"join-code\",\"identity\":{\"capabilities\":[\"review.comment.create\",\"review.annotation.create\"]}}",
                                    &reviewer, &error));
    CHECK(otio_ext_sync_session_connect(reviewer, &error));
    CHECK(send_frames(reviewer, &error));
    CHECK(receive(reviewer, WELCOME, &error));
    for (;;) {
        OtioBuffer event = {0};
        OtioStatus status = otio_ext_sync_session_poll_event(reviewer, &event, &error);
        if (nothing_more(status, &error)) {
            break;
        }
        CHECK(status);
        otio_buffer_free(event);
    }

    /* Commit the stroke with a note. The session sends it again after every
     * reconnect until Furnace stores it, so it is not lost. */
    OtioDocument *seen = NULL;
    OtioNode seen_root;
    OtioBuffer id = {0};
    CHECK(otio_document_from_json(CUT, &seen, &error));
    CHECK(otio_document_root(seen, &seen_root, &error));
    CHECK(otio_ext_ori_commit(reviewer, seen, seen_root, ANCHOR, "Too dark here.", STROKE, "", &id,
                              &error));
    printf("committed %s\n", id.data);
    otio_buffer_free(id);
    otio_document_free(seen);
    CHECK(send_frames(reviewer, &error));

    /* The host hears the stored comment and folds it into its timeline. */
    uint64_t host = 0;
    CHECK(otio_ext_sync_session_new("{\"token\":\"host-token\",\"identity\":{\"capabilities\":[\"review.commit.receive\"]}}",
                                    &host, &error));
    CHECK(otio_ext_sync_session_connect(host, &error));
    CHECK(send_frames(host, &error));
    CHECK(receive(host, HOST_WELCOME, &error));
    CHECK(receive(host, RELAY, &error));
    OtioDocument *document = NULL;
    OtioNode timeline;
    CHECK(otio_document_from_json(CUT, &document, &error));
    CHECK(otio_document_root(document, &timeline, &error));
    for (;;) {
        OtioBuffer event = {0}, partial = {0};
        OtioStatus status = otio_ext_sync_session_poll_event(host, &event, &error);
        if (nothing_more(status, &error)) {
            break;
        }
        CHECK(status);

        /* A stroke still being drawn: draw it over the picture, then drop it. */
        status = otio_ext_ori_read_partial(event.data, &partial, &error);
        if (status == OTIO_STATUS_OK) {
            printf("drawing %s\n", partial.data);
            otio_buffer_free(partial);
        } else if (!nothing_more(status, &error)) {
            CHECK(status);
        } else if (strstr(event.data, "\"type\":\"review_relay\"") != NULL) {
            OtioNode clip;
            bool already = false;
            OtioBuffer about = {0}, owner = {0};
            CHECK(otio_ext_ori_fold(document, timeline, event.data, &clip, &already, &error));
            CHECK(otio_ext_ori_describe(document, clip, &about, &error));
            CHECK(otio_ext_ori_owner_of(document, clip, "s1", &owner, &error));
            printf("folded %s already there: %s\n", about.data, already ? "yes" : "no");
            printf("the stroke belongs to %s\n", owner.data);
            otio_buffer_free(about);
            otio_buffer_free(owner);
        }
        otio_buffer_free(event);
    }

    otio_document_free(document);
    CHECK(otio_ext_sync_session_close(reviewer, &error));
    CHECK(otio_ext_sync_session_close(host, &error));
    return 0;
}
