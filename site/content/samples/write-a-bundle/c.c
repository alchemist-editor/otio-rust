#include <stdio.h>

#include "otio.h"

int main(void) {
    OtioDocument *document = otio_document_new();

    OtioNode timeline, stack, track;
    otio_timeline_new(document, "Cut", &timeline, NULL);
    otio_timeline_tracks(document, timeline, &stack, NULL);
    otio_track_new(document, "V1", "Video", &track, NULL);
    otio_composition_append_child(document, stack, track, NULL);

    /* A cut of two clips: one whose media is a file beside the program, and
     * one whose media is on the web. */
    const char *names[] = {"A001C003", "A001C004"};
    const char *urls[] = {"shot.mov", "https://example.com/remote.mov"};
    for (size_t index = 0; index < 2; index += 1) {
        OtioNode reference, clip;
        otio_external_reference_new(document, NULL, urls[index], &reference, NULL);
        otio_clip_new(document, names[index], &clip, NULL);
        otio_clip_set_media_reference(document, clip, "DEFAULT_MEDIA", reference, NULL);
        otio_clip_set_active_media_reference_key(document, clip, "DEFAULT_MEDIA", NULL);
        otio_composition_append_child(document, track, clip, NULL);
    }

    /* A bundle holds a timeline, and what is written is the document's root. */
    otio_document_set_root(document, timeline, NULL);

    /* Every clip whose media is a file has the file copied into the bundle
     * and its reference pointed at the copy. Media that is not a file would
     * stop the write, so it is made missing instead. OTIO_FORMAT_OTIOD
     * writes the same layout as a directory. */
    OtioWriteOptions options = otio_write_options_default();
    options.bundle_media_policy = OTIO_BUNDLE_MEDIA_POLICY_MISSING_IF_NOT_FILE;

    OtioBuffer error;
    if (otio_write_to_file(OTIO_FORMAT_OTIOZ, document, "cut.otioz", &options, &error) != OTIO_STATUS_OK) {
        fprintf(stderr, "%s\n", error.data);
        otio_buffer_free(error);
        return 1;
    }
    otio_document_free(document);

    /* Unpacked, with each reference made absolute, the media is ready to
     * use. */
    OtioReadOptions read = otio_read_options_default();
    read.bundle_extract_path = "cut";
    read.bundle_absolute_media_paths = true;

    OtioDocument *bundled = NULL;
    if (otio_read_from_file(OTIO_FORMAT_OTIOZ, "cut.otioz", &read, &bundled, &error) != OTIO_STATUS_OK) {
        fprintf(stderr, "%s\n", error.data);
        otio_buffer_free(error);
        return 1;
    }

    OtioNode root, clips[2];
    size_t count = 0;
    otio_document_root(bundled, &root, NULL);
    otio_node_find_clips(bundled, root, clips, 2, &count, NULL);

    for (size_t index = 0; index < count; index += 1) {
        OtioNode media;
        OtioNodeKind kind;
        OtioBuffer url;
        otio_clip_media_reference(bundled, clips[index], NULL, &media, NULL);
        otio_node_kind(bundled, media, &kind, NULL);
        if (kind == OTIO_NODE_KIND_EXTERNAL_REFERENCE
            && otio_external_reference_target_url(bundled, media, &url, NULL) == OTIO_STATUS_OK) {
            printf("%s\n", url.data);
            otio_buffer_free(url);
        } else {
            printf("missing\n");
        }
    }

    otio_document_free(bundled);
    return 0;
}
