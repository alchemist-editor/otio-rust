/*
 * otio.h - a C interface to the otio-rust core.
 *
 * OpenTimelineIO's data model, its composition algorithms, its ten edit
 * operations and its file-format adapters, for any language with a C FFI.
 *
 * Link against `otio`: `libotio.so`, `libotio.dylib`, `otio.dll` or the
 * static `libotio.a`.
 *
 *
 * THE SHAPE OF THE INTERFACE
 *
 * Objects are handles, not structs. A caller holds an `OtioNode` and asks its
 * `OtioDocument` about it. Nothing hands out a pointer into the document, so
 * an edit that moves objects around cannot leave a caller holding a dangling
 * one.
 *
 * Values are plain structs. A time is two doubles, so `OtioRationalTime`
 * crosses by value. Only things with identity go behind handles.
 *
 * Failure is a status and a message. Every call that can fail returns an
 * `OtioStatus`, delivers its result through out-parameters, and takes one
 * more out-parameter last: `OtioBuffer *out_error`, where it writes the
 * sentence describing what went wrong. `OTIO_STATUS_OK` is zero, so
 * `if (otio_...(...))` reads as "if it failed". The message comes back from
 * the call itself, so it does not matter which thread asks; see ERRORS below.
 * `OTIO_STATUS_NO_VALUE` means the question has an answer and the answer is
 * "nothing", which is not an error: an item with no source range reports it.
 *
 * Everything of variable length is an owned buffer. A string, or a whole
 * written file, comes back as an `OtioBuffer` released with
 * `otio_buffer_free`.
 *
 * Lists work in two passes. A call that answers with a list takes a buffer
 * and its capacity and sets `out_count` to how many there really are, whether
 * or not they fit. Call once with a capacity of zero to size the buffer, then
 * again to fill it.
 *
 *
 * THE CONTRACT EVERY CALL ASSUMES
 *
 * 1. A pointer argument is either null, where the call says null is allowed,
 *    or a valid, aligned pointer to an initialized value of the stated type,
 *    writable if the parameter is named `out_`.
 * 2. A `const char *` is a NUL-terminated string in UTF-8.
 * 3. An `OtioDocument *` came from a call that produced one and has not been
 *    passed to `otio_document_free`.
 * 4. An `OtioBuffer` is released once, with `otio_buffer_free`, and not read
 *    afterwards.
 * 5. A document is not internally synchronized. Several threads may read one
 *    at the same time; a thread that edits one must be the only thread
 *    touching it. Two threads working on two documents never interfere.
 *
 * A panic inside the Rust core is caught here and reported as
 * `OTIO_STATUS_PANIC` rather than unwinding into C.
 *
 *
 * ERRORS
 *
 * `out_error` may be null, which says the caller wants only the status. When
 * it is not null the call writes it on every return, success included, so a
 * caller can free it unconditionally:
 *
 * - after `OTIO_STATUS_OK` it is empty: `data` is null and `len` is zero;
 * - after any other status it holds the message, which the caller releases
 *   with `otio_buffer_free`.
 *
 * `OTIO_STATUS_NO_VALUE` is not a failure, but it still says what had no
 * value, so it carries a message too.
 *
 *
 * A WHOLE SESSION
 *
 *     OtioDocument *document = NULL;
 *     OtioBuffer error;
 *     if (otio_read_from_file(OTIO_FORMAT_CMX_3600, "cut.edl", NULL, &document,
 *                             &error)) {
 *         fprintf(stderr, "%s\n", error.data);
 *         otio_buffer_free(error);
 *         return 1;
 *     }
 *
 *     OtioNode timeline;
 *     otio_document_root(document, &timeline, NULL);
 *
 *     size_t count = 0;
 *     otio_node_find_clips(document, timeline, NULL, 0, &count, NULL);
 *     printf("%zu clips\n", count);
 *
 *     otio_write_to_file(OTIO_FORMAT_OTIO_JSON, document, "cut.otio", NULL, NULL);
 *     otio_document_free(document);
 *
 *
 * This file is checked against the library's exported functions by
 * `tests/header.rs`, which compares every declaration below with the Rust
 * entry point of the same name. The two cannot drift apart without CI saying
 * so.
 */

#ifndef OTIO_H
#define OTIO_H

#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

/* ===================================================================== *
 * Status
 * ===================================================================== */

/** What a call did, or why it could not. */
typedef enum OtioStatus {
    /** The call succeeded. */
    OTIO_STATUS_OK = 0,
    /** A pointer argument that may not be null was null. */
    OTIO_STATUS_NULL_POINTER = 1,
    /** A string argument was not valid UTF-8. */
    OTIO_STATUS_INVALID_UTF8 = 2,
    /** The call succeeded, and the answer is that there is no value. */
    OTIO_STATUS_NO_VALUE = 3,
    /** A node handle named an object that no longer exists. */
    OTIO_STATUS_STALE_HANDLE = 4,
    /** An argument was outside the range the call accepts. */
    OTIO_STATUS_INVALID_ARGUMENT = 5,
    /** The document could not answer the question asked of it. */
    OTIO_STATUS_CORE_ERROR = 6,
    /** A timecode or time string could not be read or written. */
    OTIO_STATUS_TIME_ERROR = 7,
    /** A file was not valid for the format it was read as. */
    OTIO_STATUS_PARSE_ERROR = 8,
    /** The document holds something the target format cannot express. */
    OTIO_STATUS_UNSUPPORTED = 9,
    /** A file could not be read from or written to disk. */
    OTIO_STATUS_IO_ERROR = 10,
    /** A panic in the Rust core was caught at the boundary. */
    OTIO_STATUS_PANIC = 11
} OtioStatus;

/* ===================================================================== *
 * Values
 * ===================================================================== */

/**
 * A block of bytes owned by the library.
 *
 * `data` is always NUL-terminated, so a buffer holding text can be used as a
 * C string directly; `len` counts the bytes before the terminator, which is
 * what a caller holding binary data needs.
 */
typedef struct OtioBuffer {
    char *data;
    size_t len;
} OtioBuffer;

/** A measure of time, as a `value` in units of `1 / rate` seconds. */
typedef struct OtioRationalTime {
    double value;
    double rate;
} OtioRationalTime;

/** A span of time: where it starts and how long it lasts. */
typedef struct OtioTimeRange {
    OtioRationalTime start_time;
    OtioRationalTime duration;
} OtioTimeRange;

/** An offset, a speed change and a rate change, applied together. */
typedef struct OtioTimeTransform {
    OtioRationalTime offset;
    double scale;
    double rate;
} OtioTimeTransform;

/**
 * A colour, as four components from 0 to 1.
 *
 * OTIO's colours also carry a name, which the calls that read and write one
 * take separately: a name is variable-length and a colour is not.
 */
typedef struct OtioColor {
    double r;
    double g;
    double b;
    double a;
} OtioColor;

/** A point, or a size, in two dimensions. */
typedef struct OtioV2d {
    double x;
    double y;
} OtioV2d;

/** An axis-aligned rectangle. */
typedef struct OtioBox2d {
    OtioV2d min;
    OtioV2d max;
} OtioBox2d;

/** Whether a timecode is written in drop-frame form. */
typedef enum OtioDropFrame {
    /** Use drop-frame form if the rate is a drop-frame rate. */
    OTIO_DROP_FRAME_INFER_FROM_RATE = 0,
    /** Never use drop-frame form. */
    OTIO_DROP_FRAME_FORCE_NO = 1,
    /** Use drop-frame form, failing if the rate has none. */
    OTIO_DROP_FRAME_FORCE_YES = 2
} OtioDropFrame;

/* ===================================================================== *
 * Documents and handles
 * ===================================================================== */

/**
 * An OTIO document: the arena that owns every object in a timeline.
 *
 * Releasing it releases every object in it, so handles into it go stale
 * rather than dangling.
 */
typedef struct OtioDocument OtioDocument;

/**
 * A handle to an object in a document.
 *
 * An index into the document's arena and the generation of the slot it was
 * issued for. If the object is removed and the slot reused, the generation no
 * longer matches, so a stale handle fails a lookup instead of reaching the
 * new occupant.
 */
typedef struct OtioNode {
    uint32_t index;
    uint32_t generation;
} OtioNode;

/** What kind of object a handle names. */
typedef enum OtioNodeKind {
    OTIO_NODE_KIND_ITEM = 0,
    OTIO_NODE_KIND_CLIP = 1,
    OTIO_NODE_KIND_GAP = 2,
    OTIO_NODE_KIND_TRACK = 3,
    OTIO_NODE_KIND_STACK = 4,
    OTIO_NODE_KIND_TIMELINE = 5,
    OTIO_NODE_KIND_TRANSITION = 6,
    OTIO_NODE_KIND_MARKER = 7,
    OTIO_NODE_KIND_EFFECT = 8,
    OTIO_NODE_KIND_TIME_EFFECT = 9,
    OTIO_NODE_KIND_LINEAR_TIME_WARP = 10,
    OTIO_NODE_KIND_FREEZE_FRAME = 11,
    OTIO_NODE_KIND_EXTERNAL_REFERENCE = 12,
    OTIO_NODE_KIND_MISSING_REFERENCE = 13,
    OTIO_NODE_KIND_GENERATOR_REFERENCE = 14,
    OTIO_NODE_KIND_IMAGE_SEQUENCE_REFERENCE = 15,
    OTIO_NODE_KIND_SERIALIZABLE_COLLECTION = 16,
    OTIO_NODE_KIND_SERIALIZABLE_OBJECT = 17,
    OTIO_NODE_KIND_SERIALIZABLE_OBJECT_WITH_METADATA = 18,
    OTIO_NODE_KIND_COMPOSABLE = 19,
    OTIO_NODE_KIND_COMPOSITION = 20,
    OTIO_NODE_KIND_MEDIA_REFERENCE = 21,
    /** An object whose schema this library does not know. */
    OTIO_NODE_KIND_UNKNOWN_SCHEMA = 22,
    /** A schema added to the core since this header was written. */
    OTIO_NODE_KIND_OTHER = 23
} OtioNodeKind;

/** What to show for a frame an image sequence is missing. */
typedef enum OtioMissingFramePolicy {
    OTIO_MISSING_FRAME_ERROR = 0,
    OTIO_MISSING_FRAME_BLACK = 1,
    OTIO_MISSING_FRAME_HOLD = 2
} OtioMissingFramePolicy;

/**
 * The numbers that say how an image sequence is laid out on disk.
 *
 * The three parts of a frame's filename are strings, so they are read and
 * written by their own calls rather than sitting in here.
 */
typedef struct OtioImageSequence {
    int64_t start_frame;
    int64_t frame_step;
    double rate;
    int64_t frame_zero_padding;
    OtioMissingFramePolicy missing_frame_policy;
} OtioImageSequence;

/** What kind of value sits at a metadata path. */
typedef enum OtioValueKind {
    OTIO_VALUE_NULL = 0,
    OTIO_VALUE_BOOL = 1,
    OTIO_VALUE_INT = 2,
    OTIO_VALUE_UINT = 3,
    OTIO_VALUE_DOUBLE = 4,
    OTIO_VALUE_STRING = 5,
    OTIO_VALUE_RATIONAL_TIME = 6,
    OTIO_VALUE_TIME_RANGE = 7,
    OTIO_VALUE_TIME_TRANSFORM = 8,
    OTIO_VALUE_COLOR = 9,
    OTIO_VALUE_V2D = 10,
    OTIO_VALUE_BOX2D = 11,
    OTIO_VALUE_VECTOR = 12,
    OTIO_VALUE_DICTIONARY = 13,
    /** A whole OTIO object, named by a handle. */
    OTIO_VALUE_OBJECT = 14,
    /** A kind added to the core since this header was written. */
    OTIO_VALUE_OTHER = 15
} OtioValueKind;

/**
 * What to do about a transition at the very start or end of a track when
 * asking for its neighbours.
 */
typedef enum OtioNeighborGapPolicy {
    /** Report no neighbour, which is the literal truth. */
    OTIO_NEIGHBOR_GAP_NEVER = 0,
    /** Report a gap the length of the transition's overhang. */
    OTIO_NEIGHBOR_GAP_AROUND_TRANSITIONS = 1
} OtioNeighborGapPolicy;

/** How far a transition reaches on each side, where it reaches at all. */
typedef struct OtioHandles {
    bool has_before;
    OtioRationalTime before;
    bool has_after;
    OtioRationalTime after;
} OtioHandles;

/** Which clock a three- or four-point edit lines its media up against. */
typedef enum OtioReferencePoint {
    /** Use the media's own timing, and take as much of it as fits. */
    OTIO_REFERENCE_POINT_SOURCE = 0,
    /** Line the media up against the track, trimming it to the gap. */
    OTIO_REFERENCE_POINT_SEQUENCE = 1,
    /** Stretch or squeeze the media to fill the gap exactly. */
    OTIO_REFERENCE_POINT_FIT = 2
} OtioReferencePoint;

/* ===================================================================== *
 * Formats
 * ===================================================================== */

/** A file format this library reads and writes. */
typedef enum OtioFormat {
    /** OpenTimelineIO's own JSON, the `.otio` file. */
    OTIO_FORMAT_OTIO_JSON = 0,
    /** Avid Log Exchange, the `.ale` file. */
    OTIO_FORMAT_ALE = 1,
    /** CMX 3600 EDL, the `.edl` file. */
    OTIO_FORMAT_CMX_3600 = 2,
    /** Final Cut Pro 7 XML, the `.xml` file. */
    OTIO_FORMAT_FCP7_XML = 3,
    /** Final Cut Pro X XML, the `.fcpxml` file. */
    OTIO_FORMAT_FCPX_XML = 4,
    /** The Advanced Authoring Format, the `.aaf` file. */
    OTIO_FORMAT_AAF = 5
} OtioFormat;

/** Which system's conventions an EDL is written for. */
typedef enum OtioEdlStyle {
    /** Avid Media Composer. */
    OTIO_EDL_STYLE_AVID = 0,
    /** Nucoda. */
    OTIO_EDL_STYLE_NUCODA = 1,
    /** Adobe Premiere Pro. */
    OTIO_EDL_STYLE_PREMIERE = 2
} OtioEdlStyle;

/**
 * What to do while reading a file.
 *
 * A field a format does not use is ignored, so one of these can be filled in
 * once and used for several. Pass a null pointer for the usual behaviour, or
 * start from `otio_read_options_default()`.
 *
 * Every field is named so that zero is upstream's default, which is why the
 * AAF fields are spelled as what turning a pass off does.
 *
 * This struct may gain fields before the ABI is declared stable.
 */
typedef struct OtioReadOptions {
    /** The rate timecode is read at. Zero means the format's own default. */
    double rate;
    /** ALE: the column a clip takes its name from. Null means "Name". */
    const char *name_column;
    /** EDL: accept a file whose record timecode does not add up. */
    bool ignore_timecode_mismatch;
    /** AAF: keep the nesting AAF has and OTIO does not need, as upstream's
     *  `simplify=False` does. */
    bool aaf_keep_nesting;
    /** AAF: leave each marker on the slot that carries it, as upstream's
     *  `attach_markers=False` does. */
    bool aaf_markers_on_slots;
    /** AAF: record each keyframed effect parameter's value at every frame of
     *  its effect, as upstream's `bake_keyframed_properties=True` does. */
    bool aaf_bake_keyframes;
} OtioReadOptions;

/** What to do while writing a file. As `OtioReadOptions`. */
typedef struct OtioWriteOptions {
    /** The rate timecode is written at. Zero takes it from the document. */
    double rate;
    /** EDL: which system's conventions to write for. */
    OtioEdlStyle edl_style;
    /** EDL: how many characters to pad or truncate a reel name to. */
    size_t reelname_len;
    /** ALE: the VIDEO_FORMAT to state in the heading. */
    const char *video_format;
    /** AAF: look for a clip's MobID in the AAF file its media names before
     *  looking in its metadata. */
    bool aaf_prefer_file_mob_id;
    /** AAF: make up a MobID for a clip that has none anywhere. Off, such a
     *  clip stops the write. */
    bool aaf_use_empty_mob_ids;
    /** AAF: embed each clip's media in the file. */
    bool aaf_embed_essence;
    /** AAF: give each master clip an edge code slot carrying its media's
     *  range. */
    bool aaf_create_edgecode;
    /** AAF: whom a marker with no user of its own is credited to. Null finds
     *  the user from LOGNAME, USER, LNAME or USERNAME, as upstream does. */
    const char *aaf_user;
    /** AAF: the time the file records, in seconds since the Unix epoch.
     *  Zero reads the system clock. */
    int64_t aaf_time;
    /** AAF: seeds the identifiers the file gives itself and each new clip.
     *  Zero draws fresh ones. */
    uint64_t aaf_id_seed;
} OtioWriteOptions;

/* ===================================================================== *
 * Status and version
 * ===================================================================== */

/**
 * Returns the name of a status code, such as `"OTIO_STATUS_OK"`.
 *
 * The string is static and needs no freeing.
 */
const char *otio_status_name(OtioStatus status);

/**
 * Returns the library's version, as `"MAJOR.MINOR.PATCH"`.
 *
 * The string is static and needs no freeing.
 */
const char *otio_version(void);

/* ===================================================================== *
 * Buffers
 * ===================================================================== */
/**
 * Releases a buffer the library handed out.
 *
 * Passing a buffer whose `data` is null does nothing. Passing the same
 * buffer twice, or one this library did not produce, is undefined.
 */
void otio_buffer_free(OtioBuffer buffer);

/* ===================================================================== *
 * Handles
 * ===================================================================== */
/**
 * Returns the handle that names no object.
 *
 * Fields that may be absent, such as a timeline's tracks before one is set,
 * report this. Compare against it with
 * `otio_node_is_none`.
 */
OtioNode otio_node_none(void);

/**
 * Returns whether a handle names no object.
 */
bool otio_node_is_none(OtioNode node);

/**
 * Returns whether two handles name the same object.
 *
 * This is object identity: two handles are equal only if they were issued for
 * the same occupant of the same slot.
 */
bool otio_node_equal(OtioNode left, OtioNode right);

/* ===================================================================== *
 * Documents, JSON and files
 * ===================================================================== */
/**
 * Creates an empty document with no root.
 *
 * Returns null only if the allocation fails. Release it with
 * [`otio_document_free`].
 */
OtioDocument *otio_document_new(void);

/**
 * Releases a document and every object in it.
 *
 * Passing null does nothing. Every handle into the document is stale
 * afterwards; using one is a programming error this library cannot detect,
 * because the arena it would be checked against is gone.
 */
void otio_document_free(OtioDocument *document);

/**
 * Copies a document, objects and all.
 *
 * Handles into the original name the same objects in the copy, because the
 * copy keeps the arena's layout.
 */
OtioStatus otio_document_clone(
    const OtioDocument *source,
    OtioDocument **out_document,
    OtioBuffer *out_error);

/**
 * Reads a document from OTIO JSON.
 */
OtioStatus otio_document_from_json(
    const char *json,
    OtioDocument **out_document,
    OtioBuffer *out_error);

/**
 * Reads a document from a `.otio` file on disk.
 */
OtioStatus otio_document_read_from_file(
    const char *path,
    OtioDocument **out_document,
    OtioBuffer *out_error);

/**
 * Writes a document as OTIO JSON, starting from its root.
 *
 * `indent` is how many spaces each level is indented by;
 * [`otio_default_indent`] is what upstream's Python bindings use.
 */
OtioStatus otio_document_to_json(
    const OtioDocument *source,
    size_t indent,
    OtioBuffer *out_json,
    OtioBuffer *out_error);

/**
 * Writes one object of a document as OTIO JSON.
 *
 * Upstream's `write_to_string` takes any object, not only a timeline, so this
 * does too.
 */
OtioStatus otio_node_to_json(
    const OtioDocument *source,
    OtioNode node,
    size_t indent,
    OtioBuffer *out_json,
    OtioBuffer *out_error);

/**
 * Writes a document to a `.otio` file on disk.
 */
OtioStatus otio_document_write_to_file(
    const OtioDocument *source,
    const char *path,
    size_t indent,
    OtioBuffer *out_error);

/**
 * Returns the indentation upstream's Python bindings write by default.
 */
size_t otio_default_indent(void);

/**
 * Returns the document's root object.
 *
 * Reports `OTIO_STATUS_NO_VALUE` for a document that has none, which is what
 * a freshly created one is.
 */
OtioStatus otio_document_root(
    const OtioDocument *source,
    OtioNode *out_node,
    OtioBuffer *out_error);

/**
 * Sets the document's root object.
 *
 * Passing `otio_node_none` clears it.
 */
OtioStatus otio_document_set_root(
    OtioDocument *target,
    OtioNode node,
    OtioBuffer *out_error);

/**
 * Returns how many live objects the document holds.
 */
size_t otio_document_node_count(const OtioDocument *source);

/**
 * Returns whether a handle still names a live object.
 */
bool otio_document_contains(const OtioDocument *source, OtioNode node);

/**
 * Removes one object from the document.
 *
 * Anything that referred to it still holds a handle, and that handle is now
 * stale: a lookup fails rather than reaching whatever takes the slot next. To
 * remove an object together with everything hanging off it, use
 * [`otio_document_remove_recursive`].
 */
OtioStatus otio_document_remove(
    OtioDocument *target,
    OtioNode node,
    OtioBuffer *out_error);

/**
 * Removes an object and everything it owns: children, markers, effects and
 * media references.
 */
OtioStatus otio_document_remove_recursive(
    OtioDocument *target,
    OtioNode node,
    OtioBuffer *out_error);

/**
 * Moves every object out of one document into another.
 *
 * This is the call that lets a binding offer the API OpenTimelineIO's own
 * Python and C++ users expect, where a `Clip` is built on its own and put
 * inside a `Track` afterwards. A handle means nothing outside the document it
 * was issued for, so an object built in one document has to be moved into the
 * other rather than pointed at.
 *
 * `source` is consumed. On success it is released and `*source` is set to
 * NULL, so there is nothing left to free and no way to free it twice. On
 * failure nothing moves and `*source` is left alone.
 *
 * The source's root is not adopted, because the target has its own.
 *
 * Every object arrives under a new handle. `out_from` and `out_to` are filled
 * with the old handle and the new one for each object moved, in the same
 * order, and `out_count` is set to how many pairs there are.
 *
 * Unlike the other calls that answer with a list, this one cannot be asked
 * twice to size the buffer: the first call would already have consumed the
 * source. Size the arrays with `otio_document_node_count` on the source before
 * calling, which is exactly how many objects will move. A smaller capacity is
 * OTIO_STATUS_INVALID_ARGUMENT, and nothing moves.
 *
 *     size_t moving = otio_document_node_count(clip_document);
 *     OtioNode *from = malloc(moving * sizeof(OtioNode));
 *     OtioNode *to = malloc(moving * sizeof(OtioNode));
 *     size_t moved = 0;
 *     otio_document_absorb(timeline_document, &clip_document, from, to, moving, &moved);
 */
OtioStatus otio_document_absorb(
    OtioDocument *target,
    OtioDocument **source,
    OtioNode *out_from,
    OtioNode *out_to,
    size_t capacity,
    size_t *out_count,
    OtioBuffer *out_error);

/**
 * Copies an object and everything it owns, into the same document.
 *
 * The copy has no parent, whatever the original had.
 */
OtioStatus otio_document_deep_clone(
    OtioDocument *target,
    OtioNode node,
    OtioNode *out_node,
    OtioBuffer *out_error);

/* ===================================================================== *
 * Building objects, and their fields
 * ===================================================================== */
/**
 * Creates a clip. `name` may be null for an unnamed one.
 */
OtioStatus otio_clip_new(
    OtioDocument *target,
    const char *name,
    OtioNode *out_node,
    OtioBuffer *out_error);

/**
 * Creates a gap.
 */
OtioStatus otio_gap_new(
    OtioDocument *target,
    const char *name,
    OtioNode *out_node,
    OtioBuffer *out_error);

/**
 * Creates a bare item: something that occupies time without saying what fills
 * it.
 */
OtioStatus otio_item_new(
    OtioDocument *target,
    const char *name,
    OtioNode *out_node,
    OtioBuffer *out_error);

/**
 * Creates a track. `kind` may be null, which means `"Video"`, as upstream's
 * default does.
 */
OtioStatus otio_track_new(
    OtioDocument *target,
    const char *name,
    const char *kind,
    OtioNode *out_node,
    OtioBuffer *out_error);

/**
 * Creates a stack.
 */
OtioStatus otio_stack_new(
    OtioDocument *target,
    const char *name,
    OtioNode *out_node,
    OtioBuffer *out_error);

/**
 * Creates a bare composition: children with no layout of its own.
 */
OtioStatus otio_composition_new(
    OtioDocument *target,
    const char *name,
    OtioNode *out_node,
    OtioBuffer *out_error);

/**
 * Creates a composable: something that sits in a composition and nothing
 * more.
 */
OtioStatus otio_composable_new(
    OtioDocument *target,
    const char *name,
    OtioNode *out_node,
    OtioBuffer *out_error);

/**
 * Creates a timeline. Its tracks stack is not created with it; make one with
 * [`otio_stack_new`] and hand it over with [`otio_timeline_set_tracks`].
 */
OtioStatus otio_timeline_new(
    OtioDocument *target,
    const char *name,
    OtioNode *out_node,
    OtioBuffer *out_error);

/**
 * Creates a transition. Its offsets start at zero.
 */
OtioStatus otio_transition_new(
    OtioDocument *target,
    const char *name,
    const char *transition_type,
    OtioNode *out_node,
    OtioBuffer *out_error);

/**
 * Creates a marker covering `marked_range`.
 */
OtioStatus otio_marker_new(
    OtioDocument *target,
    const char *name,
    OtioTimeRange marked_range,
    OtioNode *out_node,
    OtioBuffer *out_error);

/**
 * Creates an effect. `effect_name` is the effect's own name, such as
 * `"Blur"`, which is separate from the object's name.
 */
OtioStatus otio_effect_new(
    OtioDocument *target,
    const char *name,
    const char *effect_name,
    OtioNode *out_node,
    OtioBuffer *out_error);

/**
 * Creates a time effect: an effect that alters timing and has no parameters.
 */
OtioStatus otio_time_effect_new(
    OtioDocument *target,
    const char *name,
    const char *effect_name,
    OtioNode *out_node,
    OtioBuffer *out_error);

/**
 * Creates a constant-rate speed change. A `time_scalar` of 2.0 plays twice as
 * fast.
 */
OtioStatus otio_linear_time_warp_new(
    OtioDocument *target,
    const char *name,
    double time_scalar,
    OtioNode *out_node,
    OtioBuffer *out_error);

/**
 * Creates a freeze frame: a hold on a single frame.
 */
OtioStatus otio_freeze_frame_new(
    OtioDocument *target,
    const char *name,
    OtioNode *out_node,
    OtioBuffer *out_error);

/**
 * Creates a media reference pointing at a URL.
 */
OtioStatus otio_external_reference_new(
    OtioDocument *target,
    const char *name,
    const char *target_url,
    OtioNode *out_node,
    OtioBuffer *out_error);

/**
 * Creates a media reference for media known to exist somewhere unknown.
 */
OtioStatus otio_missing_reference_new(
    OtioDocument *target,
    const char *name,
    OtioNode *out_node,
    OtioBuffer *out_error);

/**
 * Creates a media reference for generated media, such as colour bars.
 */
OtioStatus otio_generator_reference_new(
    OtioDocument *target,
    const char *name,
    const char *generator_kind,
    OtioNode *out_node,
    OtioBuffer *out_error);

/**
 * Creates a media reference for a numbered sequence of image files.
 *
 * The filename parts and the numbers start empty and at zero; set them with
 * [`otio_image_sequence_reference_set_numbers`] and the calls beside it.
 */
OtioStatus otio_image_sequence_reference_new(
    OtioDocument *target,
    const char *name,
    OtioNode *out_node,
    OtioBuffer *out_error);

/**
 * Creates a serializable collection: a group of objects with no timing.
 */
OtioStatus otio_serializable_collection_new(
    OtioDocument *target,
    const char *name,
    OtioNode *out_node,
    OtioBuffer *out_error);

/**
 * Returns what kind of object a handle names.
 */
OtioStatus otio_node_kind(
    const OtioDocument *source,
    OtioNode node_handle,
    OtioNodeKind *out_kind,
    OtioBuffer *out_error);

/**
 * Returns the schema name an object serializes as, such as `"Clip"`.
 */
OtioStatus otio_node_schema_name(
    const OtioDocument *source,
    OtioNode node_handle,
    OtioBuffer *out_name,
    OtioBuffer *out_error);

/**
 * Returns the schema version an object serializes as.
 */
OtioStatus otio_node_schema_version(
    const OtioDocument *source,
    OtioNode node_handle,
    uint32_t *out_version,
    OtioBuffer *out_error);

/**
 * Returns an object's name, which is empty for an object that has none.
 */
OtioStatus otio_node_name(
    const OtioDocument *source,
    OtioNode node_handle,
    OtioBuffer *out_name,
    OtioBuffer *out_error);

/**
 * Sets an object's name.
 */
OtioStatus otio_node_set_name(
    OtioDocument *target,
    OtioNode node_handle,
    const char *name,
    OtioBuffer *out_error);

/**
 * Returns the composition an object sits in.
 *
 * Reports `OTIO_STATUS_NO_VALUE` for an object that is in none, which is what
 * a freshly created one is.
 */
OtioStatus otio_node_parent(
    const OtioDocument *source,
    OtioNode node_handle,
    OtioNode *out_parent,
    OtioBuffer *out_error);

/**
 * Returns whether an object covers what is beneath it when its composition is
 * flattened.
 */
OtioStatus otio_node_visible(
    const OtioDocument *source,
    OtioNode node_handle,
    bool *out_visible,
    OtioBuffer *out_error);

/**
 * Returns whether an object sits over its neighbours rather than beside them.
 *
 * Only a transition does, which is why one does not advance the playhead when
 * a track is laid out.
 */
OtioStatus otio_node_overlapping(
    const OtioDocument *source,
    OtioNode node_handle,
    bool *out_overlapping,
    OtioBuffer *out_error);

/**
 * Returns the portion of its media an item uses.
 *
 * Reports `OTIO_STATUS_NO_VALUE` for an item that takes all of it.
 */
OtioStatus otio_item_source_range(
    const OtioDocument *source,
    OtioNode node_handle,
    OtioTimeRange *out_range,
    OtioBuffer *out_error);

/**
 * Sets the portion of its media an item uses.
 */
OtioStatus otio_item_set_source_range(
    OtioDocument *target,
    OtioNode node_handle,
    OtioTimeRange range,
    OtioBuffer *out_error);

/**
 * Clears an item's source range, so that it takes all of its media.
 */
OtioStatus otio_item_clear_source_range(
    OtioDocument *target,
    OtioNode node_handle,
    OtioBuffer *out_error);

/**
 * Returns whether an item contributes to its composition.
 */
OtioStatus otio_item_enabled(
    const OtioDocument *source,
    OtioNode node_handle,
    bool *out_enabled,
    OtioBuffer *out_error);

/**
 * Sets whether an item contributes to its composition.
 */
OtioStatus otio_item_set_enabled(
    OtioDocument *target,
    OtioNode node_handle,
    bool enabled,
    OtioBuffer *out_error);

/**
 * Returns an item's display tint, and the name that goes with it.
 *
 * `out_name` may be null if the name is not wanted. Reports
 * `OTIO_STATUS_NO_VALUE` for an untinted item.
 */
OtioStatus otio_item_color(
    const OtioDocument *source,
    OtioNode node_handle,
    OtioColor *out_color,
    OtioBuffer *out_name,
    OtioBuffer *out_error);

/**
 * Sets an item's display tint. `name` may be null for an unnamed colour.
 */
OtioStatus otio_item_set_color(
    OtioDocument *target,
    OtioNode node_handle,
    OtioColor color,
    const char *name,
    OtioBuffer *out_error);

/**
 * Clears an item's display tint.
 */
OtioStatus otio_item_clear_color(
    OtioDocument *target,
    OtioNode node_handle,
    OtioBuffer *out_error);

/**
 * Returns how many markers an item carries.
 */
OtioStatus otio_item_marker_count(
    const OtioDocument *source,
    OtioNode node_handle,
    size_t *out_count,
    OtioBuffer *out_error);

/**
 * Returns one of an item's markers.
 */
OtioStatus otio_item_marker_at(
    const OtioDocument *source,
    OtioNode node_handle,
    size_t index,
    OtioNode *out_marker,
    OtioBuffer *out_error);

/**
 * Adds a marker to an item.
 */
OtioStatus otio_item_append_marker(
    OtioDocument *target,
    OtioNode node_handle,
    OtioNode marker_handle,
    OtioBuffer *out_error);

/**
 * Removes one of an item's markers, and returns it.
 *
 * The marker stays in the document; remove it with
 * `otio_document_remove_recursive`
 * if nothing else holds it.
 */
OtioStatus otio_item_remove_marker(
    OtioDocument *target,
    OtioNode node_handle,
    size_t index,
    OtioNode *out_marker,
    OtioBuffer *out_error);

/**
 * Returns how many effects an item carries.
 */
OtioStatus otio_item_effect_count(
    const OtioDocument *source,
    OtioNode node_handle,
    size_t *out_count,
    OtioBuffer *out_error);

/**
 * Returns one of an item's effects.
 */
OtioStatus otio_item_effect_at(
    const OtioDocument *source,
    OtioNode node_handle,
    size_t index,
    OtioNode *out_effect,
    OtioBuffer *out_error);

/**
 * Adds an effect to an item. Effects apply in the order they are added.
 */
OtioStatus otio_item_append_effect(
    OtioDocument *target,
    OtioNode node_handle,
    OtioNode effect_handle,
    OtioBuffer *out_error);

/**
 * Removes one of an item's effects, and returns it.
 */
OtioStatus otio_item_remove_effect(
    OtioDocument *target,
    OtioNode node_handle,
    size_t index,
    OtioNode *out_effect,
    OtioBuffer *out_error);

/**
 * Returns which of a clip's media references is in use.
 */
OtioStatus otio_clip_active_media_reference_key(
    const OtioDocument *source,
    OtioNode node_handle,
    OtioBuffer *out_key,
    OtioBuffer *out_error);

/**
 * Sets which of a clip's media references is in use.
 */
OtioStatus otio_clip_set_active_media_reference_key(
    OtioDocument *target,
    OtioNode node_handle,
    const char *key,
    OtioBuffer *out_error);

/**
 * Returns how many media references a clip holds.
 */
OtioStatus otio_clip_media_reference_count(
    const OtioDocument *source,
    OtioNode node_handle,
    size_t *out_count,
    OtioBuffer *out_error);

/**
 * Returns the key of one of a clip's media references.
 *
 * The references are ordered by key, so walking the indices walks them in a
 * stable order.
 */
OtioStatus otio_clip_media_reference_key_at(
    const OtioDocument *source,
    OtioNode node_handle,
    size_t index,
    OtioBuffer *out_key,
    OtioBuffer *out_error);

/**
 * Returns one of a clip's media references.
 *
 * A null `key` means the active one. Reports `OTIO_STATUS_NO_VALUE` if the
 * key names nothing, which for the active key is a clip with no media.
 */
OtioStatus otio_clip_media_reference(
    const OtioDocument *source,
    OtioNode node_handle,
    const char *key,
    OtioNode *out_reference,
    OtioBuffer *out_error);

/**
 * Sets one of a clip's media references, adding it if the key is new.
 */
OtioStatus otio_clip_set_media_reference(
    OtioDocument *target,
    OtioNode node_handle,
    const char *key,
    OtioNode reference,
    OtioBuffer *out_error);

/**
 * Removes one of a clip's media references, and returns it.
 */
OtioStatus otio_clip_remove_media_reference(
    OtioDocument *target,
    OtioNode node_handle,
    const char *key,
    OtioNode *out_reference,
    OtioBuffer *out_error);

/**
 * Returns what a track carries, such as `"Video"` or `"Audio"`.
 */
OtioStatus otio_track_kind(
    const OtioDocument *source,
    OtioNode node_handle,
    OtioBuffer *out_kind,
    OtioBuffer *out_error);

/**
 * Sets what a track carries.
 */
OtioStatus otio_track_set_kind(
    OtioDocument *target,
    OtioNode node_handle,
    const char *kind,
    OtioBuffer *out_error);

/**
 * Returns the stack holding a timeline's tracks.
 *
 * Reports `OTIO_STATUS_NO_VALUE` for a timeline that has none.
 */
OtioStatus otio_timeline_tracks(
    const OtioDocument *source,
    OtioNode node_handle,
    OtioNode *out_tracks,
    OtioBuffer *out_error);

/**
 * Sets the stack holding a timeline's tracks.
 *
 * Passing `otio_node_none` clears it. The stack's
 * parent is set to the timeline, as upstream's does.
 */
OtioStatus otio_timeline_set_tracks(
    OtioDocument *target,
    OtioNode node_handle,
    OtioNode tracks,
    OtioBuffer *out_error);

/**
 * Returns where a timeline begins, such as `01:00:00:00`.
 *
 * Reports `OTIO_STATUS_NO_VALUE` for a timeline that does not say.
 */
OtioStatus otio_timeline_global_start_time(
    const OtioDocument *source,
    OtioNode node_handle,
    OtioRationalTime *out_time,
    OtioBuffer *out_error);

/**
 * Sets where a timeline begins.
 */
OtioStatus otio_timeline_set_global_start_time(
    OtioDocument *target,
    OtioNode node_handle,
    OtioRationalTime time,
    OtioBuffer *out_error);

/**
 * Clears where a timeline begins.
 */
OtioStatus otio_timeline_clear_global_start_time(
    OtioDocument *target,
    OtioNode node_handle,
    OtioBuffer *out_error);

/**
 * Returns how far a transition reaches into the item before it.
 */
OtioStatus otio_transition_in_offset(
    const OtioDocument *source,
    OtioNode node_handle,
    OtioRationalTime *out_offset,
    OtioBuffer *out_error);

/**
 * Sets how far a transition reaches into the item before it.
 */
OtioStatus otio_transition_set_in_offset(
    OtioDocument *target,
    OtioNode node_handle,
    OtioRationalTime offset,
    OtioBuffer *out_error);

/**
 * Returns how far a transition reaches into the item after it.
 */
OtioStatus otio_transition_out_offset(
    const OtioDocument *source,
    OtioNode node_handle,
    OtioRationalTime *out_offset,
    OtioBuffer *out_error);

/**
 * Sets how far a transition reaches into the item after it.
 */
OtioStatus otio_transition_set_out_offset(
    OtioDocument *target,
    OtioNode node_handle,
    OtioRationalTime offset,
    OtioBuffer *out_error);

/**
 * Returns the kind of transition, such as `"SMPTE_Dissolve"`.
 */
OtioStatus otio_transition_type(
    const OtioDocument *source,
    OtioNode node_handle,
    OtioBuffer *out_type,
    OtioBuffer *out_error);

/**
 * Sets the kind of transition.
 */
OtioStatus otio_transition_set_type(
    OtioDocument *target,
    OtioNode node_handle,
    const char *transition_type,
    OtioBuffer *out_error);

/**
 * Returns whether a transition is applied.
 */
OtioStatus otio_transition_enabled(
    const OtioDocument *source,
    OtioNode node_handle,
    bool *out_enabled,
    OtioBuffer *out_error);

/**
 * Sets whether a transition is applied.
 */
OtioStatus otio_transition_set_enabled(
    OtioDocument *target,
    OtioNode node_handle,
    bool enabled,
    OtioBuffer *out_error);

/**
 * Returns the span a marker covers.
 */
OtioStatus otio_marker_marked_range(
    const OtioDocument *source,
    OtioNode node_handle,
    OtioTimeRange *out_range,
    OtioBuffer *out_error);

/**
 * Sets the span a marker covers.
 */
OtioStatus otio_marker_set_marked_range(
    OtioDocument *target,
    OtioNode node_handle,
    OtioTimeRange range,
    OtioBuffer *out_error);

/**
 * Returns a marker's note.
 */
OtioStatus otio_marker_comment(
    const OtioDocument *source,
    OtioNode node_handle,
    OtioBuffer *out_comment,
    OtioBuffer *out_error);

/**
 * Sets a marker's note.
 */
OtioStatus otio_marker_set_comment(
    OtioDocument *target,
    OtioNode node_handle,
    const char *comment,
    OtioBuffer *out_error);

/**
 * Returns a marker's tint, and the name that goes with it.
 *
 * `out_name` may be null if the name is not wanted. Reports
 * `OTIO_STATUS_NO_VALUE` for an untinted marker.
 */
OtioStatus otio_marker_color(
    const OtioDocument *source,
    OtioNode node_handle,
    OtioColor *out_color,
    OtioBuffer *out_name,
    OtioBuffer *out_error);

/**
 * Sets a marker's tint. `name` may be null for an unnamed colour.
 */
OtioStatus otio_marker_set_color(
    OtioDocument *target,
    OtioNode node_handle,
    OtioColor color,
    const char *name,
    OtioBuffer *out_error);

/**
 * Returns an effect's own name, such as `"LinearTimeWarp"`.
 */
OtioStatus otio_effect_effect_name(
    const OtioDocument *source,
    OtioNode node_handle,
    OtioBuffer *out_name,
    OtioBuffer *out_error);

/**
 * Sets an effect's own name.
 */
OtioStatus otio_effect_set_effect_name(
    OtioDocument *target,
    OtioNode node_handle,
    const char *effect_name,
    OtioBuffer *out_error);

/**
 * Returns whether an effect is applied.
 */
OtioStatus otio_effect_enabled(
    const OtioDocument *source,
    OtioNode node_handle,
    bool *out_enabled,
    OtioBuffer *out_error);

/**
 * Sets whether an effect is applied.
 */
OtioStatus otio_effect_set_enabled(
    OtioDocument *target,
    OtioNode node_handle,
    bool enabled,
    OtioBuffer *out_error);

/**
 * Returns a speed change's multiplier.
 *
 * Only a `LinearTimeWarp` and a `FreezeFrame` have one; anything else reports
 * `OTIO_STATUS_CORE_ERROR`.
 */
OtioStatus otio_effect_time_scalar(
    const OtioDocument *source,
    OtioNode node_handle,
    double *out_scalar,
    OtioBuffer *out_error);

/**
 * Sets a speed change's multiplier.
 */
OtioStatus otio_effect_set_time_scalar(
    OtioDocument *target,
    OtioNode node_handle,
    double scalar,
    OtioBuffer *out_error);

/**
 * Returns the span of media a reference says is available.
 *
 * Reports `OTIO_STATUS_NO_VALUE` for a reference that does not say.
 */
OtioStatus otio_media_reference_available_range(
    const OtioDocument *source,
    OtioNode node_handle,
    OtioTimeRange *out_range,
    OtioBuffer *out_error);

/**
 * Sets the span of media a reference says is available.
 */
OtioStatus otio_media_reference_set_available_range(
    OtioDocument *target,
    OtioNode node_handle,
    OtioTimeRange range,
    OtioBuffer *out_error);

/**
 * Clears the span of media a reference says is available.
 */
OtioStatus otio_media_reference_clear_available_range(
    OtioDocument *target,
    OtioNode node_handle,
    OtioBuffer *out_error);

/**
 * Returns the image bounds a reference says its media has.
 */
OtioStatus otio_media_reference_available_image_bounds(
    const OtioDocument *source,
    OtioNode node_handle,
    OtioBox2d *out_bounds,
    OtioBuffer *out_error);

/**
 * Sets the image bounds a reference says its media has.
 */
OtioStatus otio_media_reference_set_available_image_bounds(
    OtioDocument *target,
    OtioNode node_handle,
    OtioBox2d bounds,
    OtioBuffer *out_error);

/**
 * Clears the image bounds a reference says its media has.
 */
OtioStatus otio_media_reference_clear_available_image_bounds(
    OtioDocument *target,
    OtioNode node_handle,
    OtioBuffer *out_error);

/**
 * Returns where an external reference's media lives.
 */
OtioStatus otio_external_reference_target_url(
    const OtioDocument *source,
    OtioNode node_handle,
    OtioBuffer *out_url,
    OtioBuffer *out_error);

/**
 * Sets where an external reference's media lives.
 */
OtioStatus otio_external_reference_set_target_url(
    OtioDocument *target,
    OtioNode node_handle,
    const char *url,
    OtioBuffer *out_error);

/**
 * Returns which generator a generator reference names, such as
 * `"SMPTEBars"`.
 */
OtioStatus otio_generator_reference_kind(
    const OtioDocument *source,
    OtioNode node_handle,
    OtioBuffer *out_kind,
    OtioBuffer *out_error);

/**
 * Sets which generator a generator reference names.
 */
OtioStatus otio_generator_reference_set_kind(
    OtioDocument *target,
    OtioNode node_handle,
    const char *kind,
    OtioBuffer *out_error);

/**
 * Returns the numbers describing how an image sequence is laid out.
 */
OtioStatus otio_image_sequence_reference_numbers(
    const OtioDocument *source,
    OtioNode node_handle,
    OtioImageSequence *out_numbers,
    OtioBuffer *out_error);

/**
 * Sets the numbers describing how an image sequence is laid out.
 */
OtioStatus otio_image_sequence_reference_set_numbers(
    OtioDocument *target,
    OtioNode node_handle,
    OtioImageSequence numbers,
    OtioBuffer *out_error);

/**
 * Returns the directory an image sequence's frames sit in.
 */
OtioStatus otio_image_sequence_reference_target_url_base(
    const OtioDocument *source,
    OtioNode node_handle,
    OtioBuffer *out_url_base,
    OtioBuffer *out_error);

/**
 * Sets the directory an image sequence's frames sit in.
 */
OtioStatus otio_image_sequence_reference_set_target_url_base(
    OtioDocument *target,
    OtioNode node_handle,
    const char *url_base,
    OtioBuffer *out_error);

/**
 * Returns the part of each frame's filename before the frame number.
 */
OtioStatus otio_image_sequence_reference_name_prefix(
    const OtioDocument *source,
    OtioNode node_handle,
    OtioBuffer *out_prefix,
    OtioBuffer *out_error);

/**
 * Sets the part of each frame's filename before the frame number.
 */
OtioStatus otio_image_sequence_reference_set_name_prefix(
    OtioDocument *target,
    OtioNode node_handle,
    const char *prefix,
    OtioBuffer *out_error);

/**
 * Returns the part of each frame's filename after the frame number.
 */
OtioStatus otio_image_sequence_reference_name_suffix(
    const OtioDocument *source,
    OtioNode node_handle,
    OtioBuffer *out_suffix,
    OtioBuffer *out_error);

/**
 * Sets the part of each frame's filename after the frame number.
 */
OtioStatus otio_image_sequence_reference_set_name_suffix(
    OtioDocument *target,
    OtioNode node_handle,
    const char *suffix,
    OtioBuffer *out_error);

/* ===================================================================== *
 * Metadata
 * ===================================================================== */
/**
 * Returns what kind of value sits at a path.
 *
 * An empty or null path names the metadata dictionary itself, which is always
 * `OTIO_VALUE_DICTIONARY`.
 */
OtioStatus otio_metadata_kind(
    const OtioDocument *source,
    OtioNode node_handle,
    const char *path,
    OtioValueKind *out_kind,
    OtioBuffer *out_error);

/**
 * Returns whether anything sits at a path.
 */
OtioStatus otio_metadata_contains(
    const OtioDocument *source,
    OtioNode node_handle,
    const char *path,
    bool *out_contains,
    OtioBuffer *out_error);

/**
 * Returns how many entries a dictionary or an array at a path holds.
 */
OtioStatus otio_metadata_len(
    const OtioDocument *source,
    OtioNode node_handle,
    const char *path,
    size_t *out_len,
    OtioBuffer *out_error);

/**
 * Returns the key at an index of a dictionary at a path.
 *
 * Keys are ordered, so walking the indices walks them the same way twice.
 */
OtioStatus otio_metadata_key_at(
    const OtioDocument *source,
    OtioNode node_handle,
    const char *path,
    size_t index,
    OtioBuffer *out_key,
    OtioBuffer *out_error);

/**
 * Reads a boolean from the metadata.
 */
OtioStatus otio_metadata_get_bool(
    const OtioDocument *source,
    OtioNode node_handle,
    const char *path,
    bool *out_value,
    OtioBuffer *out_error);

/**
 * Reads a signed integer from the metadata.
 */
OtioStatus otio_metadata_get_int(
    const OtioDocument *source,
    OtioNode node_handle,
    const char *path,
    int64_t *out_value,
    OtioBuffer *out_error);

/**
 * Reads an unsigned integer from the metadata.
 */
OtioStatus otio_metadata_get_uint(
    const OtioDocument *source,
    OtioNode node_handle,
    const char *path,
    uint64_t *out_value,
    OtioBuffer *out_error);

/**
 * Reads a number from the metadata.
 */
OtioStatus otio_metadata_get_double(
    const OtioDocument *source,
    OtioNode node_handle,
    const char *path,
    double *out_value,
    OtioBuffer *out_error);

/**
 * Reads a time from the metadata.
 */
OtioStatus otio_metadata_get_rational_time(
    const OtioDocument *source,
    OtioNode node_handle,
    const char *path,
    OtioRationalTime *out_value,
    OtioBuffer *out_error);

/**
 * Reads a span from the metadata.
 */
OtioStatus otio_metadata_get_time_range(
    const OtioDocument *source,
    OtioNode node_handle,
    const char *path,
    OtioTimeRange *out_value,
    OtioBuffer *out_error);

/**
 * Reads a transform from the metadata.
 */
OtioStatus otio_metadata_get_time_transform(
    const OtioDocument *source,
    OtioNode node_handle,
    const char *path,
    OtioTimeTransform *out_value,
    OtioBuffer *out_error);

/**
 * Reads a point from the metadata.
 */
OtioStatus otio_metadata_get_v2d(
    const OtioDocument *source,
    OtioNode node_handle,
    const char *path,
    OtioV2d *out_value,
    OtioBuffer *out_error);

/**
 * Reads a rectangle from the metadata.
 */
OtioStatus otio_metadata_get_box2d(
    const OtioDocument *source,
    OtioNode node_handle,
    const char *path,
    OtioBox2d *out_value,
    OtioBuffer *out_error);

/**
 * Reads a handle to an OTIO object held in the metadata.
 */
OtioStatus otio_metadata_get_object(
    const OtioDocument *source,
    OtioNode node_handle,
    const char *path,
    OtioNode *out_value,
    OtioBuffer *out_error);

/**
 * Reads a string from the metadata.
 */
OtioStatus otio_metadata_get_string(
    const OtioDocument *source,
    OtioNode node_handle,
    const char *path,
    OtioBuffer *out_value,
    OtioBuffer *out_error);

/**
 * Reads a colour, and the name that goes with it, from the metadata.
 *
 * `out_name` may be null if the name is not wanted.
 */
OtioStatus otio_metadata_get_color(
    const OtioDocument *source,
    OtioNode node_handle,
    const char *path,
    OtioColor *out_value,
    OtioBuffer *out_name,
    OtioBuffer *out_error);

/**
 * Writes a boolean into the metadata.
 */
OtioStatus otio_metadata_set_bool(
    OtioDocument *target,
    OtioNode node_handle,
    const char *path,
    bool value,
    OtioBuffer *out_error);

/**
 * Writes a signed integer into the metadata.
 */
OtioStatus otio_metadata_set_int(
    OtioDocument *target,
    OtioNode node_handle,
    const char *path,
    int64_t value,
    OtioBuffer *out_error);

/**
 * Writes an unsigned integer into the metadata.
 */
OtioStatus otio_metadata_set_uint(
    OtioDocument *target,
    OtioNode node_handle,
    const char *path,
    uint64_t value,
    OtioBuffer *out_error);

/**
 * Writes a number into the metadata.
 */
OtioStatus otio_metadata_set_double(
    OtioDocument *target,
    OtioNode node_handle,
    const char *path,
    double value,
    OtioBuffer *out_error);

/**
 * Writes a time into the metadata.
 */
OtioStatus otio_metadata_set_rational_time(
    OtioDocument *target,
    OtioNode node_handle,
    const char *path,
    OtioRationalTime value,
    OtioBuffer *out_error);

/**
 * Writes a span into the metadata.
 */
OtioStatus otio_metadata_set_time_range(
    OtioDocument *target,
    OtioNode node_handle,
    const char *path,
    OtioTimeRange value,
    OtioBuffer *out_error);

/**
 * Writes a transform into the metadata.
 */
OtioStatus otio_metadata_set_time_transform(
    OtioDocument *target,
    OtioNode node_handle,
    const char *path,
    OtioTimeTransform value,
    OtioBuffer *out_error);

/**
 * Writes a point into the metadata.
 */
OtioStatus otio_metadata_set_v2d(
    OtioDocument *target,
    OtioNode node_handle,
    const char *path,
    OtioV2d value,
    OtioBuffer *out_error);

/**
 * Writes a rectangle into the metadata.
 */
OtioStatus otio_metadata_set_box2d(
    OtioDocument *target,
    OtioNode node_handle,
    const char *path,
    OtioBox2d value,
    OtioBuffer *out_error);

/**
 * Writes a handle to an OTIO object into the metadata.
 *
 * The object stays where it is and the metadata names it. Nothing checks
 * that the handle is live, because a document being built may not hold the
 * object yet.
 */
OtioStatus otio_metadata_set_object(
    OtioDocument *target,
    OtioNode node_handle,
    const char *path,
    OtioNode value,
    OtioBuffer *out_error);

/**
 * Writes a string into the metadata.
 */
OtioStatus otio_metadata_set_string(
    OtioDocument *target,
    OtioNode node_handle,
    const char *path,
    const char *value,
    OtioBuffer *out_error);

/**
 * Writes a colour into the metadata. `name` may be null for an unnamed one.
 */
OtioStatus otio_metadata_set_color(
    OtioDocument *target,
    OtioNode node_handle,
    const char *path,
    OtioColor value,
    const char *name,
    OtioBuffer *out_error);

/**
 * Writes a null into the metadata.
 */
OtioStatus otio_metadata_set_null(
    OtioDocument *target,
    OtioNode node_handle,
    const char *path,
    OtioBuffer *out_error);

/**
 * Writes an empty dictionary into the metadata, to be filled through deeper
 * paths.
 */
OtioStatus otio_metadata_set_dictionary(
    OtioDocument *target,
    OtioNode node_handle,
    const char *path,
    OtioBuffer *out_error);

/**
 * Writes an array of `len` nulls into the metadata, to be filled by index.
 */
OtioStatus otio_metadata_set_vector(
    OtioDocument *target,
    OtioNode node_handle,
    const char *path,
    size_t len,
    OtioBuffer *out_error);

/**
 * Removes whatever sits at a path.
 *
 * Removing a key of a dictionary takes the key with it; removing an element
 * of an array shortens the array.
 */
OtioStatus otio_metadata_remove(
    OtioDocument *target,
    OtioNode node_handle,
    const char *path,
    OtioBuffer *out_error);

/**
 * Empties an object's metadata.
 */
OtioStatus otio_metadata_clear(
    OtioDocument *target,
    OtioNode node_handle,
    OtioBuffer *out_error);

/* ===================================================================== *
 * The tree, and where things sit in time
 * ===================================================================== */
/**
 * Returns how many children an object holds.
 *
 * Tracks, stacks, bare compositions and serializable collections hold
 * children; anything else reports `OTIO_STATUS_CORE_ERROR`.
 */
OtioStatus otio_node_child_count(
    const OtioDocument *source,
    OtioNode parent,
    size_t *out_count,
    OtioBuffer *out_error);

/**
 * Returns one of an object's children.
 */
OtioStatus otio_node_child_at(
    const OtioDocument *source,
    OtioNode parent,
    size_t index,
    OtioNode *out_child,
    OtioBuffer *out_error);

/**
 * Returns an object's children.
 */
OtioStatus otio_node_children(
    const OtioDocument *source,
    OtioNode parent,
    OtioNode *out_nodes,
    size_t capacity,
    size_t *out_count,
    OtioBuffer *out_error);

/**
 * Adds a child to a composition at an index.
 *
 * A negative index counts from the end, as upstream's Python does. The child
 * must not already be in a composition.
 */
OtioStatus otio_composition_insert_child(
    OtioDocument *target,
    OtioNode parent,
    int64_t index,
    OtioNode child,
    OtioBuffer *out_error);

/**
 * Adds a child to the end of a composition.
 */
OtioStatus otio_composition_append_child(
    OtioDocument *target,
    OtioNode parent,
    OtioNode child,
    OtioBuffer *out_error);

/**
 * Removes a child by index, and returns it.
 *
 * The child stays in the document with no parent.
 */
OtioStatus otio_composition_remove_child(
    OtioDocument *target,
    OtioNode parent,
    int64_t index,
    OtioNode *out_child,
    OtioBuffer *out_error);

/**
 * Removes a child by handle.
 */
OtioStatus otio_composition_detach_child(
    OtioDocument *target,
    OtioNode parent,
    OtioNode child,
    OtioBuffer *out_error);

/**
 * Removes every child of a composition, and returns them.
 */
OtioStatus otio_composition_clear_children(
    OtioDocument *target,
    OtioNode parent,
    OtioNode *out_nodes,
    size_t capacity,
    size_t *out_count,
    OtioBuffer *out_error);

/**
 * Returns where a child sits in its composition.
 */
OtioStatus otio_composition_index_of_child(
    const OtioDocument *source,
    OtioNode parent,
    OtioNode child,
    size_t *out_index,
    OtioBuffer *out_error);

/**
 * Returns whether a composition holds an object directly.
 */
OtioStatus otio_composition_has_child(
    const OtioDocument *source,
    OtioNode parent,
    OtioNode child,
    bool *out_has,
    OtioBuffer *out_error);

/**
 * Returns whether an object descends from a composition at any depth.
 */
OtioStatus otio_composition_is_parent_of(
    const OtioDocument *source,
    OtioNode parent,
    OtioNode other,
    bool *out_is,
    OtioBuffer *out_error);

/**
 * Returns the outermost object above this one.
 */
OtioStatus otio_node_highest_ancestor(
    const OtioDocument *source,
    OtioNode node_handle,
    OtioNode *out_ancestor,
    OtioBuffer *out_error);

/**
 * Returns every clip at or below an object, in order.
 */
OtioStatus otio_node_find_clips(
    const OtioDocument *source,
    OtioNode node_handle,
    OtioNode *out_nodes,
    size_t capacity,
    size_t *out_count,
    OtioBuffer *out_error);

/**
 * Returns every object of a kind at or below a composition.
 *
 * A null `search_range` searches all of it. `shallow` stops the walk at the
 * composition's own children.
 */
OtioStatus otio_composition_find_children_of_kind(
    const OtioDocument *source,
    OtioNode parent,
    OtioNodeKind kind,
    const OtioTimeRange *search_range,
    bool shallow,
    OtioNode *out_nodes,
    size_t capacity,
    size_t *out_count,
    OtioBuffer *out_error);

/**
 * Returns how long an object occupies its parent's timeline.
 */
OtioStatus otio_item_duration(
    const OtioDocument *source,
    OtioNode node_handle,
    OtioRationalTime *out_duration,
    OtioBuffer *out_error);

/**
 * Returns the span of media an object could draw on, before trimming.
 */
OtioStatus otio_item_available_range(
    const OtioDocument *source,
    OtioNode node_handle,
    OtioTimeRange *out_range,
    OtioBuffer *out_error);

/**
 * Returns the span of media an object uses, in its own clock.
 */
OtioStatus otio_item_trimmed_range(
    const OtioDocument *source,
    OtioNode node_handle,
    OtioTimeRange *out_range,
    OtioBuffer *out_error);

/**
 * Returns the span of media an object shows, including what its transitions
 * reach into.
 */
OtioStatus otio_item_visible_range(
    const OtioDocument *source,
    OtioNode node_handle,
    OtioTimeRange *out_range,
    OtioBuffer *out_error);

/**
 * Returns where an object sits in its parent's clock.
 */
OtioStatus otio_item_range_in_parent(
    const OtioDocument *source,
    OtioNode node_handle,
    OtioTimeRange *out_range,
    OtioBuffer *out_error);

/**
 * Returns where an object sits in its parent's clock, after the parent's own
 * trim.
 *
 * Reports `OTIO_STATUS_NO_VALUE` when the parent's trim excludes the object
 * entirely.
 */
OtioStatus otio_item_trimmed_range_in_parent(
    const OtioDocument *source,
    OtioNode node_handle,
    OtioTimeRange *out_range,
    OtioBuffer *out_error);

/**
 * Returns where the child at an index sits in its composition's clock.
 *
 * A negative index counts from the end.
 */
OtioStatus otio_composition_range_of_child_at_index(
    const OtioDocument *source,
    OtioNode parent,
    int64_t index,
    OtioTimeRange *out_range,
    OtioBuffer *out_error);

/**
 * Returns where the child at an index sits, after the composition's own trim.
 */
OtioStatus otio_composition_trimmed_range_of_child_at_index(
    const OtioDocument *source,
    OtioNode parent,
    int64_t index,
    OtioTimeRange *out_range,
    OtioBuffer *out_error);

/**
 * Returns where a child sits in a composition's clock, at any depth.
 */
OtioStatus otio_composition_range_of_child(
    const OtioDocument *source,
    OtioNode parent,
    OtioNode child,
    OtioTimeRange *out_range,
    OtioBuffer *out_error);

/**
 * Returns where a child sits after the composition's trim, at any depth.
 *
 * Reports `OTIO_STATUS_NO_VALUE` when the trim excludes it entirely.
 */
OtioStatus otio_composition_trimmed_range_of_child(
    const OtioDocument *source,
    OtioNode parent,
    OtioNode child,
    OtioTimeRange *out_range,
    OtioBuffer *out_error);

/**
 * Clips a range to a composition's own trim.
 *
 * Reports `OTIO_STATUS_NO_VALUE` when nothing of it is left.
 */
OtioStatus otio_composition_trim_child_range(
    const OtioDocument *source,
    OtioNode parent,
    OtioTimeRange child_range,
    OtioTimeRange *out_range,
    OtioBuffer *out_error);

/**
 * Returns where every child of a composition sits, in one pass.
 *
 * `out_nodes` and `out_ranges` are filled in step, so entry `i` of one goes
 * with entry `i` of the other. Either may be null when only the count is
 * wanted.
 */
OtioStatus otio_composition_ranges_of_children(
    const OtioDocument *source,
    OtioNode parent,
    OtioNode *out_nodes,
    OtioTimeRange *out_ranges,
    size_t capacity,
    size_t *out_count,
    OtioBuffer *out_error);

/**
 * Returns the child of a composition that covers an instant.
 *
 * `shallow` stops at the composition's own children rather than descending
 * into nested ones. Reports `OTIO_STATUS_NO_VALUE` when nothing covers it.
 */
OtioStatus otio_composition_child_at_time(
    const OtioDocument *source,
    OtioNode parent,
    OtioRationalTime time,
    bool shallow,
    OtioNode *out_child,
    OtioBuffer *out_error);

/**
 * Returns the children of a composition that touch a span.
 */
OtioStatus otio_composition_children_in_range(
    const OtioDocument *source,
    OtioNode parent,
    OtioTimeRange search_range,
    OtioNode *out_nodes,
    size_t capacity,
    size_t *out_count,
    OtioBuffer *out_error);

/**
 * Returns how much unused media a child has on each side.
 */
OtioStatus otio_composition_handles_of_child(
    const OtioDocument *source,
    OtioNode parent,
    OtioNode child,
    OtioHandles *out_handles,
    OtioBuffer *out_error);

/**
 * Returns the children on either side of one, or
 * `otio_node_none` where there is none.
 *
 * With `OTIO_NEIGHBOR_GAP_AROUND_TRANSITIONS`, a transition at the head or
 * tail of a track gets a gap materialized for its overhang, which is why this
 * takes a document it may edit.
 */
OtioStatus otio_composition_neighbors_of(
    OtioDocument *target,
    OtioNode parent,
    OtioNode child,
    OtioNeighborGapPolicy policy,
    OtioNode *out_before,
    OtioNode *out_after,
    OtioBuffer *out_error);

/**
 * Restates an instant from one object's clock in another's.
 */
OtioStatus otio_node_transformed_time(
    const OtioDocument *source,
    OtioRationalTime time,
    OtioNode from,
    OtioNode to,
    OtioRationalTime *out_time,
    OtioBuffer *out_error);

/**
 * Restates a span from one object's clock in another's.
 */
OtioStatus otio_node_transformed_time_range(
    const OtioDocument *source,
    OtioTimeRange range,
    OtioNode from,
    OtioNode to,
    OtioTimeRange *out_range,
    OtioBuffer *out_error);

/* ===================================================================== *
 * The edit operations
 * ===================================================================== */
/**
 * Lays an item over a span of a composition, replacing what was there.
 *
 * `fill_template` is the item to fill any gap the edit opens with, or
 * `otio_node_none` for a plain gap.
 */
OtioStatus otio_edit_overwrite(
    OtioDocument *target,
    OtioNode item,
    OtioNode composition,
    OtioTimeRange range,
    bool remove_transitions,
    OtioNode fill_template,
    OtioBuffer *out_error);

/**
 * Inserts an item at an instant, pushing what follows later.
 */
OtioStatus otio_edit_insert(
    OtioDocument *target,
    OtioNode item,
    OtioNode composition,
    OtioRationalTime time,
    bool remove_transitions,
    OtioNode fill_template,
    OtioBuffer *out_error);

/**
 * Moves an item's in and out points without moving its neighbours.
 */
OtioStatus otio_edit_trim(
    OtioDocument *target,
    OtioNode item,
    OtioRationalTime delta_in,
    OtioRationalTime delta_out,
    OtioNode fill_template,
    OtioBuffer *out_error);

/**
 * Cuts whatever sits at an instant into two.
 */
OtioStatus otio_edit_slice(
    OtioDocument *target,
    OtioNode composition,
    OtioRationalTime time,
    bool remove_transitions,
    OtioBuffer *out_error);

/**
 * Moves the media inside an item without moving the item.
 */
OtioStatus otio_edit_slip(
    OtioDocument *target,
    OtioNode item,
    OtioRationalTime delta,
    OtioBuffer *out_error);

/**
 * Moves an item along its track, taking the time from its neighbours.
 */
OtioStatus otio_edit_slide(
    OtioDocument *target,
    OtioNode item,
    OtioRationalTime delta,
    OtioBuffer *out_error);

/**
 * Moves an item's in and out points, sliding everything after it.
 */
OtioStatus otio_edit_ripple(
    OtioDocument *target,
    OtioNode item,
    OtioRationalTime delta_in,
    OtioRationalTime delta_out,
    OtioBuffer *out_error);

/**
 * Moves the cut between an item and its neighbour.
 */
OtioStatus otio_edit_roll(
    OtioDocument *target,
    OtioNode item,
    OtioRationalTime delta_in,
    OtioRationalTime delta_out,
    OtioBuffer *out_error);

/**
 * Drops an item into a gap on a track, fitting it as the reference point
 * says.
 */
OtioStatus otio_edit_fill(
    OtioDocument *target,
    OtioNode item,
    OtioNode track,
    OtioRationalTime track_time,
    OtioReferencePoint reference_point,
    OtioBuffer *out_error);

/**
 * Takes whatever sits at an instant out of a composition.
 *
 * With `fill` set, a gap takes its place; without, what follows moves up.
 */
OtioStatus otio_edit_remove(
    OtioDocument *target,
    OtioNode composition,
    OtioRationalTime time,
    bool fill,
    OtioNode fill_template,
    OtioBuffer *out_error);

/* ===================================================================== *
 * Algorithms
 * ===================================================================== */
/**
 * Returns a copy of a track holding only what falls inside a span.
 *
 * The copy is added to the same document and has no parent.
 */
OtioStatus otio_algorithm_track_trimmed_to_range(
    OtioDocument *target,
    OtioNode track,
    OtioTimeRange trim_range,
    OtioNode *out_track,
    OtioBuffer *out_error);

/**
 * Collapses a stack's tracks into one, top layer winning where it is visible.
 */
OtioStatus otio_algorithm_flatten_stack(
    OtioDocument *target,
    OtioNode stack,
    OtioNode *out_track,
    OtioBuffer *out_error);

/**
 * Collapses a list of tracks into one, lowest first.
 */
OtioStatus otio_algorithm_flatten_tracks(
    OtioDocument *target,
    const OtioNode *tracks,
    size_t count,
    OtioNode *out_track,
    OtioBuffer *out_error);

/* ===================================================================== *
 * Times, spans and transforms
 * ===================================================================== */
/**
 * The tolerance, in seconds, that the range predicates use by default.
 *
 * Upstream's C++ takes this as a default argument; C has none, so the value
 * is a call rather than a constant in the header, which keeps the two from
 * drifting apart.
 */
double otio_default_epsilon_s(void);

/**
 * Returns whether a time is usable: both parts finite and the rate positive.
 */
bool otio_rational_time_is_valid(OtioRationalTime time);

/**
 * Returns the same instant expressed at another rate.
 */
OtioRationalTime otio_rational_time_rescaled_to(OtioRationalTime time, double rate);

/**
 * Returns the same instant expressed at another time's rate.
 */
OtioRationalTime otio_rational_time_rescaled_to_time(
    OtioRationalTime time,
    OtioRationalTime other);

/**
 * Returns what this time's value would be at another rate.
 */
double otio_rational_time_value_rescaled_to(OtioRationalTime time, double rate);

/**
 * Returns whether two times are within `delta` of each other.
 */
bool otio_rational_time_almost_equal(
    OtioRationalTime left,
    OtioRationalTime right,
    double delta);

/**
 * Returns whether two times are the same instant.
 *
 * Times at different rates that name the same instant compare equal; this is
 * upstream's `==`, not a field-by-field comparison. For that, use
 * [`otio_rational_time_strictly_equal`].
 */
bool otio_rational_time_equal(OtioRationalTime left, OtioRationalTime right);

/**
 * Returns whether two times have the same value and the same rate.
 */
bool otio_rational_time_strictly_equal(OtioRationalTime left, OtioRationalTime right);

/**
 * Orders two times: -1 if `left` is earlier, 1 if later, 0 if the same.
 *
 * A comparison involving a NaN has no answer, and reports 0.
 */
int32_t otio_rational_time_compare(OtioRationalTime left, OtioRationalTime right);

/**
 * Returns the sum of two times, at the higher of the two rates.
 */
OtioRationalTime otio_rational_time_add(OtioRationalTime left, OtioRationalTime right);

/**
 * Returns the difference of two times, at the higher of the two rates.
 */
OtioRationalTime otio_rational_time_subtract(
    OtioRationalTime left,
    OtioRationalTime right);

/**
 * Returns the time with its sign flipped.
 */
OtioRationalTime otio_rational_time_negate(OtioRationalTime time);

/**
 * Returns the time rounded towards negative infinity.
 */
OtioRationalTime otio_rational_time_floor(OtioRationalTime time);

/**
 * Returns the time rounded towards positive infinity.
 */
OtioRationalTime otio_rational_time_ceil(OtioRationalTime time);

/**
 * Returns the time rounded to the nearest whole value, halves away from zero.
 */
OtioRationalTime otio_rational_time_round(OtioRationalTime time);

/**
 * Returns how long it is from one instant to another, the end excluded.
 */
OtioRationalTime otio_rational_time_duration_from_start_end_time(
    OtioRationalTime start_time,
    OtioRationalTime end_time_exclusive);

/**
 * Returns how long it is from one instant to another, the end included.
 */
OtioRationalTime otio_rational_time_duration_from_start_end_time_inclusive(
    OtioRationalTime start_time,
    OtioRationalTime end_time_inclusive);

/**
 * Builds a time from a frame number at a rate.
 */
OtioRationalTime otio_rational_time_from_frames(double frame, double rate);

/**
 * Builds a time from a number of seconds at a rate.
 */
OtioRationalTime otio_rational_time_from_seconds_at_rate(double seconds, double rate);

/**
 * Builds a time from a number of seconds, at a rate of one.
 */
OtioRationalTime otio_rational_time_from_seconds(double seconds);

/**
 * Returns the frame number this time falls on.
 */
int32_t otio_rational_time_to_frames(OtioRationalTime time);

/**
 * Returns the frame number this time falls on at another rate.
 */
int32_t otio_rational_time_to_frames_at_rate(OtioRationalTime time, double rate);

/**
 * Returns the time in seconds.
 */
double otio_rational_time_to_seconds(OtioRationalTime time);

/**
 * Returns whether a rate is one SMPTE timecode is defined for.
 */
bool otio_is_smpte_timecode_rate(double rate);

/**
 * Returns the SMPTE timecode rate closest to a rate.
 */
double otio_nearest_smpte_timecode_rate(double rate);

/**
 * Returns whether a rate is a drop-frame rate.
 */
bool otio_is_drop_frame_rate(double rate);

/**
 * Reads a time from a `HH:MM:SS:FF` timecode at a rate.
 */
OtioStatus otio_rational_time_from_timecode(
    const char *timecode,
    double rate,
    OtioRationalTime *out_time,
    OtioBuffer *out_error);

/**
 * Reads a time from a `[-]HH:MM:SS.sss` time string at a rate.
 */
OtioStatus otio_rational_time_from_time_string(
    const char *time_string,
    double rate,
    OtioRationalTime *out_time,
    OtioBuffer *out_error);

/**
 * Writes a time as a timecode at its own rate.
 */
OtioStatus otio_rational_time_to_timecode(
    OtioRationalTime time,
    OtioBuffer *out_timecode,
    OtioBuffer *out_error);

/**
 * Writes a time as a timecode at a given rate and drop-frame setting.
 */
OtioStatus otio_rational_time_to_timecode_at(
    OtioRationalTime time,
    double rate,
    OtioDropFrame drop_frame,
    OtioBuffer *out_timecode,
    OtioBuffer *out_error);

/**
 * Writes a time as a timecode, rounding to the nearest frame first.
 */
OtioStatus otio_rational_time_to_nearest_timecode_at(
    OtioRationalTime time,
    double rate,
    OtioDropFrame drop_frame,
    OtioBuffer *out_timecode,
    OtioBuffer *out_error);

/**
 * Writes a time as a `HH:MM:SS.sss` time string.
 */
OtioStatus otio_rational_time_to_time_string(
    OtioRationalTime time,
    OtioBuffer *out_string,
    OtioBuffer *out_error);

/**
 * Builds a range from its start and the instant after its end.
 */
OtioTimeRange otio_time_range_from_start_end_time(
    OtioRationalTime start_time,
    OtioRationalTime end_time_exclusive);

/**
 * Builds a range from its start and its last instant.
 */
OtioTimeRange otio_time_range_from_start_end_time_inclusive(
    OtioRationalTime start_time,
    OtioRationalTime end_time_inclusive);

/**
 * Returns whether a range is usable: valid times and a duration of at least
 * zero.
 */
bool otio_time_range_is_valid(OtioTimeRange range);

/**
 * Returns the instant just after the range's end.
 */
OtioRationalTime otio_time_range_end_time_exclusive(OtioTimeRange range);

/**
 * Returns the last instant the range covers.
 */
OtioRationalTime otio_time_range_end_time_inclusive(OtioTimeRange range);

/**
 * Returns the range with its duration lengthened.
 */
OtioTimeRange otio_time_range_duration_extended_by(
    OtioTimeRange range,
    OtioRationalTime by);

/**
 * Returns the smallest range covering both of two ranges.
 */
OtioTimeRange otio_time_range_extended_by(OtioTimeRange range, OtioTimeRange other);

/**
 * Returns the instant pulled inside the range, if it lies outside it.
 */
OtioRationalTime otio_time_range_clamped_time(
    OtioTimeRange range,
    OtioRationalTime time);

/**
 * Returns the range pulled inside this one, where it lies outside it.
 */
OtioTimeRange otio_time_range_clamped_range(OtioTimeRange range, OtioTimeRange other);

/**
 * Returns whether an instant falls inside the range, the end excluded.
 */
bool otio_time_range_contains_time(OtioTimeRange range, OtioRationalTime time);

/**
 * Returns whether another range falls entirely inside this one.
 */
bool otio_time_range_contains_range(
    OtioTimeRange range,
    OtioTimeRange other,
    double epsilon_s);

/**
 * Returns whether an instant falls inside the range, the ends included.
 */
bool otio_time_range_overlaps_time(OtioTimeRange range, OtioRationalTime time);

/**
 * Returns whether two ranges share any time at all.
 */
bool otio_time_range_overlaps_range(
    OtioTimeRange range,
    OtioTimeRange other,
    double epsilon_s);

/**
 * Returns whether this range ends before another begins.
 */
bool otio_time_range_before_range(
    OtioTimeRange range,
    OtioTimeRange other,
    double epsilon_s);

/**
 * Returns whether this range ends before an instant.
 */
bool otio_time_range_before_time(
    OtioTimeRange range,
    OtioRationalTime time,
    double epsilon_s);

/**
 * Returns whether this range ends exactly where another begins.
 */
bool otio_time_range_meets(OtioTimeRange range, OtioTimeRange other, double epsilon_s);

/**
 * Returns whether another range starts where this one does.
 */
bool otio_time_range_begins_range(
    OtioTimeRange range,
    OtioTimeRange other,
    double epsilon_s);

/**
 * Returns whether this range starts at an instant.
 */
bool otio_time_range_begins_time(
    OtioTimeRange range,
    OtioRationalTime time,
    double epsilon_s);

/**
 * Returns whether another range ends where this one does.
 */
bool otio_time_range_finishes_range(
    OtioTimeRange range,
    OtioTimeRange other,
    double epsilon_s);

/**
 * Returns whether this range ends at an instant.
 */
bool otio_time_range_finishes_time(
    OtioTimeRange range,
    OtioRationalTime time,
    double epsilon_s);

/**
 * Returns whether two ranges share more than a single boundary instant.
 */
bool otio_time_range_intersects(
    OtioTimeRange range,
    OtioTimeRange other,
    double epsilon_s);

/**
 * Returns the transform applied to an instant.
 */
OtioRationalTime otio_time_transform_applied_to_time(
    OtioTimeTransform transform,
    OtioRationalTime time);

/**
 * Returns the transform applied to a span.
 */
OtioTimeRange otio_time_transform_applied_to_range(
    OtioTimeTransform transform,
    OtioTimeRange range);

/**
 * Returns the transform applied to another transform.
 */
OtioTimeTransform otio_time_transform_applied_to_transform(
    OtioTimeTransform transform,
    OtioTimeTransform other);

/* ===================================================================== *
 * Reading and writing the interchange formats
 * ===================================================================== */
/**
 * Returns the defaults, for a caller that wants to change one field.
 */
OtioReadOptions otio_read_options_default(void);

/**
 * Returns the defaults, for a caller that wants to change one field.
 */
OtioWriteOptions otio_write_options_default(void);

/**
 * Returns an adapter's name, as upstream's plugin manifest spells it.
 *
 * The string is static and needs no freeing.
 */
const char *otio_format_name(OtioFormat format);

/**
 * Returns the format that claims a filename suffix, such as `"edl"`.
 *
 * The suffix is matched without its dot and without regard to case. Reports
 * `OTIO_STATUS_NO_VALUE` for a suffix no format claims.
 */
OtioStatus otio_format_from_suffix(
    const char *suffix,
    OtioFormat *out_format,
    OtioBuffer *out_error);

/**
 * Reads a document from the bytes of a file in some format.
 *
 * `options` may be null for the format's usual behaviour.
 */
OtioStatus otio_read_from_bytes(
    OtioFormat format,
    const uint8_t *data,
    size_t len,
    const OtioReadOptions *options,
    OtioDocument **out_document,
    OtioBuffer *out_error);

/**
 * Reads a document from a file on disk in some format.
 */
OtioStatus otio_read_from_file(
    OtioFormat format,
    const char *path,
    const OtioReadOptions *options,
    OtioDocument **out_document,
    OtioBuffer *out_error);

/**
 * Writes a document as the bytes of a file in some format.
 *
 * The buffer is NUL-terminated, so a text format's output can be used as a C
 * string; `len` is what matters for a binary one.
 */
OtioStatus otio_write_to_bytes(
    OtioFormat format,
    const OtioDocument *source,
    const OtioWriteOptions *options,
    OtioBuffer *out_bytes,
    OtioBuffer *out_error);

/**
 * Writes a document to a file on disk in some format.
 */
OtioStatus otio_write_to_file(
    OtioFormat format,
    const OtioDocument *source,
    const char *path,
    const OtioWriteOptions *options,
    OtioBuffer *out_error);


#ifdef __cplusplus
}
#endif

#endif /* OTIO_H */
