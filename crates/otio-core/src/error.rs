//! Errors produced when reading, writing and working with OTIO documents.
//!
//! # On the wording
//!
//! Each message is upstream's, character for character. Upstream reports a
//! failure as an `ErrorStatus` outcome plus details, and its Python bindings
//! (`otio_errorStatusHandler.cpp`) turn that into the text of the exception
//! they raise; code in the wild matches on that text, so it is part of the
//! observable behaviour rather than a detail. `Display` gives exactly that
//! text, with one exception: where upstream appends `": "` and the `str()` of
//! the object concerned (its `object_details`), the object is not rendered
//! here, because only Python knows how to print it. [`Error::object`] names
//! it instead, so that a binding can append it as upstream does.
//!
//! The Python exception each variant becomes is noted on it, and follows the
//! same handler.
//!
//! A few variants have no upstream counterpart, because they report things
//! upstream cannot get wrong — a stale handle, a node that is not a
//! composition — and keep their own wording.

use std::fmt;

use crate::arena::NodeId;

/// Where upstream's reader says a reading error happened.
///
/// Upstream reads a document bottom-up and reports an error in terms of the
/// innermost object being read when it happened: "near line N", N being the
/// line of that object's closing brace, and, for an object rather than a
/// value type such as a `TimeRange`, its name and C++ type as well.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReadLocation {
    /// The line of the closing brace of the object being read.
    pub line: usize,
    /// The object being read, unless it was a value type.
    pub object: Option<ReadObject>,
}

/// An object upstream's reader names in an error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReadObject {
    /// Its `name` field, or `<unknown>` if that is absent or not a string.
    pub name: String,
    /// Its C++ type, as upstream spells it: the type's `typeid` name, which
    /// GCC and Clang give in mangled form, such as
    /// `N14opentimelineio5v0_194ClipE`.
    pub type_name: String,
}

/// An error produced while reading, writing or working with a document.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Error {
    /// The input was not well-formed JSON. Upstream's `JSON_PARSE_ERROR`,
    /// raised in Python as `ValueError`.
    Json {
        /// RapidJSON's message and position, in upstream's words: see
        /// [`crate::json::ParseError`].
        message: String,
    },

    /// An object sat where the reader needed one with an `OTIO_SCHEMA`, and
    /// had none. Upstream reads such an object as a plain dictionary, and
    /// then reports a `TYPE_MISMATCH` (`ValueError`) because a dictionary is
    /// not the object it wanted.
    MissingSchema {
        /// The C++ type the reader wanted there, as upstream spells it.
        expected: &'static str,
        /// Where in the document the object sits, as a JSON-ish path.
        path: String,
        /// Where upstream would say the error is; filled in by
        /// [`crate::from_str`].
        at: Option<ReadLocation>,
    },

    /// An `OTIO_SCHEMA` value was not of the form `Name.Version`. Upstream's
    /// `MALFORMED_SCHEMA`, raised as `ValueError`.
    MalformedSchema {
        /// The value that could not be split into a name and a version.
        schema: String,
        /// Where in the document the object sits.
        path: String,
        /// The line of the object's closing brace; filled in by
        /// [`crate::from_str`].
        line: Option<usize>,
    },

    /// A value was not of the type its field requires. Upstream's
    /// `TYPE_MISMATCH`, raised as `ValueError`.
    TypeMismatch {
        /// Upstream's description of the mismatch, such as
        /// `expected type string under key 'name': found type l instead`.
        detail: String,
        /// Where in the document the value sits.
        path: String,
        /// Where upstream would say the error is; filled in by
        /// [`crate::from_str`].
        at: Option<ReadLocation>,
    },

    /// An image sequence named a missing-frame policy that does not exist.
    /// Upstream reports it as a `JSON_PARSE_ERROR` (`ValueError`).
    UnknownMissingFramePolicy {
        /// The name given, or nothing if it was not a string.
        name: String,
        /// Where in the document the value sits.
        path: String,
        /// Where upstream would say the error is; filled in by
        /// [`crate::from_str`].
        at: Option<ReadLocation>,
    },

    /// A required field was absent.
    ///
    /// Only the writer reports this, for a document with no root object,
    /// which upstream has no way to express; the wording is this library's
    /// own.
    MissingField {
        /// The absent field.
        field: &'static str,
        /// Where in the document the owning object sits.
        path: String,
    },

    /// A `SerializableObjectRef.1` named an id no object declared. Upstream's
    /// `UNRESOLVED_OBJECT_REFERENCE`, raised as `ValueError`.
    UnresolvedReference {
        /// The id that was never declared.
        id: String,
        /// Where in the document the reference sits.
        path: String,
        /// The line of the closing brace of the object holding the
        /// reference; filled in by [`crate::from_str`].
        line: Option<usize>,
    },

    /// A colour could not be read from the text or the numbers given.
    /// Upstream throws `std::invalid_argument`, which pybind11 raises as
    /// `ValueError`.
    BadColor {
        /// Upstream's message: `Invalid hex format`, `List must have exactly
        /// 3 or 4 elements`, or `stoi` for a hex digit `std::stoi` rejects.
        text: String,
    },

    /// A node handle outlived the object it referred to.
    ///
    /// The slot it named has since been reused, so the handle is stale.
    StaleHandle,

    /// An operation needed the object's parent, and it has none. Upstream's
    /// `NOT_A_CHILD`, raised as `NotAChildError`.
    NotAChild {
        /// The schema of the object with no parent.
        schema: String,
        /// The object with no parent, which upstream names.
        object: NodeId,
        /// What a transition adds to the message; nothing for an item.
        details: Option<&'static str>,
    },

    /// An object was looked up in a composition it does not belong to.
    /// Upstream's `NOT_A_CHILD_OF`, raised as `NotAChildError`.
    NotAChildOf {
        /// The schema of the composition it was looked up in.
        parent: String,
        /// The composition, which upstream names.
        object: Option<NodeId>,
    },

    /// An object was looked up in a composition it does not descend from.
    /// Upstream's `NOT_DESCENDED_FROM`, raised as `NotAChildError`.
    NotDescendedFrom {
        /// The schema of the composition it was looked up in.
        parent: String,
        /// The composition, which upstream names.
        object: Option<NodeId>,
    },

    /// A child index fell outside the composition. Upstream's
    /// `ILLEGAL_INDEX`, raised as `IndexError`.
    IllegalIndex {
        /// The index that was asked for.
        index: i64,
        /// How many there are: a composition's children, or the images in an
        /// image sequence.
        len: usize,
    },

    /// An image sequence holds no images at all, so no URL or frame time can
    /// be asked for. Upstream's `ILLEGAL_INDEX` with details of its own,
    /// raised as `IndexError`.
    NoImagesInSequence {
        /// Why there are none, in upstream's words.
        reason: &'static str,
    },

    /// Trimming left a range that does not exist, or a time fell outside the
    /// range it was looked up in. Upstream's `INVALID_TIME_RANGE`, raised as
    /// `ValueError`.
    InvalidTimeRange,

    /// This kind of object has no duration of its own. Upstream's
    /// `OBJECT_WITHOUT_DURATION`, raised as `ValueError`.
    ///
    /// Markers, effects and media references do not sit in time.
    NoDuration {
        /// The schema of the object asked for a duration.
        schema: String,
        /// That object, which upstream names.
        object: NodeId,
    },

    /// The object's available range is not knowable. Upstream's
    /// `CANNOT_COMPUTE_AVAILABLE_RANGE`, raised as
    /// `CannotComputeAvailableRangeError`.
    ///
    /// A clip whose media reference has no `available_range` and no
    /// `source_range` is the usual case: nothing says how long it is.
    NoAvailableRange {
        /// The schema of the object asked for an available range.
        schema: String,
        /// Why not, in upstream's words, such as `No available_range set on
        /// media reference on clip`.
        reason: &'static str,
        /// The clip upstream names, if it names one.
        object: Option<NodeId>,
    },

    /// A clip's image bounds are not knowable. Upstream's
    /// `CANNOT_COMPUTE_BOUNDS`, raised as `ValueError`.
    NoImageBounds {
        /// Why not, in upstream's words.
        reason: &'static str,
        /// The clip, which upstream names.
        object: NodeId,
    },

    /// A clip's `active_media_reference_key` names no entry. Upstream's
    /// `MEDIA_REFERENCES_DO_NOT_CONTAIN_ACTIVE_KEY`, raised as `ValueError`.
    NoActiveMediaReference {
        /// The key that named nothing.
        key: String,
    },

    /// An object was added to a composition while still in another.
    /// Upstream's `CHILD_ALREADY_PARENTED`, raised as `ValueError`.
    ///
    /// Upstream refuses rather than silently re-parenting, because the object
    /// would then appear in two places at once.
    ChildAlreadyParented,

    /// The object does not implement this operation. Upstream's
    /// `NOT_IMPLEMENTED`, raised as `NotImplementedError`.
    ///
    /// Upstream's base classes leave some questions to their subclasses: a
    /// bare `Item` cannot say what media sits behind it, because only a
    /// `Clip`, a `Track` or a `Stack` knows.
    NotImplemented {
        /// The operation that has no implementation here.
        operation: &'static str,
        /// The schema of the object asked for it.
        schema: String,
    },

    /// The composition does not say where its children sit.
    ///
    /// A bare `Composition` holds children but has no layout of its own:
    /// only a `Track` lays them end to end and only a `Stack` starts them
    /// together. Upstream reports the same case as `NOT_IMPLEMENTED` from
    /// `Composition::range_of_child_at_index`, so it reads the same.
    NoLayout,

    /// An operation needed a composition and was given something else.
    ///
    /// Upstream's types rule this out before any call is made, so the
    /// wording is this library's own.
    NotAComposition {
        /// The schema of the object that is not a composition.
        schema: String,
    },

    /// A trim fell in the middle of a transition. Upstream's
    /// `CANNOT_TRIM_TRANSITION`, raised as `ValueError`.
    ///
    /// A transition is defined by how far it reaches into the items on either
    /// side, so cutting one in half has no meaning.
    CannotTrimTransition,

    /// An operation needed an item and found something else, or nothing.
    /// Upstream's `NOT_AN_ITEM`, raised as `ValueError`.
    NotAnItem,

    /// An operation needed a gap and found something else, or nothing.
    /// Upstream's `NOT_A_GAP`, raised as `ValueError`.
    NotAGap,

    /// A composition held a child of a kind it cannot hold. Upstream's
    /// `TYPE_MISMATCH` from its track and stack algorithms, raised as
    /// `ValueError`.
    UnexpectedChild {
        /// The schema of the child that does not belong.
        schema: String,
        /// The schema of the composition holding it.
        parent: String,
        /// The child, which upstream names.
        object: Option<NodeId>,
    },

    /// An object carried a schema version newer than the one registered.
    /// Upstream's `SCHEMA_VERSION_UNSUPPORTED`, raised in Python as
    /// `UnsupportedSchemaError`.
    ///
    /// Upstream refuses such an object rather than reading it as the older
    /// version it knows, because fields may have changed meaning in between.
    UnsupportedSchemaVersion {
        /// The schema's name.
        schema: String,
        /// The version that was asked for.
        version: u32,
        /// The newest version this library knows.
        highest: u32,
        /// Where in the document the object sits, when it was read from one.
        path: String,
        /// The line of the object's closing brace, when it was read from a
        /// document; filled in by [`crate::from_str`]. Upstream's reader
        /// then gives only the line, as it does for any error it meets
        /// before it knows what the object is.
        line: Option<usize>,
    },

    /// An object was asked to become a schema nobody registered. Upstream's
    /// `SCHEMA_NOT_REGISTERED`, raised as `ValueError`.
    SchemaNotRegistered {
        /// The schema that is not registered.
        schema: String,
    },

    /// An object was met again while it was still being written or copied,
    /// so it holds itself somewhere below it.
    ///
    /// Upstream refuses such a cycle rather than writing forever; holding the
    /// same object in two places is allowed, because neither is inside the
    /// other. Upstream's `OBJECT_CYCLE`, raised as `ValueError`.
    ObjectCycle {
        /// The schema of the object met twice.
        schema: String,
    },

    /// A document was to be written for an older release, and nothing
    /// registered says how to take this schema back that far. Upstream
    /// reports it as an `INTERNAL_ERROR`, raised as `ValueError`.
    NoDowngradeFunction {
        /// The schema that could not be downgraded.
        schema: String,
        /// The version it had reached.
        from: u32,
        /// The version it was to reach.
        to: u32,
    },

    /// A registered upgrade or downgrade function reported a failure.
    VersionFunctionFailed {
        /// The schema the function was registered for.
        schema: String,
        /// What the function reported.
        message: String,
    },
}

impl Error {
    /// The object upstream names at the end of this error's message, if any.
    ///
    /// Upstream's Python bindings append `": "` and the object's `str()`; a
    /// binding that can print the object should do the same.
    #[must_use]
    pub const fn object(&self) -> Option<NodeId> {
        match self {
            Self::NotAChild { object, .. }
            | Self::NoDuration { object, .. }
            | Self::NoImageBounds { object, .. } => Some(*object),
            Self::NotAChildOf { object, .. }
            | Self::NotDescendedFrom { object, .. }
            | Self::NoAvailableRange { object, .. }
            | Self::UnexpectedChild { object, .. } => *object,
            _ => None,
        }
    }
}

/// Writes a reading error the way upstream's `Reader::_error` shapes it.
///
/// Inside a value type, upstream drops the details and keeps only the line;
/// inside an object, it names the object and appends the line.
fn write_read_error(
    f: &mut fmt::Formatter<'_>,
    prefix: &str,
    detail: &dyn fmt::Display,
    at: Option<&ReadLocation>,
) -> fmt::Result {
    match at {
        None => write!(f, "{prefix}{detail}"),
        Some(ReadLocation { line, object: None }) => write!(f, "{prefix}near line {line}"),
        Some(ReadLocation {
            line,
            object: Some(object),
        }) => write!(
            f,
            "{prefix}While reading object named '{}' (of type '{}'): {detail} (near line {line})",
            object.name, object.type_name
        ),
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Json { message } => write!(f, "JSON parse error while reading: {message}"),
            Self::MissingSchema { expected, at, .. } => write_read_error(
                f,
                "type mismatch while decoding: ",
                &format_args!(
                    "expected to read a {expected}, found a {} instead",
                    crate::cxx::ANY_DICTIONARY
                ),
                at.as_ref(),
            ),
            Self::MalformedSchema { schema, line, .. } => match line {
                Some(line) => write!(f, "Illegal/malformed schema: near line {line}"),
                None => write!(
                    f,
                    "Illegal/malformed schema: badly formed schema version string '{schema}'"
                ),
            },
            Self::TypeMismatch { detail, at, .. } => {
                write_read_error(f, "type mismatch while decoding: ", detail, at.as_ref())
            }
            Self::UnknownMissingFramePolicy { name, at, .. } => write_read_error(
                f,
                "JSON parse error while reading: ",
                &format_args!("Unknown missing_frame_policy: {name}"),
                at.as_ref(),
            ),
            Self::MissingField { field, path } => {
                write!(f, "object at {path} is missing required field '{field}'")
            }
            Self::UnresolvedReference { id, line, .. } => match line {
                Some(line) => write!(
                    f,
                    "Unresolved object reference while reading: {id} (near line {line})"
                ),
                None => write!(f, "Unresolved object reference while reading: {id}"),
            },
            Self::BadColor { text } => write!(f, "{text}"),
            Self::StaleHandle => {
                write!(f, "node handle refers to an object that no longer exists")
            }
            Self::NotAChild { details, .. } => match details {
                Some(details) => write!(f, "item has no parent: {details}"),
                None => write!(f, "item has no parent"),
            },
            Self::NotAChildOf { .. } => write!(f, "item is not a child of specified object"),
            Self::NotDescendedFrom { .. } => {
                write!(f, "item is not a descendent of specified object")
            }
            Self::IllegalIndex { .. } => write!(f, "illegal index"),
            Self::NoImagesInSequence { reason } => write!(f, "{reason}"),
            Self::InvalidTimeRange => write!(f, "computed time range would be invalid"),
            Self::NoDuration { .. } => write!(
                f,
                "cannot compute duration on this type of object: \
                 Cannot determine duration from this kind of object"
            ),
            Self::NoAvailableRange { reason, .. } => {
                write!(f, "Cannot compute available range: {reason}")
            }
            Self::NoImageBounds { reason, .. } => {
                write!(f, "cannot compute image bounds: {reason}")
            }
            Self::NoActiveMediaReference { .. } => {
                write!(f, "The media references do not contain the active key")
            }
            Self::ChildAlreadyParented => write!(f, "child already has a parent"),
            Self::NotImplemented { .. } | Self::NoLayout => {
                write!(f, "method not implemented for this class")
            }
            Self::NotAComposition { schema } => {
                write!(f, "a {schema} is not a composition")
            }
            Self::NotAnItem => write!(f, "object is not descendent of Item type"),
            Self::NotAGap => write!(f, "object is not descendent of Gap type"),
            Self::CannotTrimTransition => write!(f, "cannot trim transition"),
            Self::UnexpectedChild { parent, .. } => {
                // A stack can hold only tracks; a track, only items and
                // transitions.
                let wanted = if parent == "Stack" {
                    "Track*"
                } else {
                    "Item* || Transition*"
                };
                write!(
                    f,
                    "type mismatch while decoding: expected item of type {wanted}"
                )
            }
            Self::UnsupportedSchemaVersion {
                schema,
                version,
                highest,
                line,
                ..
            } => match line {
                Some(line) => write!(f, "unsupported schema version: near line {line}"),
                None => write!(
                    f,
                    "unsupported schema version: Schema {schema} has highest version \
                     {highest}, but the requested schema version {version} is even greater."
                ),
            },
            Self::SchemaNotRegistered { schema } => {
                write!(f, "schema is not registered/known: {schema}")
            }
            Self::ObjectCycle { schema } => write!(
                f,
                "Detected SerializableObject cycle while copying/serializing: \
                 cyclically encountered object has schema {schema}"
            ),
            // Upstream's handler writes no space after the colon.
            Self::NoDowngradeFunction { from, to, .. } => write!(
                f,
                "Internal error (aka \"this is a bug\"):No downgrader function \
                 available for going from version {from} to version {to}."
            ),
            Self::VersionFunctionFailed { schema, message } => {
                write!(f, "a version function for {schema} failed: {message}")
            }
        }
    }
}

impl std::error::Error for Error {}

impl From<crate::json::ParseError> for Error {
    fn from(error: crate::json::ParseError) -> Self {
        Self::Json {
            message: error.to_string(),
        }
    }
}

/// Shorthand for a result carrying an [`Error`].
pub type Result<T> = std::result::Result<T, Error>;

#[cfg(test)]
mod tests {
    use super::*;

    /// Each message as upstream's Python bindings raise it, taken from
    /// OpenTimelineIO itself; the `str()` of any object upstream appends is
    /// the binding's to add.
    #[test]
    fn messages_are_upstreams() {
        let object = NodeId::from_raw(0, 0);
        let cases: Vec<(Error, &str)> = vec![
            (
                Error::NotAChild {
                    schema: "Clip".into(),
                    object,
                    details: None,
                },
                "item has no parent",
            ),
            (
                Error::NotAChild {
                    schema: "Transition".into(),
                    object,
                    details: Some("cannot compute range in parent because item has no parent"),
                },
                "item has no parent: cannot compute range in parent because item has no parent",
            ),
            (
                Error::NotAChildOf {
                    parent: "Track".into(),
                    object: Some(object),
                },
                "item is not a child of specified object",
            ),
            (
                Error::NotDescendedFrom {
                    parent: "Track".into(),
                    object: Some(object),
                },
                "item is not a descendent of specified object",
            ),
            (Error::IllegalIndex { index: 5, len: 0 }, "illegal index"),
            (
                Error::InvalidTimeRange,
                "computed time range would be invalid",
            ),
            (
                Error::NoDuration {
                    schema: "Marker".into(),
                    object,
                },
                "cannot compute duration on this type of object: \
                 Cannot determine duration from this kind of object",
            ),
            (
                Error::NoAvailableRange {
                    schema: "Clip".into(),
                    reason: "No available_range set on media reference on clip",
                    object: Some(object),
                },
                "Cannot compute available range: \
                 No available_range set on media reference on clip",
            ),
            (
                Error::NoImageBounds {
                    reason: "No image bounds set on media reference on clip",
                    object,
                },
                "cannot compute image bounds: No image bounds set on media reference on clip",
            ),
            (
                Error::NoActiveMediaReference { key: "x".into() },
                "The media references do not contain the active key",
            ),
            (Error::ChildAlreadyParented, "child already has a parent"),
            (
                Error::NotImplemented {
                    operation: "available_range",
                    schema: "Gap".into(),
                },
                "method not implemented for this class",
            ),
            (Error::NoLayout, "method not implemented for this class"),
            (Error::NotAnItem, "object is not descendent of Item type"),
            (Error::NotAGap, "object is not descendent of Gap type"),
            (Error::CannotTrimTransition, "cannot trim transition"),
            (
                Error::UnexpectedChild {
                    schema: "Clip".into(),
                    parent: "Stack".into(),
                    object: Some(object),
                },
                "type mismatch while decoding: expected item of type Track*",
            ),
            (
                Error::UnexpectedChild {
                    schema: "Marker".into(),
                    parent: "Track".into(),
                    object: Some(object),
                },
                "type mismatch while decoding: expected item of type Item* || Transition*",
            ),
            (
                Error::BadColor {
                    text: "Invalid hex format".into(),
                },
                "Invalid hex format",
            ),
            (
                Error::UnsupportedSchemaVersion {
                    schema: "Clip".into(),
                    version: 99,
                    highest: 2,
                    path: String::new(),
                    line: None,
                },
                "unsupported schema version: Schema Clip has highest version 2, but the \
                 requested schema version 99 is even greater.",
            ),
            (
                Error::UnsupportedSchemaVersion {
                    schema: "Clip".into(),
                    version: 99,
                    highest: 2,
                    path: "$".into(),
                    line: Some(3),
                },
                "unsupported schema version: near line 3",
            ),
            (
                Error::ObjectCycle {
                    schema: "Clip".into(),
                },
                "Detected SerializableObject cycle while copying/serializing: \
                 cyclically encountered object has schema Clip",
            ),
            (
                Error::NoDowngradeFunction {
                    schema: "Clip".into(),
                    from: 1,
                    to: 0,
                },
                "Internal error (aka \"this is a bug\"):No downgrader function available \
                 for going from version 1 to version 0.",
            ),
        ];
        for (error, expected) in cases {
            assert_eq!(error.to_string(), expected, "{error:?}");
        }
    }

    #[test]
    fn reading_errors_take_upstreams_shape() {
        let in_object = ReadLocation {
            line: 100,
            object: Some(ReadObject {
                name: "shot".into(),
                type_name: "N14opentimelineio5v0_194ClipE".into(),
            }),
        };
        let in_value = ReadLocation {
            line: 41,
            object: None,
        };
        let mismatch = |at: Option<ReadLocation>| Error::TypeMismatch {
            detail: "expected type b under key 'enabled': found type l instead".into(),
            path: "$.enabled".into(),
            at,
        };
        assert_eq!(
            mismatch(Some(in_object)).to_string(),
            "type mismatch while decoding: While reading object named 'shot' \
             (of type 'N14opentimelineio5v0_194ClipE'): expected type b under key \
             'enabled': found type l instead (near line 100)"
        );
        assert_eq!(
            mismatch(Some(in_value)).to_string(),
            "type mismatch while decoding: near line 41"
        );
        assert_eq!(
            Error::MalformedSchema {
                schema: "Clip".into(),
                path: "$".into(),
                line: Some(4),
            }
            .to_string(),
            "Illegal/malformed schema: near line 4"
        );
        assert_eq!(
            Error::UnresolvedReference {
                id: "nope".into(),
                path: "$.metadata.x".into(),
                line: Some(105),
            }
            .to_string(),
            "Unresolved object reference while reading: nope (near line 105)"
        );
        assert_eq!(
            Error::UnknownMissingFramePolicy {
                name: "zzz".into(),
                path: "$".into(),
                at: Some(ReadLocation {
                    line: 175,
                    object: Some(ReadObject {
                        name: String::new(),
                        type_name: "N14opentimelineio5v0_1922ImageSequenceReferenceE".into(),
                    }),
                }),
            }
            .to_string(),
            "JSON parse error while reading: While reading object named '' (of type \
             'N14opentimelineio5v0_1922ImageSequenceReferenceE'): Unknown \
             missing_frame_policy: zzz (near line 175)"
        );
        assert_eq!(
            Error::MissingSchema {
                expected: "N14opentimelineio5v0_1910ComposableE",
                path: "$.children[0]".into(),
                at: Some(ReadLocation {
                    line: 31,
                    object: Some(ReadObject {
                        name: "V1".into(),
                        type_name: "N14opentimelineio5v0_195TrackE".into(),
                    }),
                }),
            }
            .to_string(),
            "type mismatch while decoding: While reading object named 'V1' (of type \
             'N14opentimelineio5v0_195TrackE'): expected to read a \
             N14opentimelineio5v0_1910ComposableE, found a \
             N14opentimelineio5v0_1913AnyDictionaryE instead (near line 31)"
        );
    }
}
