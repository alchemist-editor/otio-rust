//! Status codes, and the message left behind by the last failing call.
//!
//! Every entry point that can fail returns an [`OtioStatus`] and delivers its
//! result through an out-parameter. A caller that wants more than the code
//! calls [`otio_error_message`], which describes the last failure on the
//! calling thread.

use std::cell::RefCell;
use std::ffi::{CString, c_char};
use std::panic::{self, AssertUnwindSafe};

/// What a call did, or why it could not.
///
/// `OTIO_STATUS_OK` is zero, so `if (otio_...(...)) { /* failed */ }` reads
/// the way a C programmer expects.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OtioStatus {
    /// The call succeeded.
    Ok = 0,
    /// A pointer argument that may not be null was null.
    NullPointer = 1,
    /// A string argument was not valid UTF-8.
    InvalidUtf8 = 2,
    /// The call succeeded, and the answer is that there is no value.
    ///
    /// An item with no `source_range` and a clip with no active media
    /// reference both report this. It is not an error.
    NoValue = 3,
    /// A node handle named an object that no longer exists.
    StaleHandle = 4,
    /// An argument was outside the range the call accepts.
    InvalidArgument = 5,
    /// The document could not answer the question asked of it.
    ///
    /// Asking a marker for its duration, or a track for a child it does not
    /// hold, lands here. [`otio_error_message`] says which.
    CoreError = 6,
    /// A timecode or time string could not be read or written.
    TimeError = 7,
    /// A file was not valid for the format it was read as.
    ParseError = 8,
    /// The document holds something the target format cannot express.
    Unsupported = 9,
    /// A file could not be read from or written to disk.
    IoError = 10,
    /// A panic in the Rust core was caught at the boundary.
    ///
    /// The library is left in an unspecified state; a caller that sees this
    /// should stop using the document it was working on.
    Panic = 11,
}

impl OtioStatus {
    /// Returns the status's name, as it is spelled in `otio.h`.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Ok => "OTIO_STATUS_OK",
            Self::NullPointer => "OTIO_STATUS_NULL_POINTER",
            Self::InvalidUtf8 => "OTIO_STATUS_INVALID_UTF8",
            Self::NoValue => "OTIO_STATUS_NO_VALUE",
            Self::StaleHandle => "OTIO_STATUS_STALE_HANDLE",
            Self::InvalidArgument => "OTIO_STATUS_INVALID_ARGUMENT",
            Self::CoreError => "OTIO_STATUS_CORE_ERROR",
            Self::TimeError => "OTIO_STATUS_TIME_ERROR",
            Self::ParseError => "OTIO_STATUS_PARSE_ERROR",
            Self::Unsupported => "OTIO_STATUS_UNSUPPORTED",
            Self::IoError => "OTIO_STATUS_IO_ERROR",
            Self::Panic => "OTIO_STATUS_PANIC",
        }
    }
}

/// A status code and the sentence that goes with it.
///
/// This is the crate's internal error type. It never crosses the boundary as
/// itself: the status is returned and the message is left for
/// [`otio_error_message`].
#[derive(Debug)]
pub(crate) struct Fault {
    pub(crate) status: OtioStatus,
    pub(crate) message: String,
}

impl Fault {
    /// Builds a fault with an explicit status and message.
    pub(crate) fn new(status: OtioStatus, message: impl Into<String>) -> Self {
        Self {
            status,
            message: message.into(),
        }
    }

    /// A null pointer was passed where one is required.
    pub(crate) fn null(what: &str) -> Self {
        Self::new(OtioStatus::NullPointer, format!("{what} must not be null"))
    }

    /// The call has no value to report, which is an answer rather than a
    /// failure.
    pub(crate) fn no_value(what: &str) -> Self {
        Self::new(OtioStatus::NoValue, format!("{what} is not set"))
    }

    /// An argument was outside the accepted range.
    pub(crate) fn invalid(message: impl Into<String>) -> Self {
        Self::new(OtioStatus::InvalidArgument, message)
    }
}

impl From<otio_core::Error> for Fault {
    fn from(error: otio_core::Error) -> Self {
        let status = if matches!(error, otio_core::Error::StaleHandle) {
            OtioStatus::StaleHandle
        } else if matches!(error, otio_core::Error::Json { .. }) {
            OtioStatus::ParseError
        } else {
            OtioStatus::CoreError
        };
        Self::new(status, error.to_string())
    }
}

impl From<opentime::TimeError> for Fault {
    fn from(error: opentime::TimeError) -> Self {
        Self::new(OtioStatus::TimeError, error.to_string())
    }
}

impl From<otio_adapter::Error> for Fault {
    fn from(error: otio_adapter::Error) -> Self {
        let status = match &error {
            otio_adapter::Error::Io(_) => OtioStatus::IoError,
            otio_adapter::Error::Unsupported { .. } => OtioStatus::Unsupported,
            otio_adapter::Error::Core(core) => return Self::from(core.clone()),
            otio_adapter::Error::Time(time) => return Self::from(time.clone()),
            _ => OtioStatus::ParseError,
        };
        Self::new(status, error.to_string())
    }
}

/// The result of an internal step, before it is flattened into a status.
pub(crate) type Outcome<T> = Result<T, Fault>;

thread_local! {
    /// The message from the last failing call on this thread.
    static LAST_ERROR: RefCell<CString> = RefCell::new(CString::default());
}

/// Records a message for [`otio_error_message`] to hand back.
fn set_error(message: &str) {
    // A message with an interior NUL cannot be a C string; truncating at the
    // NUL keeps as much of it as C can carry rather than losing all of it.
    let sanitized = match message.find('\0') {
        Some(index) => &message[..index],
        None => message,
    };
    let value = CString::new(sanitized).unwrap_or_default();
    LAST_ERROR.with(|slot| *slot.borrow_mut() = value);
}

/// Runs an entry point's body, turning its outcome into a status.
///
/// Every entry point goes through here, so a panic becomes
/// [`OtioStatus::Panic`] rather than unwinding into C, and every failure
/// leaves a message behind.
pub(crate) fn guard(body: impl FnOnce() -> Outcome<()>) -> OtioStatus {
    match panic::catch_unwind(AssertUnwindSafe(body)) {
        Ok(Ok(())) => {
            set_error("");
            OtioStatus::Ok
        }
        Ok(Err(fault)) => {
            set_error(&fault.message);
            fault.status
        }
        Err(_) => {
            set_error("a panic in the Rust core was caught at the C boundary");
            OtioStatus::Panic
        }
    }
}

/// Runs an entry point's body that returns a plain value rather than a status.
///
/// These are the calls that cannot fail: they answer a question about values
/// the caller already holds. A panic still has to be caught, so they take the
/// answer to give if one happens.
pub(crate) fn guard_value<T>(fallback: T, body: impl FnOnce() -> T) -> T {
    match panic::catch_unwind(AssertUnwindSafe(body)) {
        Ok(value) => value,
        Err(_) => {
            set_error("a panic in the Rust core was caught at the C boundary");
            fallback
        }
    }
}

/// Returns the message describing the last failing call on this thread.
///
/// The string is owned by the library and stays valid until the next `otio_*`
/// call on this thread. After a call that succeeded it is empty. It is never
/// null.
#[unsafe(no_mangle)]
pub extern "C" fn otio_error_message() -> *const c_char {
    LAST_ERROR.with(|slot| slot.borrow().as_ptr())
}

/// Returns the name of a status code, such as `"OTIO_STATUS_OK"`.
///
/// The string is static and needs no freeing.
#[unsafe(no_mangle)]
pub extern "C" fn otio_status_name(status: OtioStatus) -> *const c_char {
    let name: &'static str = match status {
        OtioStatus::Ok => "OTIO_STATUS_OK\0",
        OtioStatus::NullPointer => "OTIO_STATUS_NULL_POINTER\0",
        OtioStatus::InvalidUtf8 => "OTIO_STATUS_INVALID_UTF8\0",
        OtioStatus::NoValue => "OTIO_STATUS_NO_VALUE\0",
        OtioStatus::StaleHandle => "OTIO_STATUS_STALE_HANDLE\0",
        OtioStatus::InvalidArgument => "OTIO_STATUS_INVALID_ARGUMENT\0",
        OtioStatus::CoreError => "OTIO_STATUS_CORE_ERROR\0",
        OtioStatus::TimeError => "OTIO_STATUS_TIME_ERROR\0",
        OtioStatus::ParseError => "OTIO_STATUS_PARSE_ERROR\0",
        OtioStatus::Unsupported => "OTIO_STATUS_UNSUPPORTED\0",
        OtioStatus::IoError => "OTIO_STATUS_IO_ERROR\0",
        OtioStatus::Panic => "OTIO_STATUS_PANIC\0",
    };
    name.as_ptr().cast::<c_char>()
}

/// Returns the library's version, as `"MAJOR.MINOR.PATCH"`.
///
/// The string is static and needs no freeing.
#[unsafe(no_mangle)]
pub extern "C" fn otio_version() -> *const c_char {
    concat!(env!("CARGO_PKG_VERSION"), "\0")
        .as_ptr()
        .cast::<c_char>()
}
