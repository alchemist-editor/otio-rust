#include <stdio.h>

#include "otio.h"

static void print_range(const char *label, OtioTimeRange span) {
    printf("%s %g for %g\n", label, span.start_time.value, span.duration.value);
}

int main(void) {
    OtioDocument *document = otio_document_new();

    /* Ten seconds of rushes on disk. The available range belongs to the
     * media, not to the clip: it is what the file offers, whoever uses it. */
    OtioNode media;
    otio_external_reference_new(document, "A001", "file:///A001.mov", &media, NULL);

    OtioTimeRange available;
    available.start_time.value = 0.0;
    available.start_time.rate = 24.0;
    available.duration.value = 240.0;
    available.duration.rate = 24.0;
    otio_media_reference_set_available_range(document, media, available, NULL);

    /* Three seconds of it, starting two seconds in. A source range is in the
     * media's clock, which is why it starts at 48 rather than at 0. */
    OtioNode clip;
    otio_clip_new(document, "shot", &clip, NULL);
    otio_clip_set_media_reference(document, clip, "DEFAULT_MEDIA", media, NULL);

    OtioTimeRange source;
    source.start_time.value = 48.0;
    source.start_time.rate = 24.0;
    source.duration.value = 72.0;
    source.duration.rate = 24.0;
    otio_item_set_source_range(document, clip, source, NULL);

    /* A second of black in front of it, so the clip does not start the
     * track. */
    OtioNode head;
    otio_gap_new(document, NULL, &head, NULL);

    OtioTimeRange black;
    black.start_time.value = 0.0;
    black.start_time.rate = 24.0;
    black.duration.value = 24.0;
    black.duration.rate = 24.0;
    otio_item_set_source_range(document, head, black, NULL);

    OtioNode track;
    otio_track_new(document, "V1", "Video", &track, NULL);
    otio_composition_append_child(document, track, head, NULL);
    otio_composition_append_child(document, track, clip, NULL);

    /* The same clip, asked four questions. The first three answer in the
     * media's clock; the last answers in the track's. */
    OtioTimeRange range;
    otio_item_available_range(document, clip, &range, NULL);
    print_range("available:", range);
    otio_item_trimmed_range(document, clip, &range, NULL);
    print_range("trimmed:  ", range);
    otio_item_visible_range(document, clip, &range, NULL);
    print_range("visible:  ", range);
    otio_item_range_in_parent(document, clip, &range, NULL);
    print_range("in parent:", range);

    otio_document_free(document);
    return 0;
}
