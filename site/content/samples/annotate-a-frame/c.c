#include <stdio.h>

#include "otio_ext_ori.h"

/* A cut with one shot, which sync knows by the GUID `shot`. */
static const char CUT[] =
    "{\"OTIO_SCHEMA\": \"Timeline.1\", \"name\": \"review\","
    " \"tracks\": {\"OTIO_SCHEMA\": \"Stack.1\", \"children\": ["
    "  {\"OTIO_SCHEMA\": \"Track.1\", \"name\": \"V1\", \"kind\": \"Video\", \"children\": ["
    "   {\"OTIO_SCHEMA\": \"Clip.2\", \"name\": \"shot\","
    "    \"metadata\": {\"sync\": {\"guid\": \"shot\"}}}]}]}}";

/* Stops at the first failure, saying why. */
#define CHECK(call)                                                       \
    do {                                                                  \
        if ((call) != OTIO_STATUS_OK) {                                   \
            fprintf(stderr, "%s\n", error.data ? error.data : #call);     \
            return 1;                                                     \
        }                                                                 \
    } while (0)

int main(void) {
    OtioBuffer error = {0};

    /* ORI Annotations is an extension; enabling it enables sync too. */
    CHECK(otio_ext_ori_enable(&error));

    OtioDocument *document = NULL;
    OtioNode timeline;
    CHECK(otio_document_from_json(CUT, &document, &error));
    CHECK(otio_document_root(document, &timeline, &error));

    /* The Annotations track, and the clip on it for frame 10 of the shot. */
    OtioNode track, frame;
    CHECK(otio_ext_ori_ensure_track(document, timeline, &track, &error));
    CHECK(otio_ext_ori_ensure_clip(document, track, "shot", 10, 24, &frame, &error));

    /* ORI's coordinates are height-normalised with the origin at the
     * centre, so convert from the pixels of the 1920x1080 picture. */
    const double pixels[3][2] = {{200, 200}, {960, 540}, {1720, 880}};
    double x[3], y[3];
    for (int i = 0; i < 3; i++) {
        CHECK(otio_ext_ori_pixels_to_ori(1920, 1080, pixels[i][0], pixels[i][1], &x[i], &y[i],
                                         &error));
    }

    /* A stroke is a PaintStart, its points and a PaintEnd sharing one uuid.
     * Fields left out take the defaults ORI's classes give them. */
    OtioBuffer uuid = {0};
    CHECK(otio_ext_ori_new_uuid(&uuid, &error));
    char stroke[1024];
    snprintf(stroke, sizeof stroke,
             "[{\"OTIO_SCHEMA\":\"PaintStart.1\",\"uuid\":\"%s\",\"rgba\":[1,0.25,0.25,1],"
             "\"participant_hash\":\"reviewer-1\"},"
             "{\"OTIO_SCHEMA\":\"PaintPoint.1\",\"uuid\":\"%s\",\"points\":{"
             "\"OTIO_SCHEMA\":\"PaintVertices.1\",\"x\":[%.17g,%.17g,%.17g],"
             "\"y\":[%.17g,%.17g,%.17g],\"size\":[0.01,0.01,0.01],\"alpha\":[1,1,1]}},"
             "{\"OTIO_SCHEMA\":\"PaintEnd.1\",\"uuid\":\"%s\"}]",
             uuid.data, uuid.data, x[0], x[1], x[2], y[0], y[1], y[2], uuid.data);
    otio_buffer_free(uuid);
    CHECK(otio_ext_ori_add_commands(document, frame, stroke, &error));

    /* Read them back: every annotation on the shot. */
    OtioNode holders[8];
    size_t count = 0;
    CHECK(otio_ext_ori_list_for_clip(document, timeline, "shot", holders, 8, &count, &error));
    for (size_t i = 0; i < count && i < 8; i++) {
        OtioBuffer about = {0};
        CHECK(otio_ext_ori_describe(document, holders[i], &about, &error));
        printf("%s\n", about.data);
        otio_buffer_free(about);
    }

    /* The file is plain OpenTimelineIO with ORI's schemas in it. */
    OtioBuffer json = {0};
    CHECK(otio_document_to_json(document, 4, &json, &error));
    printf("%s\n", json.data);
    otio_buffer_free(json);
    otio_document_free(document);
    return 0;
}
