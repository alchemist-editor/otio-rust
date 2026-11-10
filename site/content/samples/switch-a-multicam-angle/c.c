#include <stdio.h>
#include <stdlib.h>

#include "otio.h"

/* Two cameras on one interview, and an edit with two cuts on camera A. */
static const char *EDIT =
    "{\n"
    "  \"OTIO_SCHEMA\": \"Timeline.1\",\n"
    "  \"name\": \"interview\",\n"
    "  \"multicams\": {\n"
    "    \"mc1\": {\"OTIO_SCHEMA\": \"Multicam.1\", \"name\": \"Interview\", \"children\": [\n"
    "      {\"OTIO_SCHEMA\": \"Stack.1\", \"name\": \"A\", \"children\": [\n"
    "        {\"OTIO_SCHEMA\": \"Track.1\", \"name\": \"A V\", \"kind\": \"Video\", \"children\": [\n"
    "          {\"OTIO_SCHEMA\": \"Clip.2\", \"name\": \"A_take\",\n"
    "           \"source_range\": {\"OTIO_SCHEMA\": \"TimeRange.1\",\n"
    "             \"start_time\": {\"OTIO_SCHEMA\": \"RationalTime.1\", \"value\": 0, \"rate\": 24},\n"
    "             \"duration\": {\"OTIO_SCHEMA\": \"RationalTime.1\", \"value\": 240, \"rate\": 24}}}]}]},\n"
    "      {\"OTIO_SCHEMA\": \"Stack.1\", \"name\": \"B\", \"children\": [\n"
    "        {\"OTIO_SCHEMA\": \"Track.1\", \"name\": \"B V\", \"kind\": \"Video\", \"children\": [\n"
    "          {\"OTIO_SCHEMA\": \"Clip.2\", \"name\": \"B_take\",\n"
    "           \"source_range\": {\"OTIO_SCHEMA\": \"TimeRange.1\",\n"
    "             \"start_time\": {\"OTIO_SCHEMA\": \"RationalTime.1\", \"value\": 0, \"rate\": 24},\n"
    "             \"duration\": {\"OTIO_SCHEMA\": \"RationalTime.1\", \"value\": 240, \"rate\": 24}}}]}]}]}\n"
    "  },\n"
    "  \"tracks\": {\"OTIO_SCHEMA\": \"Stack.1\", \"children\": [\n"
    "    {\"OTIO_SCHEMA\": \"Track.1\", \"name\": \"V1\", \"kind\": \"Video\", \"children\": [\n"
    "      {\"OTIO_SCHEMA\": \"MulticamClip.1\", \"name\": \"cut 1\", \"multicam\": \"mc1\", \"angle\": \"A\",\n"
    "       \"source_range\": {\"OTIO_SCHEMA\": \"TimeRange.1\",\n"
    "         \"start_time\": {\"OTIO_SCHEMA\": \"RationalTime.1\", \"value\": 0, \"rate\": 24},\n"
    "         \"duration\": {\"OTIO_SCHEMA\": \"RationalTime.1\", \"value\": 48, \"rate\": 24}}},\n"
    "      {\"OTIO_SCHEMA\": \"MulticamClip.1\", \"name\": \"cut 2\", \"multicam\": \"mc1\", \"angle\": \"A\",\n"
    "       \"source_range\": {\"OTIO_SCHEMA\": \"TimeRange.1\",\n"
    "         \"start_time\": {\"OTIO_SCHEMA\": \"RationalTime.1\", \"value\": 48, \"rate\": 24},\n"
    "         \"duration\": {\"OTIO_SCHEMA\": \"RationalTime.1\", \"value\": 48, \"rate\": 24}}}]}]}\n"
    "}\n";

#define CHECK(call)                                                        \
    do {                                                                   \
        if ((call) != OTIO_STATUS_OK) {                                    \
            fprintf(stderr, "%.*s\n", (int)error.len, (const char *)error.data); \
            return 1;                                                      \
        }                                                                  \
    } while (0)

int main(void) {
    OtioBuffer error = {0};
    OtioDocument *document = NULL;
    CHECK(otio_document_from_json(EDIT, &document, &error));

    OtioNode timeline, stack, track, cuts[2];
    size_t count = 0;
    CHECK(otio_document_root(document, &timeline, &error));
    CHECK(otio_timeline_tracks(document, timeline, &stack, &error));
    CHECK(otio_node_child_at(document, stack, 0, &track, &error));
    CHECK(otio_node_children(document, track, cuts, 2, &count, &error));

    /* Switch the second cut to camera B. It keeps its place and its range. */
    CHECK(otio_multicam_clip_set_angle(document, cuts[1], "B", 0, &error));

    /* What each cut plays: a track of one angle. A null kind means the kind
       of the track the cut is on. */
    for (size_t index = 0; index < count; index += 1) {
        OtioNode played;
        OtioBuffer name;
        CHECK(otio_multicam_resolve(document, timeline, cuts[index], NULL, &played, &error));
        CHECK(otio_node_name(document, played, &name, &error));
        printf("cut %zu plays %.*s\n", index + 1, (int)name.len, (const char *)name.data);
        otio_buffer_free(name);
    }

    /* For a reader without multicam: each cut becomes the material it plays. */
    OtioBuffer lost;
    CHECK(otio_multicam_flatten(document, timeline, &lost, &error));
    printf("flattening lost: %.*s\n", (int)lost.len, (const char *)lost.data);
    otio_buffer_free(lost);

    otio_document_write_to_file(document, "flat.otio", otio_default_indent(), NULL);
    otio_document_free(document);
    otio_buffer_free(error);
    return 0;
}
