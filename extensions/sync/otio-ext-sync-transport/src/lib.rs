//! Wire profiles and transports for timeline sync.
//!
//! **This is an otio-rust extension, not part of OpenTimelineIO.** It
//! carries an [`otio_ext_sync::session::Session`] between programs. The
//! session decides what to say; a wire [`Profile`] turns that into frames and
//! back; a transport moves the frames.
//!
//! ```text
//!  program ──► Session ──Outbound──► Profile ──Frame──► transport ──► server
//!  program ◄── Session ◄──Inbound─── Profile ◄─Frame─── transport ◄── server
//! ```
//!
//! A [`Connection`] joins a session to a profile, so a program that owns its
//! own socket, in any language, hands it frames and the time and gets frames
//! and events back. This crate has:
//!
//! - [`furnace`]: Alchemist's Furnace protocol v2, the first profile.
//! - [`compression`]: gzip and Brotli, negotiated by the profile.
//! - [`loopback`]: an in-memory room for tests, which relays as Furnace does.
//! - `websocket` (on by default): a blocking WebSocket client.
//!
//! ORI Sync's envelope and AMQP come later (epic #115).

pub mod compression;
pub mod furnace;
pub mod loopback;
#[cfg(feature = "websocket")]
pub mod websocket;

use std::fmt;

use otio_ext_sync::session::{Event, Inbound, Outbound, Session};

pub use furnace::Furnace;

/// One WebSocket message: JSON text, or a binary frame holding compressed
/// JSON ([`compression`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Frame {
    /// The frame's bytes.
    pub data: Vec<u8>,
    /// Whether it is a binary frame rather than a text frame.
    pub binary: bool,
}

impl Frame {
    /// A text frame.
    #[must_use]
    pub fn text(text: impl Into<String>) -> Self {
        Self {
            data: text.into().into_bytes(),
            binary: false,
        }
    }

    /// A binary frame.
    #[must_use]
    pub const fn binary(data: Vec<u8>) -> Self {
        Self { data, binary: true }
    }

    /// The frame as text, if it is a text frame of UTF-8.
    #[must_use]
    pub fn as_text(&self) -> Option<&str> {
        if self.binary {
            return None;
        }
        std::str::from_utf8(&self.data).ok()
    }
}

/// Why a frame could not be made, read or moved.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Error {
    /// A message is not the JSON the profile expects.
    Json(String),
    /// A frame breaks the protocol.
    Protocol(String),
    /// The server's frames skipped some: the connection lost messages, so
    /// the program should reconnect.
    Gap {
        /// The order the next frame should have had.
        expected: u64,
        /// The order it had.
        got: u64,
    },
    /// The profile cannot carry the message.
    Unsupported(String),
    /// A frame could not be compressed or expanded.
    Compression(String),
    /// The connection closed.
    Closed {
        /// The WebSocket close code, if there was one.
        code: Option<u16>,
        /// The reason given.
        reason: String,
    },
    /// The socket failed.
    Io(String),
    /// The session refused something.
    Session(otio_ext_sync::session::Error),
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Json(message) => write!(formatter, "invalid message: {message}"),
            Self::Protocol(message) => write!(formatter, "protocol violation: {message}"),
            Self::Gap { expected, got } => {
                write!(
                    formatter,
                    "frames were lost: expected order {expected}, got {got}"
                )
            }
            Self::Unsupported(message) => formatter.write_str(message),
            Self::Compression(message) => write!(formatter, "compression: {message}"),
            Self::Closed { code, reason } => match code {
                Some(code) => write!(formatter, "the connection closed ({code}): {reason}"),
                None => write!(formatter, "the connection closed: {reason}"),
            },
            Self::Io(message) => formatter.write_str(message),
            Self::Session(error) => error.fmt(formatter),
        }
    }
}

impl std::error::Error for Error {}

impl From<otio_ext_sync::session::Error> for Error {
    fn from(error: otio_ext_sync::session::Error) -> Self {
        Self::Session(error)
    }
}

/// Turns a session's messages into frames and back, for one kind of server.
pub trait Profile {
    /// Forgets the connection: the next frame is the first of a new one.
    fn reset(&mut self);

    /// The frame for `message`, sent at `now` (Unix milliseconds).
    ///
    /// # Errors
    ///
    /// [`Error::Unsupported`] if this wire cannot carry the message, and
    /// [`Error::Json`] if a JSON field in it is not JSON.
    fn encode(&mut self, message: &Outbound, now: i64) -> Result<Frame, Error>;

    /// The messages in `frame`.
    ///
    /// # Errors
    ///
    /// Any [`Error`] but [`Error::Unsupported`] and [`Error::Session`]; all
    /// of them mean the connection is no longer usable.
    fn decode(&mut self, frame: &Frame) -> Result<Vec<Inbound>, Error>;
}

/// A session on a wire: frames and the time in, frames and events out.
///
/// ```
/// use otio_ext_sync::session::{Config, Session};
/// use otio_ext_sync_transport::{Connection, Furnace};
///
/// let mut connection = Connection::new(Session::new(Config::default()), Furnace::new());
/// connection.connect();
/// let hello = connection.poll_transmit(1_700_000_000_000)?.expect("the hello");
/// assert!(hello.as_text().unwrap().contains(r#""type":"hello""#));
/// # Ok::<(), otio_ext_sync_transport::Error>(())
/// ```
#[derive(Debug)]
pub struct Connection<P: Profile = Furnace> {
    session: Session,
    profile: P,
}

impl<P: Profile> Connection<P> {
    /// Puts `session` on the wire `profile` describes.
    pub const fn new(session: Session, profile: P) -> Self {
        Self { session, profile }
    }

    /// The session.
    pub const fn session(&self) -> &Session {
        &self.session
    }

    /// The session, to publish, move the playhead and so on.
    pub const fn session_mut(&mut self) -> &mut Session {
        &mut self.session
    }

    /// The profile.
    pub const fn profile(&self) -> &P {
        &self.profile
    }

    /// Starts a new connection: forgets the old one's numbering and queues
    /// the hello. Call it once the socket is open, and again after a
    /// reconnect.
    pub fn connect(&mut self) {
        self.profile.reset();
        self.session.connect();
    }

    /// Takes in a frame that arrived.
    ///
    /// # Errors
    ///
    /// What the profile's [`Profile::decode`] returns. The session has not
    /// seen the frame, and the connection should be dropped and made again.
    pub fn receive(&mut self, frame: &Frame) -> Result<(), Error> {
        for message in self.profile.decode(frame)? {
            self.session.receive(message);
        }
        Ok(())
    }

    /// The next frame to send, at `now` (Unix milliseconds), if any.
    ///
    /// # Errors
    ///
    /// [`Error::Unsupported`] for a message this wire cannot carry; it is
    /// dropped, and the next call goes on with the one after it.
    pub fn poll_transmit(&mut self, now: i64) -> Result<Option<Frame>, Error> {
        match self.session.poll_outbound() {
            Some(message) => self.profile.encode(&message, now).map(Some),
            None => Ok(None),
        }
    }

    /// The next event, if any.
    pub fn poll_event(&mut self) -> Option<Event> {
        self.session.poll_event()
    }

    /// The socket closed: the session waits for [`Connection::connect`].
    pub fn disconnected(&mut self) {
        self.session.disconnected();
    }
}

/// The time now, in Unix milliseconds, for [`Connection::poll_transmit`].
#[must_use]
pub fn now_millis() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| {
            i64::try_from(elapsed.as_millis()).unwrap_or(i64::MAX)
        })
}
