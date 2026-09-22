#include <stdio.h>

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

    /* Nothing happens in between. The timeline an EDL parses to is the same
     * timeline FCP X writes out, so converting is a read and a write: the
     * object model is the interchange, and the file formats are two ways of
     * spelling it. */
    if (otio_write_to_file(OTIO_FORMAT_FCPX_XML, document, "cut.fcpxml", NULL) != OTIO_STATUS_OK) {
        fprintf(stderr, "%s\n", otio_error_message());
        otio_document_free(document);
        return 1;
    }

    otio_document_free(document);
    return 0;
}
