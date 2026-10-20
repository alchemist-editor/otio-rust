//! What a session says and hears, whatever the wire.

use crate::patch::{Patch, Snapshot};

/// How a channel's messages are delivered.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Delivery {
    /// Only the newest message matters: one not yet sent is replaced by a
    /// newer one, and a receiver may miss the ones in between. The playhead,
    /// a selection and presence travel this way.
    LatestWins,
    /// Every message arrives, in the order it was sent. Timeline changes
    /// travel this way.
    Ordered,
    /// Every message is sent once and never stored or sent again after a
    /// reconnect.
    Transient,
    /// Every message is kept until the server acknowledges it, sent again
    /// after a reconnect, and acknowledged once however often it is sent:
    /// Furnace's review ledger.
    Ledger,
}

/// A named stream of messages with one [`Delivery`] rule.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Channel {
    /// The timeline: snapshots and patches. [`Delivery::Ordered`].
    Timeline,
    /// The playhead. [`Delivery::LatestWins`].
    Playback,
    /// The objects someone has selected, by GUID. [`Delivery::LatestWins`].
    Selection,
    /// Who is in the session. [`Delivery::LatestWins`].
    Presence,
    /// Review comments and annotations committed to the session.
    /// [`Delivery::Ledger`].
    Review,
    /// Messages between two peers, such as WebRTC signalling.
    /// [`Delivery::Transient`].
    Signal,
    /// Session control: leadership, muting and leaving.
    /// [`Delivery::Ordered`].
    Control,
    /// A channel another extension registered, by name.
    Named(String),
}

impl Channel {
    /// The channel's name: `timeline`, `playback`, `selection`, `presence`,
    /// `review`, `signal`, `control`, or a named channel's own.
    #[must_use]
    pub fn name(&self) -> &str {
        match self {
            Self::Timeline => "timeline",
            Self::Playback => "playback",
            Self::Selection => "selection",
            Self::Presence => "presence",
            Self::Review => "review",
            Self::Signal => "signal",
            Self::Control => "control",
            Self::Named(name) => name,
        }
    }

    /// The channel called `name`; any name sync does not define is a
    /// [`Channel::Named`].
    #[must_use]
    pub fn from_name(name: &str) -> Self {
        match name {
            "timeline" => Self::Timeline,
            "playback" => Self::Playback,
            "selection" => Self::Selection,
            "presence" => Self::Presence,
            "review" => Self::Review,
            "signal" => Self::Signal,
            "control" => Self::Control,
            other => Self::Named(other.to_string()),
        }
    }

    /// How the channels sync defines are delivered. A named channel's rule
    /// is the one it was registered with
    /// ([`Session::register_channel`](super::Session::register_channel));
    /// this returns [`Delivery::Ordered`] for it.
    #[must_use]
    pub const fn delivery(&self) -> Delivery {
        match self {
            Self::Playback | Self::Selection | Self::Presence => Delivery::LatestWins,
            Self::Timeline | Self::Control | Self::Named(_) => Delivery::Ordered,
            Self::Signal => Delivery::Transient,
            Self::Review => Delivery::Ledger,
        }
    }
}

/// A participant's part in a session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Role {
    /// The participant the server appointed to own the timeline: Furnace's
    /// host. Only the host may change the timeline.
    Host,
    /// Everyone else.
    Viewer,
    /// A role this version does not know, by the name the wire gave it.
    Other(String),
}

impl Role {
    /// The role's name on the wire: `host`, `viewer` or the other name.
    #[must_use]
    pub fn as_str(&self) -> &str {
        match self {
            Self::Host => "host",
            Self::Viewer => "viewer",
            Self::Other(name) => name,
        }
    }

    /// The role called `name`.
    #[must_use]
    pub fn from_name(name: &str) -> Self {
        match name {
            "host" => Self::Host,
            "viewer" => Self::Viewer,
            other => Self::Other(other.to_string()),
        }
    }
}

/// Who a session says it is when it joins.
///
/// Furnace's protocol v2 requires every field but `capabilities`; the
/// defaults describe a desktop editor.
#[derive(Clone, PartialEq, Eq)]
pub struct Identity {
    /// The name shown to other participants.
    pub display_name: String,
    /// The program, such as `Alchemist`.
    pub app: String,
    /// The program's version.
    pub app_version: String,
    /// An unguessable secret naming the person, so their devices are grouped
    /// as one member. The default is a fresh random value.
    pub credential: String,
    /// A stable id for this device.
    pub device_id: String,
    /// What kind of client this is. Furnace knows `native-editor`, `web`,
    /// `apple-tv` and `apple-second-screen`.
    pub client_kind: String,
    /// The device's shape: `desktop`, `browser`, `tv`, `phone` or `tablet`.
    pub form_factor: String,
    /// The device's name, as its owner would recognise it.
    pub device_name: String,
    /// What this client can do, such as `review.comment.create`. Furnace
    /// checks these before relaying review commits.
    pub capabilities: Vec<String>,
}

impl Default for Identity {
    fn default() -> Self {
        Self {
            display_name: "otio-rust".to_string(),
            app: "otio-rust".to_string(),
            app_version: env!("CARGO_PKG_VERSION").to_string(),
            credential: format!("guest_{}", secret_hex()),
            device_id: format!("device_{}", crate::guid::new_guid().replace('-', "")),
            client_kind: "native-editor".to_string(),
            form_factor: "desktop".to_string(),
            device_name: "otio-rust".to_string(),
            capabilities: Vec::new(),
        }
    }
}

/// 128 bits from the operating system's secure random source, as hex: the
/// default credential, which stands for a person and must not be guessable.
/// Where there is no such source (WebAssembly), the GUID generator's bits,
/// which are not secret: a program there passes its own credential.
fn secret_hex() -> String {
    #[cfg(not(target_family = "wasm"))]
    {
        let mut bytes = [0_u8; 16];
        if getrandom::fill(&mut bytes).is_ok() {
            return bytes.iter().map(|byte| format!("{byte:02x}")).collect();
        }
    }
    crate::guid::new_guid().replace('-', "")
}

/// How a session joins: the token the server issued and who it is.
#[derive(Clone, Default, PartialEq, Eq)]
pub struct Config {
    /// The token that admits this client, from the server.
    pub token: String,
    /// Who the client is.
    pub identity: Identity,
}

/// What `Debug` shows of a secret: whether there is one, never its value.
const fn redacted(secret: &str) -> &'static str {
    if secret.is_empty() { "" } else { "<redacted>" }
}

impl std::fmt::Debug for Identity {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Identity")
            .field("display_name", &self.display_name)
            .field("app", &self.app)
            .field("app_version", &self.app_version)
            .field("credential", &redacted(&self.credential))
            .field("device_id", &self.device_id)
            .field("client_kind", &self.client_kind)
            .field("form_factor", &self.form_factor)
            .field("device_name", &self.device_name)
            .field("capabilities", &self.capabilities)
            .finish()
    }
}

impl std::fmt::Debug for Config {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Config")
            .field("token", &redacted(&self.token))
            .field("identity", &self.identity)
            .finish()
    }
}

/// Where someone's playhead is.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Playhead {
    /// The sequence it is in.
    pub sequence_id: String,
    /// The frame it is on.
    pub frame: f64,
    /// Whether it is playing.
    pub playing: bool,
    /// The frame rate it plays at.
    pub rate: f64,
}

/// What someone has selected.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Selection {
    /// The sequence the objects are in.
    pub sequence_id: String,
    /// The selected objects' GUIDs, in the order they were selected.
    pub guids: Vec<String>,
}

/// One participant: one connected device.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Peer {
    /// The connection's id, which leadership, muting and signalling use.
    pub id: String,
    /// The person's id, shared by all their devices, if the server groups
    /// devices by person.
    pub user_id: Option<String>,
    /// The name shown for them.
    pub display_name: String,
    /// Their role.
    pub role: Role,
    /// When they joined, in Unix milliseconds.
    pub joined_at: i64,
    /// `connected`, or `reconnecting` while a host is away.
    pub connection_state: String,
    /// Whether they are muted.
    pub muted: bool,
    /// Their client kind, if the server reports it.
    pub client_kind: Option<String>,
    /// Their device's form factor, if the server reports it.
    pub form_factor: Option<String>,
    /// Their device's name, if the server reports it.
    pub device_name: Option<String>,
    /// What their client can do, if the server reports it.
    pub capabilities: Vec<String>,
}

/// What happened to the participant a roster update names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RosterEvent {
    /// They joined.
    Join,
    /// Something about them changed, such as their connection or mute state.
    Update,
    /// They left.
    Leave,
}

impl RosterEvent {
    /// The event's name on the wire.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Join => "join",
            Self::Update => "update",
            Self::Leave => "leave",
        }
    }
}

/// A review comment, with its annotations, for the session's ledger.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ReviewCommit {
    /// The id that makes committing this again harmless. Unique per person.
    pub client_mutation_id: String,
    /// Where the comment is anchored, as JSON: Furnace's `anchor`.
    pub anchor: String,
    /// The comment, in Markdown.
    pub body_markdown: String,
    /// ORI annotation commands, each a JSON object.
    pub annotation_commands: Vec<String>,
}

/// The server's acknowledgement of a [`ReviewCommit`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ReviewAck {
    /// The commit's id.
    pub client_mutation_id: String,
    /// The comment's id, which the server assigned.
    pub comment_id: String,
    /// Where it is in the session's ledger.
    pub server_order: u64,
    /// The comment's revision.
    pub revision: u64,
}

/// A committed review comment, as the server relays it to the host.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ReviewRelay {
    /// What was committed.
    pub commit: ReviewCommit,
    /// The server's acknowledgement of it.
    pub ack: ReviewAck,
    /// Who wrote it, by person.
    pub author_user_id: String,
    /// Who wrote it, by device.
    pub author_presence_id: String,
    /// When the server accepted it, in Unix milliseconds.
    pub created_at: i64,
}

/// What the server sends a client that has joined.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Welcome {
    /// The session's id.
    pub session_id: String,
    /// When the session was created, in Unix milliseconds.
    pub created_at: i64,
    /// This client's id: the connection's.
    pub self_id: String,
    /// This client's person id, if the server groups devices by person.
    pub self_user_id: Option<String>,
    /// This client's role.
    pub role: Option<Role>,
    /// Everyone in the session, this client included.
    pub peers: Vec<Peer>,
    /// Who leads playback, if anyone.
    pub leader: Option<String>,
    /// The timeline as it stands, if there is one yet.
    pub snapshot: Option<Snapshot>,
    /// Acknowledgements of this person's review commits that they may not
    /// have received.
    pub acknowledgements: Vec<ReviewAck>,
    /// The whole welcome as the wire carried it, as JSON, for what this
    /// type does not model (Furnace's `iceServers`, `thumbBaseUrl`, session
    /// features and LAN mode).
    pub raw: String,
}

/// A message from the server or another participant, as a wire profile
/// decoded it.
#[derive(Debug, Clone, PartialEq)]
pub enum Inbound {
    /// This client has joined.
    Welcome(Welcome),
    /// The whole timeline, from the host.
    Timeline {
        /// Who sent it.
        sender: Option<String>,
        /// The timeline.
        snapshot: Snapshot,
    },
    /// Someone's playhead.
    Playhead {
        /// Whose.
        sender: String,
        /// Where it is.
        playhead: Playhead,
    },
    /// Someone's selection.
    Selection {
        /// Whose.
        sender: String,
        /// What they selected.
        selection: Selection,
    },
    /// Someone joined, changed or left.
    Roster {
        /// What happened.
        event: RosterEvent,
        /// To whom.
        peer: Peer,
        /// Everyone now in the session.
        peers: Vec<Peer>,
    },
    /// The playback leader changed.
    Leader {
        /// The new leader, if there is one.
        leader: Option<String>,
    },
    /// A review commit, relayed to the host.
    ReviewRelay(ReviewRelay),
    /// The server accepted this client's review commit.
    ReviewCommitted(ReviewAck),
    /// The server refused this client's review commit.
    ReviewRejected {
        /// Which commit, if the server said.
        client_mutation_id: Option<String>,
        /// Why, as a code.
        code: String,
        /// Why, in words.
        message: String,
    },
    /// The session ended.
    Ended {
        /// Why, such as `host_left`.
        reason: String,
    },
    /// The server refused something.
    Error {
        /// What, as a code.
        code: String,
        /// What, in words.
        message: String,
        /// Whether the server is closing the connection.
        fatal: bool,
    },
    /// Any other message, passed through: Furnace's thumbnail, waveform,
    /// bin, signal and LAN messages among them.
    Other {
        /// Its type on the wire.
        kind: String,
        /// Who sent it, if anyone.
        sender: Option<String>,
        /// Its payload, as JSON.
        payload: String,
    },
}

/// A message for the server or other participants, for a wire profile to
/// encode.
#[derive(Debug, Clone, PartialEq)]
pub enum Outbound {
    /// The first message: who this client is and how it is admitted.
    Hello(Config),
    /// The timeline, from the host.
    Timeline {
        /// The timeline as it now is.
        snapshot: Snapshot,
        /// The patches from the generation before, which a profile may send
        /// instead of the snapshot when the server relays patches.
        patches: Vec<Patch>,
        /// The generation the patches apply to, if there was one.
        base_generation: Option<i64>,
    },
    /// This client's playhead.
    Playhead(Playhead),
    /// This client's selection.
    Selection(Selection),
    /// A review commit.
    ReviewCommit(ReviewCommit),
    /// Makes another participant the playback leader.
    SetLeader(String),
    /// Mutes or unmutes a participant.
    SetMuted {
        /// Whom.
        participant: String,
        /// Whether to mute them.
        muted: bool,
    },
    /// Leaves the session.
    Bye,
    /// Any other message.
    Other {
        /// The channel it travels on.
        channel: Channel,
        /// Its type on the wire.
        kind: String,
        /// Its payload, as JSON.
        payload: String,
    },
}

impl Outbound {
    /// The channel the message travels on.
    #[must_use]
    pub fn channel(&self) -> Channel {
        match self {
            Self::Hello(_) | Self::SetLeader(_) | Self::SetMuted { .. } | Self::Bye => {
                Channel::Control
            }
            Self::Timeline { .. } => Channel::Timeline,
            Self::Playhead(_) => Channel::Playback,
            Self::Selection(_) => Channel::Selection,
            Self::ReviewCommit(_) => Channel::Review,
            Self::Other { channel, .. } => channel.clone(),
        }
    }
}

/// A change to a timeline, for a program to apply to its copy with
/// [`super::handle`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TimelineChange {
    /// Which sequence changed.
    pub sequence_id: String,
    /// Its generation now.
    pub generation: i64,
    /// Who changed it, if the wire said.
    pub sender: Option<String>,
    /// The patches from the version this client had to this one; a single
    /// [`Op::ReplaceTimeline`](crate::Op::ReplaceTimeline) when it had none.
    pub patches: Vec<Patch>,
    /// The timeline as it now is.
    pub snapshot: Snapshot,
    /// Whether this is the timeline the client joined to.
    pub joining: bool,
}

/// Something a program acts on, from [`super::Session::poll_event`].
#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    /// The client joined, or joined again after a reconnect. Comes after the
    /// timeline it joined to.
    Joined {
        /// Its role.
        role: Role,
        /// Its id.
        self_id: String,
        /// Whether it had joined before.
        rejoined: bool,
    },
    /// A timeline changed.
    Timeline(TimelineChange),
    /// Someone's playhead moved.
    Playhead {
        /// Whose.
        sender: String,
        /// Where it is.
        playhead: Playhead,
    },
    /// Someone's selection changed.
    Selection {
        /// Whose.
        sender: String,
        /// What they selected.
        selection: Selection,
    },
    /// Someone joined, changed or left; [`super::Session::peers`] has
    /// everyone.
    Roster {
        /// What happened.
        event: RosterEvent,
        /// To whom.
        peer: Peer,
    },
    /// The playback leader changed.
    Leader {
        /// The new leader, if there is one.
        leader: Option<String>,
    },
    /// A review commit, relayed to the host.
    ReviewRelay(ReviewRelay),
    /// The server accepted one of this client's review commits.
    ReviewCommitted(ReviewAck),
    /// The server refused one of this client's review commits.
    ReviewRejected {
        /// Which commit, if the server said.
        client_mutation_id: Option<String>,
        /// Why, as a code.
        code: String,
        /// Why, in words.
        message: String,
    },
    /// The server refused something.
    Error {
        /// What, as a code.
        code: String,
        /// What, in words.
        message: String,
        /// Whether the connection is closing.
        fatal: bool,
    },
    /// The session ended.
    Ended {
        /// Why.
        reason: String,
    },
    /// Any other message, passed through.
    Message {
        /// Its type on the wire.
        kind: String,
        /// Who sent it, if anyone.
        sender: Option<String>,
        /// Its payload, as JSON.
        payload: String,
    },
}
