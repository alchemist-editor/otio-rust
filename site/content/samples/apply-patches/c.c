#include <stdio.h>

#include "otio_ext_sync.h"

/* A cut, as an .otio file would hold it. Nothing in it has a GUID yet. */
static const char CUT[] =
    "{\"OTIO_SCHEMA\": \"Timeline.1\", \"name\": \"cut\","
    " \"tracks\": {\"OTIO_SCHEMA\": \"Stack.1\", \"children\": ["
    "{\"OTIO_SCHEMA\": \"Track.1\", \"name\": \"V1\", \"kind\": \"Video\", \"children\": ["
    "{\"OTIO_SCHEMA\": \"Clip.2\", \"name\": \"A\"},"
    " {\"OTIO_SCHEMA\": \"Clip.2\", \"name\": \"B\"}]}]}}";

int main(void) {
    OtioBuffer error = {0}, before = {0}, json = {0}, patches = {0}, name = {0};
    OtioDocument *editor = NULL, *viewer = NULL;
    OtioNode timeline = otio_node_none(), shown = otio_node_none();
    int failed = 1;

    if (otio_ext_sync_enable(&error) != OTIO_STATUS_OK ||
        otio_document_from_json(CUT, &editor, &error) != OTIO_STATUS_OK ||
        otio_document_root(editor, &timeline, &error) != OTIO_STATUS_OK) {
        goto done;
    }

    /* Beginning a transaction gives every object a GUID, and writes the
     * timeline as it was as a snapshot: that snapshot is the transaction.
     * A second program starts from the same stamped timeline. */
    if (otio_ext_sync_begin(editor, timeline, "main", 1, &before, &error) != OTIO_STATUS_OK ||
        otio_document_to_json(editor, 0, &json, &error) != OTIO_STATUS_OK ||
        otio_document_from_json(json.data, &viewer, &error) != OTIO_STATUS_OK ||
        otio_document_root(viewer, &shown, &error) != OTIO_STATUS_OK) {
        goto done;
    }

    /* Edit as usual, then commit for the patches that say what changed. */
    if (otio_node_set_name(editor, timeline, "fine cut", &error) != OTIO_STATUS_OK ||
        otio_ext_sync_commit(editor, timeline, before.data, &patches, &error) != OTIO_STATUS_OK) {
        goto done;
    }
    printf("%s\n", patches.data);

    /* The other copy applies them in place: what it held still names the
     * same objects. */
    if (otio_ext_sync_apply_to(viewer, shown, patches.data, &error) != OTIO_STATUS_OK ||
        otio_node_name(viewer, shown, &name, &error) != OTIO_STATUS_OK) {
        goto done;
    }
    printf("the viewer now shows %s\n", name.data);
    failed = 0;

done:
    if (failed) {
        fprintf(stderr, "%s\n", error.data);
    }
    otio_buffer_free(error);
    otio_buffer_free(before);
    otio_buffer_free(json);
    otio_buffer_free(patches);
    otio_buffer_free(name);
    otio_document_free(editor);
    otio_document_free(viewer);
    return failed;
}
