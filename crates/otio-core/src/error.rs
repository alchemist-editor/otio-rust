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

    /// A node handle outlived the object it referred to.
    ///
    /// The slot it named has since been reused, so the handle is stale.
    StaleHandle,
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
            Self::StaleHandle => {
                write!(f, "node handle refers to an object that no longer exists")
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
