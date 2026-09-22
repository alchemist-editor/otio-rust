#include <stdio.h>

#include "otio.h"

int main(void) {
    /* A time is a value and a rate, not a number of seconds. Four seconds at
     * 24 is 96 units; the rate travels with it so nothing has to guess
     * later. */
    OtioRationalTime start;
    if (otio_rational_time_from_timecode("01:00:00:00", 24.0, &start) != OTIO_STATUS_OK) {
        fprintf(stderr, "%s\n", otio_error_message());
        return 1;
    }
    OtioRationalTime duration = otio_rational_time_from_frames(96.0, 24.0);

    OtioRationalTime end = otio_rational_time_add(start, duration);

    OtioBuffer timecode;
    if (otio_rational_time_to_timecode(end, &timecode) != OTIO_STATUS_OK) {
        fprintf(stderr, "%s\n", otio_error_message());
        return 1;
    }
    printf("%s for %g seconds\n", timecode.data, otio_rational_time_to_seconds(duration));
    otio_buffer_free(timecode);

    /* Comparison rescales first, so the same instant at two rates is
     * equal. */
    OtioRationalTime a = { 24.0, 24.0 };
    OtioRationalTime b = { 48.0, 48.0 };
    return otio_rational_time_equal(a, b) ? 0 : 1;
}
