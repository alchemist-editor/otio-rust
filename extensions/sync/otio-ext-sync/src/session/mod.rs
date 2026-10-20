//! A sync session with no networking of its own.
//!
//! A [`Session`] is a state machine. A program feeds it what arrived
//! ([`Session::receive`]), as [`Inbound`] messages a wire profile decoded,
//! and takes back what to send ([`Session::poll_outbound`]) and what
//! happened ([`Session::poll_event`]). It never reads a socket or a clock,
//! so the same session runs over Furnace's WebSocket, over a test's
//! in-memory loopback, or over a socket a program in another language owns.
//! `otio-ext-sync-transport` has the wire profiles and transports.
//!
//! ```text
//!            receive(Inbound)          poll_event() ──► Event ──► handle(document, …)
//! wire ──► profile ──────────► Session
//!            poll_outbound() ◄──────── publish / set_playhead / commit_review …
//! ```
//!
//! # Channels
//!
//! Everything a session sends travels on a [`Channel`] with a [`Delivery`]
//! rule. A playhead not yet sent is replaced by a newer one
//! ([`Delivery::LatestWins`]); timeline changes go in order
//! ([`Delivery::Ordered`]); review commits are kept until the server
//! acknowledges them and sent again after a reconnect
//! ([`Delivery::Ledger`]); signalling is sent once ([`Delivery::Transient`]).
//!
//! # Authority and leases
//!
//! The server appoints the host, as Furnace does, and only the host may
//! change the timeline. A session refuses locally to send what its role may
//! not ([`Error::NoLease`]) rather than let the server close the connection.
//! ORI's elected master comes with ORI's wire profile.
//!
//! # Applying changes
//!
//! A [`TimelineChange`] carries patches. [`handle`] applies them to the
//! program's timeline in place, so handles survive, running the
//! `sync_before_apply` and `sync_after_apply` hooks around it and the media
//! linker on inserted clips; it runs `sync_joined` once the join is done.
//! [`Session::publish`] runs `sync_before_send` before the host's changes
//! leave.

mod handle;
mod types;

use std::collections::{BTreeMap, HashMap, VecDeque};
use std::fmt;

use otio_core::{Any, AnyDictionary, Document, NodeId};

pub use handle::{ApplyOptions, Handled, handle};
pub use types::{
    Channel, Config, Delivery, Event, Identity, Inbound, Outbound, Peer, Playhead, ReviewAck,
    ReviewCommit, ReviewRelay, Role, RosterEvent, Selection, TimelineChange, Welcome,
};

use crate::patch::{Op, Patch, Snapshot};

/// The hook [`Session::publish`] runs before the host's timeline changes
/// leave. A script that fails stops them: [`Error::Vetoed`].
pub const SYNC_BEFORE_SEND: &str = "sync_before_send";
/// The hook [`handle`] runs before applying a remote timeline change. A
/// script that fails rejects it: [`Handled::Rejected`].
pub const SYNC_BEFORE_APPLY: &str = "sync_before_apply";
/// The hook [`handle`] runs after applying a remote timeline change.
pub const SYNC_AFTER_APPLY: &str = "sync_after_apply";
/// The hook [`handle`] runs when the session has joined, after the timeline
/// it joined to.
pub const SYNC_JOINED: &str = "sync_joined";

/// The hooks a session runs, which sync declares when it is enabled.
pub const HOOKS: [&str; 4] = [
    SYNC_BEFORE_SEND,
    SYNC_BEFORE_APPLY,
    SYNC_AFTER_APPLY,
    SYNC_JOINED,
];

/// Why a session refused something.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Error {
    /// The session has not joined yet, or has left.
    NotJoined,
    /// This client's role may not send on the channel.
    NoLease(Channel),
    /// A `sync_before_send` script stopped the change, with its message.
    Vetoed(String),
    /// A review commit reused an id for different content.
    MutationConflict(String),
    /// Making or applying patches failed.
    Patch(crate::Error),
    /// A hook or media linker failed.
    Plugin(String),
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotJoined => formatter.write_str("the session has not joined"),
            Self::NoLease(channel) => {
                write!(
                    formatter,
                    "this client may not send on the {} channel",
                    channel.name()
                )
            }
            Self::Vetoed(message) => write!(
                formatter,
                "{SYNC_BEFORE_SEND} stopped the change: {message}"
            ),
            Self::MutationConflict(id) => {
                write!(
                    formatter,
                    "review commit {id} was already sent with different content"
                )
            }
            Self::Patch(error) => error.fmt(formatter),
            Self::Plugin(message) => formatter.write_str(message),
        }
    }
}

impl std::error::Error for Error {}

impl From<crate::Error> for Error {
    fn from(error: crate::Error) -> Self {
        Self::Patch(error)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    Idle,
    Joining,
    Joined,
    Ended,
}

/// A sync session: see the module documentation.
#[derive(Debug)]
pub struct Session {
    config: Config,
    state: State,
    joined_before: bool,
    session_id: Option<String>,
    self_id: Option<String>,
    self_user_id: Option<String>,
    role: Option<Role>,
    leader: Option<String>,
    peers: Vec<Peer>,
    welcome: Option<Welcome>,
    timelines: BTreeMap<String, Snapshot>,
    active: Option<String>,
    outbox: VecDeque<Outbound>,
    buffered: Vec<Inbound>,
    events: VecDeque<Event>,
    ledger: Vec<ReviewCommit>,
    channels: HashMap<String, Delivery>,
}

impl Session {
    /// A session that will join with `config`. It sends nothing until
    /// [`Session::connect`].
    #[must_use]
    pub fn new(config: Config) -> Self {
        Self {
            config,
            state: State::Idle,
            joined_before: false,
            session_id: None,
            self_id: None,
            self_user_id: None,
            role: None,
            leader: None,
            peers: Vec::new(),
            welcome: None,
            timelines: BTreeMap::new(),
            active: None,
            outbox: VecDeque::new(),
            buffered: Vec::new(),
            events: VecDeque::new(),
            ledger: Vec::new(),
            channels: HashMap::new(),
        }
    }

    /// Starts joining on a new connection: queues the hello.
    ///
    /// Calling it again, after the connection dropped, joins again. What was
    /// waiting to be sent is dropped, except review commits the server has
    /// not acknowledged, which are sent again once joined; the timelines are
    /// kept, so the join arrives as patches against them.
    pub fn connect(&mut self) {
        self.outbox.clear();
        self.buffered.clear();
        self.state = State::Joining;
        self.outbox.push_back(Outbound::Hello(self.config.clone()));
    }

    /// Registers the named channel `name`, delivered by `delivery`, so
    /// [`Session::send`] can use it. Registering it again changes its rule.
    pub fn register_channel(&mut self, name: impl Into<String>, delivery: Delivery) {
        self.channels.insert(name.into(), delivery);
    }

    /// How `channel`'s messages are delivered.
    #[must_use]
    pub fn delivery(&self, channel: &Channel) -> Delivery {
        match channel {
            Channel::Named(name) => self
                .channels
                .get(name)
                .copied()
                .unwrap_or(Delivery::Ordered),
            other => other.delivery(),
        }
    }

    /// Whether the session has joined.
    #[must_use]
    pub fn is_joined(&self) -> bool {
        self.state == State::Joined
    }

    /// Whether the session ended: the server ended it or the client left.
    #[must_use]
    pub fn has_ended(&self) -> bool {
        self.state == State::Ended
    }

    /// What the session joins with.
    #[must_use]
    pub const fn config(&self) -> &Config {
        &self.config
    }

    /// The session's id, once joined.
    #[must_use]
    pub fn session_id(&self) -> Option<&str> {
        self.session_id.as_deref()
    }

    /// This client's id, once joined.
    #[must_use]
    pub fn self_id(&self) -> Option<&str> {
        self.self_id.as_deref()
    }

    /// This client's person id, once joined, if the server groups devices.
    #[must_use]
    pub fn self_user_id(&self) -> Option<&str> {
        self.self_user_id.as_deref()
    }

    /// This client's role, once joined.
    #[must_use]
    pub const fn role(&self) -> Option<&Role> {
        self.role.as_ref()
    }

    /// Who leads playback, if anyone.
    #[must_use]
    pub fn leader(&self) -> Option<&str> {
        self.leader.as_deref()
    }

    /// Everyone in the session, this client included.
    #[must_use]
    pub fn peers(&self) -> &[Peer] {
        &self.peers
    }

    /// The welcome the session joined with.
    #[must_use]
    pub const fn welcome(&self) -> Option<&Welcome> {
        self.welcome.as_ref()
    }

    /// The sequence that changed last.
    #[must_use]
    pub fn active_sequence(&self) -> Option<&str> {
        self.active.as_deref()
    }

    /// The latest snapshot of `sequence_id` this session sent or received.
    #[must_use]
    pub fn timeline(&self, sequence_id: &str) -> Option<&Snapshot> {
        self.timelines.get(sequence_id)
    }

    /// The ids of the sequences this session has a snapshot of.
    pub fn sequences(&self) -> impl Iterator<Item = &str> {
        self.timelines.keys().map(String::as_str)
    }

    /// The review commits the server has not acknowledged yet.
    #[must_use]
    pub fn pending_reviews(&self) -> &[ReviewCommit] {
        &self.ledger
    }

    /// Whether this client may send on `channel` now.
    #[must_use]
    pub fn may_send(&self, channel: &Channel) -> bool {
        if self.state != State::Joined {
            return false;
        }
        match channel {
            Channel::Timeline => self.role == Some(Role::Host),
            Channel::Review => self.has_capability("review.comment.create"),
            _ => true,
        }
    }

    fn has_capability(&self, capability: &str) -> bool {
        self.config
            .identity
            .capabilities
            .iter()
            .any(|have| have == capability)
    }

    fn require(&self, channel: &Channel) -> Result<(), Error> {
        if self.state != State::Joined {
            return Err(Error::NotJoined);
        }
        if !self.may_send(channel) {
            return Err(Error::NoLease(channel.clone()));
        }
        Ok(())
    }

    /// Takes the next message to send, if any.
    pub fn poll_outbound(&mut self) -> Option<Outbound> {
        self.outbox.pop_front()
    }

    /// Takes the next event, if any.
    pub fn poll_event(&mut self) -> Option<Event> {
        self.events.pop_front()
    }

    fn queue(&mut self, message: Outbound) {
        let channel = message.channel();
        if self.delivery(&channel) == Delivery::LatestWins {
            if let Some(waiting) = self
                .outbox
                .iter_mut()
                .find(|waiting| waiting.channel() == channel)
            {
                *waiting = message;
                return;
            }
        }
        self.outbox.push_back(message);
    }

    /// Sends `timeline` as the next generation of `sequence_id`, and
    /// returns the patches from the generation before: none if nothing
    /// changed, in which case nothing is sent.
    ///
    /// Stamps GUIDs on objects that lack one first. The first publish of a
    /// sequence is one [`Op::ReplaceTimeline`]. Runs `sync_before_send` with
    /// `channel`, `sequence_id`, `generation` and `patches` (as JSON).
    ///
    /// # Errors
    ///
    /// [`Error::NotJoined`], [`Error::NoLease`] unless this client is the
    /// host, [`Error::Vetoed`] if a script stops it, and [`Error::Patch`] if
    /// the timeline cannot be written or diffed.
    pub fn publish(
        &mut self,
        document: &mut Document,
        timeline: NodeId,
        sequence_id: &str,
    ) -> Result<Vec<Patch>, Error> {
        self.require(&Channel::Timeline)?;
        crate::guid::stamp(document, timeline)
            .map_err(|error| Error::Patch(crate::Error::Document(error.to_string())))?;
        let before = self.timelines.get(sequence_id);
        let generation = before.map_or(1, |before| before.generation.saturating_add(1));
        let snapshot = crate::document::snapshot(document, timeline, sequence_id, generation)?;
        let patches = match before {
            Some(before) if crate::json_eq(&before.timeline, &snapshot.timeline)? => {
                return Ok(Vec::new());
            }
            Some(before) => crate::diff(before, &snapshot)?.0,
            None => vec![replace_patch(&snapshot)],
        };
        let arguments = hook_arguments(&Channel::Timeline, &snapshot, None, &patches);
        if otio_adapter::plugins::registry().has_hook(SYNC_BEFORE_SEND) {
            otio_adapter::plugins::run_hook(SYNC_BEFORE_SEND, document, timeline, &arguments)
                .map_err(|error| Error::Vetoed(plugin_message(&error)))?;
        }
        let base_generation = before.map(|before| before.generation);
        self.queue(Outbound::Timeline {
            snapshot: snapshot.clone(),
            patches: patches.clone(),
            base_generation,
        });
        self.active = Some(sequence_id.to_string());
        self.timelines.insert(sequence_id.to_string(), snapshot);
        Ok(patches)
    }

    /// Sends this client's playhead. Only the newest one not yet sent goes.
    ///
    /// # Errors
    ///
    /// [`Error::NotJoined`].
    pub fn set_playhead(&mut self, playhead: Playhead) -> Result<(), Error> {
        self.require(&Channel::Playback)?;
        self.queue(Outbound::Playhead(playhead));
        Ok(())
    }

    /// Sends this client's selection. Only the newest one not yet sent goes.
    ///
    /// # Errors
    ///
    /// [`Error::NotJoined`].
    pub fn set_selection(&mut self, selection: Selection) -> Result<(), Error> {
        self.require(&Channel::Selection)?;
        self.queue(Outbound::Selection(selection));
        Ok(())
    }

    /// Commits a review comment to the session's ledger. It is sent again
    /// after every reconnect until the server acknowledges it
    /// ([`Event::ReviewCommitted`]) or refuses it
    /// ([`Event::ReviewRejected`]). An empty `client_mutation_id` is given a
    /// fresh one, which is returned.
    ///
    /// # Errors
    ///
    /// [`Error::NotJoined`], [`Error::NoLease`] unless the identity has the
    /// `review.comment.create` capability, and [`Error::MutationConflict`]
    /// if the id is waiting with different content.
    pub fn commit_review(&mut self, mut commit: ReviewCommit) -> Result<String, Error> {
        self.require(&Channel::Review)?;
        if commit.client_mutation_id.is_empty() {
            commit.client_mutation_id = format!("mutation_{}", crate::guid::new_guid());
        }
        let id = commit.client_mutation_id.clone();
        match self
            .ledger
            .iter()
            .find(|waiting| waiting.client_mutation_id == id)
        {
            Some(waiting) if *waiting == commit => return Ok(id),
            Some(_) => return Err(Error::MutationConflict(id)),
            None => {}
        }
        self.ledger.push(commit.clone());
        self.queue(Outbound::ReviewCommit(commit));
        Ok(id)
    }

    /// Takes the review commit `client_mutation_id` out of the ledger, so it
    /// is not sent again. Returns whether it was waiting. For a refusal
    /// that does not say which commit it refuses
    /// ([`Event::ReviewRejected`] with no id), which leaves the ledger as
    /// it was.
    pub fn cancel_review(&mut self, client_mutation_id: &str) -> bool {
        let before = self.ledger.len();
        self.ledger
            .retain(|waiting| waiting.client_mutation_id != client_mutation_id);
        self.outbox.retain(|message| {
            !matches!(message, Outbound::ReviewCommit(commit)
                if commit.client_mutation_id == client_mutation_id)
        });
        self.ledger.len() != before
    }

    /// Makes `participant` the playback leader. The server decides who may.
    ///
    /// # Errors
    ///
    /// [`Error::NotJoined`].
    pub fn set_leader(&mut self, participant: impl Into<String>) -> Result<(), Error> {
        self.require(&Channel::Control)?;
        self.queue(Outbound::SetLeader(participant.into()));
        Ok(())
    }

    /// Mutes or unmutes `participant`. The server decides who may.
    ///
    /// # Errors
    ///
    /// [`Error::NotJoined`].
    pub fn set_muted(&mut self, participant: impl Into<String>, muted: bool) -> Result<(), Error> {
        self.require(&Channel::Control)?;
        self.queue(Outbound::SetMuted {
            participant: participant.into(),
            muted,
        });
        Ok(())
    }

    /// Sends a message of type `kind` with the JSON `payload` on `channel`,
    /// under that channel's delivery rule.
    ///
    /// # Errors
    ///
    /// [`Error::NotJoined`], and [`Error::NoLease`] if the channel is one
    /// this client may not send on.
    pub fn send(
        &mut self,
        channel: Channel,
        kind: impl Into<String>,
        payload: impl Into<String>,
    ) -> Result<(), Error> {
        self.require(&channel)?;
        self.queue(Outbound::Other {
            channel,
            kind: kind.into(),
            payload: payload.into(),
        });
        Ok(())
    }

    /// Leaves the session: queues a goodbye and ends it.
    pub fn leave(&mut self) {
        if self.state == State::Joined {
            self.outbox.push_back(Outbound::Bye);
        }
        self.state = State::Ended;
    }

    /// Takes in one message that arrived.
    ///
    /// Before the welcome, everything but errors and the end of the session
    /// is held back, then replayed after it, so nothing is applied to a
    /// timeline the client does not have yet.
    pub fn receive(&mut self, message: Inbound) {
        if self.state == State::Ended {
            return;
        }
        if self.state != State::Joined
            && !matches!(
                message,
                Inbound::Welcome(_) | Inbound::Error { .. } | Inbound::Ended { .. }
            )
        {
            self.buffered.push(message);
            return;
        }
        match message {
            Inbound::Welcome(welcome) => self.welcomed(welcome),
            Inbound::Timeline { sender, snapshot } => {
                self.timeline_arrived(sender, snapshot, false)
            }
            Inbound::Playhead { sender, playhead } => {
                self.events.push_back(Event::Playhead { sender, playhead });
            }
            Inbound::Selection { sender, selection } => {
                self.events
                    .push_back(Event::Selection { sender, selection });
            }
            Inbound::Roster { event, peer, peers } => {
                self.peers = peers;
                self.events.push_back(Event::Roster { event, peer });
            }
            Inbound::Leader { leader } => {
                self.leader.clone_from(&leader);
                self.events.push_back(Event::Leader { leader });
            }
            Inbound::ReviewRelay(relay) => self.events.push_back(Event::ReviewRelay(relay)),
            Inbound::ReviewCommitted(ack) => {
                self.ledger
                    .retain(|waiting| waiting.client_mutation_id != ack.client_mutation_id);
                self.events.push_back(Event::ReviewCommitted(ack));
            }
            Inbound::ReviewRejected {
                client_mutation_id,
                code,
                message,
            } => {
                if let Some(id) = &client_mutation_id {
                    self.ledger
                        .retain(|waiting| &waiting.client_mutation_id != id);
                }
                self.events.push_back(Event::ReviewRejected {
                    client_mutation_id,
                    code,
                    message,
                });
            }
            Inbound::Ended { reason } => {
                self.state = State::Ended;
                self.events.push_back(Event::Ended { reason });
            }
            Inbound::Error {
                code,
                message,
                fatal,
            } => {
                if fatal {
                    self.state = State::Idle;
                }
                self.events.push_back(Event::Error {
                    code,
                    message,
                    fatal,
                });
            }
            Inbound::Other {
                kind,
                sender,
                payload,
            } => self.events.push_back(Event::Message {
                kind,
                sender,
                payload,
            }),
        }
    }

    /// The connection dropped: the session waits to [`connect`] again.
    ///
    /// [`connect`]: Session::connect
    pub fn disconnected(&mut self) {
        if self.state != State::Ended {
            self.state = State::Idle;
        }
    }

    fn welcomed(&mut self, welcome: Welcome) {
        let rejoined = self.joined_before;
        self.state = State::Joined;
        self.joined_before = true;
        self.session_id = Some(welcome.session_id.clone());
        self.self_id = Some(welcome.self_id.clone());
        self.self_user_id.clone_from(&welcome.self_user_id);
        self.role.clone_from(&welcome.role);
        self.leader.clone_from(&welcome.leader);
        self.peers.clone_from(&welcome.peers);
        let snapshot = welcome.snapshot.clone();
        let acknowledgements = welcome.acknowledgements.clone();
        let role = welcome.role.clone().unwrap_or(Role::Viewer);
        let self_id = welcome.self_id.clone();
        self.welcome = Some(welcome);

        if self.role == Some(Role::Host) {
            // The host owns the timeline: what the server holds is its own
            // last word, kept only so the next generation follows it. A
            // server that lost it (it restarted) or missed the latest one
            // (the connection dropped as it went, perhaps while switching
            // to another sequence) gets the host's active timeline again.
            let mine = self
                .active
                .as_ref()
                .and_then(|active| self.timelines.get(active));
            let behind = match (&snapshot, mine) {
                (_, None) => None,
                (None, Some(mine)) => Some(mine),
                (Some(theirs), Some(mine)) => (mine.sequence_id != theirs.sequence_id
                    || mine.generation > theirs.generation)
                    .then_some(mine),
            };
            if let Some(mine) = behind {
                self.outbox.push_back(Outbound::Timeline {
                    snapshot: mine.clone(),
                    patches: vec![replace_patch(mine)],
                    base_generation: None,
                });
            } else if let Some(theirs) = snapshot {
                let keep = mine.is_some_and(|mine| mine.generation >= theirs.generation);
                if !keep {
                    self.active = Some(theirs.sequence_id.clone());
                    self.timelines.insert(theirs.sequence_id.clone(), theirs);
                }
            }
        } else if let Some(snapshot) = snapshot {
            self.timeline_arrived(None, snapshot, true);
        }

        for ack in acknowledgements {
            if let Some(position) = self
                .ledger
                .iter()
                .position(|waiting| waiting.client_mutation_id == ack.client_mutation_id)
            {
                self.ledger.remove(position);
                self.events.push_back(Event::ReviewCommitted(ack));
            }
        }
        for commit in self.ledger.clone() {
            self.outbox.push_back(Outbound::ReviewCommit(commit));
        }

        // What came before the welcome is replayed first, so `Joined` is
        // the last event of the join.
        for message in std::mem::take(&mut self.buffered) {
            self.receive(message);
        }
        self.events.push_back(Event::Joined {
            role,
            self_id,
            rejoined,
        });
    }

    fn timeline_arrived(&mut self, sender: Option<String>, snapshot: Snapshot, joining: bool) {
        let patches = match self.timelines.get(&snapshot.sequence_id) {
            Some(before) => match crate::json_eq(&before.timeline, &snapshot.timeline) {
                Ok(true) => Vec::new(),
                Ok(false) => match crate::diff(before, &snapshot) {
                    Ok((patches, _)) => patches,
                    Err(_) => vec![replace_patch(&snapshot)],
                },
                Err(_) => vec![replace_patch(&snapshot)],
            },
            None => vec![replace_patch(&snapshot)],
        };
        self.active = Some(snapshot.sequence_id.clone());
        self.timelines
            .insert(snapshot.sequence_id.clone(), snapshot.clone());
        if patches.is_empty() && !joining {
            return;
        }
        self.events.push_back(Event::Timeline(TimelineChange {
            sequence_id: snapshot.sequence_id.clone(),
            generation: snapshot.generation,
            sender,
            patches,
            snapshot,
            joining,
        }));
    }
}

/// The one patch that puts `snapshot`'s timeline in place.
fn replace_patch(snapshot: &Snapshot) -> Patch {
    Patch {
        op: Op::ReplaceTimeline,
        timeline: Some(snapshot.timeline.clone()),
        ..Patch::default()
    }
}

/// The arguments the session's hooks get.
fn hook_arguments(
    channel: &Channel,
    snapshot: &Snapshot,
    sender: Option<&str>,
    patches: &[Patch],
) -> AnyDictionary {
    let mut arguments = AnyDictionary::from([
        (
            "channel".to_string(),
            Any::String(channel.name().to_string()),
        ),
        (
            "sequence_id".to_string(),
            Any::String(snapshot.sequence_id.clone()),
        ),
        ("generation".to_string(), Any::Int(snapshot.generation)),
        (
            "patches".to_string(),
            Any::String(crate::patch::to_json(patches)),
        ),
    ]);
    if let Some(sender) = sender {
        arguments.insert("sender".to_string(), Any::String(sender.to_string()));
    }
    arguments
}

fn plugin_message(error: &otio_adapter::Error) -> String {
    match error {
        otio_adapter::Error::Plugin { message, .. } => message.clone(),
        other => other.to_string(),
    }
}
