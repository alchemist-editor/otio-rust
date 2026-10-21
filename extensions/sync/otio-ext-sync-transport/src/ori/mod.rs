//! ORI Sync's wire: the envelope ORI's peers send each other over RabbitMQ.
//!
//! ORI Sync (the ORI Annotations project's `otio_sync_core`, in Python) has
//! no server. Every peer in a session publishes to one fanout exchange and
//! hears everything, its own messages included, which it drops by
//! `source_guid`. One peer is the master, elected among them, and answers a
//! joiner's `STATE_REQUEST` with every timeline. Every message is one
//! envelope:
//!
//! ```json
//! {"session": "<id>", "source_guid": "<peer>",
//!  "payload": {"command_schema": "OTIO_SESSION_1.0",
//!              "command": {"event": "INSERT_CHILD", "payload": {…}}}}
//! ```
//!
//! [`Ori`] speaks it for a [`Session`]: it runs the join (`WHO_IS_MASTER`,
//! then electing itself if no master answers in two seconds, or
//! `STATE_REQUEST` and the master's `STATE_SNAPSHOT`), announces this peer
//! every five seconds and forgets peers it has not heard from in fifteen,
//! answers state requests while it is the master, and takes over as master
//! when one stops answering. The session's patches become ORI's
//! `OTIO_SESSION_1.0` edits, or a `REPLACE_TIMELINE` when ORI cannot say a
//! change (it moves a child only within its parent, cannot remove a
//! property, and indexes only the top of an inserted subtree). ORI's edits
//! are applied as ORI's own patcher applies them, edits it would ignore
//! included, and reach the session as timelines it turns into patches.
//!
//! | ORI | Session |
//! | --- | --- |
//! | `TIMELINE_1.0` `ADD_TIMELINE`, `REPLACE_TIMELINE`, `RENAME_TIMELINE` | [`Inbound::Timeline`], not activating |
//! | `TIMELINE_1.0` `REMOVE_TIMELINE` | [`Inbound::RemoveTimeline`] |
//! | `OTIO_SESSION_1.0` edits | [`Inbound::Timeline`] of the edited timeline |
//! | `PLAYBACK_SETTINGS_1.0` `SET` | [`Inbound::Playhead`], [`Inbound::Selection`] of `clip_guid`, [`Inbound::Activate`] of `timeline_guid` |
//! | `DISPLAY_SETTINGS_1.0` `SET` | `metadata.display_settings` on the active timeline, as ORI keeps it, and [`Inbound::Other`] |
//! | `PEER_ANNOUNCE`, `PEER_DEPART`, silence | [`Inbound::Roster`] |
//! | anything else (`Annotation.1` among them) | [`Inbound::Other`], `kind` `"<schema>/<event>"` |
//!
//! A sequence's id on this wire is the ORI `timeline_guid`. A program sends
//! any ORI message with [`Session::send`], `kind` `"<schema>/<event>"` and
//! the message's payload.
//!
//! Every peer is a [`Role::Driver`] unless [`Options::role`] says otherwise,
//! as in ORI, and like ORI's receivers this profile applies what arrives
//! whatever the sender's role or lease. Snapshots and messages over 1 MB
//! may travel zlib-compressed, as ORI's v1 proposal has it: they are always
//! read, and sent only when [`Options`] asks, since ORI's peers today read
//! neither.

mod tree;
mod wire;

use std::collections::{BTreeMap, VecDeque};

use serde_json::{Map, Value, json};

use otio_ext_sync::patch::{Op, Patch, Snapshot};
use otio_ext_sync::session::{
    Inbound, Outbound, Peer, Playhead, Role, RosterEvent, Selection, Session, Welcome,
};

use crate::{Error, Frame, Profile};
use tree::{OriEdit, apply_ori, apply_patch, guid_of, has_descendants, locate, unescape};

/// The schema of session messages: discovery, peers and state transfer.
pub const LIVE_SESSION: &str = "LiveSession.1";
/// The schema of whole-timeline messages.
pub const TIMELINE: &str = "TIMELINE_1.0";
/// The schema of edits inside a timeline.
pub const OTIO_SESSION: &str = "OTIO_SESSION_1.0";
/// The schema of the playhead and the active clip.
pub const PLAYBACK: &str = "PLAYBACK_SETTINGS_1.0";
/// The schema of the viewer's pan, zoom, exposure and channel.
pub const DISPLAY: &str = "DISPLAY_SETTINGS_1.0";
/// The top-level `schema` an `I_AM_MASTER` carries.
pub const ENVELOPE_SCHEMA: &str = "SYNC_REVIEW_1.0";
/// Snapshots and messages larger than this many bytes may be compressed.
pub const COMPRESS_OVER: usize = 1 << 20;

/// How a peer joins an ORI session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Options {
    /// The session's id: the `session` in every envelope, and the
    /// `sync_session_<id>` exchange on RabbitMQ.
    pub session_id: String,
    /// This peer's GUID, its `source_guid`. The default is a fresh one.
    pub peer_guid: String,
    /// This peer's role: [`Role::Driver`] (the default) may change the
    /// timeline, others may not. Announced as ORI's `driver`, `reviewer` or
    /// `viewer`.
    pub role: Role,
    /// What this peer announces it can do.
    pub capabilities: Vec<String>,
    /// How long, in milliseconds, to wait for a master before electing
    /// itself: ORI's two seconds.
    pub discovery_timeout: i64,
    /// How long, in milliseconds, to wait for the master's snapshot before
    /// looking for a master again: ORI's five seconds.
    pub state_request_timeout: i64,
    /// How often, in milliseconds, to announce this peer: ORI's five
    /// seconds.
    pub heartbeat_interval: i64,
    /// How long, in milliseconds, a silent peer is kept: ORI's fifteen
    /// seconds.
    pub peer_timeout: i64,
    /// Whether to send snapshots of more than [`COMPRESS_OVER`] bytes with
    /// their timelines zlib-compressed and a `compressed` flag, as ORI's v1
    /// proposal does. Off by default: ORI's peers today cannot read it.
    pub compress_snapshots: bool,
    /// Whether to send messages of more than [`COMPRESS_OVER`] bytes
    /// zlib-compressed, as binary frames (AMQP `content_encoding`
    /// `deflate`). Off by default, for the same reason.
    pub compress_messages: bool,
}

impl Options {
    /// The defaults ORI uses, for the session `session_id`.
    #[must_use]
    pub fn new(session_id: impl Into<String>) -> Self {
        Self {
            session_id: session_id.into(),
            peer_guid: otio_ext_sync::guid::new_guid(),
            role: Role::Driver,
            capabilities: Vec::new(),
            discovery_timeout: 2_000,
            state_request_timeout: 5_000,
            heartbeat_interval: 5_000,
            peer_timeout: 15_000,
            compress_snapshots: false,
            compress_messages: false,
        }
    }
}

/// Where the join has got to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    /// Not connected.
    Idle,
    /// Asked who the master is, at `since`.
    Discovering { since: i64 },
    /// Asked the master for its state, at `since`.
    Joining { since: i64 },
    /// Has the session's state.
    Synced,
}

/// One envelope that arrived.
#[derive(Debug, Clone)]
struct Message {
    session: Option<String>,
    source: String,
    schema: String,
    event: String,
    payload: Value,
}

impl Message {
    fn text(&self, key: &str) -> Option<&str> {
        self.payload.get(key).and_then(Value::as_str)
    }

    /// When the sender made the change, in seconds; `0` if it did not say.
    fn sync_timestamp(&self) -> f64 {
        self.payload
            .get("sync_timestamp")
            .and_then(Value::as_f64)
            .unwrap_or(0.0)
    }
}

/// A peer this one has heard from.
#[derive(Debug, Clone)]
struct Known {
    /// What it announced: `app`, `capabilities`, and `role` and `identity`
    /// if it gave them, as a snapshot's `peers` carries it.
    entry: Map<String, Value>,
    /// When it was last heard from.
    last_seen: i64,
}

/// ORI Sync's wire profile; see the [module documentation](self).
#[derive(Debug, Clone)]
pub struct Ori {
    options: Options,
    now: i64,
    phase: Phase,
    is_master: bool,
    master: Option<String>,
    /// When a `WHO_IS_MASTER` went unanswered while synced.
    who_is_master_at: Option<i64>,
    last_announce: Option<i64>,
    app: String,
    display_name: String,
    identity: Map<String, Value>,
    peers: BTreeMap<String, Known>,
    /// Envelopes waiting to be made sense of against the session.
    incoming: VecDeque<Message>,
    /// What arrived while discovering, applied if this peer elects itself.
    early: Vec<Message>,
    /// What arrived while joining, replayed after the snapshot.
    buffered: Vec<Message>,
    /// Messages for the session that need no envelope.
    ready: VecDeque<Vec<Inbound>>,
    outgoing: VecDeque<Frame>,
    /// Peers whose state requests this master has yet to answer.
    requests: VecDeque<String>,
    playback: Option<Map<String, Value>>,
    display: Option<Value>,
    /// Clip timelines, by GUID, with the GUID of the clip each is for.
    clip_timelines: BTreeMap<String, String>,
}

impl Ori {
    /// A profile for the ORI session `options` describes.
    #[must_use]
    pub fn new(options: Options) -> Self {
        Self {
            options,
            now: 0,
            phase: Phase::Idle,
            is_master: false,
            master: None,
            who_is_master_at: None,
            last_announce: None,
            app: String::new(),
            display_name: String::new(),
            identity: Map::new(),
            peers: BTreeMap::new(),
            incoming: VecDeque::new(),
            early: Vec::new(),
            buffered: Vec::new(),
            ready: VecDeque::new(),
            outgoing: VecDeque::new(),
            requests: VecDeque::new(),
            playback: None,
            display: None,
            clip_timelines: BTreeMap::new(),
        }
    }

    /// How this profile was set up.
    #[must_use]
    pub const fn options(&self) -> &Options {
        &self.options
    }

    /// This peer's GUID.
    #[must_use]
    pub fn peer_guid(&self) -> &str {
        &self.options.peer_guid
    }

    /// Whether this peer is the session's master.
    #[must_use]
    pub const fn is_master(&self) -> bool {
        self.is_master
    }

    /// The master this peer knows of, if any: itself when it is the master.
    #[must_use]
    pub fn master(&self) -> Option<&str> {
        self.master.as_deref()
    }

    /// The playback state ORI peers last agreed on, as ORI's
    /// `playback_state`.
    #[must_use]
    pub fn playback_state(&self) -> Option<&Map<String, Value>> {
        self.playback.as_ref()
    }

    /// The display state ORI peers last agreed on, as ORI's
    /// `display_state`.
    #[must_use]
    pub const fn display_state(&self) -> Option<&Value> {
        self.display.as_ref()
    }

    fn timestamp(&self) -> f64 {
        #[allow(clippy::cast_precision_loss)]
        let seconds = self.now as f64 / 1000.0;
        seconds
    }

    fn envelope(&self, schema: &str, event: &str, payload: &str) -> String {
        let mut text = format!(
            "{{\"session\":{},\"source_guid\":{},\"payload\":{{\"command_schema\":{},\"command\":{{\"event\":{},\"payload\":{payload}}}}}",
            quote(&self.options.session_id),
            quote(&self.options.peer_guid),
            quote(schema),
            quote(event),
        );
        if schema == LIVE_SESSION && event == "I_AM_MASTER" {
            text.push_str(",\"schema\":");
            text.push_str(&quote(ENVELOPE_SCHEMA));
        }
        text.push('}');
        text
    }

    fn frame(&self, text: String) -> Result<Frame, Error> {
        if self.options.compress_messages && text.len() > COMPRESS_OVER {
            return Ok(Frame::binary(wire::deflate(text.as_bytes())?));
        }
        Ok(Frame::text(text))
    }

    fn message(&self, schema: &str, event: &str, payload: &Value) -> Result<Frame, Error> {
        self.frame(self.envelope(schema, event, &payload.to_string()))
    }

    fn queue(&mut self, schema: &str, event: &str, payload: &Value) {
        if let Ok(frame) = self.message(schema, event, payload) {
            self.outgoing.push_back(frame);
        }
    }

    fn role_name(&self) -> &str {
        match &self.options.role {
            Role::Host | Role::Driver => "driver",
            other => other.as_str(),
        }
    }

    fn announce(&mut self) {
        self.last_announce = Some(self.now);
        let mut payload = json!({
            "peer_guid": self.options.peer_guid,
            "app": self.app,
            "capabilities": self.options.capabilities,
            "role": self.role_name(),
        });
        if !self.identity.is_empty() {
            payload["identity"] = Value::Object(self.identity.clone());
        }
        self.queue(LIVE_SESSION, "PEER_ANNOUNCE", &payload);
    }

    fn discover(&mut self) {
        self.phase = Phase::Discovering { since: self.now };
        self.master = None;
        let payload = json!({ "requester_guid": self.options.peer_guid });
        self.queue(LIVE_SESSION, "WHO_IS_MASTER", &payload);
    }

    fn claim_master(&mut self) {
        let payload = json!({ "master_guid": self.options.peer_guid });
        self.queue(LIVE_SESSION, "I_AM_MASTER", &payload);
    }

    /// Becomes the master, as ORI does when no master answers: a peer still
    /// joining joins with the timelines it has.
    fn elect_self(&mut self) {
        self.is_master = true;
        self.master = Some(self.options.peer_guid.clone());
        self.who_is_master_at = None;
        self.claim_master();
        if self.phase != Phase::Synced {
            self.phase = Phase::Synced;
            self.ready
                .push_back(vec![Inbound::Welcome(self.welcome(None, String::new()))]);
            for message in std::mem::take(&mut self.early).into_iter().rev() {
                self.incoming.push_front(message);
            }
        }
    }

    /// ORI's preference between two masters: a driver over anyone else.
    fn rank(&self, guid: &str) -> u8 {
        let role = if guid == self.options.peer_guid {
            Some(self.role_name())
        } else {
            self.peers
                .get(guid)
                .and_then(|known| known.entry.get("role"))
                .and_then(Value::as_str)
        };
        match role {
            None | Some("driver") => 0,
            Some(_) => 1,
        }
    }

    fn self_peer(&self) -> Peer {
        Peer {
            id: self.options.peer_guid.clone(),
            user_id: None,
            display_name: self.display_name.clone(),
            role: self.options.role.clone(),
            joined_at: self.now,
            connection_state: "connected".to_string(),
            muted: false,
            client_kind: None,
            form_factor: None,
            device_name: None,
            capabilities: self.options.capabilities.clone(),
        }
    }

    fn peer(&self, guid: &str, known: &Known) -> Peer {
        let entry = &known.entry;
        let identity = entry.get("identity");
        let field = |key: &str| {
            identity
                .and_then(|identity| identity.get(key))
                .and_then(Value::as_str)
                .filter(|text| !text.is_empty())
        };
        let name = match (field("first_name"), field("last_name")) {
            (Some(first), Some(last)) => format!("{first} {last}"),
            (Some(name), None) | (None, Some(name)) => name.to_string(),
            (None, None) => field("user")
                .or_else(|| entry.get("app").and_then(Value::as_str))
                .unwrap_or(guid)
                .to_string(),
        };
        Peer {
            id: guid.to_string(),
            user_id: field("user").map(str::to_string),
            display_name: name,
            role: Role::from_name(
                entry
                    .get("role")
                    .and_then(Value::as_str)
                    .unwrap_or("driver"),
            ),
            joined_at: known.last_seen,
            connection_state: "connected".to_string(),
            muted: false,
            client_kind: entry.get("app").and_then(Value::as_str).map(str::to_string),
            form_factor: None,
            device_name: field("host").map(str::to_string),
            capabilities: entry
                .get("capabilities")
                .and_then(Value::as_array)
                .map(|items| {
                    items
                        .iter()
                        .filter_map(Value::as_str)
                        .map(str::to_string)
                        .collect()
                })
                .unwrap_or_default(),
        }
    }

    fn peer_list(&self) -> Vec<Peer> {
        let mut peers = vec![self.self_peer()];
        peers.extend(
            self.peers
                .iter()
                .map(|(guid, known)| self.peer(guid, known)),
        );
        peers
    }

    fn roster(&self, event: RosterEvent, peer: Peer) -> Inbound {
        Inbound::Roster {
            event,
            peer,
            peers: self.peer_list(),
        }
    }

    fn welcome(&self, snapshot: Option<Snapshot>, raw: String) -> Welcome {
        Welcome {
            session_id: self.options.session_id.clone(),
            created_at: 0,
            self_id: self.options.peer_guid.clone(),
            self_user_id: None,
            role: Some(self.options.role.clone()),
            peers: self.peer_list(),
            leader: None,
            snapshot,
            acknowledgements: Vec::new(),
            raw,
        }
    }

    /// The session's timelines, sequence timelines first and `first` before
    /// the rest, as ORI's object map finds a GUID that two of them hold.
    fn search_order<'a>(&self, session: &'a Session, first: Option<&'a str>) -> Vec<&'a str> {
        let mut order: Vec<&str> = Vec::new();
        if let Some(first) = first.filter(|first| !self.clip_timelines.contains_key(*first)) {
            order.push(first);
        }
        for clip in [false, true] {
            for sequence in session.sequences() {
                if self.clip_timelines.contains_key(sequence) == clip && !order.contains(&sequence)
                {
                    order.push(sequence);
                }
            }
        }
        order
    }

    /// A new version of `sequence`'s timeline, one generation on.
    fn next(session: &Session, sequence: &str, timeline: String) -> Snapshot {
        Snapshot {
            sequence_id: sequence.to_string(),
            generation: session
                .timeline(sequence)
                .map_or(1, |before| before.generation.saturating_add(1)),
            timeline,
        }
    }

    fn note_clip_timeline(&mut self, guid: &str, timeline: &Value) {
        if let Some(clip) = timeline
            .get("metadata")
            .and_then(|metadata| metadata.get("clip_timeline_for"))
            .and_then(Value::as_str)
            .filter(|clip| !clip.is_empty())
        {
            self.clip_timelines
                .insert(guid.to_string(), clip.to_string());
        } else {
            self.clip_timelines.remove(guid);
        }
    }

    fn handle(&mut self, message: Message, session: &Session) -> Vec<Inbound> {
        if message.schema == LIVE_SESSION {
            return self.live(message, session);
        }
        match self.phase {
            Phase::Idle => Vec::new(),
            Phase::Discovering { .. } => {
                self.remember_view(&message);
                self.early.push(message);
                Vec::new()
            }
            Phase::Joining { .. } => {
                self.buffered.push(message);
                Vec::new()
            }
            Phase::Synced => self.apply(message, session),
        }
    }

    /// Keeps the playback or display state a message carries, as ORI does
    /// even before it has joined.
    fn remember_view(&mut self, message: &Message) {
        if message.event != "SET" {
            return;
        }
        if message.schema == PLAYBACK {
            self.playback = message.payload.as_object().cloned();
        } else if message.schema == DISPLAY {
            self.display = Some(message.payload.clone());
        }
    }

    fn live(&mut self, message: Message, session: &Session) -> Vec<Inbound> {
        match message.event.as_str() {
            "WHO_IS_MASTER" => {
                if self.is_master {
                    self.claim_master();
                } else if self.phase == Phase::Synced {
                    self.who_is_master_at = Some(self.now);
                }
                Vec::new()
            }
            "I_AM_MASTER" => {
                let claimant = message
                    .text("master_guid")
                    .unwrap_or(&message.source)
                    .to_string();
                if self.is_master
                    && claimant != self.options.peer_guid
                    && self.rank(&self.options.peer_guid) < self.rank(&claimant)
                {
                    self.claim_master();
                    return Vec::new();
                }
                self.master = Some(claimant.clone());
                self.who_is_master_at = None;
                if matches!(self.phase, Phase::Discovering { .. }) {
                    self.phase = Phase::Joining { since: self.now };
                    let payload = json!({
                        "target_guid": claimant,
                        "requester_guid": self.options.peer_guid,
                    });
                    self.queue(LIVE_SESSION, "STATE_REQUEST", &payload);
                }
                Vec::new()
            }
            "PEER_ANNOUNCE" => self.peer_announced(&message),
            "PEER_DEPART" => {
                let guid = message.text("peer_guid").unwrap_or(&message.source);
                if self.master.as_deref() == Some(guid) {
                    self.master = None;
                }
                match self.peers.remove(guid) {
                    Some(known) => {
                        let peer = self.peer(guid, &known);
                        vec![self.roster(RosterEvent::Leave, peer)]
                    }
                    None => Vec::new(),
                }
            }
            "STATE_REQUEST" => {
                if self.is_master {
                    let requester = message
                        .text("requester_guid")
                        .unwrap_or(&message.source)
                        .to_string();
                    self.requests.push_back(requester);
                }
                Vec::new()
            }
            "STATE_SNAPSHOT" => {
                if message.text("target_guid") == Some(self.options.peer_guid.as_str())
                    && self.phase != Phase::Idle
                {
                    self.snapshot_arrived(&message, session)
                } else {
                    Vec::new()
                }
            }
            _ => self.other(&message),
        }
    }

    fn peer_announced(&mut self, message: &Message) -> Vec<Inbound> {
        let guid = message
            .text("peer_guid")
            .unwrap_or(&message.source)
            .to_string();
        if guid == self.options.peer_guid {
            return Vec::new();
        }
        let known = self.peers.get(&guid);
        let mut entry = Map::new();
        entry.insert(
            "app".to_string(),
            message.payload.get("app").cloned().unwrap_or(Value::Null),
        );
        entry.insert(
            "capabilities".to_string(),
            message
                .payload
                .get("capabilities")
                .filter(|capabilities| capabilities.is_array())
                .cloned()
                .unwrap_or_else(|| json!([])),
        );
        // A role or identity left out keeps the one announced before.
        for key in ["role", "identity"] {
            match message.payload.get(key).filter(|value| !value.is_null()) {
                Some(value) => {
                    entry.insert(key.to_string(), value.clone());
                }
                None => {
                    if let Some(value) = known.and_then(|known| known.entry.get(key)) {
                        entry.insert(key.to_string(), value.clone());
                    }
                }
            }
        }
        let event = match known {
            None => Some(RosterEvent::Join),
            Some(known) if known.entry != entry => Some(RosterEvent::Update),
            Some(_) => None,
        };
        let known = Known {
            entry,
            last_seen: self.now,
        };
        let peer = self.peer(&guid, &known);
        self.peers.insert(guid, known);
        event
            .map(|event| vec![self.roster(event, peer)])
            .unwrap_or_default()
    }

    fn snapshot_arrived(&mut self, message: &Message, session: &Session) -> Vec<Inbound> {
        let Some(timelines) = read_timelines(&message.payload) else {
            return Vec::new();
        };
        let active = message
            .text("active_timeline_guid")
            .filter(|active| timelines.contains_key(*active))
            .map(str::to_string)
            .or_else(|| timelines.keys().next().cloned());
        let snapshot_timestamp = message
            .payload
            .get("snapshot_timestamp")
            .and_then(Value::as_f64)
            .unwrap_or(0.0);
        if let Some(Value::Object(peers)) = message.payload.get("peers") {
            for (guid, entry) in peers {
                if guid == &self.options.peer_guid {
                    continue;
                }
                if let Some(entry) = entry.as_object() {
                    self.peers.insert(
                        guid.clone(),
                        Known {
                            entry: entry.clone(),
                            last_seen: self.now,
                        },
                    );
                }
            }
        }
        if let Some(Value::Object(playback)) = message.payload.get("playback_state") {
            self.playback = Some(playback.clone());
        }
        if let Some(display) = message
            .payload
            .get("display_state")
            .filter(|d| !d.is_null())
        {
            self.display = Some(display.clone());
        }
        self.clip_timelines.clear();
        let mut snapshots = BTreeMap::new();
        for (guid, timeline) in &timelines {
            self.note_clip_timeline(guid, timeline);
            snapshots.insert(
                guid.clone(),
                Self::next(session, guid, timeline.to_string()),
            );
        }

        let mut inbound = Vec::new();
        let joining = self.phase != Phase::Synced;
        let mut active_snapshot = active.as_ref().and_then(|active| snapshots.remove(active));
        if joining {
            let mut raw = message.payload.clone();
            if let Some(object) = raw.as_object_mut() {
                object.remove("timelines");
            }
            inbound.push(Inbound::Welcome(
                self.welcome(active_snapshot.take(), raw.to_string()),
            ));
        } else if let Some(snapshot) = active_snapshot.take() {
            inbound.push(Inbound::Timeline {
                sender: Some(message.source.clone()),
                snapshot,
                activate: true,
            });
        }
        for snapshot in snapshots.into_values() {
            inbound.push(Inbound::Timeline {
                sender: Some(message.source.clone()),
                snapshot,
                activate: false,
            });
        }
        // The snapshot is the whole session: what it leaves out is gone.
        for sequence in session.sequences() {
            if !timelines.contains_key(sequence) {
                inbound.push(Inbound::RemoveTimeline {
                    sender: Some(message.source.clone()),
                    sequence_id: sequence.to_string(),
                });
            }
        }
        if let Some(playback) = self.playback.clone() {
            inbound.extend(self.playback_inbound(&message.source, &playback, None));
        }
        if let Some(display) = &self.display {
            inbound.push(Inbound::Other {
                kind: format!("{DISPLAY}/SET"),
                sender: Some(message.source.clone()),
                payload: display.to_string(),
            });
        }

        self.phase = Phase::Synced;
        self.master.get_or_insert_with(|| message.source.clone());
        self.early.clear();
        // What arrived while joining and is newer than the snapshot is
        // replayed, as ORI's `apply_snapshot` does.
        let replay: Vec<Message> = std::mem::take(&mut self.buffered)
            .into_iter()
            .filter(|buffered| buffered.sync_timestamp() > snapshot_timestamp)
            .collect();
        for buffered in replay.into_iter().rev() {
            self.incoming.push_front(buffered);
        }
        inbound
    }

    fn other(&self, message: &Message) -> Vec<Inbound> {
        if self.phase != Phase::Synced {
            return Vec::new();
        }
        vec![Inbound::Other {
            kind: format!("{}/{}", message.schema, message.event),
            sender: Some(message.source.clone()),
            payload: message.payload.to_string(),
        }]
    }

    fn playback_inbound(
        &self,
        sender: &str,
        playback: &Map<String, Value>,
        session: Option<&Session>,
    ) -> Vec<Inbound> {
        let sequence = playback
            .get("timeline_guid")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        let time = playback.get("current_time");
        let number = |key: &str| {
            time.and_then(|time| time.get(key))
                .and_then(Value::as_f64)
                .unwrap_or(0.0)
        };
        let mut inbound = vec![Inbound::Playhead {
            sender: sender.to_string(),
            playhead: Playhead {
                sequence_id: sequence.clone(),
                frame: number("value"),
                playing: playback
                    .get("playing")
                    .and_then(Value::as_bool)
                    .unwrap_or(false),
                rate: number("rate"),
            },
        }];
        if let Some(clip) = playback.get("clip_guid").and_then(Value::as_str) {
            inbound.push(Inbound::Selection {
                sender: sender.to_string(),
                selection: Selection {
                    sequence_id: sequence.clone(),
                    guids: vec![clip.to_string()],
                },
            });
        }
        // Passive peers follow the timeline being played, but not into a
        // clip's own timeline.
        if let Some(session) = session {
            if session.timeline(&sequence).is_some()
                && !self.clip_timelines.contains_key(&sequence)
                && session.active_sequence() != Some(sequence.as_str())
            {
                inbound.push(Inbound::Activate {
                    sender: Some(sender.to_string()),
                    sequence_id: sequence,
                });
            }
        }
        inbound
    }

    fn timeline_inbound(
        &mut self,
        session: &Session,
        sender: &str,
        sequence: &str,
        timeline: &Value,
        activate: bool,
    ) -> Inbound {
        self.note_clip_timeline(sequence, timeline);
        Inbound::Timeline {
            sender: Some(sender.to_string()),
            snapshot: Self::next(session, sequence, timeline.to_string()),
            activate,
        }
    }

    fn apply(&mut self, message: Message, session: &Session) -> Vec<Inbound> {
        let sender = message.source.clone();
        match (message.schema.as_str(), message.event.as_str()) {
            (TIMELINE, "ADD_TIMELINE") => {
                let (Some(guid), Some(timeline)) = (
                    message
                        .text("timeline_guid")
                        .filter(|guid| !guid.is_empty()),
                    message.payload.get("timeline").filter(|t| !t.is_null()),
                ) else {
                    return Vec::new();
                };
                if session.timeline(guid).is_some() {
                    return Vec::new();
                }
                vec![self.timeline_inbound(session, &sender, guid, timeline, false)]
            }
            (TIMELINE, "REPLACE_TIMELINE") => {
                let (Some(guid), Some(timeline)) = (
                    message
                        .text("timeline_guid")
                        .filter(|guid| !guid.is_empty()),
                    message.payload.get("timeline").filter(|t| !t.is_null()),
                ) else {
                    return Vec::new();
                };
                let activate = session.active_sequence().is_none();
                vec![self.timeline_inbound(session, &sender, guid, timeline, activate)]
            }
            (TIMELINE, "RENAME_TIMELINE") => {
                let (Some(guid), Some(name)) = (
                    message.text("timeline_guid"),
                    message.text("name").filter(|name| !name.is_empty()),
                ) else {
                    return Vec::new();
                };
                let Some(mut timeline) = session
                    .timeline(guid)
                    .and_then(|snapshot| serde_json::from_str::<Value>(&snapshot.timeline).ok())
                else {
                    return Vec::new();
                };
                if timeline.get("name").and_then(Value::as_str) == Some(name) {
                    return Vec::new();
                }
                timeline["name"] = Value::String(name.to_string());
                vec![self.timeline_inbound(session, &sender, guid, &timeline, false)]
            }
            (TIMELINE, "REMOVE_TIMELINE") => {
                let Some(guid) = message.text("timeline_guid") else {
                    return Vec::new();
                };
                let Some(removed) = session.timeline(guid) else {
                    return Vec::new();
                };
                // Removing a timeline removes the clip timelines of its clips.
                let mut gone: Vec<String> = self
                    .clip_timelines
                    .iter()
                    .filter(|(_, clip)| removed.timeline.contains(&quote(clip)))
                    .map(|(timeline, _)| timeline.clone())
                    .collect();
                gone.push(guid.to_string());
                gone.into_iter()
                    .map(|sequence| {
                        self.clip_timelines.remove(&sequence);
                        Inbound::RemoveTimeline {
                            sender: Some(sender.clone()),
                            sequence_id: sequence,
                        }
                    })
                    .collect()
            }
            (OTIO_SESSION, _) => self.edit(&message, session),
            (PLAYBACK, "SET") => {
                let Some(playback) = message.payload.as_object().cloned() else {
                    return Vec::new();
                };
                let inbound = self.playback_inbound(&sender, &playback, Some(session));
                self.playback = Some(playback);
                inbound
            }
            (DISPLAY, "SET") => {
                self.display = Some(message.payload.clone());
                let mut inbound = Vec::new();
                // ORI keeps the display settings on its active timeline too,
                // without telling anyone.
                let root = session
                    .active_sequence()
                    .or_else(|| session.sequences().next());
                if let Some((sequence, mut timeline)) = root.and_then(|sequence| {
                    let snapshot = session.timeline(sequence)?;
                    Some((
                        sequence,
                        serde_json::from_str::<Value>(&snapshot.timeline).ok()?,
                    ))
                }) {
                    let mut settings = message.payload.clone();
                    if let Some(settings) = settings.as_object_mut() {
                        settings.remove("sync_timestamp");
                    }
                    if let Some(metadata) = timeline
                        .as_object_mut()
                        .map(|timeline| {
                            timeline
                                .entry("metadata")
                                .or_insert_with(|| Value::Object(Map::new()))
                        })
                        .and_then(Value::as_object_mut)
                    {
                        if metadata.get("display_settings") != Some(&settings) {
                            metadata.insert("display_settings".to_string(), settings);
                            inbound.push(
                                self.timeline_inbound(session, &sender, sequence, &timeline, false),
                            );
                        }
                    }
                }
                inbound.extend(self.other(&message));
                inbound
            }
            _ => self.other(&message),
        }
    }

    /// Applies one of ORI's `OTIO_SESSION_1.0` edits to the timeline that
    /// holds its object, as ORI's patcher does.
    fn edit(&mut self, message: &Message, session: &Session) -> Vec<Inbound> {
        let payload = &message.payload;
        let text = |key: &str| payload.get(key).and_then(Value::as_str);
        let edit = match message.event.as_str() {
            "SET_PROPERTY" => match (text("target_uuid"), text("path")) {
                (Some(target), Some(path)) => OriEdit::SetProperty {
                    target,
                    path,
                    value: payload.get("value").cloned().unwrap_or(Value::Null),
                },
                _ => return Vec::new(),
            },
            "INSERT_CHILD" => match (text("parent_uuid"), payload.get("child_data")) {
                (Some(parent), Some(child)) if child.is_object() => OriEdit::InsertChild {
                    parent,
                    index: payload.get("index").and_then(Value::as_i64).unwrap_or(-1),
                    child: child.clone(),
                },
                _ => return Vec::new(),
            },
            "MOVE_CHILD" => match (text("parent_uuid"), text("child_uuid")) {
                (Some(parent), Some(child)) => OriEdit::MoveChild {
                    parent,
                    child,
                    to_index: payload.get("to_index").and_then(Value::as_i64).unwrap_or(0),
                },
                _ => return Vec::new(),
            },
            "REMOVE_CHILD" => match (text("parent_uuid"), text("child_uuid")) {
                (Some(parent), Some(child)) => OriEdit::RemoveChild { parent, child },
                _ => return Vec::new(),
            },
            "REPLACE_ANNOTATION_COMMANDS" => match text("annotation_clip_guid") {
                Some(clip) => OriEdit::ReplaceAnnotationCommands {
                    clip,
                    commands: payload
                        .get("commands")
                        .cloned()
                        .unwrap_or_else(|| json!([])),
                },
                None => return Vec::new(),
            },
            _ => return self.other(message),
        };
        let anchor = quote(edit.anchor());
        for sequence in self.search_order(session, session.active_sequence()) {
            let Some(snapshot) = session.timeline(sequence) else {
                continue;
            };
            if !snapshot.timeline.contains(&anchor) {
                continue;
            }
            let Ok(mut timeline) = serde_json::from_str::<Value>(&snapshot.timeline) else {
                continue;
            };
            if locate(&timeline, edit.anchor()).is_none() {
                continue;
            }
            if !apply_ori(&mut timeline, edit) {
                return Vec::new();
            }
            return vec![self.timeline_inbound(
                session,
                &message.source,
                sequence,
                &timeline,
                false,
            )];
        }
        Vec::new()
    }

    fn playback_payload(&mut self, playhead: Option<&Playhead>, sequence: Option<&str>) -> Value {
        let mut playback = self.playback.clone().unwrap_or_default();
        if let Some(playhead) = playhead {
            playback.insert("playing".to_string(), Value::Bool(playhead.playing));
            playback.insert(
                "current_time".to_string(),
                json!({
                    "OTIO_SCHEMA": "RationalTime.1",
                    "rate": playhead.rate,
                    "value": playhead.frame,
                }),
            );
        }
        if let Some(sequence) = sequence.filter(|sequence| !sequence.is_empty()) {
            playback.insert(
                "timeline_guid".to_string(),
                Value::String(sequence.to_string()),
            );
        }
        playback
            .entry("view_mode")
            .or_insert_with(|| Value::String("sequence".to_string()));
        playback.insert("sync_timestamp".to_string(), json!(self.timestamp()));
        self.playback = Some(playback.clone());
        Value::Object(playback)
    }

    fn timeline_frames(
        &mut self,
        snapshot: &Snapshot,
        patches: &[Patch],
        previous: Option<&Snapshot>,
    ) -> Result<Vec<String>, Error> {
        let sequence = snapshot.sequence_id.as_str();
        let timestamp = json!(self.timestamp()).to_string();
        let whole = || {
            format!(
                "{{\"timeline_guid\":{},\"timeline\":{},\"sync_timestamp\":{timestamp}}}",
                quote(sequence),
                snapshot.timeline.trim(),
            )
        };
        if patches.is_empty() {
            // Switching to a sequence: ORI's peers follow the timeline being
            // played.
            let payload = self.playback_payload(None, Some(sequence));
            return Ok(vec![self.envelope(PLAYBACK, "SET", &payload.to_string())]);
        }
        let Some(previous) = previous else {
            return Ok(vec![self.envelope(TIMELINE, "ADD_TIMELINE", &whole())]);
        };
        let edits = if matches!(patches, [patch] if patch.op == Op::ReplaceTimeline) {
            None
        } else {
            translate(previous, patches, self.timestamp())
        };
        Ok(match edits {
            Some(edits) => edits
                .into_iter()
                .map(|(event, payload)| self.envelope(OTIO_SESSION, event, &payload.to_string()))
                .collect(),
            None => vec![self.envelope(TIMELINE, "REPLACE_TIMELINE", &whole())],
        })
    }

    fn state_snapshot(&self, target: &str, session: &Session) -> Result<String, Error> {
        let mut timelines = String::from("{");
        for (index, sequence) in session.sequences().enumerate() {
            if index > 0 {
                timelines.push(',');
            }
            let snapshot = session.timeline(sequence).expect("the session listed it");
            timelines.push_str(&quote(sequence));
            timelines.push(':');
            timelines.push_str(snapshot.timeline.trim());
        }
        timelines.push('}');
        let mut peers = Map::new();
        let mut own = Map::new();
        own.insert("app".to_string(), Value::String(self.app.clone()));
        own.insert("capabilities".to_string(), json!(self.options.capabilities));
        own.insert(
            "role".to_string(),
            Value::String(self.role_name().to_string()),
        );
        if !self.identity.is_empty() {
            own.insert("identity".to_string(), Value::Object(self.identity.clone()));
        }
        peers.insert(self.options.peer_guid.clone(), Value::Object(own));
        for (guid, known) in &self.peers {
            peers.insert(guid.clone(), Value::Object(known.entry.clone()));
        }
        let mut payload = Map::new();
        payload.insert("target_guid".to_string(), Value::String(target.to_string()));
        payload.insert(
            "active_timeline_guid".to_string(),
            session
                .active_sequence()
                .map_or(Value::Null, |active| Value::String(active.to_string())),
        );
        payload.insert("snapshot_timestamp".to_string(), json!(self.timestamp()));
        if let Some(playback) = &self.playback {
            payload.insert(
                "playback_state".to_string(),
                Value::Object(playback.clone()),
            );
        }
        if let Some(display) = &self.display {
            payload.insert("display_state".to_string(), display.clone());
        }
        payload.insert(
            "host_guid".to_string(),
            Value::String(self.options.peer_guid.clone()),
        );
        payload.insert("peers".to_string(), Value::Object(peers));
        let rest = Value::Object(payload).to_string();
        let timelines = if self.options.compress_snapshots && timelines.len() > COMPRESS_OVER {
            format!(
                "{},\"compressed\":true",
                quote(&wire::base64_encode(&wire::deflate(timelines.as_bytes())?))
            )
        } else {
            timelines
        };
        // `rest` is an object with at least one member: the timelines go
        // first in it.
        let payload = format!("{{\"timelines\":{timelines},{}", &rest[1..]);
        Ok(self.envelope(LIVE_SESSION, "STATE_SNAPSHOT", &payload))
    }
}

/// `text` as a JSON string.
fn quote(text: &str) -> String {
    serde_json::to_string(text).expect("a string always serialises")
}

/// A snapshot's timelines, expanding them if they are compressed.
fn read_timelines(payload: &Value) -> Option<Map<String, Value>> {
    let timelines = payload.get("timelines")?;
    if let Some(packed) = timelines.as_str() {
        if payload.get("compressed").and_then(Value::as_bool) != Some(true) {
            return None;
        }
        let bytes = wire::inflate(&wire::base64_decode(packed).ok()?).ok()?;
        return serde_json::from_slice::<Value>(&bytes)
            .ok()?
            .as_object()
            .cloned();
    }
    timelines.as_object().cloned()
}

/// The ORI edits that make `patches` of `previous`, or `None` if ORI cannot
/// say them.
fn translate(
    previous: &Snapshot,
    patches: &[Patch],
    timestamp: f64,
) -> Option<Vec<(&'static str, Value)>> {
    let mut tree: Value = serde_json::from_str(&previous.timeline).ok()?;
    let mut edits = Vec::new();
    for patch in patches {
        match patch.op {
            Op::SetProperty => {
                if patch.removed {
                    return None;
                }
                let value: Value = serde_json::from_str(patch.value.as_deref()?).ok()?;
                let tokens: Vec<String> = patch
                    .path
                    .strip_prefix('/')?
                    .split('/')
                    .map(unescape)
                    .collect();
                let path = match tokens.as_slice() {
                    // ORI sets a plain property with `setattr`, so only a
                    // value Python can assign goes that way.
                    [key] if !value.is_object() && !value.is_array() => key.clone(),
                    [metadata, key] if metadata == "metadata" && !key.contains('/') => {
                        format!("metadata/{key}")
                    }
                    _ => return None,
                };
                edits.push((
                    "SET_PROPERTY",
                    json!({
                        "target_uuid": patch.guid,
                        "path": path,
                        "value": value,
                        "sync_timestamp": timestamp,
                    }),
                ));
            }
            Op::InsertChild => {
                let node: Value = serde_json::from_str(patch.node.as_deref()?).ok()?;
                if has_descendants(&node) {
                    return None;
                }
                edits.push((
                    "INSERT_CHILD",
                    json!({
                        "parent_uuid": patch.parent_guid,
                        "index": patch.index?,
                        "child_data": node,
                        "sync_timestamp": timestamp,
                    }),
                ));
            }
            Op::MoveChild => {
                let moving = locate(&tree, &patch.guid)?;
                let (source, _) = moving.parent.as_ref()?;
                let source_guid = guid_of(tree.pointer(source)?)?.to_string();
                if source_guid == patch.parent_guid {
                    edits.push((
                        "MOVE_CHILD",
                        json!({
                            "parent_uuid": patch.parent_guid,
                            "child_uuid": patch.guid,
                            "to_index": patch.index?,
                            "sync_timestamp": timestamp,
                        }),
                    ));
                } else {
                    // ORI moves only within a parent: elsewhere is a removal
                    // and an insert of the child as it now is.
                    let node = tree.pointer(&moving.pointer)?.clone();
                    if has_descendants(&node) {
                        return None;
                    }
                    edits.push((
                        "REMOVE_CHILD",
                        json!({
                            "parent_uuid": source_guid,
                            "child_uuid": patch.guid,
                            "sync_timestamp": timestamp,
                        }),
                    ));
                    edits.push((
                        "INSERT_CHILD",
                        json!({
                            "parent_uuid": patch.parent_guid,
                            "index": patch.index?,
                            "child_data": node,
                            "sync_timestamp": timestamp,
                        }),
                    ));
                }
            }
            Op::RemoveChild => edits.push((
                "REMOVE_CHILD",
                json!({
                    "parent_uuid": patch.parent_guid,
                    "child_uuid": patch.guid,
                    "sync_timestamp": timestamp,
                }),
            )),
            Op::ReplaceTimeline | Op::Other(_) => return None,
        }
        apply_patch(&mut tree, patch)?;
    }
    Some(edits)
}

/// Reads one envelope.
fn parse(frame: &Frame) -> Result<Message, Error> {
    let bytes = if frame.binary {
        wire::inflate(&frame.data)?
    } else {
        frame.data.clone()
    };
    let envelope: Value =
        serde_json::from_slice(&bytes).map_err(|error| Error::Json(error.to_string()))?;
    let source = envelope
        .get("source_guid")
        .and_then(Value::as_str)
        .unwrap_or("unknown")
        .to_string();
    let inner = envelope
        .get("payload")
        .ok_or_else(|| Error::Json("the envelope has no payload".into()))?;
    let schema = inner
        .get("command_schema")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let command = inner.get("command");
    let event = command
        .and_then(|command| command.get("event"))
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let payload = command
        .and_then(|command| command.get("payload"))
        .cloned()
        .unwrap_or_else(|| json!({}));
    Ok(Message {
        session: envelope
            .get("session")
            .and_then(Value::as_str)
            .map(str::to_string),
        source,
        schema,
        event,
        payload,
    })
}

impl Profile for Ori {
    fn reset(&mut self) {
        let options = self.options.clone();
        let playback = self.playback.take();
        let display = self.display.take();
        *self = Self::new(options);
        self.playback = playback;
        self.display = display;
    }

    fn encode(&mut self, message: &Outbound, now: i64) -> Result<Frame, Error> {
        self.now = now;
        let mut envelopes = match message {
            Outbound::Hello(config) => {
                self.app.clone_from(&config.identity.app);
                self.display_name.clone_from(&config.identity.display_name);
                self.identity = Map::new();
                self.identity.insert(
                    "user".to_string(),
                    Value::String(config.identity.display_name.clone()),
                );
                self.identity.insert(
                    "host".to_string(),
                    Value::String(config.identity.device_name.clone()),
                );
                self.identity
                    .insert("source".to_string(), Value::String("otio-rust".to_string()));
                self.discover();
                self.announce();
                Vec::new()
            }
            Outbound::Timeline {
                snapshot,
                patches,
                previous,
                ..
            } => {
                self.note_clip_timeline(
                    &snapshot.sequence_id,
                    &serde_json::from_str(&snapshot.timeline).unwrap_or(Value::Null),
                );
                self.timeline_frames(snapshot, patches, previous.as_ref())?
            }
            Outbound::Playhead(playhead) => {
                let payload = self.playback_payload(Some(playhead), Some(&playhead.sequence_id));
                vec![self.envelope(PLAYBACK, "SET", &payload.to_string())]
            }
            Outbound::Selection(selection) => {
                let mut payload = self.playback_payload(None, Some(&selection.sequence_id));
                payload["clip_guid"] = selection
                    .guids
                    .first()
                    .map_or(Value::Null, |guid| Value::String(guid.clone()));
                if let (Some(playback), Value::Object(payload)) = (&mut self.playback, &payload) {
                    playback.clone_from(payload);
                }
                vec![self.envelope(PLAYBACK, "SET", &payload.to_string())]
            }
            Outbound::Bye => {
                self.phase = Phase::Idle;
                let payload = json!({ "peer_guid": self.options.peer_guid });
                vec![self.envelope(LIVE_SESSION, "PEER_DEPART", &payload.to_string())]
            }
            Outbound::Other { kind, payload, .. } => {
                let Some((schema, event)) = kind.rsplit_once('/') else {
                    return Err(Error::Unsupported(format!(
                        "an ORI message's kind is \"<schema>/<event>\", not {kind:?}"
                    )));
                };
                let parsed: Value = serde_json::from_str(payload)
                    .map_err(|error| Error::Json(error.to_string()))?;
                if schema == DISPLAY && event == "SET" {
                    self.display = Some(parsed.clone());
                }
                if schema == PLAYBACK && event == "SET" {
                    self.playback = parsed.as_object().cloned();
                }
                vec![self.envelope(schema, event, &parsed.to_string())]
            }
            Outbound::ReviewCommit(_) | Outbound::SetLeader(_) | Outbound::SetMuted { .. } => {
                return Err(Error::Unsupported(
                    "ORI's wire has no review ledger, playback leader or muting".into(),
                ));
            }
        }
        .into_iter();
        let first = match envelopes.next() {
            Some(first) => self.frame(first)?,
            // The hello queued its frames: the first of them goes now.
            None => self
                .outgoing
                .pop_front()
                .ok_or_else(|| Error::Protocol("nothing to send".into()))?,
        };
        for envelope in envelopes {
            let frame = self.frame(envelope)?;
            self.outgoing.push_back(frame);
        }
        Ok(first)
    }

    fn decode(&mut self, frame: &Frame) -> Result<Vec<Inbound>, Error> {
        let message = parse(frame)?;
        if message.source == self.options.peer_guid
            || message
                .session
                .as_deref()
                .is_some_and(|id| id != self.options.session_id)
        {
            return Ok(Vec::new());
        }
        self.incoming.push_back(message);
        Ok(Vec::new())
    }

    fn advance(&mut self, now: i64) {
        self.now = now;
        match self.phase {
            Phase::Idle => return,
            Phase::Discovering { since } => {
                if now - since >= self.options.discovery_timeout {
                    self.elect_self();
                }
            }
            Phase::Joining { since } => {
                if now - since > self.options.state_request_timeout {
                    self.discover();
                }
            }
            Phase::Synced => {
                if !self.is_master
                    && self
                        .who_is_master_at
                        .is_some_and(|asked| now - asked > self.options.discovery_timeout)
                {
                    self.elect_self();
                }
            }
        }
        if self
            .last_announce
            .is_none_or(|last| now - last >= self.options.heartbeat_interval)
        {
            self.announce();
        }
        let stale: Vec<String> = self
            .peers
            .iter()
            .filter(|(_, known)| now - known.last_seen > self.options.peer_timeout)
            .map(|(guid, _)| guid.clone())
            .collect();
        for guid in stale {
            if let Some(known) = self.peers.remove(&guid) {
                let peer = self.peer(&guid, &known);
                let roster = self.roster(RosterEvent::Leave, peer);
                self.ready.push_back(vec![roster]);
            }
        }
    }

    fn poll_inbound(&mut self, session: &Session) -> Option<Vec<Inbound>> {
        if let Some(ready) = self.ready.pop_front() {
            return Some(ready);
        }
        let message = self.incoming.pop_front()?;
        Some(self.handle(message, session))
    }

    fn poll_frame(&mut self, session: &Session) -> Result<Option<Frame>, Error> {
        if let Some(frame) = self.outgoing.pop_front() {
            return Ok(Some(frame));
        }
        if self.is_master {
            if let Some(requester) = self.requests.pop_front() {
                let text = self.state_snapshot(&requester, session)?;
                return self.frame(text).map(Some);
            }
        }
        Ok(None)
    }
}
