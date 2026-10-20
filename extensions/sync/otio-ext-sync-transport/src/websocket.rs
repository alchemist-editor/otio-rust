//! A blocking WebSocket client: a [`Connection`] on a real socket.
//!
//! ```no_run
//! use std::time::Duration;
//!
//! use otio_ext_sync::session::{Config, Event, Session};
//! use otio_ext_sync_transport::websocket::Client;
//!
//! // `wsUrl` and a token from Furnace's `POST /v1/sessions`.
//! let config = Config { token: "…".to_string(), ..Config::default() };
//! let mut client = Client::connect("wss://furnace.example/v1/sessions/s_…/ws", Session::new(config))?;
//! while let Some(event) = client.next_event(Duration::from_secs(5))? {
//!     if let Event::Joined { role, .. } = event {
//!         println!("joined as {}", role.as_str());
//!         break;
//!     }
//! }
//! client.close()?;
//! # Ok::<(), otio_ext_sync_transport::Error>(())
//! ```
//!
//! The client reads and writes only inside its calls, so a program calls
//! [`Client::next_event`] often enough to answer the server's pings (Furnace
//! sends one every 20 seconds and drops a connection that misses two). TLS
//! (`wss://`) goes through rustls with the Mozilla root certificates.

use std::io::ErrorKind;
use std::net::TcpStream;
use std::time::{Duration, Instant};

use otio_ext_sync::session::{Event, Session};
use tungstenite::protocol::frame::coding::CloseCode;
use tungstenite::stream::MaybeTlsStream;
use tungstenite::{Message, WebSocket};

use crate::{Connection, Error, Frame, Furnace, Profile, now_millis};

type Socket = WebSocket<MaybeTlsStream<TcpStream>>;

/// A session joined over a WebSocket; see the module documentation.
#[derive(Debug)]
pub struct Client<P: Profile = Furnace> {
    url: String,
    socket: Socket,
    connection: Connection<P>,
}

impl Client<Furnace> {
    /// Opens a socket to `url`, Furnace's `wsUrl`, and starts joining
    /// `session` over Furnace's protocol. The session's token is added to
    /// the URL as `token` unless the URL has one.
    ///
    /// # Errors
    ///
    /// [`Error::Io`] if the socket cannot be opened.
    pub fn connect(url: &str, session: Session) -> Result<Self, Error> {
        let url = with_token(url, &session.config().token);
        Self::connect_with(&url, Connection::new(session, Furnace::new()))
    }
}

impl<P: Profile> Client<P> {
    /// Opens a socket to `url` and starts joining over `connection`. The
    /// URL is used as given: a server that wants the token in it, as
    /// Furnace does, needs it there already.
    ///
    /// # Errors
    ///
    /// [`Error::Io`] if the socket cannot be opened.
    pub fn connect_with(url: &str, mut connection: Connection<P>) -> Result<Self, Error> {
        let url = url.to_string();
        let socket = open(&url)?;
        connection.connect();
        let mut client = Self {
            url,
            socket,
            connection,
        };
        client.flush()?;
        Ok(client)
    }

    /// The connection.
    pub const fn connection(&self) -> &Connection<P> {
        &self.connection
    }

    /// The connection, mutably.
    pub const fn connection_mut(&mut self) -> &mut Connection<P> {
        &mut self.connection
    }

    /// The session.
    pub const fn session(&self) -> &Session {
        self.connection.session()
    }

    /// The session, to publish, move the playhead and so on; what it queues
    /// goes at the next [`Client::flush`] or [`Client::next_event`].
    pub const fn session_mut(&mut self) -> &mut Session {
        self.connection.session_mut()
    }

    /// Sends everything the session has queued.
    ///
    /// # Errors
    ///
    /// [`Error::Unsupported`] for a message the wire cannot carry, which is
    /// dropped (call again to send the rest), and [`Error::Io`] or
    /// [`Error::Closed`] if the socket fails.
    pub fn flush(&mut self) -> Result<(), Error> {
        while let Some(frame) = self.connection.poll_transmit(now_millis())? {
            let message = if frame.binary {
                Message::Binary(frame.data.into())
            } else {
                let text = String::from_utf8(frame.data)
                    .map_err(|_| Error::Protocol("a text frame is not UTF-8".to_string()))?;
                Message::Text(text.into())
            };
            self.socket
                .send(message)
                .map_err(|error| self.socket_error(error))?;
        }
        Ok(())
    }

    /// Sends what is queued, then waits up to `timeout` for the next event.
    /// Returns `None` if none came.
    ///
    /// # Errors
    ///
    /// [`Error::Closed`] once the connection has closed (an [`Event::Error`]
    /// or [`Event::Ended`] before it says why), [`Error::Gap`] if frames
    /// were lost, and the errors of [`Client::flush`]. After any of them,
    /// [`Client::reconnect`] joins again.
    pub fn next_event(&mut self, timeout: Duration) -> Result<Option<Event>, Error> {
        let deadline = Instant::now() + timeout;
        loop {
            self.flush()?;
            if let Some(event) = self.connection.poll_event() {
                return Ok(Some(event));
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Ok(None);
            }
            set_read_timeout(&self.socket, remaining.max(Duration::from_millis(1)));
            match self.socket.read() {
                Ok(Message::Text(text)) => self.connection.receive(&Frame::text(text.as_str()))?,
                Ok(Message::Binary(data)) => {
                    self.connection.receive(&Frame::binary(data.to_vec()))?
                }
                Ok(Message::Close(frame)) => {
                    self.connection.disconnected();
                    if let Some(event) = self.connection.poll_event() {
                        return Ok(Some(event));
                    }
                    return Err(Error::Closed {
                        code: frame.as_ref().map(|frame| u16::from(frame.code)),
                        reason: frame
                            .map(|frame| frame.reason.to_string())
                            .unwrap_or_default(),
                    });
                }
                Ok(_) => {}
                Err(tungstenite::Error::Io(error))
                    if matches!(error.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => {}
                Err(error) => return Err(self.socket_error(error)),
            }
        }
    }

    /// Opens a new socket to the same URL and joins again. The session
    /// keeps its timelines and unacknowledged review commits.
    ///
    /// # Errors
    ///
    /// [`Error::Io`] if the socket cannot be opened.
    pub fn reconnect(&mut self) -> Result<(), Error> {
        self.socket = open(&self.url)?;
        self.connection.connect();
        self.flush()
    }

    /// Leaves the session and closes the socket.
    ///
    /// # Errors
    ///
    /// [`Error::Io`] if the goodbye cannot be sent.
    pub fn close(mut self) -> Result<(), Error> {
        self.connection.session_mut().leave();
        self.flush()?;
        let _ = self.socket.close(Some(tungstenite::protocol::CloseFrame {
            code: CloseCode::Normal,
            reason: "bye".into(),
        }));
        let _ = self.socket.flush();
        Ok(())
    }

    fn socket_error(&mut self, error: tungstenite::Error) -> Error {
        match error {
            tungstenite::Error::ConnectionClosed | tungstenite::Error::AlreadyClosed => {
                self.connection.disconnected();
                Error::Closed {
                    code: None,
                    reason: "the socket closed".to_string(),
                }
            }
            other => {
                self.connection.disconnected();
                Error::Io(other.to_string())
            }
        }
    }
}

fn open(url: &str) -> Result<Socket, Error> {
    let (socket, _) = tungstenite::connect(url).map_err(|error| Error::Io(error.to_string()))?;
    Ok(socket)
}

fn set_read_timeout(socket: &Socket, timeout: Duration) {
    let stream = match socket.get_ref() {
        MaybeTlsStream::Plain(stream) => stream,
        MaybeTlsStream::Rustls(stream) => stream.get_ref(),
        _ => return,
    };
    let _ = stream.set_read_timeout(Some(timeout));
}

/// `url` with `token=<token>` in its query, unless it has a token already.
fn with_token(url: &str, token: &str) -> String {
    let has_token = url
        .split_once('?')
        .map(|(_, query)| query.split('#').next().unwrap_or_default())
        .is_some_and(|query| {
            query
                .split('&')
                .any(|pair| pair.split('=').next() == Some("token"))
        });
    if token.is_empty() || has_token {
        return url.to_string();
    }
    let separator = if url.contains('?') { '&' } else { '?' };
    let encoded: String = token
        .bytes()
        .map(|byte| match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                char::from(byte).to_string()
            }
            other => format!("%{other:02X}"),
        })
        .collect();
    format!("{url}{separator}token={encoded}")
}

#[cfg(test)]
mod tests {
    use super::with_token;

    #[test]
    fn adds_the_token_to_the_query() {
        assert_eq!(with_token("ws://h/ws", "a.b"), "ws://h/ws?token=a.b");
        assert_eq!(
            with_token("ws://h/ws?x=1", "a b"),
            "ws://h/ws?x=1&token=a%20b"
        );
        assert_eq!(
            with_token("ws://h/ws?token=t", "other"),
            "ws://h/ws?token=t"
        );
        assert_eq!(
            with_token("ws://h/ws?access_token=x&next=token=y", "t"),
            "ws://h/ws?access_token=x&next=token=y&token=t"
        );
        assert_eq!(
            with_token("ws://h/ws?a=1&token=t", "u"),
            "ws://h/ws?a=1&token=t"
        );
    }
}
