//! The ORI schemas the extension registers, under ORI's own names.
//!
//! ORI defines these in Python, in its schemadef plugin
//! (`otio_event_plugin/schemadefs/SyncEvent.py`). The names cannot take a
//! namespace without breaking every ORI file and every RV and xStudio peer,
//! so they stay exactly as ORI writes them ([ADR 0012]). One name differs
//! from ORI's own prose: the point batch is written `PaintPoint.1`, which is
//! what ORI's code, its example files and Alchemist all write, although ORI's
//! spec and docstrings say `PaintPoints.1`.
//!
//! Every schema here except `AnnotationEffect.1` derives in ORI from a
//! `SyncEvent` class with no OpenTimelineIO base beyond `SerializableObject`,
//! so each registers as a dynamic schema: an object of it is a
//! [`DynamicObject`](otio_core::schema::DynamicObject) whose fields are
//! ORI's, read and written exactly as they are in the file. `SyncEvent`
//! itself is never registered by ORI, so it is not here either.
//! `AnnotationEffect.1` derives from `Effect` and registers as a subclass of
//! it, so it still sits in an item's `effects`.
//!
//! [ADR 0012]: https://github.com/alchemist-editor/otio-rust/blob/main/docs/adr/0012-ori-annotations.md

use otio_core::registry::{self, DynamicBase, SchemaKind};

/// Opens a stroke: its colour, brush and owner.
pub const PAINT_START: &str = "PaintStart";
/// A batch of points on an open stroke.
pub const PAINT_POINT: &str = "PaintPoint";
/// Closes a stroke.
pub const PAINT_END: &str = "PaintEnd";
/// Text placed on the image.
pub const TEXT_ANNOTATION: &str = "TextAnnotation";
/// An ellipse within a box.
pub const ELLIPSE_ANNOTATION: &str = "EllipseAnnotation";
/// A rectangle.
pub const RECTANGLE_ANNOTATION: &str = "RectangleAnnotation";
/// An arrow from one point to another.
pub const ARROW_ANNOTATION: &str = "ArrowAnnotation";
/// The points of a [`PAINT_POINT`] batch, as parallel arrays.
pub const PAINT_VERTICES: &str = "PaintVertices";
/// One point. ORI registers it; its own code writes [`PAINT_VERTICES`].
pub const PAINT_VERTEX: &str = "PaintVertex";
/// Playback started or stopped.
pub const PLAY: &str = "Play";
/// The playhead moved.
pub const SET_CURRENT_FRAME: &str = "SetCurrentFrame";
/// A peer asked for the session.
pub const GET_SESSION: &str = "GetSession";
/// A peer asked for the playback state.
pub const REQUEST_SYNC_PLAYBACK: &str = "RequestSyncPlayback";
/// The whole playback state.
pub const SYNC_PLAYBACK: &str = "SyncPlayback";
/// The media in view changed.
pub const MEDIA_CHANGE: &str = "MediaChange";
/// An effect on a clip holding that clip's annotation commands; an older
/// form, read but never written.
pub const ANNOTATION_EFFECT: &str = "AnnotationEffect";

/// The version of every ORI schema. All of them are at 1.
pub const VERSION: u32 = 1;

/// The schemas that are annotation commands, which sit in an annotation
/// clip's `annotation_commands`.
pub const COMMANDS: [&str; 7] = [
    PAINT_START,
    PAINT_POINT,
    PAINT_END,
    TEXT_ANNOTATION,
    ELLIPSE_ANNOTATION,
    RECTANGLE_ANNOTATION,
    ARROW_ANNOTATION,
];

/// ORI's playback events. They are registered so ORI's recordings read as
/// typed objects; live playback travels on timeline sync's own channel.
pub const PLAYBACK: [&str; 6] = [
    PLAY,
    SET_CURRENT_FRAME,
    GET_SESSION,
    REQUEST_SYNC_PLAYBACK,
    SYNC_PLAYBACK,
    MEDIA_CHANGE,
];

/// Every schema the extension registers, as it claims them.
pub const NAMES: [&str; 16] = [
    PAINT_START,
    PAINT_POINT,
    PAINT_END,
    TEXT_ANNOTATION,
    ELLIPSE_ANNOTATION,
    RECTANGLE_ANNOTATION,
    ARROW_ANNOTATION,
    PAINT_VERTICES,
    PAINT_VERTEX,
    PLAY,
    SET_CURRENT_FRAME,
    GET_SESSION,
    REQUEST_SYNC_PLAYBACK,
    SYNC_PLAYBACK,
    MEDIA_CHANGE,
    ANNOTATION_EFFECT,
];

/// The kind of schema each name registers as.
fn kind_of(name: &str) -> SchemaKind {
    if name == ANNOTATION_EFFECT {
        SchemaKind::Subclass("Effect")
    } else {
        SchemaKind::Dynamic(DynamicBase::SerializableObject)
    }
}

/// Registers every ORI schema with otio-core.
///
/// A schema someone has already registered the same way, such as a program
/// that loaded ORI's own classes first, is left as it is.
///
/// # Errors
///
/// A sentence naming the first schema that is registered some other way.
pub(crate) fn register() -> Result<(), String> {
    for name in NAMES {
        let wanted = kind_of(name);
        let added = match wanted {
            SchemaKind::Subclass(base) => registry::register_subclass(name, VERSION, base),
            SchemaKind::Dynamic(base) => registry::register_type(name, VERSION, base),
            _ => false,
        };
        if added {
            continue;
        }
        let existing = registry::schema_kind(name);
        if existing != Some(wanted) || registry::schema_version(name) != Some(VERSION) {
            return Err(format!(
                "schema '{name}' is already registered as something other than ORI's \
                 {name}.{VERSION}"
            ));
        }
    }
    Ok(())
}

/// The schema name an ORI schema is written with, such as `PaintPoint.1`.
#[must_use]
pub fn label(name: &str) -> String {
    format!("{name}.{VERSION}")
}
