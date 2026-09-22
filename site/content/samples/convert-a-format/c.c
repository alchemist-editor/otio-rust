#include <stdio.h>

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

    /* Nothing happens in between. The timeline an EDL parses to is the same
     * timeline FCP X writes out, so converting is a read and a write: the
     * object model is the interchange, and the file formats are two ways of
     * spelling it. */
    if (otio_write_to_file(OTIO_FORMAT_FCPX_XML, document, "cut.fcpxml", NULL, &error) != OTIO_STATUS_OK) {
        fprintf(stderr, "%s\n", error.data);
        otio_buffer_free(error);
        otio_document_free(document);
        return 1;
    }

    otio_document_free(document);
    return 0;
}
