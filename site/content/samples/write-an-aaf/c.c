#include <stdio.h>
#include <stdlib.h>

#include "otio.h"

int main(void) {
    OtioDocument *document = otio_document_new();

    OtioNode timeline, stack, track;
    otio_timeline_new(document, "Cut", &timeline, NULL);
    otio_stack_new(document, "tracks", &stack, NULL);
    otio_track_new(document, "V1", "Video", &track, NULL);
    otio_timeline_set_tracks(document, timeline, stack, NULL);
    otio_composition_append_child(document, stack, track, NULL);

    OtioTimeRange one_second;
    one_second.start_time.value = 0.0;
    one_second.start_time.rate = 24.0;
    one_second.duration.value = 24.0;
    one_second.duration.rate = 24.0;

    /* An AAF clip is cut from media of a known length, so each clip's media
     * says how much of it there is. A new clip has no media at all, so its
     * reference goes in under upstream's key and is made the active one. */
    const char *names[] = {"A001C003", "A001C004"};
    for (size_t index = 0; index < 2; index += 1) {
        char url[64];
        snprintf(url, sizeof url, "file:///media/%s.mov", names[index]);

        OtioNode reference;
        otio_external_reference_new(document, NULL, url, &reference, NULL);
        otio_media_reference_set_available_range(document, reference, one_second, NULL);

        OtioNode clip;
        otio_clip_new(document, names[index], &clip, NULL);
        otio_clip_set_media_reference(document, clip, "DEFAULT_MEDIA", reference, NULL);
        otio_clip_set_active_media_reference_key(document, clip, "DEFAULT_MEDIA", NULL);
        otio_item_set_source_range(document, clip, one_second, NULL);

        otio_composition_append_child(document, track, clip, NULL);
    }

    otio_document_set_root(document, timeline, NULL);

    /* Every clip needs a MobID, from its metadata, its media's metadata or
     * the AAF its media names. A cut built from scratch has none, so let the
     * writer make them up rather than refuse the clip. */
    OtioWriteOptions options = otio_write_options_default();
    options.aaf_use_empty_mob_ids = true;

    OtioBuffer error;
    if (otio_write_to_file(OTIO_FORMAT_AAF, document, "cut.aaf", &options, &error) != OTIO_STATUS_OK) {
        fprintf(stderr, "%s\n", error.data);
        otio_buffer_free(error);
        return 1;
    }
    otio_document_free(document);

    OtioDocument *written = NULL;
    if (otio_read_from_file(OTIO_FORMAT_AAF, "cut.aaf", NULL, &written, &error) != OTIO_STATUS_OK) {
        fprintf(stderr, "%s\n", error.data);
        otio_buffer_free(error);
        return 1;
    }

    OtioNode root;
    otio_document_root(written, &root, NULL);

    size_t count = 0;
    otio_node_find_clips(written, root, NULL, 0, &count, NULL);

    OtioNode *clips = malloc(count * sizeof *clips);
    otio_node_find_clips(written, root, clips, count, &count, NULL);

    for (size_t index = 0; index < count; index += 1) {
        OtioBuffer name;
        if (otio_node_name(written, clips[index], &name, NULL) == OTIO_STATUS_OK) {
            printf("%s\n", name.data);
            otio_buffer_free(name);
        }
    }

    free(clips);
    otio_document_free(written);
    return 0;
}
