//! A C ABI for the otio-rust core.
//!
//! This crate builds `libotio`, a shared and a static library whose interface
//! is the C header in [`include/otio.h`](../../include/otio.h). It is the
//! layer every language binding other than Python is meant to sit on: a
//! language with a C FFI can drive the whole data model, the composition
//! algorithms, the ten edit operations and the file-format adapters without
//! knowing anything about Rust.
//!
//! # The shape of the interface
//!
//! - **Objects are handles, not structs.** A caller holds an [`OtioNode`] —
//!   the arena index and generation of `otio-core`'s `NodeId`, spelled for C
//!   — and asks its [`OtioDocument`] about it. Nothing hands out a pointer
//!   into the arena, so an edit that moves objects around cannot leave a
//!   caller holding a dangling one. This is what
//!   `docs/adr/0001-ownership-model.md` anticipated when it chose the arena.
//! - **Values are plain structs.** A `RationalTime` is two doubles, so
//!   [`OtioRationalTime`] crosses by value. Only the things with identity go
//!   behind handles.
//! - **Failure is a status and a message.** Every call that can fail returns
//!   an [`OtioStatus`], delivers its result through out-parameters, and
//!   takes one more last, `out_error`, where it writes the sentence
//!   describing the failure. The message comes back from the call that
//!   failed, so no caller has to keep two calls on one thread to read it.
//!   `out_error` may be null; when it is not, it is written on every return,
//!   empty after success, and the caller frees it.
//!   `OTIO_STATUS_NO_VALUE` means the question has an answer and the answer
//!   is "nothing", which is not the same as an error.
//! - **Everything of variable length is an owned buffer.** A string or a
//!   written file comes back as an [`OtioBuffer`] the caller releases with
//!   [`otio_buffer_free`].
//!
//! # The contract every entry point assumes
//!
//! This is stated once here rather than repeated on two hundred functions.
//! Any call that breaks it is undefined behaviour, which is why the whole
//! interface is `unsafe` from Rust.
//!
//! 1. A pointer argument is either null, where the call's documentation says
//!    null is allowed, or a valid, aligned pointer to an initialized value of
//!    the stated type, writable if the parameter is named `out_`.
//! 2. A `const char *` is a NUL-terminated string in UTF-8.
//! 3. An [`OtioDocument`] pointer came from a call in this library that
//!    produced one and has not been passed to [`otio_document_free`].
//! 4. An [`OtioBuffer`] is released once, with [`otio_buffer_free`], and not
//!    read afterwards.
//! 5. A document is not internally synchronized. Several threads may read one
//!    at the same time; a thread that edits one must be the only thread
//!    touching it. Documents are independent, so two threads working on two
//!    documents never interfere.
//!
//! A panic inside the core is caught at the boundary and reported as
//! `OTIO_STATUS_PANIC` rather than unwinding into C.
//!
//! # A whole session, in C
//!
//! ```c
//! OtioDocument *document = NULL;
//! OtioBuffer error;
//! if (otio_read_from_file(OTIO_FORMAT_CMX_3600, "cut.edl", NULL, &document, &error)) {
//!     fprintf(stderr, "%s\n", error.data);
//!     otio_buffer_free(error);
//!     return 1;
//! }
//!
//! OtioNode timeline;
//! otio_document_root(document, &timeline, NULL);
//!
//! size_t count = 0;
//! otio_node_find_clips(document, timeline, NULL, 0, &count, NULL);
//! printf("%zu clips\n", count);
//!
//! otio_write_to_file(OTIO_FORMAT_OTIO_JSON, document, "cut.otio", NULL, NULL);
//! otio_document_free(document);
//! ```

// Every function here is an `unsafe extern "C"` entry point whose contract is
// the numbered list above, which is the whole crate's contract rather than a
// different one per function. Repeating it two hundred times as a `# Safety`
// section would bury the part of each doc comment that says what the call
// actually does.
#![allow(clippy::missing_safety_doc)]

mod adapter;
mod algorithm;
mod buffer;
mod composition;
mod document;
mod edit;
mod handle;
mod metadata;
mod node;
mod status;
mod time;
mod value;

pub use adapter::{
    OtioBundleMediaPolicy, OtioEdlStyle, OtioFormat, OtioReadOptions, OtioWriteOptions,
    otio_format_from_suffix, otio_format_name, otio_read_from_bytes, otio_read_from_file,
    otio_read_options_default, otio_write_options_default, otio_write_to_bytes, otio_write_to_file,
};
pub use algorithm::{
    otio_algorithm_flatten_stack, otio_algorithm_flatten_tracks,
    otio_algorithm_track_trimmed_to_range,
};
pub use buffer::{OtioBuffer, otio_buffer_free};
pub use composition::{
    OtioHandles, OtioNeighborGapPolicy, otio_composition_append_child,
    otio_composition_child_at_time, otio_composition_children_in_range,
    otio_composition_clear_children, otio_composition_detach_child,
    otio_composition_find_children_of_kind, otio_composition_handles_of_child,
    otio_composition_has_child, otio_composition_index_of_child, otio_composition_insert_child,
    otio_composition_is_parent_of, otio_composition_neighbors_of, otio_composition_range_of_child,
    otio_composition_range_of_child_at_index, otio_composition_ranges_of_children,
    otio_composition_remove_child, otio_composition_trim_child_range,
    otio_composition_trimmed_range_of_child, otio_composition_trimmed_range_of_child_at_index,
    otio_item_available_range, otio_item_duration, otio_item_range_in_parent,
    otio_item_trimmed_range, otio_item_trimmed_range_in_parent, otio_item_visible_range,
    otio_node_child_at, otio_node_child_count, otio_node_children, otio_node_find_clips,
    otio_node_highest_ancestor, otio_node_transformed_time, otio_node_transformed_time_range,
};
pub use document::{
    otio_default_indent, otio_document_absorb, otio_document_clone, otio_document_contains,
    otio_document_deep_clone, otio_document_free, otio_document_from_json, otio_document_new,
    otio_document_node_count, otio_document_read_from_file, otio_document_remove,
    otio_document_remove_recursive, otio_document_root, otio_document_set_root,
    otio_document_to_json, otio_document_write_to_file, otio_node_to_json,
};
pub use edit::{
    OtioReferencePoint, otio_edit_fill, otio_edit_insert, otio_edit_overwrite, otio_edit_remove,
    otio_edit_ripple, otio_edit_roll, otio_edit_slice, otio_edit_slide, otio_edit_slip,
    otio_edit_trim,
};
pub use handle::{OtioDocument, OtioNode, otio_node_equal, otio_node_is_none, otio_node_none};
pub use metadata::{
    OtioValueKind, otio_metadata_clear, otio_metadata_contains, otio_metadata_get_bool,
    otio_metadata_get_box2d, otio_metadata_get_color, otio_metadata_get_double,
    otio_metadata_get_int, otio_metadata_get_object, otio_metadata_get_rational_time,
    otio_metadata_get_string, otio_metadata_get_time_range, otio_metadata_get_time_transform,
    otio_metadata_get_uint, otio_metadata_get_v2d, otio_metadata_key_at, otio_metadata_kind,
    otio_metadata_len, otio_metadata_remove, otio_metadata_set_bool, otio_metadata_set_box2d,
    otio_metadata_set_color, otio_metadata_set_dictionary, otio_metadata_set_double,
    otio_metadata_set_int, otio_metadata_set_null, otio_metadata_set_object,
    otio_metadata_set_rational_time, otio_metadata_set_string, otio_metadata_set_time_range,
    otio_metadata_set_time_transform, otio_metadata_set_uint, otio_metadata_set_v2d,
    otio_metadata_set_vector,
};
pub use node::{
    OtioImageSequence, OtioMissingFramePolicy, OtioNodeKind, otio_clip_active_media_reference_key,
    otio_clip_media_reference, otio_clip_media_reference_count, otio_clip_media_reference_key_at,
    otio_clip_new, otio_clip_remove_media_reference, otio_clip_set_active_media_reference_key,
    otio_clip_set_media_reference, otio_composable_new, otio_composition_new,
    otio_effect_effect_name, otio_effect_enabled, otio_effect_new, otio_effect_set_effect_name,
    otio_effect_set_enabled, otio_effect_set_time_scalar, otio_effect_time_scalar,
    otio_external_reference_new, otio_external_reference_set_target_url,
    otio_external_reference_target_url, otio_freeze_frame_new, otio_gap_new,
    otio_generator_reference_kind, otio_generator_reference_new, otio_generator_reference_set_kind,
    otio_image_sequence_reference_name_prefix, otio_image_sequence_reference_name_suffix,
    otio_image_sequence_reference_new, otio_image_sequence_reference_numbers,
    otio_image_sequence_reference_set_name_prefix, otio_image_sequence_reference_set_name_suffix,
    otio_image_sequence_reference_set_numbers, otio_image_sequence_reference_set_target_url_base,
    otio_image_sequence_reference_target_url_base, otio_item_append_effect,
    otio_item_append_marker, otio_item_clear_color, otio_item_clear_source_range, otio_item_color,
    otio_item_effect_at, otio_item_effect_count, otio_item_enabled, otio_item_marker_at,
    otio_item_marker_count, otio_item_new, otio_item_remove_effect, otio_item_remove_marker,
    otio_item_set_color, otio_item_set_enabled, otio_item_set_source_range, otio_item_source_range,
    otio_linear_time_warp_new, otio_marker_color, otio_marker_comment, otio_marker_marked_range,
    otio_marker_new, otio_marker_set_color, otio_marker_set_comment, otio_marker_set_marked_range,
    otio_media_reference_available_image_bounds, otio_media_reference_available_range,
    otio_media_reference_clear_available_image_bounds, otio_media_reference_clear_available_range,
    otio_media_reference_set_available_image_bounds, otio_media_reference_set_available_range,
    otio_missing_reference_new, otio_node_kind, otio_node_name, otio_node_overlapping,
    otio_node_parent, otio_node_schema_name, otio_node_schema_version, otio_node_set_name,
    otio_node_visible, otio_serializable_collection_new, otio_stack_new, otio_time_effect_new,
    otio_timeline_clear_global_start_time, otio_timeline_global_start_time, otio_timeline_new,
    otio_timeline_set_global_start_time, otio_timeline_set_tracks, otio_timeline_tracks,
    otio_track_kind, otio_track_new, otio_track_set_kind, otio_transition_enabled,
    otio_transition_in_offset, otio_transition_new, otio_transition_out_offset,
    otio_transition_set_enabled, otio_transition_set_in_offset, otio_transition_set_out_offset,
    otio_transition_set_type, otio_transition_type,
};
pub use status::{OtioStatus, otio_status_name, otio_version};
pub use time::{
    OtioDropFrame, OtioRationalTime, OtioTimeRange, OtioTimeTransform, otio_default_epsilon_s,
    otio_is_drop_frame_rate, otio_is_smpte_timecode_rate, otio_nearest_smpte_timecode_rate,
    otio_rational_time_add, otio_rational_time_almost_equal, otio_rational_time_ceil,
    otio_rational_time_compare, otio_rational_time_duration_from_start_end_time,
    otio_rational_time_duration_from_start_end_time_inclusive, otio_rational_time_equal,
    otio_rational_time_floor, otio_rational_time_from_frames, otio_rational_time_from_seconds,
    otio_rational_time_from_seconds_at_rate, otio_rational_time_from_time_string,
    otio_rational_time_from_timecode, otio_rational_time_is_valid, otio_rational_time_negate,
    otio_rational_time_rescaled_to, otio_rational_time_rescaled_to_time, otio_rational_time_round,
    otio_rational_time_strictly_equal, otio_rational_time_subtract, otio_rational_time_to_frames,
    otio_rational_time_to_frames_at_rate, otio_rational_time_to_nearest_timecode_at,
    otio_rational_time_to_seconds, otio_rational_time_to_time_string,
    otio_rational_time_to_timecode, otio_rational_time_to_timecode_at,
    otio_rational_time_value_rescaled_to, otio_time_range_before_range,
    otio_time_range_before_time, otio_time_range_begins_range, otio_time_range_begins_time,
    otio_time_range_clamped_range, otio_time_range_clamped_time, otio_time_range_contains_range,
    otio_time_range_contains_time, otio_time_range_duration_extended_by,
    otio_time_range_end_time_exclusive, otio_time_range_end_time_inclusive,
    otio_time_range_extended_by, otio_time_range_finishes_range, otio_time_range_finishes_time,
    otio_time_range_from_start_end_time, otio_time_range_from_start_end_time_inclusive,
    otio_time_range_intersects, otio_time_range_is_valid, otio_time_range_meets,
    otio_time_range_overlaps_range, otio_time_range_overlaps_time,
    otio_time_transform_applied_to_range, otio_time_transform_applied_to_time,
    otio_time_transform_applied_to_transform,
};
pub use value::{OtioBox2d, OtioColor, OtioV2d};

// The header states these sizes, so a caller built against it and a library
// built from this crate agree on the layout of everything passed by value.
// The C test program asserts the same numbers from its side.
const _: () = {
    assert!(size_of::<OtioNode>() == 8);
    assert!(size_of::<OtioRationalTime>() == 16);
    assert!(size_of::<OtioTimeRange>() == 32);
    assert!(size_of::<OtioTimeTransform>() == 32);
    assert!(size_of::<OtioColor>() == 32);
    assert!(size_of::<OtioV2d>() == 16);
    assert!(size_of::<OtioBox2d>() == 32);
};
