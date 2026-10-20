//! An in-memory room that relays as Furnace does, for tests.
//!
//! A [`Room`] is a small stand-in for one Furnace session: it takes the
//! frames clients send and queues the frames Furnace would send back, with
//! no sockets and no threads. It covers joining (protocol v1 and v2), the
//! host's snapshots, playheads, the roster, leadership, muting, signalling,
//! the review ledger and compression, which is enough to run several
//! [`Connection`]s against each other in a test. It does not cover tokens,
//! join codes, thumbnails, waveforms or LAN joining, and it is not a server:
//! the real one is svc-furnace.
//!
//! ```
//! use otio_ext_sync::session::{Config, Event, Session};
//! use otio_ext_sync_transport::loopback::{Room, pump};
//! use otio_ext_sync_transport::{Connection, Furnace};
//!
//! let mut room = Room::new();
//! let config = |token: &str| Config { token: token.to_string(), ..Config::default() };
//! let mut host = Connection::new(Session::new(config(Room::HOST_TOKEN)), Furnace::new());
//! let host_id = room.open(Room::HOST_TOKEN);
//! host.connect();
//! pump(&mut room, &mut [(host_id, &mut host)], 0)?;
//! assert!(host.session().is_joined());
//! # Ok::<(), otio_ext_sync_transport::Error>(())
//! ```

use std::collections::{BTreeMap, VecDeque};

use serde::Serialize;
use serde_json::value::RawValue;

use otio_ext_sync::Snapshot;
use otio_ext_sync::session::{Config, Outbound};

use crate::compression::{self, Encoding};
use crate::furnace::{
    self, AckPayload, Envelope, ErrorPayload, LeaderPayload, Member, Participant, PlayheadPayload,
    Presence, ReviewCommitPayload, ReviewRejectedPayload, ReviewRelayPayload, SnapshotPayload,
};
use crate::{Connection, Error, Frame, Profile};

/// One client's place in a [`Room`]: a socket that opened.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ClientId(usize);

#[derive(Debug)]
struct Slot {
    token: String,
    joined: bool,
    open: bool,
    version: u8,
    id: String,
    user_id: String,
    capabilities: Vec<String>,
    accepted: Vec<Encoding>,
    order: u64,
    outbox: VecDeque<Frame>,
}

#[derive(Debug, Clone)]
struct LedgerEntry {
    user_id: String,
    commit: String,
    ack: AckPayload,
    relay: String,
}

/// An in-memory Furnace session; see the module documentation.
#[derive(Debug)]
pub struct Room {
    session_id: String,
    created_at: i64,
    encodings: Vec<Encoding>,
    slots: Vec<Slot>,
    participants: BTreeMap<String, Participant>,
    presences: BTreeMap<String, Presence>,
    host: Option<usize>,
    host_id: Option<String>,
    leader: String,
    snapshot: Option<Snapshot>,
    users: BTreeMap<String, String>,
    ledger: Vec<LedgerEntry>,
    next: u64,
}

impl Default for Room {
    fn default() -> Self {
        Self::new()
    }
}

impl Room {
    /// The token that admits the host.
    pub const HOST_TOKEN: &'static str = "host-token";
    /// The token that admits a viewer.
    pub const JOIN_TOKEN: &'static str = "join-token";

    /// An empty room that accepts every encoding.
    #[must_use]
    pub fn new() -> Self {
        Self::with_encodings(Encoding::ALL.to_vec())
    }

    /// An empty room that accepts only `encodings` from clients and lists
    /// them in its welcome; none at all behaves as Furnace does today.
    #[must_use]
    pub fn with_encodings(encodings: Vec<Encoding>) -> Self {
        Self {
            session_id: "s_loopback".to_string(),
            created_at: 1_700_000_000_000,
            encodings,
            slots: Vec::new(),
            participants: BTreeMap::new(),
            presences: BTreeMap::new(),
            host: None,
            host_id: None,
            leader: String::new(),
            snapshot: None,
            users: BTreeMap::new(),
            ledger: Vec::new(),
            next: 0,
        }
    }

    /// The room's session id.
    #[must_use]
    pub fn session_id(&self) -> &str {
        &self.session_id
    }

    /// The timeline the host last sent.
    #[must_use]
    pub const fn snapshot(&self) -> Option<&Snapshot> {
        self.snapshot.as_ref()
    }

    /// A socket opened with `token`, which is [`Room::HOST_TOKEN`] for the
    /// host and anything else for a viewer.
    pub fn open(&mut self, token: &str) -> ClientId {
        self.slots.push(Slot {
            token: token.to_string(),
            joined: false,
            open: true,
            version: 1,
            id: String::new(),
            user_id: String::new(),
            capabilities: Vec::new(),
            accepted: Vec::new(),
            order: 0,
            outbox: VecDeque::new(),
        });
        ClientId(self.slots.len() - 1)
    }

    /// Whether the client's socket is still open. The room closes it after
    /// a fatal error, as Furnace does.
    #[must_use]
    pub fn is_open(&self, client: ClientId) -> bool {
        self.slots.get(client.0).is_some_and(|slot| slot.open)
    }

    /// Takes the next frame the room sends `client`.
    pub fn take(&mut self, client: ClientId) -> Option<Frame> {
        self.slots.get_mut(client.0)?.outbox.pop_front()
    }

    /// The client's socket closed.
    pub fn close(&mut self, client: ClientId) {
        let Some(slot) = self.slots.get_mut(client.0) else {
            return;
        };
        let was_joined = slot.joined;
        slot.open = false;
        slot.joined = false;
        if !was_joined {
            return;
        }
        let id = slot.id.clone();
        if self.host == Some(client.0) {
            self.host = None;
            if let Some(participant) = self.participants.get_mut(&id) {
                participant.connection_state = "reconnecting".to_string();
            }
            if let Some(presence) = self.presences.get_mut(&id) {
                presence.connection_state = "reconnecting".to_string();
            }
            self.broadcast_roster("update", &id, None);
            if !self.leader.is_empty() {
                self.leader.clear();
                self.broadcast_leader(None);
            }
        } else {
            self.broadcast_roster("leave", &id, None);
            self.participants.remove(&id);
            self.presences.remove(&id);
        }
    }

    /// Takes in a frame `client` sent.
    ///
    /// # Errors
    ///
    /// The error that made the room close the client's socket, after
    /// queueing Furnace's fatal `error` for it.
    pub fn deliver(&mut self, client: ClientId, frame: &Frame) -> Result<(), Error> {
        if !self.is_open(client) {
            return Err(Error::Closed {
                code: None,
                reason: "the socket is closed".to_string(),
            });
        }
        let result = if self.slots[client.0].joined {
            self.inbound(client, frame)
        } else {
            self.hello(client, frame)
        };
        if let Err(error) = &result {
            let code = match error {
                Error::Unsupported(_) => "unknown_type",
                _ => "protocol_violation",
            };
            self.send_error(client.0, code, &error.to_string(), true);
            self.close(client);
        }
        result
    }

    fn next_id(&mut self, prefix: &str) -> String {
        self.next += 1;
        format!("{prefix}{}", self.next)
    }

    fn hello(&mut self, client: ClientId, frame: &Frame) -> Result<(), Error> {
        let (versions, offered) = furnace::hello_offer(frame)?;
        let (_, message) = furnace::decode_client(frame)?;
        let Outbound::Hello(Config { token, identity }) = message else {
            return Err(Error::Protocol("hello must be the first frame".to_string()));
        };
        let Some(version) = [2, 1]
            .into_iter()
            .find(|version| versions.contains(version))
        else {
            return Err(Error::Protocol("no shared protocol version".to_string()));
        };
        if token != self.slots[client.0].token {
            return Err(Error::Protocol(
                "hello token does not match connection token".to_string(),
            ));
        }
        if version == 2
            && [
                &identity.credential,
                &identity.device_id,
                &identity.client_kind,
                &identity.form_factor,
                &identity.device_name,
            ]
            .iter()
            .any(|field| field.is_empty())
        {
            return Err(Error::Protocol("a v2 hello needs its identity".to_string()));
        }

        let host = token == Self::HOST_TOKEN;
        let id = match (&self.host_id, host) {
            (Some(id), true) => id.clone(),
            _ => self.next_id("p_"),
        };
        let user_id = match self.users.get(&identity.credential) {
            Some(user) if !identity.credential.is_empty() => user.clone(),
            _ => {
                let user = self.next_id("u_");
                self.users.insert(identity.credential.clone(), user.clone());
                user
            }
        };
        let joined_at = self.created_at + i64::try_from(self.next).unwrap_or(0);
        let role = if host { "host" } else { "viewer" };
        self.participants.insert(
            id.clone(),
            Participant {
                id: id.clone(),
                display_name: identity.display_name.clone(),
                role: role.to_string(),
                joined_at,
                connection_state: "connected".to_string(),
                muted: self.participants.get(&id).is_some_and(|old| old.muted),
            },
        );
        self.presences.insert(
            id.clone(),
            Presence {
                presence_id: id.clone(),
                user_id: user_id.clone(),
                device_id: identity.device_id.clone(),
                client_kind: identity.client_kind.clone(),
                form_factor: identity.form_factor.clone(),
                device_display_name: identity.device_name.clone(),
                capabilities: identity.capabilities.clone(),
                joined_at,
                connection_state: "connected".to_string(),
                muted: false,
            },
        );
        if host {
            if let Some(old) = self.host.filter(|old| *old != client.0) {
                self.slots[old].open = false;
                self.slots[old].joined = false;
            }
            self.host = Some(client.0);
            self.host_id = Some(id.clone());
            self.leader.clone_from(&id);
        }
        let slot = &mut self.slots[client.0];
        slot.joined = true;
        slot.version = version;
        slot.id.clone_from(&id);
        slot.user_id = user_id;
        slot.capabilities = identity.capabilities;
        slot.accepted = offered;

        let welcome = self.welcome(client.0)?;
        self.send(client.0, "welcome", None, &welcome);
        if version == 2
            && self.slots[client.0]
                .capabilities
                .iter()
                .any(|c| c == "review.commit.receive")
        {
            for entry in self.ledger.clone() {
                let relay = RawValue::from_string(entry.relay).map_err(json_error)?;
                self.send(client.0, "review_commit", None, &relay);
            }
        }
        self.broadcast_roster("join", &id, Some(client.0));
        if host {
            self.broadcast_leader(Some(client.0));
        }
        Ok(())
    }

    fn welcome(&self, slot: usize) -> Result<Box<RawValue>, Error> {
        let me = &self.slots[slot];
        let mut welcome = serde_json::json!({
            "selfId": me.id,
            "role": if self.host == Some(slot) { "host" } else { "viewer" },
            "session": {
                "id": self.session_id,
                "createdAt": self.created_at,
                "features": {"programVideo": true, "programAudio": true, "voiceChat": true,
                             "reviews": true, "referenceModeRequests": false},
            },
            "roster": self.roster(),
            "leaderId": self.leader,
            "iceServers": [],
            "thumbBaseUrl": format!("/v1/sessions/{}/thumbs/", self.session_id),
        });
        let object = welcome.as_object_mut().expect("a welcome is an object");
        if me.version == 2 {
            let acks: Vec<AckPayload> = self
                .ledger
                .iter()
                .filter(|entry| entry.user_id == me.user_id)
                .map(|entry| entry.ack.clone())
                .collect();
            object.insert("selfUserId".into(), me.user_id.clone().into());
            object.insert("selfPresenceId".into(), me.id.clone().into());
            object.insert("members".into(), to_value(&self.members())?);
            object.insert("presences".into(), to_value(&self.presence_list())?);
            object.insert("leaderPresenceId".into(), self.leader.clone().into());
            object.insert("pendingMutationAcknowledgements".into(), to_value(&acks)?);
            object.insert("lanMode".into(), "ask".into());
            if self
                .encodings
                .iter()
                .any(|encoding| *encoding != Encoding::Identity)
            {
                object.insert(
                    "encodings".into(),
                    compression::names(&self.encodings).into(),
                );
            }
        }
        if let Some(snapshot) = &self.snapshot {
            object.insert(
                "snapshot".into(),
                to_value(&SnapshotPayload::from_snapshot(snapshot)?)?,
            );
        }
        RawValue::from_string(welcome.to_string()).map_err(json_error)
    }

    fn roster(&self) -> Vec<Participant> {
        let mut roster: Vec<Participant> = self.participants.values().cloned().collect();
        roster.sort_by_key(|participant| participant.joined_at);
        roster
    }

    fn presence_list(&self) -> Vec<Presence> {
        let mut presences: Vec<Presence> = self
            .presences
            .values()
            .map(|presence| {
                let mut presence = presence.clone();
                presence.muted = self
                    .participants
                    .get(&presence.presence_id)
                    .is_some_and(|p| p.muted);
                presence
            })
            .collect();
        presences.sort_by_key(|presence| presence.joined_at);
        presences
    }

    fn members(&self) -> Vec<Member> {
        let mut members: Vec<Member> = Vec::new();
        for presence in self.presence_list() {
            let participant = &self.participants[&presence.presence_id];
            match members
                .iter_mut()
                .find(|member| member.user_id == presence.user_id)
            {
                Some(member) => member.presence_ids.push(presence.presence_id.clone()),
                None => members.push(Member {
                    user_id: presence.user_id.clone(),
                    display_name: participant.display_name.clone(),
                    role: participant.role.clone(),
                    presence_ids: vec![presence.presence_id.clone()],
                }),
            }
        }
        members
    }

    fn inbound(&mut self, client: ClientId, frame: &Frame) -> Result<(), Error> {
        let (version, message) = furnace::decode_client(frame)?;
        let slot = client.0;
        if version != self.slots[slot].version {
            return Err(Error::Protocol(
                "the envelope version changed after hello".to_string(),
            ));
        }
        let sender = self.slots[slot].id.clone();
        match message {
            Outbound::Hello(_) => return Err(Error::Protocol("a second hello".to_string())),
            Outbound::Timeline { snapshot, .. } => {
                if self.host != Some(slot) {
                    return Err(Error::Protocol(
                        "only a host may send snapshots".to_string(),
                    ));
                }
                let payload = raw(&SnapshotPayload::from_snapshot(&snapshot)?)?;
                self.snapshot = Some(snapshot);
                self.broadcast("snapshot", Some(&sender), &payload, Some(slot));
            }
            Outbound::Playhead(playhead) => {
                let payload = raw(&serde_json::json!({
                    "playhead": PlayheadPayload::from_playhead(&playhead)?,
                }))?;
                self.broadcast("presence", Some(&sender), &payload, Some(slot));
            }
            Outbound::SetLeader(target) => {
                if self.host != Some(slot) {
                    self.send_error(slot, "forbidden", "only the host may set the leader", false);
                } else if !self.connected(&target) {
                    self.send_error(
                        slot,
                        "invalid_target",
                        "no such connected participant",
                        false,
                    );
                } else {
                    self.leader = target;
                    self.broadcast_leader(None);
                }
            }
            Outbound::SetMuted { participant, muted } => {
                if participant != sender && self.host != Some(slot) {
                    self.send_error(slot, "forbidden", "only the host may mute others", false);
                } else if let Some(entry) = self.participants.get_mut(&participant) {
                    entry.muted = muted;
                    self.broadcast_roster("update", &participant, None);
                } else {
                    self.send_error(slot, "invalid_target", "no such participant", false);
                }
            }
            Outbound::Bye => self.close(client),
            Outbound::ReviewCommit(commit) => self.review(slot, &sender, &commit)?,
            Outbound::Other { kind, payload, .. } if kind == "signal" => {
                let mut value: serde_json::Value =
                    serde_json::from_str(&payload).map_err(json_error)?;
                let to = value["to"].as_str().unwrap_or_default().to_string();
                value["from"] = sender.clone().into();
                match self.slot_of(&to) {
                    Some(target) => {
                        let payload = raw(&value)?;
                        self.send(target, "signal", Some(&sender), &payload);
                    }
                    None => self.send_error(
                        slot,
                        "invalid_target",
                        "no such connected participant",
                        false,
                    ),
                }
            }
            Outbound::Other { kind, payload, .. } if kind == "bin" => {
                if self.host == Some(slot) {
                    let payload = RawValue::from_string(payload).map_err(json_error)?;
                    self.broadcast("bin", None, &payload, Some(slot));
                } else {
                    self.send_error(slot, "forbidden", "only the host may send the bin", false);
                }
            }
            Outbound::Other { kind, .. } => {
                self.send_error(
                    slot,
                    "bad_request",
                    &format!("the loopback room does not handle `{kind}`"),
                    false,
                );
            }
            Outbound::Selection(_) => {
                return Err(Error::Unsupported(
                    "Furnace does not relay selections".to_string(),
                ));
            }
        }
        Ok(())
    }

    fn review(
        &mut self,
        slot: usize,
        sender: &str,
        commit: &otio_ext_sync::session::ReviewCommit,
    ) -> Result<(), Error> {
        let reject = |room: &mut Self, code: &str, message: &str| {
            let payload = raw(&ReviewRejectedPayload {
                client_mutation_id: Some(commit.client_mutation_id.clone()),
                code: code.to_string(),
                message: message.to_string(),
            });
            if let Ok(payload) = payload {
                room.send(slot, "review_rejected", None, &payload);
            }
        };
        let me = &self.slots[slot];
        if me.version != 2 || !me.capabilities.iter().any(|c| c == "review.comment.create") {
            reject(self, "forbidden", "this presence may not comment");
            return Ok(());
        }
        let user_id = me.user_id.clone();
        let text = serde_json::to_string(&ReviewCommitPayload::from_commit(commit)?)
            .map_err(json_error)?;
        if let Some(entry) = self.ledger.iter().find(|entry| {
            entry.user_id == user_id && entry.ack.client_mutation_id == commit.client_mutation_id
        }) {
            if entry.commit == text {
                let ack = raw(&entry.ack)?;
                self.send(slot, "review_committed", None, &ack);
            } else {
                reject(
                    self,
                    "mutation_conflict",
                    "clientMutationId was already used for a different review commit",
                );
            }
            return Ok(());
        }
        let Some(host) = self.host.filter(|host| {
            self.slots[*host].version == 2
                && self.slots[*host]
                    .capabilities
                    .iter()
                    .any(|c| c == "review.commit.receive")
        }) else {
            reject(
                self,
                "host_unavailable",
                "no connected host can receive review commits",
            );
            return Ok(());
        };
        let server_order = u64::try_from(self.ledger.len()).unwrap_or(u64::MAX) + 1;
        let ack = AckPayload {
            client_mutation_id: commit.client_mutation_id.clone(),
            comment_id: self.next_id("comment_"),
            server_order,
            revision: 1,
        };
        let payload = ReviewCommitPayload::from_commit(commit)?;
        let relay = ReviewRelayPayload {
            client_mutation_id: ack.client_mutation_id.clone(),
            comment_id: ack.comment_id.clone(),
            author_user_id: user_id.clone(),
            author_presence_id: sender.to_string(),
            server_order,
            revision: 1,
            created_at: self.created_at + i64::try_from(server_order).unwrap_or(0),
            anchor: payload.anchor,
            body_markdown: payload.body_markdown,
            annotation_commands: payload.annotation_commands,
        };
        let relay = serde_json::to_string(&relay).map_err(json_error)?;
        self.ledger.push(LedgerEntry {
            user_id,
            commit: text,
            ack: ack.clone(),
            relay: relay.clone(),
        });
        let relay = RawValue::from_string(relay).map_err(json_error)?;
        self.send(host, "review_commit", Some(sender), &relay);
        let ack = raw(&ack)?;
        self.send(slot, "review_committed", None, &ack);
        Ok(())
    }

    fn connected(&self, id: &str) -> bool {
        self.slot_of(id).is_some()
    }

    fn slot_of(&self, id: &str) -> Option<usize> {
        self.slots
            .iter()
            .position(|slot| slot.joined && slot.id == id)
    }

    fn send_error(&mut self, slot: usize, code: &str, message: &str, fatal: bool) {
        if let Ok(payload) = raw(&ErrorPayload {
            code: code.to_string(),
            message: message.to_string(),
            fatal,
        }) {
            self.send(slot, "error", None, &payload);
        }
    }

    fn broadcast_leader(&mut self, except: Option<usize>) {
        for slot in self.joined_except(except) {
            let presence = (self.slots[slot].version == 2).then(|| self.leader.clone());
            if let Ok(payload) = raw(&LeaderPayload {
                leader_id: self.leader.clone(),
                leader_presence_id: presence,
            }) {
                self.send(slot, "leader", None, &payload);
            }
        }
    }

    fn broadcast_roster(&mut self, event: &str, id: &str, except: Option<usize>) {
        let Some(participant) = self.participants.get(id).cloned() else {
            return;
        };
        let roster: Vec<Participant> = self
            .roster()
            .into_iter()
            .filter(|entry| event != "leave" || entry.id != id)
            .collect();
        let presence = self.presences.get(id).cloned().map(|mut presence| {
            presence.muted = participant.muted;
            presence
                .connection_state
                .clone_from(&participant.connection_state);
            presence
        });
        for slot in self.joined_except(except) {
            let mut payload = serde_json::json!({
                "event": event,
                "participant": participant,
                "roster": roster,
            });
            if self.slots[slot].version == 2 {
                let object = payload.as_object_mut().expect("a roster is an object");
                object.insert("presenceEvent".into(), event.into());
                if let Some(presence) = &presence {
                    object.insert(
                        "presence".into(),
                        serde_json::to_value(presence).unwrap_or_default(),
                    );
                }
                let leaving = |presence: &Presence| event == "leave" && presence.presence_id == id;
                let presences: Vec<Presence> = self
                    .presence_list()
                    .into_iter()
                    .filter(|p| !leaving(p))
                    .collect();
                let members: Vec<Member> = self
                    .members()
                    .into_iter()
                    .map(|mut member| {
                        member
                            .presence_ids
                            .retain(|presence| event != "leave" || presence != id);
                        member
                    })
                    .filter(|member| !member.presence_ids.is_empty())
                    .collect();
                object.insert(
                    "presences".into(),
                    serde_json::to_value(presences).unwrap_or_default(),
                );
                object.insert(
                    "members".into(),
                    serde_json::to_value(members).unwrap_or_default(),
                );
            }
            if let Ok(payload) = raw(&payload) {
                self.send(slot, "roster", None, &payload);
            }
        }
    }

    fn joined_except(&self, except: Option<usize>) -> Vec<usize> {
        (0..self.slots.len())
            .filter(|slot| self.slots[*slot].joined && Some(*slot) != except)
            .collect()
    }

    fn broadcast(
        &mut self,
        kind: &str,
        sender: Option<&str>,
        payload: &RawValue,
        except: Option<usize>,
    ) {
        for slot in self.joined_except(except) {
            self.send(slot, kind, sender, payload);
        }
    }

    fn send(&mut self, slot: usize, kind: &str, sender: Option<&str>, payload: &RawValue) {
        let session_id = self.session_id.clone();
        let target = &mut self.slots[slot];
        target.order += 1;
        let envelope = Envelope {
            v: target.version,
            kind,
            session_id: Some(session_id),
            sender_id: sender.map(str::to_string),
            sender_presence_id: sender.filter(|_| target.version == 2).map(str::to_string),
            seq: 0,
            ts: self.created_at,
            order: Some(target.order),
            payload,
        };
        let text = serde_json::to_string(&envelope).unwrap_or_default();
        let accepted: Vec<Encoding> = if kind == "welcome" {
            Vec::new()
        } else {
            target
                .accepted
                .iter()
                .copied()
                .filter(|encoding| self.encodings.contains(encoding))
                .collect()
        };
        let frame =
            compression::encode(text.clone(), &accepted).unwrap_or_else(|_| Frame::text(text));
        target.outbox.push_back(frame);
    }
}

fn json_error(error: serde_json::Error) -> Error {
    Error::Json(error.to_string())
}

fn raw<T: Serialize>(value: &T) -> Result<Box<RawValue>, Error> {
    serde_json::value::to_raw_value(value).map_err(json_error)
}

fn to_value<T: Serialize>(value: &T) -> Result<serde_json::Value, Error> {
    serde_json::to_value(value).map_err(json_error)
}

/// Moves frames between `room` and `clients`, each a connection and the
/// [`ClientId`] its socket has in the room, until none moves, at `now`.
/// A connection whose socket the room closed is told it disconnected.
///
/// # Errors
///
/// The first error a connection returns while reading or writing frames,
/// or the room returns for a frame it refuses (after closing that client).
pub fn pump<P: Profile>(
    room: &mut Room,
    clients: &mut [(ClientId, &mut Connection<P>)],
    now: i64,
) -> Result<(), Error> {
    loop {
        let mut moved = false;
        for (id, connection) in clients.iter_mut() {
            while let Some(frame) = connection.poll_transmit(now)? {
                moved = true;
                room.deliver(*id, &frame)?;
            }
            while let Some(frame) = room.take(*id) {
                moved = true;
                connection.receive(&frame)?;
            }
            if !room.is_open(*id) && connection.session().is_joined() {
                connection.disconnected();
            }
        }
        if !moved {
            return Ok(());
        }
    }
}
