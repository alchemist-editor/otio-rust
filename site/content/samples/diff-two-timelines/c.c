#include <stdio.h>

#include "otio_ext_sync.h"

/* Two versions of one sequence, as Furnace sends them: in the second, clip B
 * has moved in front of clip A. Every object carries its GUID in
 * metadata.sync.guid, which is how a patch names it. */
#define CLIP(name, guid) \
    "{\"OTIO_SCHEMA\": \"Clip.2\", \"name\": \"" name "\"," \
    " \"metadata\": {\"sync\": {\"guid\": \"" guid "\"}}}"
#define SNAPSHOT(generation, first, second) \
    "{\"sequenceId\": \"main\", \"generation\": " generation ", \"otioJson\": {" \
    "\"OTIO_SCHEMA\": \"Timeline.1\", \"name\": \"cut\"," \
    " \"metadata\": {\"sync\": {\"guid\": \"cut\"}}," \
    " \"tracks\": {\"OTIO_SCHEMA\": \"Stack.1\"," \
    " \"metadata\": {\"sync\": {\"guid\": \"tracks\"}}, \"children\": [" \
    "{\"OTIO_SCHEMA\": \"Track.1\", \"name\": \"V1\", \"kind\": \"Video\"," \
    " \"metadata\": {\"sync\": {\"guid\": \"v1\"}}," \
    " \"children\": [" first ", " second "]}]}}}"

static const char BEFORE[] = SNAPSHOT("1", CLIP("A", "a"), CLIP("B", "b"));
static const char AFTER[] = SNAPSHOT("2", CLIP("B", "b"), CLIP("A", "a"));

int main(void) {
    OtioBuffer error = {0}, patches = {0}, stats = {0};

    if (otio_ext_sync_enable(&error) != OTIO_STATUS_OK ||
        otio_ext_sync_diff(BEFORE, AFTER, true, &patches, &error) != OTIO_STATUS_OK ||
        otio_ext_sync_diff_stats(BEFORE, AFTER, &stats, &error) != OTIO_STATUS_OK) {
        fprintf(stderr, "%s\n", error.data);
        otio_buffer_free(error);
        return 1;
    }

    /* One move, of A to after B: a patch names objects by GUID. */
    printf("%s\n%s\n", patches.data, stats.data);
    otio_buffer_free(patches);
    otio_buffer_free(stats);
    return 0;
}
