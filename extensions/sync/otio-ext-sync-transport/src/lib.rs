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
//! - [`ori`]: ORI Sync's envelope, peer to peer with an elected master.
//! - [`compression`]: gzip and Brotli, negotiated by the profile.
//! - [`json`]: a session's settings, events and state as JSON, for the C
//!   interface and the bindings over it.
//! - [`loopback`]: an in-memory room for tests, which relays as Furnace does.
//! - `websocket` (on by default): a blocking WebSocket client.
//! - `amqp`: a blocking AMQP 0-9-1 client for RabbitMQ, ORI's transport.

#[cfg(feature = "amqp")]
pub mod amqp;
pub mod compression;
pub mod furnace;
pub mod json;
pub mod loopback;
pub mod ori;
#[cfg(feature = "websocket")]
pub mod websocket;

use std::fmt;

use otio_ext_sync::session::{Event, Inbound, Outbound, Session};

pub use furnace::Furnace;
pub use ori::Ori;

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
    /// A profile that has to read the session to make sense of a frame (a
    /// peer-to-peer wire, such as ORI's) returns none here and keeps the
    /// frame for [`Profile::poll_inbound`].
    ///
    /// # Errors
    ///
    /// Any [`Error`] but [`Error::Unsupported`] and [`Error::Session`]; all
    /// of them mean the connection is no longer usable.
    fn decode(&mut self, frame: &Frame) -> Result<Vec<Inbound>, Error>;

    /// Tells the profile the time, in Unix milliseconds, before the
    /// connection asks it for anything else. A wire with no server keeps
    /// its own timers here (ORI's election and heartbeats).
    fn advance(&mut self, now: i64) {
        let _ = now;
    }

    /// The next messages for the session, made against `session` as it now
    /// is, or `None` when the profile has nothing waiting. The connection
    /// gives the session each batch before asking for the next, so a batch
    /// may depend on the one before it.
    fn poll_inbound(&mut self, session: &Session) -> Option<Vec<Inbound>> {
        let _ = session;
        None
    }

    /// The next frame the profile sends of its own accord (an answer to
    /// another peer, a heartbeat, or the rest of what one message became),
    /// if any. These go before the session's next message.
    ///
    /// # Errors
    ///
    /// [`Error::Json`] if the session holds a timeline that is not JSON.
    fn poll_frame(&mut self, session: &Session) -> Result<Option<Frame>, Error> {
        let _ = session;
        Ok(None)
    }
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
    /// What the profile's [`Profile::decode`] returns, in which case the
    /// session has not seen the frame; or [`Error::Session`] with
    /// [`otio_ext_sync::session::Error::OutOfSync`] when the host's
    /// patches did not fit this client's copy of the timeline. Either way
    /// the connection should be dropped and made again.
    pub fn receive(&mut self, frame: &Frame) -> Result<(), Error> {
        let decoded = self.profile.decode(frame)?;
        self.deliver(decoded)?;
        self.deliver_waiting()
    }

    fn deliver(&mut self, messages: Vec<Inbound>) -> Result<(), Error> {
        for message in messages {
            self.session.receive(message);
            if let Some(fault) = self.session.take_fault() {
                return Err(Error::Session(fault));
            }
        }
        Ok(())
    }

    /// Hands the session what the profile has waiting, a batch at a time.
    fn deliver_waiting(&mut self) -> Result<(), Error> {
        while let Some(messages) = self.profile.poll_inbound(&self.session) {
            self.deliver(messages)?;
        }
        Ok(())
    }

    /// The next frame to send, at `now` (Unix milliseconds), if any.
    ///
    /// On a wire with no server, such as ORI's, this is also what keeps the
    /// profile's timers, so a program calls it now and then even with nothing
    /// to send (every 100 ms or so does).
    ///
    /// # Errors
    ///
    /// [`Error::Unsupported`] for a message this wire cannot carry; it is
    /// dropped, and the next call goes on with the one after it. As
    /// [`Connection::receive`] for what a profile's timers handed the
    /// session.
    pub fn poll_transmit(&mut self, now: i64) -> Result<Option<Frame>, Error> {
        self.profile.advance(now);
        self.deliver_waiting()?;
        if let Some(frame) = self.profile.poll_frame(&self.session)? {
            return Ok(Some(frame));
        }
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
