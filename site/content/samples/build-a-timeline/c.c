#include <stdio.h>

#include "otio.h"

int main(void) {
    OtioDocument *document = otio_document_new();

    OtioNode timeline, stack, track;
    otio_timeline_new(document, "Cut", &timeline);
    otio_stack_new(document, "tracks", &stack);
    otio_track_new(document, "V1", "Video", &track);
    otio_timeline_set_tracks(document, timeline, stack);
    otio_composition_append_child(document, stack, track);

    const char *names[] = {"A", "B", "C"};
    for (size_t index = 0; index < 3; index += 1) {
        OtioNode clip;
        otio_clip_new(document, names[index], &clip);

        OtioTimeRange span;
        span.start_time.value = (double)index * 24.0;
        span.start_time.rate = 24.0;
        span.duration.value = 24.0;
        span.duration.rate = 24.0;
        otio_item_set_source_range(document, clip, span);

        otio_composition_append_child(document, track, clip);
    }

    otio_document_set_root(document, timeline);

    /* Three seconds of picture, written as canonical OpenTimelineIO JSON. */
    OtioRationalTime duration;
    otio_item_duration(document, track, &duration);
    printf("%g\n", otio_rational_time_to_seconds(duration));

    otio_document_write_to_file(document, "cut.otio", otio_default_indent());
    otio_document_free(document);
    return 0;
}
