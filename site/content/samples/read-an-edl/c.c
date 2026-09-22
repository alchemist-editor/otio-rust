#include <stdio.h>
#include <stdlib.h>

#include "otio.h"

int main(void) {
    /* An EDL never says what rate its timecode is at, so this has to be
     * right: a file read at the wrong rate puts every event in the wrong
     * place rather than failing. */
    OtioReadOptions options = otio_read_options_default();
    options.rate = 24;

    OtioDocument *document = NULL;
    if (otio_read_from_file(OTIO_FORMAT_CMX_3600, "cut.edl", &options, &document) != OTIO_STATUS_OK) {
        fprintf(stderr, "%s\n", otio_error_message());
        return 1;
    }

    OtioNode root;
    otio_document_root(document, &root);

    /* A list call is asked twice: once with no buffer, to learn the count,
     * and once to fill one. */
    size_t count = 0;
    otio_node_find_clips(document, root, NULL, 0, &count);

    OtioNode *clips = malloc(count * sizeof *clips);
    otio_node_find_clips(document, root, clips, count, &count);

    for (size_t index = 0; index < count; index += 1) {
        OtioBuffer name;
        if (otio_node_name(document, clips[index], &name) == OTIO_STATUS_OK) {
            printf("%s\n", name.data);
            otio_buffer_free(name);
        }
    }

    free(clips);
    otio_document_free(document);
    return 0;
}
