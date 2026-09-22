#include <stdio.h>
#include <stdlib.h>

#include "otio.h"

int main(void) {
    /* An EDL never says what rate its timecode is at, so this has to be
     * right: a file read at the wrong rate puts every event in the wrong
     * place rather than failing. */
    OtioReadOptions options = otio_read_options_default();
    options.rate = 24;

    /* A call that can fail says why in the buffer passed last, which is
     * yours to free. Pass NULL where the status is all you want. */
    OtioDocument *document = NULL;
    OtioBuffer error;
    if (otio_read_from_file(OTIO_FORMAT_CMX_3600, "cut.edl", &options, &document, &error) != OTIO_STATUS_OK) {
        fprintf(stderr, "%s\n", error.data);
        otio_buffer_free(error);
        return 1;
    }

    OtioNode root;
    otio_document_root(document, &root, NULL);

    /* A list call is asked twice: once with no buffer, to learn the count,
     * and once to fill one. */
    size_t count = 0;
    otio_node_find_clips(document, root, NULL, 0, &count, NULL);

    OtioNode *clips = malloc(count * sizeof *clips);
    otio_node_find_clips(document, root, clips, count, &count, NULL);

    for (size_t index = 0; index < count; index += 1) {
        OtioBuffer name;
        if (otio_node_name(document, clips[index], &name, NULL) == OTIO_STATUS_OK) {
            printf("%s\n", name.data);
            otio_buffer_free(name);
        }
    }

    free(clips);
    otio_document_free(document);
    return 0;
}
