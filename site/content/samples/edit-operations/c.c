#include <stdio.h>
#include <stdlib.h>

#include "otio.h"

/* One second of picture, named. */
static OtioNode second(OtioDocument *document, const char *name) {
    OtioNode clip;
    otio_clip_new(document, name, &clip, NULL);

    OtioTimeRange span;
    span.start_time.value = 0.0;
    span.start_time.rate = 24.0;
    span.duration.value = 24.0;
    span.duration.rate = 24.0;
    otio_item_set_source_range(document, clip, span, NULL);
    return clip;
}

static void show(OtioDocument *document, OtioNode track) {
    size_t count = 0;
    otio_node_children(document, track, NULL, 0, &count, NULL);

    OtioNode *children = malloc(count * sizeof *children);
    otio_node_children(document, track, children, count, &count, NULL);

    for (size_t index = 0; index < count; index += 1) {
        OtioBuffer name;
        if (otio_node_name(document, children[index], &name, NULL) == OTIO_STATUS_OK) {
            printf("%s%s", index == 0 ? "" : " ", name.data);
            otio_buffer_free(name);
        }
    }
    free(children);

    OtioRationalTime duration;
    otio_item_duration(document, track, &duration, NULL);
    printf(" - %g frames\n", duration.value);
}

int main(void) {
    OtioDocument *document = otio_document_new();

    OtioNode track;
    otio_track_new(document, "V1", "Video", &track, NULL);

    const char *names[] = {"A", "B", "C"};
    for (size_t index = 0; index < 3; index += 1) {
        otio_composition_append_child(document, track, second(document, names[index]), NULL);
    }
    show(document, track);

    /* Insert makes room: everything from the insertion point onwards moves
     * later, and the track gets longer. */
    OtioRationalTime at = {24.0, 24.0};
    otio_edit_insert(document, second(document, "D"), track, at, false, otio_node_none(), NULL);
    show(document, track);

    /* Overwrite does not: it lays an item over a span and whatever was in
     * that span gives way. The track is the same length afterwards. */
    OtioTimeRange over;
    over.start_time.value = 48.0;
    over.start_time.rate = 24.0;
    over.duration.value = 24.0;
    over.duration.rate = 24.0;
    otio_edit_overwrite(document, second(document, "E"), track, over, false, otio_node_none(), NULL);
    show(document, track);

    otio_document_free(document);
    return 0;
}
