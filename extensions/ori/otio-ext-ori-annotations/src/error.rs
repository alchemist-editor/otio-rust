use std::fmt;

/// Why an annotation could not be read or written.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum Error {
    /// The extension is not on; call `otio_ext::enable("ori-annotations")`.
    NotEnabled,
    /// The object is not of the kind the call needs.
    WrongKind {
        /// What the call needs, such as `a timeline`.
        expected: &'static str,
        /// The schema the object is.
        found: String,
    },
    /// A command field holds a value of the wrong type.
    BadField {
        /// The command's schema, such as `PaintStart`.
        schema: String,
        /// The field.
        field: &'static str,
        /// What it should hold, such as `four numbers`.
        expected: &'static str,
    },
    /// Commands given as JSON are not a list of ORI objects.
    Json(String),
    /// Something otio-core refused.
    Core(otio_core::Error),
}

/// A result whose error is [`Error`].
pub type Result<T> = std::result::Result<T, Error>;

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotEnabled => write!(
                formatter,
                "the ori-annotations extension is not enabled; enable it first"
            ),
            Self::WrongKind { expected, found } => {
                write!(formatter, "expected {expected}, found {found}")
            }
            Self::BadField {
                schema,
                field,
                expected,
            } => write!(formatter, "{schema} field '{field}' should hold {expected}"),
            Self::Json(message) => write!(formatter, "commands JSON: {message}"),
            Self::Core(error) => write!(formatter, "{error}"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Core(error) => Some(error),
            _ => None,
        }
    }
}

impl From<otio_core::Error> for Error {
    fn from(error: otio_core::Error) -> Self {
        Self::Core(error)
    }
}
