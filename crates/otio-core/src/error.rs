//! Errors produced when reading and writing OTIO documents.

use std::fmt;

/// An error produced while reading or writing an OTIO document.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Error {
    /// The input was not well-formed JSON.
    Json {
        /// What the JSON parser reported.
        message: String,
    },

    /// An object was missing its `OTIO_SCHEMA` key.
    MissingSchema {
        /// Where in the document the object sits, as a JSON-ish path.
        path: String,
    },

    /// An `OTIO_SCHEMA` value was not of the form `Name.Version`.
    MalformedSchema {
        /// The value that could not be split into a name and a version.
        schema: String,
        /// Where in the document the object sits.
        path: String,
    },

    /// A value was not of the type its field requires.
    TypeMismatch {
        /// What the field required.
        expected: &'static str,
        /// What was found instead.
        found: String,
        /// Where in the document the value sits.
        path: String,
    },

    /// A required field was absent.
    MissingField {
        /// The absent field.
        field: &'static str,
        /// Where in the document the owning object sits.
        path: String,
    },

    /// A `SerializableObjectRef.1` named an id no object declared.
    UnresolvedReference {
        /// The id that was never declared.
        id: String,
        /// Where in the document the reference sits.
        path: String,
    },

    /// A colour could not be read from the text or the numbers given.
    BadColor {
        /// What was given, or what was wrong with it.
        text: String,
    },

    /// A node handle outlived the object it referred to.
    ///
    /// The slot it named has since been reused, so the handle is stale.
    StaleHandle,

    /// An operation needed the object's parent, and it has none.
    NotAChild {
        /// The schema of the object with no parent.
        schema: String,
    },

    /// An object was looked up in a composition it does not belong to.
    NotAChildOf {
        /// The schema of the composition it was looked up in.
        parent: String,
    },

    /// An object was looked up in a composition it does not descend from.
    NotDescendedFrom {
        /// The schema of the composition it was looked up in.
        parent: String,
    },

    /// A child index fell outside the composition.
    IllegalIndex {
        /// The index that was asked for.
        index: i64,
        /// How many there are: a composition's children, or the images in an
        /// image sequence.
        len: usize,
    },

    /// An image sequence holds no images at all, so no URL or frame time can
    /// be asked for.
    NoImagesInSequence {
        /// Why there are none. Upstream's wording, because its own tests
        /// compare the message.
        reason: &'static str,
    },

    /// Trimming left a range that does not exist.
    ///
    /// The child lies entirely outside its composition's source range.
    InvalidTimeRange,

    /// This kind of object has no duration of its own.
    ///
    /// Markers, effects and media references do not sit in time.
    NoDuration {
        /// The schema of the object asked for a duration.
        schema: String,
    },

    /// The object's available range is not knowable.
    ///
    /// A clip whose media reference has no `available_range` and no
    /// `source_range` is the usual case: nothing says how long it is.
    NoAvailableRange {
        /// The schema of the object asked for an available range.
        schema: String,
    },

    /// A clip's `active_media_reference_key` names no entry.
    NoActiveMediaReference {
        /// The key that named nothing.
        key: String,
    },

    /// An object was added to a composition while still in another.
    ///
    /// Upstream's C++ raises the same error rather than silently re-parenting,
    /// because the object would then appear in two places at once.
    ChildAlreadyParented,

    /// The object does not implement this operation.
    ///
    /// Upstream's base classes leave some questions to their subclasses: a
    /// bare `Item` cannot say what media sits behind it, because only a
    /// `Clip`, a `Track` or a `Stack` knows. Upstream reports this as
    /// `NOT_IMPLEMENTED`, and its Python bindings raise `NotImplementedError`
    /// for it, which is observable behaviour its own tests check.
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
    /// `Composition::range_of_child_at_index`.
    NoLayout,

    /// An operation needed a composition and was given something else.
    NotAComposition {
        /// The schema of the object that is not a composition.
        schema: String,
    },

    /// A trim fell in the middle of a transition.
    ///
    /// A transition is defined by how far it reaches into the items on either
    /// side, so cutting one in half has no meaning.
    CannotTrimTransition,

    /// An operation needed an item and found something else, or nothing.
    NotAnItem,

    /// An operation needed a gap and found something else, or nothing.
    NotAGap,

    /// A composition held a child of a kind it cannot hold.
    UnexpectedChild {
        /// The schema of the child that does not belong.
        schema: String,
        /// The schema of the composition holding it.
        parent: String,
    },
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Json { message } => write!(f, "invalid JSON: {message}"),
            Self::MissingSchema { path } => {
                write!(f, "object at {path} has no OTIO_SCHEMA")
            }
            Self::MalformedSchema { schema, path } => write!(
                f,
                "object at {path} has malformed OTIO_SCHEMA '{schema}'; expected 'Name.Version'"
            ),
            Self::TypeMismatch {
                expected,
                found,
                path,
            } => write!(f, "expected {expected} at {path}, found {found}"),
            Self::MissingField { field, path } => {
                write!(f, "object at {path} is missing required field '{field}'")
            }
            Self::UnresolvedReference { id, path } => {
                write!(f, "reference at {path} names undeclared object id '{id}'")
            }
            Self::BadColor { text } => write!(f, "not a colour: {text}"),
            Self::StaleHandle => {
                write!(f, "node handle refers to an object that no longer exists")
            }
            Self::NotAChild { schema } => {
                write!(f, "{schema} has no parent")
            }
            Self::NotAChildOf { parent } => {
                write!(f, "object is not a child of this {parent}")
            }
            Self::NotDescendedFrom { parent } => {
                write!(f, "object does not descend from this {parent}")
            }
            Self::IllegalIndex { index, len } => {
                write!(f, "index {index} is out of range for {len} items")
            }
            Self::NoImagesInSequence { reason } => write!(f, "{reason}"),
            Self::InvalidTimeRange => write!(
                f,
                "the child lies entirely outside its composition's source range"
            ),
            Self::NoDuration { schema } => {
                write!(f, "a {schema} has no duration")
            }
            Self::NoAvailableRange { schema } => {
                write!(f, "the available range of a {schema} is not known")
            }
            Self::NoActiveMediaReference { key } => write!(
                f,
                "active_media_reference_key '{key}' names no media reference"
            ),
            Self::ChildAlreadyParented => write!(
                f,
                "the object is already a child of another composition; remove it first"
            ),
            Self::NotImplemented { operation, schema } => {
                write!(f, "a {schema} does not implement {operation}")
            }
            Self::NoLayout => write!(
                f,
                "a bare Composition does not say where its children sit; \
                 use a Track or a Stack"
            ),
            Self::NotAComposition { schema } => {
                write!(f, "a {schema} is not a composition")
            }
            Self::NotAnItem => write!(f, "expected an item at this time"),
            Self::NotAGap => write!(f, "expected a gap at this time"),
            Self::CannotTrimTransition => {
                write!(f, "cannot trim in the middle of a transition")
            }
            Self::UnexpectedChild { schema, parent } => {
                write!(f, "a {parent} cannot hold a {schema}")
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
