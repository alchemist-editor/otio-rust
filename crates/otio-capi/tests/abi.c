/*
 * A C program that links against libotio and drives it the way a language
 * binding would.
 *
 * The point is not coverage. It is that the header compiles as C, that the
 * symbols it declares actually resolve at link time, and that the structs it
 * describes have the layout the library was built with. A Rust test cannot
 * show any of that, because it is the C side of the boundary that has to
 * believe the header.
 *
 * It prints what it is doing and returns 0 only if every check passed.
 */

#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#include "otio.h"

static int failures = 0;

/*
 * Where every fallible call below writes its message. Each call is handed
 * `err()`, which releases the message the previous call left before lending
 * the slot again, so the checks can read `last_error` after any call.
 */
static OtioBuffer last_error = {NULL, 0};

static OtioBuffer *err(void) {
    otio_buffer_free(last_error);
    last_error.data = NULL;
    last_error.len = 0;
    return &last_error;
}

static const char *last_message(void) {
    return last_error.data != NULL ? last_error.data : "";
}

#define CHECK(condition)                                                      \
    do {                                                                      \
        if (!(condition)) {                                                   \
            printf("  FAIL %s:%d: %s\n", __FILE__, __LINE__, #condition);     \
            printf("       last error: %s\n", last_message());               \
            failures += 1;                                                    \
        }                                                                     \
    } while (0)

#define CHECK_OK(call)                                                        \
    do {                                                                      \
        OtioStatus status_ = (call);                                          \
        if (status_ != OTIO_STATUS_OK) {                                      \
            printf("  FAIL %s:%d: %s\n", __FILE__, __LINE__, #call);          \
            printf("       %s: %s\n", otio_status_name(status_),              \
                   last_message());                                           \
            failures += 1;                                                    \
        }                                                                     \
    } while (0)

#define CHECK_STATUS(call, expected)                                          \
    do {                                                                      \
        OtioStatus status_ = (call);                                          \
        if (status_ != (expected)) {                                          \
            printf("  FAIL %s:%d: %s gave %s, wanted %s\n", __FILE__,         \
                   __LINE__, #call, otio_status_name(status_),                \
                   otio_status_name(expected));                               \
            failures += 1;                                                    \
        }                                                                     \
    } while (0)

/*
 * The library asserts these same sizes from its own side, so the two
 * together pin the layout of everything that crosses by value.
 */
#if defined(__STDC_VERSION__) && __STDC_VERSION__ >= 201112L
_Static_assert(sizeof(OtioNode) == 8, "OtioNode is two 32-bit fields");
_Static_assert(sizeof(OtioRationalTime) == 16, "OtioRationalTime is two doubles");
_Static_assert(sizeof(OtioTimeRange) == 32, "OtioTimeRange is two times");
_Static_assert(sizeof(OtioTimeTransform) == 32, "OtioTimeTransform is a time and two doubles");
_Static_assert(sizeof(OtioColor) == 32, "OtioColor is four doubles");
_Static_assert(sizeof(OtioV2d) == 16, "OtioV2d is two doubles");
_Static_assert(sizeof(OtioBox2d) == 32, "OtioBox2d is two points");
#endif

static OtioRationalTime at(double value, double rate)
{
    OtioRationalTime time;
    time.value = value;
    time.rate = rate;
    return time;
}

static OtioTimeRange span(double start, double duration, double rate)
{
    OtioTimeRange range;
    range.start_time = at(start, rate);
    range.duration = at(duration, rate);
    return range;
}

static int text_is(OtioBuffer buffer, const char *expected)
{
    int same = buffer.data != NULL && strcmp(buffer.data, expected) == 0;
    if (!same) {
        printf("       got \"%s\", wanted \"%s\"\n",
               buffer.data ? buffer.data : "(null)", expected);
    }
    return same;
}

static void check_version(void)
{
    printf("version and status names\n");
    CHECK(otio_version() != NULL);
    CHECK(strlen(otio_version()) > 0);
    CHECK(strcmp(otio_status_name(OTIO_STATUS_OK), "OTIO_STATUS_OK") == 0);
    CHECK(strcmp(otio_status_name(OTIO_STATUS_NO_VALUE), "OTIO_STATUS_NO_VALUE") == 0);
    CHECK(otio_default_indent() == 4);
    CHECK(otio_default_epsilon_s() > 0.0);
}

static void check_time(void)
{
    OtioRationalTime one_second, rescaled, sum;
    OtioTimeRange range;
    OtioBuffer timecode;

    printf("times, spans and timecode\n");

    one_second = at(24.0, 24.0);
    CHECK(otio_rational_time_is_valid(one_second));
    CHECK(otio_rational_time_to_seconds(one_second) == 1.0);

    rescaled = otio_rational_time_rescaled_to(one_second, 48.0);
    CHECK(rescaled.value == 48.0);
    CHECK(rescaled.rate == 48.0);
    /* Equality is about the instant, not the spelling. */
    CHECK(otio_rational_time_equal(one_second, rescaled));
    CHECK(!otio_rational_time_strictly_equal(one_second, rescaled));

    sum = otio_rational_time_add(one_second, at(1.0, 24.0));
    CHECK(sum.value == 25.0);
    CHECK(otio_rational_time_compare(one_second, sum) < 0);

    CHECK_OK(otio_rational_time_to_timecode_at(at(86400.0, 24.0), 24.0,
                                               OTIO_DROP_FRAME_FORCE_NO,
                                               &timecode, err()));
    CHECK(text_is(timecode, "01:00:00:00"));
    otio_buffer_free(timecode);

    CHECK_OK(otio_rational_time_from_timecode("01:00:00:00", 24.0, &one_second, err()));
    CHECK(one_second.value == 86400.0);

    /* A timecode that is not one is an error, with a message to show. */
    CHECK_STATUS(otio_rational_time_from_timecode("nonsense", 24.0, &one_second, err()),
                 OTIO_STATUS_TIME_ERROR);
    CHECK(strlen(last_message()) > 0);

    range = span(0.0, 48.0, 24.0);
    CHECK(otio_time_range_end_time_exclusive(range).value == 48.0);
    CHECK(otio_time_range_contains_time(range, at(24.0, 24.0)));
    CHECK(!otio_time_range_contains_time(range, at(48.0, 24.0)));
    CHECK(otio_time_range_overlaps_range(range, span(24.0, 48.0, 24.0),
                                         otio_default_epsilon_s()));
}

/* Builds a timeline of two clips on one video track. */
static OtioDocument *build_timeline(OtioNode *out_timeline, OtioNode *out_track,
                                    OtioNode *out_first, OtioNode *out_second)
{
    OtioDocument *document = otio_document_new();
    OtioNode timeline, stack, track, first, second, reference;

    CHECK(document != NULL);

    CHECK_OK(otio_timeline_new(document, "cut", &timeline, err()));
    CHECK_OK(otio_stack_new(document, "tracks", &stack, err()));
    CHECK_OK(otio_timeline_set_tracks(document, timeline, stack, err()));
    CHECK_OK(otio_timeline_set_global_start_time(document, timeline,
                                                 at(86400.0, 24.0), err()));
    CHECK_OK(otio_document_set_root(document, timeline, err()));

    CHECK_OK(otio_track_new(document, "V1", "Video", &track, err()));
    CHECK_OK(otio_composition_append_child(document, stack, track, err()));

    CHECK_OK(otio_clip_new(document, "first", &first, err()));
    CHECK_OK(otio_item_set_source_range(document, first, span(0.0, 24.0, 24.0), err()));
    CHECK_OK(otio_external_reference_new(document, "first media",
                                         "file:///media/first.mov", &reference, err()));
    CHECK_OK(otio_media_reference_set_available_range(document, reference,
                                                      span(0.0, 240.0, 24.0), err()));
    CHECK_OK(otio_clip_set_media_reference(document, first, "DEFAULT_MEDIA",
                                           reference, err()));
    CHECK_OK(otio_clip_set_active_media_reference_key(document, first,
                                                      "DEFAULT_MEDIA", err()));
    CHECK_OK(otio_composition_append_child(document, track, first, err()));

    CHECK_OK(otio_clip_new(document, "second", &second, err()));
    CHECK_OK(otio_item_set_source_range(document, second, span(48.0, 36.0, 24.0), err()));
    CHECK_OK(otio_external_reference_new(document, "second media",
                                         "file:///media/second.mov", &reference, err()));
    CHECK_OK(otio_media_reference_set_available_range(document, reference,
                                                      span(0.0, 240.0, 24.0), err()));
    CHECK_OK(otio_clip_set_media_reference(document, second, "DEFAULT_MEDIA",
                                           reference, err()));
    CHECK_OK(otio_clip_set_active_media_reference_key(document, second,
                                                      "DEFAULT_MEDIA", err()));
    CHECK_OK(otio_composition_append_child(document, track, second, err()));

    *out_timeline = timeline;
    *out_track = track;
    *out_first = first;
    *out_second = second;
    return document;
}

static void check_model(void)
{
    OtioDocument *document;
    OtioNode timeline, track, first, second, parent, marker, reference;
    OtioNodeKind kind;
    OtioBuffer text;
    OtioTimeRange range;
    OtioRationalTime duration;
    size_t count = 0;
    size_t children = 0;

    printf("building and walking a timeline\n");
    document = build_timeline(&timeline, &track, &first, &second);

    CHECK_OK(otio_node_kind(document, timeline, &kind, err()));
    CHECK(kind == OTIO_NODE_KIND_TIMELINE);
    CHECK_OK(otio_node_kind(document, first, &kind, err()));
    CHECK(kind == OTIO_NODE_KIND_CLIP);

    CHECK_OK(otio_node_schema_name(document, first, &text, err()));
    CHECK(text_is(text, "Clip"));
    otio_buffer_free(text);

    CHECK_OK(otio_node_name(document, first, &text, err()));
    CHECK(text_is(text, "first"));
    otio_buffer_free(text);

    CHECK_OK(otio_node_set_name(document, first, "renamed", err()));
    CHECK_OK(otio_node_name(document, first, &text, err()));
    CHECK(text_is(text, "renamed"));
    otio_buffer_free(text);
    CHECK_OK(otio_node_set_name(document, first, "first", err()));

    CHECK_OK(otio_node_parent(document, first, &parent, err()));
    CHECK(otio_node_equal(parent, track));

    CHECK_OK(otio_node_child_count(document, track, &children, err()));
    CHECK(children == 2);

    CHECK_OK(otio_item_duration(document, track, &duration, err()));
    CHECK(duration.value == 60.0);

    CHECK_OK(otio_composition_range_of_child(document, track, second, &range, err()));
    CHECK(range.start_time.value == 24.0);
    CHECK(range.duration.value == 36.0);

    /* Lists are sized first, then filled. */
    CHECK_OK(otio_node_find_clips(document, timeline, NULL, 0, &count, err()));
    CHECK(count == 2);
    {
        OtioNode *clips = malloc(count * sizeof(OtioNode));
        size_t again = 0;
        CHECK(clips != NULL);
        CHECK_OK(otio_node_find_clips(document, timeline, clips, count, &again, err()));
        CHECK(again == count);
        CHECK(otio_node_equal(clips[0], first));
        CHECK(otio_node_equal(clips[1], second));
        free(clips);
    }

    /* A clip's active media reference, by the key it was filed under. */
    CHECK_OK(otio_clip_media_reference(document, first, NULL, &reference, err()));
    CHECK_OK(otio_external_reference_target_url(document, reference, &text, err()));
    CHECK(text_is(text, "file:///media/first.mov"));
    otio_buffer_free(text);

    /* Markers hang off an item and are objects in their own right. */
    CHECK_OK(otio_marker_new(document, "look here", span(4.0, 1.0, 24.0), &marker, err()));
    CHECK_OK(otio_marker_set_comment(document, marker, "check the grade", err()));
    CHECK_OK(otio_item_append_marker(document, first, marker, err()));
    CHECK_OK(otio_item_marker_count(document, first, &count, err()));
    CHECK(count == 1);
    CHECK_OK(otio_item_marker_at(document, first, 0, &parent, err()));
    CHECK(otio_node_equal(parent, marker));
    CHECK_OK(otio_marker_comment(document, marker, &text, err()));
    CHECK(text_is(text, "check the grade"));
    otio_buffer_free(text);

    /* An item with no source range says so rather than failing. */
    {
        OtioNode bare;
        CHECK_OK(otio_gap_new(document, "bare", &bare, err()));
        CHECK_STATUS(otio_item_source_range(document, bare, &range, err()),
                     OTIO_STATUS_NO_VALUE);
    }

    /* A marker has no duration, and says so in upstream's words. */
    CHECK_STATUS(otio_item_duration(document, marker, &duration, err()),
                 OTIO_STATUS_CORE_ERROR);
    CHECK(strstr(last_message(), "cannot compute duration") != NULL);

    otio_document_free(document);
}

static void check_metadata(void)
{
    OtioDocument *document;
    OtioNode timeline, track, first, second;
    OtioValueKind kind;
    OtioBuffer text;
    OtioRationalTime time;
    size_t len = 0;
    double number = 0.0;
    bool flag = false;

    printf("metadata\n");
    document = build_timeline(&timeline, &track, &first, &second);

    CHECK_OK(otio_metadata_set_string(document, first, "reel", "A001", err()));
    CHECK_OK(otio_metadata_set_double(document, first, "exposure", 1.5, err()));
    CHECK_OK(otio_metadata_set_bool(document, first, "circled", true, err()));
    CHECK_OK(otio_metadata_set_rational_time(document, first, "sync",
                                             at(12.0, 24.0), err()));

    CHECK_OK(otio_metadata_get_string(document, first, "reel", &text, err()));
    CHECK(text_is(text, "A001"));
    otio_buffer_free(text);
    CHECK_OK(otio_metadata_get_double(document, first, "exposure", &number, err()));
    CHECK(number == 1.5);
    CHECK_OK(otio_metadata_get_bool(document, first, "circled", &flag, err()));
    CHECK(flag);
    CHECK_OK(otio_metadata_get_rational_time(document, first, "sync", &time, err()));
    CHECK(time.value == 12.0);

    CHECK_OK(otio_metadata_kind(document, first, "reel", &kind, err()));
    CHECK(kind == OTIO_VALUE_STRING);

    /* Asking for the wrong type is an error, not a coercion. */
    CHECK_STATUS(otio_metadata_get_double(document, first, "reel", &number, err()),
                 OTIO_STATUS_CORE_ERROR);

    /* Nested dictionaries and arrays, reached by path. */
    CHECK_OK(otio_metadata_set_dictionary(document, first, "cmx_3600", err()));
    CHECK_OK(otio_metadata_set_string(document, first, "cmx_3600.reel", "A001", err()));
    CHECK_OK(otio_metadata_set_vector(document, first, "cmx_3600.comments", 2, err()));
    CHECK_OK(otio_metadata_set_string(document, first, "cmx_3600.comments[0]",
                                      "* FROM CLIP NAME: first", err()));
    CHECK_OK(otio_metadata_set_string(document, first, "cmx_3600.comments[1]",
                                      "* OTIO REFERENCE", err()));
    CHECK_OK(otio_metadata_len(document, first, "cmx_3600.comments", &len, err()));
    CHECK(len == 2);
    CHECK_OK(otio_metadata_get_string(document, first, "cmx_3600.comments[1]",
                                      &text, err()));
    CHECK(text_is(text, "* OTIO REFERENCE"));
    otio_buffer_free(text);

    /* The keys of a dictionary can be walked without knowing them. */
    CHECK_OK(otio_metadata_len(document, first, "cmx_3600", &len, err()));
    CHECK(len == 2);
    CHECK_OK(otio_metadata_key_at(document, first, "cmx_3600", 0, &text, err()));
    CHECK(text_is(text, "comments"));
    otio_buffer_free(text);

    /* An empty path is the whole metadata dictionary. */
    CHECK_OK(otio_metadata_len(document, first, NULL, &len, err()));
    CHECK(len == 5);

    CHECK_OK(otio_metadata_remove(document, first, "cmx_3600.comments[0]", err()));
    CHECK_OK(otio_metadata_len(document, first, "cmx_3600.comments", &len, err()));
    CHECK(len == 1);

    CHECK_STATUS(otio_metadata_get_string(document, first, "absent", &text, err()),
                 OTIO_STATUS_NO_VALUE);

    CHECK_OK(otio_metadata_clear(document, first, err()));
    CHECK_OK(otio_metadata_len(document, first, NULL, &len, err()));
    CHECK(len == 0);

    otio_document_free(document);
}

static void check_edits(void)
{
    OtioDocument *document;
    OtioNode timeline, track, first, second;
    size_t children = 0;
    OtioRationalTime duration;

    printf("edit operations and algorithms\n");
    document = build_timeline(&timeline, &track, &first, &second);

    /* Cutting at 12 frames turns two clips into three. */
    CHECK_OK(otio_edit_slice(document, track, at(12.0, 24.0), false, err()));
    CHECK_OK(otio_node_child_count(document, track, &children, err()));
    CHECK(children == 3);

    /* Slicing does not change how long the track is. */
    CHECK_OK(otio_item_duration(document, track, &duration, err()));
    CHECK(duration.value == 60.0);

    /* Flattening one track gives a track of the same length back. */
    {
        OtioNode flat;
        OtioNode tracks[1];
        tracks[0] = track;
        CHECK_OK(otio_algorithm_flatten_tracks(document, tracks, 1, &flat, err()));
        CHECK_OK(otio_item_duration(document, flat, &duration, err()));
        CHECK(duration.value == 60.0);
    }

    otio_document_free(document);
}

static void check_round_trip(void)
{
    OtioDocument *document;
    OtioDocument *reread = NULL;
    OtioNode timeline, track, first, second, root;
    OtioBuffer written;
    size_t count = 0;

    printf("writing and reading files\n");
    document = build_timeline(&timeline, &track, &first, &second);

    /* OTIO JSON, out and back in. */
    CHECK_OK(otio_write_to_bytes(OTIO_FORMAT_OTIO_JSON, document, NULL, &written, err()));
    CHECK(written.len > 0);
    CHECK(strstr(written.data, "\"OTIO_SCHEMA\": \"Timeline.1\"") != NULL);
    CHECK_OK(otio_document_from_json(written.data, &reread, err()));
    otio_buffer_free(written);
    CHECK_OK(otio_document_root(reread, &root, err()));
    CHECK_OK(otio_node_find_clips(reread, root, NULL, 0, &count, err()));
    CHECK(count == 2);
    otio_document_free(reread);
    reread = NULL;

    /* An EDL, with the options a caller has to state for it. */
    {
        OtioWriteOptions write_options = otio_write_options_default();
        OtioReadOptions read_options = otio_read_options_default();
        write_options.rate = 24.0;
        write_options.edl_style = OTIO_EDL_STYLE_AVID;
        read_options.rate = 24.0;

        CHECK_OK(otio_write_to_bytes(OTIO_FORMAT_CMX_3600, document,
                                     &write_options, &written, err()));
        CHECK(strstr(written.data, "TITLE:") != NULL);
        CHECK_OK(otio_read_from_bytes(OTIO_FORMAT_CMX_3600,
                                      (const uint8_t *)written.data, written.len,
                                      &read_options, &reread, err()));
        otio_buffer_free(written);
        CHECK_OK(otio_document_root(reread, &root, err()));
        CHECK_OK(otio_node_find_clips(reread, root, NULL, 0, &count, err()));
        CHECK(count == 2);
        otio_document_free(reread);
    }

    /* Formats can be looked up by the suffix of a filename. */
    {
        OtioFormat format;
        CHECK_OK(otio_format_from_suffix("edl", &format, err()));
        CHECK(format == OTIO_FORMAT_CMX_3600);
        CHECK_OK(otio_format_from_suffix(".FCPXML", &format, err()));
        CHECK(format == OTIO_FORMAT_FCPX_XML);
        CHECK_STATUS(otio_format_from_suffix("docx", &format, err()),
                     OTIO_STATUS_NO_VALUE);
        CHECK(strcmp(otio_format_name(OTIO_FORMAT_CMX_3600), "cmx_3600") == 0);
    }

    otio_document_free(document);
}

static void check_failures(void)
{
    OtioDocument *document;
    OtioNode timeline, track, first, second;
    OtioBuffer text;
    OtioNodeKind kind;

    printf("the failure paths\n");
    document = build_timeline(&timeline, &track, &first, &second);

    /* A handle whose object is gone fails rather than reaching a new one. */
    CHECK_OK(otio_composition_detach_child(document, track, second, err()));
    CHECK_OK(otio_document_remove(document, second, err()));
    CHECK(!otio_document_contains(document, second));
    CHECK_STATUS(otio_node_kind(document, second, &kind, err()), OTIO_STATUS_STALE_HANDLE);

    /* A handle built out of nothing is checked like any other. */
    {
        OtioNode invented;
        invented.index = 9999;
        invented.generation = 7;
        CHECK_STATUS(otio_node_name(document, invented, &text, err()),
                     OTIO_STATUS_STALE_HANDLE);
    }

    /* A null out-parameter is caught rather than written through. */
    CHECK_STATUS(otio_node_name(document, first, NULL, err()), OTIO_STATUS_NULL_POINTER);
    CHECK_STATUS(otio_node_name(NULL, first, &text, err()), OTIO_STATUS_NULL_POINTER);

    /* The handle that names nothing. */
    CHECK(otio_node_is_none(otio_node_none()));
    CHECK(!otio_node_is_none(first));

    /* A succeeding call writes an empty message, with nothing to free. */
    CHECK_OK(otio_node_name(document, first, &text, err()));
    otio_buffer_free(text);
    CHECK(last_error.data == NULL && last_error.len == 0);

    /* A caller that does not want the message passes null for it. */
    CHECK_OK(otio_node_name(document, first, &text, NULL));
    otio_buffer_free(text);
    CHECK_STATUS(otio_node_name(document, otio_node_none(), &text, NULL),
                 OTIO_STATUS_STALE_HANDLE);

    otio_document_free(document);
}

/*
 * Building an object on its own and moving it into a timeline, which is the
 * shape OpenTimelineIO's Python and C++ APIs have and the reason
 * `otio_document_absorb` exists.
 */
static void check_absorb(void)
{
    OtioDocument *timeline = otio_document_new();
    OtioDocument *scratch = otio_document_new();
    OtioNode track = otio_node_none();
    OtioNode clip = otio_node_none();
    OtioNode *from = NULL;
    OtioNode *to = NULL;
    OtioNode moved = otio_node_none();
    OtioNode parent = otio_node_none();
    OtioBuffer name = {NULL, 0};
    size_t moving = 0;
    size_t count = 0;
    size_t index = 0;

    printf("moving an object between documents\n");

    CHECK_OK(otio_track_new(timeline, "V1", NULL, &track, err()));
    CHECK_OK(otio_clip_new(scratch, "shot_01", &clip, err()));

    /* The source cannot be asked twice, so its size is asked for first. */
    moving = otio_document_node_count(scratch);
    CHECK(moving == 1);
    from = malloc(moving * sizeof(OtioNode));
    to = malloc(moving * sizeof(OtioNode));
    CHECK(from != NULL && to != NULL);

    /* Too small a capacity is refused, and nothing moves. */
    CHECK_STATUS(otio_document_absorb(timeline, &scratch, from, to, 0, &count, err()),
                 OTIO_STATUS_INVALID_ARGUMENT);
    CHECK(scratch != NULL);

    CHECK_OK(otio_document_absorb(timeline, &scratch, from, to, moving, &count, err()));
    CHECK(count == moving);
    CHECK(scratch == NULL);

    for (index = 0; index < count; index += 1) {
        if (otio_node_equal(from[index], clip)) {
            moved = to[index];
        }
    }
    CHECK(!otio_node_is_none(moved));

    /* The object arrived intact and belongs to its new document. */
    CHECK_OK(otio_node_name(timeline, moved, &name, err()));
    CHECK(name.data != NULL && strcmp(name.data, "shot_01") == 0);
    otio_buffer_free(name);

    CHECK_OK(otio_composition_append_child(timeline, track, moved, err()));
    CHECK_OK(otio_node_parent(timeline, moved, &parent, err()));
    CHECK(otio_node_equal(parent, track));

    free(from);
    free(to);
    otio_document_free(timeline);
}

/* A fresh timeline arrives with the stack upstream's constructor builds. */
static void check_new_timeline(void)
{
    OtioDocument *document = otio_document_new();
    OtioNode timeline = otio_node_none();
    OtioNode tracks = otio_node_none();
    OtioNode parent = otio_node_none();
    OtioNode replacement = otio_node_none();
    OtioNode displaced = otio_node_none();
    OtioNode other = otio_node_none();
    OtioNode shared = otio_node_none();
    OtioBuffer name = {NULL, 0};
    OtioNodeKind kind = OTIO_NODE_KIND_SERIALIZABLE_OBJECT;
    size_t children = 0;

    printf("a new timeline and its tracks\n");

    CHECK_OK(otio_timeline_new(document, "cut", &timeline, err()));

    /* Upstream's Timeline() builds an empty stack named "tracks", so a
     * caller can append to a fresh timeline without making one first. */
    CHECK_OK(otio_timeline_tracks(document, timeline, &tracks, err()));
    CHECK(!otio_node_is_none(tracks));
    CHECK_OK(otio_node_kind(document, tracks, &kind, err()));
    CHECK(kind == OTIO_NODE_KIND_STACK);
    CHECK_OK(otio_node_name(document, tracks, &name, err()));
    CHECK(name.data != NULL && strcmp(name.data, "tracks") == 0);
    otio_buffer_free(name);

    /* It is empty, and it belongs to the timeline. */
    CHECK_OK(otio_node_child_count(document, tracks, &children, err()));
    CHECK(children == 0);
    CHECK_OK(otio_node_parent(document, tracks, &parent, err()));
    CHECK(otio_node_equal(parent, timeline));

    /* A caller who wants their own stack still replaces it. */
    displaced = tracks;
    CHECK_OK(otio_stack_new(document, "mine", &replacement, err()));
    CHECK_OK(otio_timeline_set_tracks(document, timeline, replacement, err()));
    CHECK_OK(otio_timeline_tracks(document, timeline, &tracks, err()));
    CHECK(otio_node_equal(tracks, replacement));
    CHECK_OK(otio_node_parent(document, replacement, &parent, err()));
    CHECK(otio_node_equal(parent, timeline));

    /* The stack that was displaced is not destroyed: it stays in the
     * document, parentless, the way a detached child does. Claiming the
     * timeline as its parent after the timeline has disowned it would make
     * every walk that trusts `parent` answer wrongly. */
    CHECK_OK(otio_node_kind(document, displaced, &kind, err()));
    CHECK(kind == OTIO_NODE_KIND_STACK);
    CHECK_STATUS(otio_node_parent(document, displaced, &parent, err()),
                 OTIO_STATUS_NO_VALUE);

    /* Setting the tracks to nothing leaves a fresh empty stack rather than
     * nothing, because upstream's own test does exactly this and then asserts
     * `tl.tracks` is still a Stack. */
    CHECK_OK(otio_timeline_set_tracks(document, timeline, otio_node_none(), err()));
    CHECK_OK(otio_timeline_tracks(document, timeline, &tracks, err()));
    CHECK(!otio_node_is_none(tracks));
    CHECK(!otio_node_equal(tracks, replacement));
    CHECK_OK(otio_node_kind(document, tracks, &kind, err()));
    CHECK(kind == OTIO_NODE_KIND_STACK);
    CHECK_OK(otio_node_child_count(document, tracks, &children, err()));
    CHECK(children == 0);
    CHECK_OK(otio_node_parent(document, tracks, &parent, err()));
    CHECK(otio_node_equal(parent, timeline));

    /* And that replacement displaced the caller's stack in its turn. */
    CHECK_STATUS(otio_node_parent(document, replacement, &parent, err()),
                 OTIO_STATUS_NO_VALUE);

    /* Setting a timeline's tracks to the stack already there is not a
     * displacement, so the stack keeps its parent. */
    CHECK_OK(otio_timeline_set_tracks(document, timeline, tracks, err()));
    CHECK_OK(otio_node_parent(document, tracks, &parent, err()));
    CHECK(otio_node_equal(parent, timeline));

    /* Nothing stops two timelines pointing at one stack. When the second one
     * takes it, the stack's parent is that second timeline, and the first
     * replacing its own tracks afterwards must not disown a stack that is no
     * longer its own. */
    CHECK_OK(otio_timeline_new(document, "other", &other, err()));
    CHECK_OK(otio_timeline_set_tracks(document, other, tracks, err()));
    CHECK_OK(otio_node_parent(document, tracks, &parent, err()));
    CHECK(otio_node_equal(parent, other));

    CHECK_OK(otio_stack_new(document, "later", &replacement, err()));
    CHECK_OK(otio_timeline_set_tracks(document, timeline, replacement, err()));
    CHECK_OK(otio_timeline_tracks(document, other, &shared, err()));
    CHECK(otio_node_equal(shared, tracks));
    CHECK_OK(otio_node_parent(document, tracks, &parent, err()));
    CHECK(otio_node_equal(parent, other));

    otio_document_free(document);
}

int main(void)
{
    printf("otio C ABI, version %s\n", otio_version());

    check_version();
    check_time();
    check_model();
    check_new_timeline();
    check_metadata();
    check_edits();
    check_round_trip();
    check_absorb();
    check_failures();
    otio_buffer_free(last_error);

    if (failures != 0) {
        printf("\n%d check(s) failed\n", failures);
        return 1;
    }
    printf("\nevery check passed\n");
    return 0;
}
