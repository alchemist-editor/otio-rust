//! A blocking AMQP 0-9-1 client for RabbitMQ: ORI Sync's transport.
//!
//! ```no_run
//! use std::time::Duration;
//!
//! use otio_ext_sync::session::{Config, Event, Session};
//! use otio_ext_sync_transport::amqp::Client;
//! use otio_ext_sync_transport::ori;
//!
//! let options = ori::Options::new("review-42");
//! let mut client = Client::connect("amqp://guest:guest@localhost:5672/", Session::new(Config::default()), options)?;
//! while let Some(event) = client.next_event(Duration::from_secs(5))? {
//!     if let Event::Joined { .. } = event {
//!         break;
//!     }
//! }
//! client.close()?;
//! # Ok::<(), otio_ext_sync_transport::Error>(())
//! ```
//!
//! ORI's peers each bind an exclusive, server-named queue to the fanout
//! exchange `sync_session_<session id>` and publish to it, so every peer
//! hears every message, its own included. This client does the same, with
//! no other dependency than a TCP socket: it speaks just the part of AMQP
//! that needs (PLAIN login, one channel, one exchange, one queue,
//! publishing, consuming without acknowledgements, and heartbeats).
//!
//! A message travels as the body's JSON. A binary [`Frame`] is a
//! zlib-compressed body and goes with the content encoding `deflate`; a body
//! that arrives with `deflate` or `zlib` is handed on as a binary frame.
//! `amqps://` goes through rustls with the Mozilla root certificates. The
//! client reads and writes only inside its calls, and an ORI session keeps
//! its timers there too, so a program calls [`Client::next_event`] often
//! (every 100 ms or so is plenty).

use std::io::{ErrorKind, Read, Write};
use std::net::TcpStream;
use std::sync::Arc;
use std::time::{Duration, Instant};

use otio_ext_sync::session::{Event, Session};

use crate::ori::{self, Ori};
use crate::{Connection, Error, Frame, Profile, now_millis};

/// The content encoding of a zlib-compressed body.
pub const DEFLATE: &str = "deflate";

const FRAME_METHOD: u8 = 1;
const FRAME_HEADER: u8 = 2;
const FRAME_BODY: u8 = 3;
const FRAME_HEARTBEAT: u8 = 8;
const FRAME_END: u8 = 0xCE;
const CHANNEL: u16 = 1;
/// The largest frame this client offers to take, as RabbitMQ's default.
const FRAME_MAX: u32 = 131_072;
/// How long timers may wait for the socket at most.
const TICK: Duration = Duration::from_millis(100);

const CONNECTION_START: (u16, u16) = (10, 10);
const CONNECTION_TUNE: (u16, u16) = (10, 30);
const CONNECTION_OPEN_OK: (u16, u16) = (10, 41);
const CONNECTION_CLOSE: (u16, u16) = (10, 50);
const CONNECTION_CLOSE_OK: (u16, u16) = (10, 51);
const CHANNEL_OPEN_OK: (u16, u16) = (20, 11);
const CHANNEL_CLOSE: (u16, u16) = (20, 40);
const CHANNEL_CLOSE_OK: (u16, u16) = (20, 41);
const EXCHANGE_DECLARE_OK: (u16, u16) = (40, 11);
const QUEUE_DECLARE_OK: (u16, u16) = (50, 11);
const QUEUE_BIND_OK: (u16, u16) = (50, 21);
const BASIC_CONSUME_OK: (u16, u16) = (60, 21);
const BASIC_DELIVER: (u16, u16) = (60, 60);

/// Where and how to reach a broker, from an `amqp://` or `amqps://` URL.
#[derive(Clone, PartialEq, Eq)]
pub struct Address {
    /// Whether the connection goes through TLS.
    pub tls: bool,
    /// The broker's host.
    pub host: String,
    /// The broker's port: 5672, or 5671 for TLS, unless the URL says.
    pub port: u16,
    /// The user to log in as: `guest` unless the URL says.
    pub user: String,
    /// The password: `guest` unless the URL says.
    pub password: String,
    /// The virtual host: `/` unless the URL's path names another.
    pub vhost: String,
}

impl std::fmt::Debug for Address {
    /// Leaves out the password.
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Address")
            .field("tls", &self.tls)
            .field("host", &self.host)
            .field("port", &self.port)
            .field("user", &self.user)
            .field("vhost", &self.vhost)
            .finish_non_exhaustive()
    }
}

impl Address {
    /// Reads an AMQP URL as pika does: `amqp[s]://user:password@host:port/vhost`,
    /// each part percent-encoded, and the query ignored.
    ///
    /// # Errors
    ///
    /// [`Error::Io`] if it is not such a URL.
    pub fn parse(url: &str) -> Result<Self, Error> {
        let invalid = || Error::Io(format!("not an AMQP URL: {}", redact(url)));
        let (tls, rest) = if let Some(rest) = url.strip_prefix("amqps://") {
            (true, rest)
        } else if let Some(rest) = url.strip_prefix("amqp://") {
            (false, rest)
        } else {
            return Err(invalid());
        };
        let rest = rest.split(['?', '#']).next().unwrap_or_default();
        let (authority, path) = rest.split_once('/').unwrap_or((rest, ""));
        let (credentials, host_port) = match authority.rsplit_once('@') {
            Some((credentials, host_port)) => (Some(credentials), host_port),
            None => (None, authority),
        };
        let (user, password) = match credentials {
            Some(credentials) => {
                let (user, password) = credentials.split_once(':').unwrap_or((credentials, ""));
                (percent_decode(user)?, percent_decode(password)?)
            }
            None => ("guest".to_string(), "guest".to_string()),
        };
        let (host, port) = if let Some(bracketed) = host_port.strip_prefix('[') {
            let (host, after) = bracketed.split_once(']').ok_or_else(invalid)?;
            (host.to_string(), after.strip_prefix(':'))
        } else {
            match host_port.rsplit_once(':') {
                Some((host, port)) => (host.to_string(), Some(port)),
                None => (host_port.to_string(), None),
            }
        };
        if host.is_empty() {
            return Err(invalid());
        }
        let port = match port.filter(|port| !port.is_empty()) {
            Some(port) => port.parse().map_err(|_| invalid())?,
            None if tls => 5671,
            None => 5672,
        };
        let vhost = if path.is_empty() {
            "/".to_string()
        } else {
            percent_decode(path)?
        };
        Ok(Self {
            tls,
            host,
            port,
            user,
            password,
            vhost,
        })
    }
}

/// `url` without its password, for messages.
fn redact(url: &str) -> String {
    match (url.find("://"), url.rfind('@')) {
        (Some(scheme), Some(at)) if at > scheme => {
            let credentials = &url[scheme + 3..at];
            let user = credentials.split(':').next().unwrap_or_default();
            format!("{}{user}:***{}", &url[..scheme + 3], &url[at..])
        }
        _ => url.to_string(),
    }
}

fn percent_decode(text: &str) -> Result<String, Error> {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            let hex = text
                .get(index + 1..index + 3)
                .and_then(|hex| u8::from_str_radix(hex, 16).ok())
                .ok_or_else(|| Error::Io("a bad percent escape in an AMQP URL".to_string()))?;
            out.push(hex);
            index += 3;
        } else {
            out.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8(out).map_err(|_| Error::Io("an AMQP URL part is not UTF-8".to_string()))
}

/// The socket, plain or through TLS.
enum Stream {
    Plain(TcpStream),
    Tls(Box<rustls::StreamOwned<rustls::ClientConnection, TcpStream>>),
}

impl Stream {
    fn tcp(&self) -> &TcpStream {
        match self {
            Self::Plain(stream) => stream,
            Self::Tls(stream) => stream.get_ref(),
        }
    }
}

impl Read for Stream {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        match self {
            Self::Plain(stream) => stream.read(buffer),
            Self::Tls(stream) => stream.read(buffer),
        }
    }
}

impl Write for Stream {
    fn write(&mut self, buffer: &[u8]) -> std::io::Result<usize> {
        match self {
            Self::Plain(stream) => stream.write(buffer),
            Self::Tls(stream) => stream.write(buffer),
        }
    }

    fn flush(&mut self) -> std::io::Result<()> {
        match self {
            Self::Plain(stream) => stream.flush(),
            Self::Tls(stream) => stream.flush(),
        }
    }
}

fn io(error: &std::io::Error) -> Error {
    Error::Io(error.to_string())
}

fn open_stream(address: &Address) -> Result<Stream, Error> {
    let tcp = TcpStream::connect((address.host.as_str(), address.port)).map_err(|e| io(&e))?;
    let _ = tcp.set_nodelay(true);
    if !address.tls {
        return Ok(Stream::Plain(tcp));
    }
    let roots = rustls::RootCertStore {
        roots: webpki_roots::TLS_SERVER_ROOTS.to_vec(),
    };
    let config = rustls::ClientConfig::builder_with_provider(Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_safe_default_protocol_versions()
    .map_err(|error| Error::Io(error.to_string()))?
    .with_root_certificates(roots)
    .with_no_client_auth();
    let name = rustls::pki_types::ServerName::try_from(address.host.clone())
        .map_err(|error| Error::Io(error.to_string()))?;
    let connection = rustls::ClientConnection::new(Arc::new(config), name)
        .map_err(|error| Error::Io(error.to_string()))?;
    Ok(Stream::Tls(Box::new(rustls::StreamOwned::new(
        connection, tcp,
    ))))
}

/// Writes AMQP's types.
#[derive(Default)]
struct Writer(Vec<u8>);

impl Writer {
    fn method((class, method): (u16, u16)) -> Self {
        let mut writer = Self::default();
        writer.u16(class).u16(method);
        writer
    }

    fn u8(&mut self, value: u8) -> &mut Self {
        self.0.push(value);
        self
    }

    fn u16(&mut self, value: u16) -> &mut Self {
        self.0.extend_from_slice(&value.to_be_bytes());
        self
    }

    fn u32(&mut self, value: u32) -> &mut Self {
        self.0.extend_from_slice(&value.to_be_bytes());
        self
    }

    fn u64(&mut self, value: u64) -> &mut Self {
        self.0.extend_from_slice(&value.to_be_bytes());
        self
    }

    fn short(&mut self, text: &str) -> &mut Self {
        let bytes = &text.as_bytes()[..text.len().min(255)];
        self.u8(u8::try_from(bytes.len()).unwrap_or(u8::MAX));
        self.0.extend_from_slice(bytes);
        self
    }

    fn long(&mut self, bytes: &[u8]) -> &mut Self {
        self.u32(u32::try_from(bytes.len()).unwrap_or(u32::MAX));
        self.0.extend_from_slice(bytes);
        self
    }

    /// A field table of strings.
    fn table(&mut self, entries: &[(&str, &str)]) -> &mut Self {
        let mut table = Self::default();
        for (key, value) in entries {
            table.short(key).u8(b'S').long(value.as_bytes());
        }
        self.long(&table.0)
    }
}

/// Reads AMQP's types.
struct Reader<'a> {
    data: &'a [u8],
}

impl Reader<'_> {
    fn take(&mut self, count: usize) -> Result<&[u8], Error> {
        if self.data.len() < count {
            return Err(Error::Protocol("an AMQP frame is cut short".to_string()));
        }
        let (taken, rest) = self.data.split_at(count);
        self.data = rest;
        Ok(taken)
    }

    fn u8(&mut self) -> Result<u8, Error> {
        Ok(self.take(1)?[0])
    }

    fn u16(&mut self) -> Result<u16, Error> {
        let bytes = self.take(2)?;
        Ok(u16::from_be_bytes([bytes[0], bytes[1]]))
    }

    fn u32(&mut self) -> Result<u32, Error> {
        let bytes = self.take(4)?;
        Ok(u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
    }

    fn u64(&mut self) -> Result<u64, Error> {
        let bytes = self.take(8)?;
        let mut array = [0; 8];
        array.copy_from_slice(bytes);
        Ok(u64::from_be_bytes(array))
    }

    fn short(&mut self) -> Result<String, Error> {
        let length = usize::from(self.u8()?);
        Ok(String::from_utf8_lossy(self.take(length)?).into_owned())
    }
}

/// A frame read off the socket.
struct RawFrame {
    kind: u8,
    channel: u16,
    payload: Vec<u8>,
}

/// A message being delivered: its encoding and the body so far.
struct Delivery {
    encoding: Option<String>,
    size: u64,
    body: Vec<u8>,
}

/// An ORI session joined over RabbitMQ; see the module documentation.
pub struct Client<P: Profile = Ori> {
    address: Address,
    exchange: String,
    stream: Stream,
    connection: Connection<P>,
    buffer: Vec<u8>,
    frame_max: u32,
    heartbeat: Option<Duration>,
    last_sent: Instant,
    last_received: Instant,
    delivery: Option<Delivery>,
}

impl<P: Profile + std::fmt::Debug> std::fmt::Debug for Client<P> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Client")
            .field("address", &self.address)
            .field("exchange", &self.exchange)
            .field("connection", &self.connection)
            .finish_non_exhaustive()
    }
}

impl Client<Ori> {
    /// Connects to the broker at `url` and starts joining the ORI session
    /// `options` names, on its exchange `sync_session_<id>`.
    ///
    /// # Errors
    ///
    /// [`Error::Io`] if the broker cannot be reached or refuses the login,
    /// and [`Error::Closed`] if it refuses the exchange or queue.
    pub fn connect(url: &str, session: Session, options: ori::Options) -> Result<Self, Error> {
        let exchange = format!("sync_session_{}", options.session_id);
        Self::connect_with(url, &exchange, Connection::new(session, Ori::new(options)))
    }
}

impl<P: Profile> Client<P> {
    /// Connects to the broker at `url`, binds a queue of its own to the
    /// fanout exchange `exchange`, and starts joining over `connection`.
    ///
    /// # Errors
    ///
    /// As [`Client::connect`].
    pub fn connect_with(
        url: &str,
        exchange: &str,
        mut connection: Connection<P>,
    ) -> Result<Self, Error> {
        let address = Address::parse(url)?;
        let stream = open_stream(&address)?;
        let now = Instant::now();
        let mut client = Self {
            address,
            exchange: exchange.to_string(),
            stream,
            connection: {
                connection.disconnected();
                connection
            },
            buffer: Vec::new(),
            frame_max: FRAME_MAX,
            heartbeat: None,
            last_sent: now,
            last_received: now,
            delivery: None,
        };
        client.handshake()?;
        client.connection.connect();
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

    /// The exchange this client publishes to and hears from.
    #[must_use]
    pub fn exchange(&self) -> &str {
        &self.exchange
    }

    fn handshake(&mut self) -> Result<(), Error> {
        let deadline = Instant::now() + Duration::from_secs(30);
        self.stream
            .write_all(b"AMQP\x00\x00\x09\x01")
            .map_err(|e| io(&e))?;
        self.expect(CONNECTION_START, deadline)?;
        let mut response = vec![0];
        response.extend_from_slice(self.address.user.as_bytes());
        response.push(0);
        response.extend_from_slice(self.address.password.as_bytes());
        let mut start_ok = Writer::method((10, 11));
        start_ok
            .table(&[
                ("product", "otio-rust"),
                ("version", env!("CARGO_PKG_VERSION")),
                ("platform", "Rust"),
            ])
            .short("PLAIN")
            .long(&response)
            .short("en_US");
        self.send_method(0, &start_ok.0)?;

        let tune = self
            .expect(CONNECTION_TUNE, deadline)
            .map_err(|error| match error {
                Error::Closed { reason, .. } => {
                    Error::Io(format!("the broker refused the login: {reason}"))
                }
                other => other,
            })?;
        let mut reader = Reader { data: &tune };
        let channel_max = reader.u16()?;
        let frame_max = reader.u32()?;
        let heartbeat = reader.u16()?;
        self.frame_max = if frame_max == 0 {
            FRAME_MAX
        } else {
            frame_max.min(FRAME_MAX)
        };
        self.heartbeat = (heartbeat > 0).then(|| Duration::from_secs(u64::from(heartbeat)));
        let mut tune_ok = Writer::method((10, 31));
        tune_ok
            .u16(if channel_max == 0 {
                CHANNEL
            } else {
                channel_max.min(CHANNEL.max(1))
            })
            .u32(self.frame_max)
            .u16(heartbeat);
        self.send_method(0, &tune_ok.0)?;

        let mut open = Writer::method((10, 40));
        open.short(&self.address.vhost.clone()).short("").u8(0);
        self.send_method(0, &open.0)?;
        self.expect(CONNECTION_OPEN_OK, deadline)?;

        let mut channel = Writer::method((20, 10));
        channel.short("");
        self.send_method(CHANNEL, &channel.0)?;
        self.expect(CHANNEL_OPEN_OK, deadline)?;

        // As ORI's peers declare it: a fanout exchange, not durable, not
        // deleted when unused.
        let mut declare = Writer::method((40, 10));
        declare
            .u16(0)
            .short(&self.exchange.clone())
            .short("fanout")
            .u8(0)
            .table(&[]);
        self.send_method(CHANNEL, &declare.0)?;
        self.expect(EXCHANGE_DECLARE_OK, deadline)?;

        // A queue of its own, named by the broker and gone with the
        // connection.
        let mut queue = Writer::method((50, 10));
        queue.u16(0).short("").u8(0b0100).table(&[]);
        self.send_method(CHANNEL, &queue.0)?;
        let declared = self.expect(QUEUE_DECLARE_OK, deadline)?;
        let queue = Reader { data: &declared }.short()?;

        let mut bind = Writer::method((50, 20));
        bind.u16(0)
            .short(&queue)
            .short(&self.exchange.clone())
            .short("")
            .u8(0)
            .table(&[]);
        self.send_method(CHANNEL, &bind.0)?;
        self.expect(QUEUE_BIND_OK, deadline)?;

        // Consumed without acknowledgements, as ORI's peers consume.
        let mut consume = Writer::method((60, 20));
        consume.u16(0).short(&queue).short("").u8(0b0010).table(&[]);
        self.send_method(CHANNEL, &consume.0)?;
        self.expect(BASIC_CONSUME_OK, deadline)?;
        Ok(())
    }

    /// Reads frames until the method `wanted` arrives, returning its
    /// arguments.
    fn expect(&mut self, wanted: (u16, u16), deadline: Instant) -> Result<Vec<u8>, Error> {
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(Error::Io("the broker did not answer".to_string()));
            }
            let Some(frame) = self.read_frame(remaining)? else {
                continue;
            };
            if frame.kind != FRAME_METHOD {
                continue;
            }
            let mut reader = Reader {
                data: &frame.payload,
            };
            let method = (reader.u16()?, reader.u16()?);
            if method == wanted {
                return Ok(reader.data.to_vec());
            }
            self.control(frame.channel, method, reader.data)?;
        }
    }

    /// Acts on a method that closes the connection or channel.
    fn control(&mut self, channel: u16, method: (u16, u16), arguments: &[u8]) -> Result<(), Error> {
        if method != CONNECTION_CLOSE && method != CHANNEL_CLOSE {
            return Ok(());
        }
        let mut reader = Reader { data: arguments };
        let code = reader.u16()?;
        let reason = reader.short()?;
        let reply = if method == CONNECTION_CLOSE {
            CONNECTION_CLOSE_OK
        } else {
            CHANNEL_CLOSE_OK
        };
        let _ = self.send_method(channel, &Writer::method(reply).0);
        self.connection.disconnected();
        Err(Error::Closed {
            code: Some(code),
            reason,
        })
    }

    /// Reads one frame, waiting up to `timeout`; `None` if none came.
    fn read_frame(&mut self, timeout: Duration) -> Result<Option<RawFrame>, Error> {
        let deadline = Instant::now() + timeout;
        loop {
            if self.buffer.len() >= 7 {
                let size = u32::from_be_bytes([
                    self.buffer[3],
                    self.buffer[4],
                    self.buffer[5],
                    self.buffer[6],
                ]) as usize;
                if size > self.frame_max as usize + 8 && size > FRAME_MAX as usize {
                    return Err(Error::Protocol(format!("an AMQP frame of {size} bytes")));
                }
                if self.buffer.len() >= 8 + size {
                    if self.buffer[7 + size] != FRAME_END {
                        return Err(Error::Protocol("an AMQP frame with no end".to_string()));
                    }
                    let frame = RawFrame {
                        kind: self.buffer[0],
                        channel: u16::from_be_bytes([self.buffer[1], self.buffer[2]]),
                        payload: self.buffer[7..7 + size].to_vec(),
                    };
                    self.buffer.drain(..8 + size);
                    self.last_received = Instant::now();
                    return Ok(Some(frame));
                }
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Ok(None);
            }
            let _ = self
                .stream
                .tcp()
                .set_read_timeout(Some(remaining.max(Duration::from_millis(1))));
            let mut chunk = [0_u8; 16 * 1024];
            match self.stream.read(&mut chunk) {
                Ok(0) => {
                    self.connection.disconnected();
                    return Err(Error::Closed {
                        code: None,
                        reason: "the broker closed the connection".to_string(),
                    });
                }
                Ok(read) => self.buffer.extend_from_slice(&chunk[..read]),
                Err(error)
                    if matches!(error.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) =>
                {
                    return Ok(None);
                }
                Err(error) if error.kind() == ErrorKind::Interrupted => {}
                Err(error) => {
                    self.connection.disconnected();
                    return Err(io(&error));
                }
            }
        }
    }

    fn write_frame(&mut self, kind: u8, channel: u16, payload: &[u8]) -> Result<(), Error> {
        let mut frame = Vec::with_capacity(payload.len() + 8);
        frame.push(kind);
        frame.extend_from_slice(&channel.to_be_bytes());
        frame.extend_from_slice(
            &u32::try_from(payload.len())
                .unwrap_or(u32::MAX)
                .to_be_bytes(),
        );
        frame.extend_from_slice(payload);
        frame.push(FRAME_END);
        if let Err(error) = self
            .stream
            .write_all(&frame)
            .and_then(|()| self.stream.flush())
        {
            self.connection.disconnected();
            return Err(io(&error));
        }
        self.last_sent = Instant::now();
        Ok(())
    }

    fn send_method(&mut self, channel: u16, payload: &[u8]) -> Result<(), Error> {
        self.write_frame(FRAME_METHOD, channel, payload)
    }

    fn publish(&mut self, frame: &Frame) -> Result<(), Error> {
        let mut publish = Writer::method((60, 40));
        publish.u16(0).short(&self.exchange.clone()).short("").u8(0);
        self.send_method(CHANNEL, &publish.0)?;
        // Content type, and content encoding for a compressed body.
        let mut header = Writer::default();
        header.u16(60).u16(0).u64(frame.data.len() as u64);
        if frame.binary {
            header
                .u16(0b1100_0000_0000_0000)
                .short("application/json")
                .short(DEFLATE);
        } else {
            header.u16(0b1000_0000_0000_0000).short("application/json");
        }
        self.write_frame(FRAME_HEADER, CHANNEL, &header.0)?;
        let chunk = (self.frame_max as usize).saturating_sub(8).max(1);
        for body in frame.data.chunks(chunk) {
            self.write_frame(FRAME_BODY, CHANNEL, body)?;
        }
        Ok(())
    }

    /// Sends everything the session and its profile have queued.
    ///
    /// # Errors
    ///
    /// [`Error::Unsupported`] for a message the wire cannot carry, which is
    /// dropped (call again to send the rest), and [`Error::Io`] if the
    /// socket fails.
    pub fn flush(&mut self) -> Result<(), Error> {
        while let Some(frame) = self.connection.poll_transmit(now_millis())? {
            self.publish(&frame)?;
        }
        if let Some(heartbeat) = self.heartbeat {
            if self.last_sent.elapsed() >= heartbeat / 2 {
                self.write_frame(FRAME_HEARTBEAT, 0, &[])?;
            }
            if self.last_received.elapsed() > heartbeat * 2 {
                self.connection.disconnected();
                return Err(Error::Io("the broker stopped answering".to_string()));
            }
        }
        Ok(())
    }

    /// Sends what is queued, then waits up to `timeout` for the next event.
    /// Returns `None` if none came.
    ///
    /// # Errors
    ///
    /// [`Error::Closed`] if the broker closed the connection,
    /// [`Error::Json`] for a message that is not JSON (the connection
    /// stays usable), and the errors of [`Client::flush`]. After a closed
    /// connection or a socket error, [`Client::reconnect`] joins again.
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
            let Some(frame) = self.read_frame(remaining.min(TICK))? else {
                continue;
            };
            self.frame_arrived(frame)?;
        }
    }

    fn frame_arrived(&mut self, frame: RawFrame) -> Result<(), Error> {
        match frame.kind {
            FRAME_METHOD => {
                let mut reader = Reader {
                    data: &frame.payload,
                };
                let method = (reader.u16()?, reader.u16()?);
                if method == BASIC_DELIVER {
                    self.delivery = Some(Delivery {
                        encoding: None,
                        size: 0,
                        body: Vec::new(),
                    });
                    return Ok(());
                }
                self.control(frame.channel, method, reader.data)
            }
            FRAME_HEADER => {
                let Some(delivery) = &mut self.delivery else {
                    return Ok(());
                };
                let mut reader = Reader {
                    data: &frame.payload,
                };
                reader.u16()?;
                reader.u16()?;
                delivery.size = reader.u64()?;
                let flags = reader.u16()?;
                if flags & 0x8000 != 0 {
                    reader.short()?;
                }
                if flags & 0x4000 != 0 {
                    delivery.encoding = Some(reader.short()?);
                }
                self.deliver_if_complete()
            }
            FRAME_BODY => {
                if let Some(delivery) = &mut self.delivery {
                    delivery.body.extend_from_slice(&frame.payload);
                }
                self.deliver_if_complete()
            }
            _ => Ok(()),
        }
    }

    fn deliver_if_complete(&mut self) -> Result<(), Error> {
        if self
            .delivery
            .as_ref()
            .is_none_or(|delivery| (delivery.body.len() as u64) < delivery.size)
        {
            return Ok(());
        }
        let delivery = self.delivery.take().expect("checked above");
        let compressed = delivery.encoding.as_deref().is_some_and(|encoding| {
            encoding.eq_ignore_ascii_case(DEFLATE) || encoding.eq_ignore_ascii_case("zlib")
        });
        let frame = if compressed {
            Frame::binary(delivery.body)
        } else {
            Frame {
                data: delivery.body,
                binary: false,
            }
        };
        self.connection.receive(&frame)
    }

    /// Connects to the broker again and joins again. The session keeps its
    /// timelines.
    ///
    /// # Errors
    ///
    /// As [`Client::connect`].
    pub fn reconnect(&mut self) -> Result<(), Error> {
        self.connection.disconnected();
        self.stream = open_stream(&self.address)?;
        self.buffer.clear();
        self.delivery = None;
        self.handshake()?;
        self.connection.connect();
        self.flush()
    }

    /// Leaves the session and closes the connection.
    ///
    /// # Errors
    ///
    /// [`Error::Io`] if the goodbye cannot be sent.
    pub fn close(mut self) -> Result<(), Error> {
        self.connection.session_mut().leave();
        self.flush()?;
        let mut close = Writer::method(CONNECTION_CLOSE);
        close.u16(200).short("bye").u16(0).u16(0);
        self.send_method(0, &close.0)?;
        let _ = self.expect(CONNECTION_CLOSE_OK, Instant::now() + Duration::from_secs(2));
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_urls_as_pika_does() {
        let address = Address::parse("amqp://localhost").unwrap();
        assert_eq!(
            (
                address.tls,
                address.host.as_str(),
                address.port,
                address.vhost.as_str()
            ),
            (false, "localhost", 5672, "/")
        );
        assert_eq!(
            (address.user.as_str(), address.password.as_str()),
            ("guest", "guest")
        );
        let address =
            Address::parse("amqps://ann:p%40ss@mq.example:1234/review%2Fone?heartbeat=30").unwrap();
        assert_eq!(
            (
                address.tls,
                address.host.as_str(),
                address.port,
                address.vhost.as_str()
            ),
            (true, "mq.example", 1234, "review/one")
        );
        assert_eq!(
            (address.user.as_str(), address.password.as_str()),
            ("ann", "p@ss")
        );
        assert_eq!(Address::parse("amqps://h/").unwrap().port, 5671);
        assert_eq!(Address::parse("amqp://[::1]:5673").unwrap().host, "::1");
        assert!(Address::parse("http://h").is_err());
        assert!(!format!("{:?}", Address::parse("amqp://u:secret@h").unwrap()).contains("secret"));
        assert_eq!(redact("amqp://u:secret@h/"), "amqp://u:***@h/");
    }

    #[test]
    fn writes_field_tables() {
        let mut writer = Writer::default();
        writer.table(&[("a", "bc")]);
        assert_eq!(
            writer.0,
            [0, 0, 0, 9, 1, b'a', b'S', 0, 0, 0, 2, b'b', b'c']
        );
    }
}
