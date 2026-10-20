//! The wire profile for Alchemist's Furnace (svc-furnace), protocol v2.
//!
//! Furnace's protocol is in its `docs/protocol.md`; its conformance corpus
//! is vendored as this crate's tests. Every message is one JSON envelope:
//!
//! ```json
//! {"v":2,"type":"presence","sessionId":"s_…","senderId":"p_…","seq":4,"ts":1730000000000,"order":19,"payload":{}}
//! ```
//!
//! The client's first frame is `hello`, in a `v:1` envelope, listing the
//! versions it speaks (`[2,1]`); the server picks the highest it shares and
//! uses it from the `welcome` on. The client numbers its frames (`seq`) and
//! omits `order`; the server numbers every frame it sends one connection
//! (`order`, from 1, with no gaps), so a gap means the connection lost
//! something and must reconnect ([`Error::Gap`]).
//!
//! # How sync's channels map to Furnace
//!
//! | Channel | Furnace |
//! | --- | --- |
//! | timeline | `patch` or `snapshot`, host only; see below |
//! | playback | `presence`'s `playhead` |
//! | presence | `welcome` and `roster` |
//! | review | `review_commit`, `review_committed`, `review_rejected` |
//! | control | `leader_set`, `leader`, `mute_set`, `bye`, `session_ended`, `error` |
//! | signal | `signal`, passed through |
//!
//! Furnace does not relay selections or other extensions' channels yet, and
//! closes a connection that sends a type it does not know, so encoding those
//! is [`Error::Unsupported`]. Thumbnail, waveform, bin and LAN messages pass
//! through as [`Inbound::Other`] and [`Outbound::Other`].
//!
//! # Patches
//!
//! A Furnace whose welcome lists `timeline.patch` in `serverCapabilities`
//! takes the host's changes as `patch` messages: the patches from the
//! generation before, which Furnace applies to its own copy. The profile
//! sends the whole timeline as a `snapshot` instead for the first
//! generation, when the diff fell back to replacing the timeline, when the
//! patches would be no smaller, and every [`CHECKPOINT`] patches, so a copy
//! that drifted is put right. An older Furnace gets snapshots only.
//!
//! The hello lists `timeline.patch.receive` among the capabilities, so
//! Furnace sends this client patches rather than rebuilt snapshots; the
//! session applies them to its copy. Furnace refuses a patch that does not
//! start from the generation it holds (`stale_patch`) or does not apply
//! (`patch_rejected`), and the host's session answers with the whole
//! timeline.
//!
//! # Compression
//!
//! The hello lists the encodings this client accepts in `encodings`, a
//! field Furnace ignores until it supports it; a welcome that lists
//! `encodings` back turns compression on for frames this client sends. See
//! [`crate::compression`].

use serde::{Deserialize, Serialize};
use serde_json::value::RawValue;

use otio_ext_sync::session::{
    Channel, Config, Identity, Inbound, Outbound, Peer, Playhead, ReviewAck, ReviewCommit,
    ReviewRelay, Role, RosterEvent, Welcome,
};
use otio_ext_sync::{Op, Snapshot};

use crate::compression::{self, Encoding};
use crate::{Error, Frame, Profile};

/// The capability a hello lists to receive patches rather than snapshots.
pub const PATCH_RECEIVE_CAPABILITY: &str = "timeline.patch.receive";

/// What a welcome's `serverCapabilities` lists when Furnace takes patches.
pub const PATCH_SERVER_CAPABILITY: &str = "timeline.patch";

/// How many patch messages the host sends between whole snapshots.
pub const CHECKPOINT: u32 = 64;

/// The protocol versions this profile speaks, best first.
pub const VERSIONS: [u8; 2] = [2, 1];

/// The client message types Furnace knows.
pub const CLIENT_TYPES: [&str; 12] = [
    "hello",
    "snapshot",
    "patch",
    "presence",
    "bye",
    "leader_set",
    "mute_set",
    "bin",
    "signal",
    "review_commit",
    "lan_mode_set",
    "lan_decision",
];

/// The client message types [`Outbound::Other`] may carry: those the
/// session has no message of its own for. The rest (`hello`, `snapshot`,
/// `review_commit` and so on) go only through their own messages, so the
/// session's roles and leases decide who sends them.
pub const PASSTHROUGH_TYPES: [&str; 4] = ["bin", "signal", "lan_mode_set", "lan_decision"];

/// Furnace's v2 wire profile; see the module documentation.
#[derive(Debug, Clone)]
pub struct Furnace {
    versions: Vec<u8>,
    offered: Vec<Encoding>,
    accepted: Vec<Encoding>,
    version: Option<u8>,
    seq: u64,
    order: Option<u64>,
    receive_patches: bool,
    server_patches: bool,
    since_snapshot: u32,
}

impl Default for Furnace {
    fn default() -> Self {
        Self::new()
    }
}

impl Furnace {
    /// The profile, speaking versions 2 and 1 and accepting every
    /// encoding.
    #[must_use]
    pub fn new() -> Self {
        Self::with_encodings(Encoding::ALL.to_vec())
    }

    /// The profile, accepting only `encodings` for frames the server sends.
    /// An empty list, or one of just [`Encoding::Identity`], leaves
    /// `encodings` out of the hello.
    #[must_use]
    pub fn with_encodings(encodings: Vec<Encoding>) -> Self {
        Self {
            versions: VERSIONS.to_vec(),
            offered: encodings,
            accepted: Vec::new(),
            version: None,
            seq: 0,
            order: None,
            receive_patches: true,
            server_patches: false,
            since_snapshot: 0,
        }
    }

    /// Whether the hello asks for patches rather than rebuilt snapshots
    /// (it does unless this turns it off).
    pub const fn set_receive_patches(&mut self, receive: bool) {
        self.receive_patches = receive;
    }

    /// Sets whether the server takes patches, which the welcome otherwise
    /// sets: for a test or a server encoding frames as a client would.
    pub const fn set_server_patches(&mut self, takes: bool) {
        self.server_patches = takes;
    }

    /// Whether the server takes patches, from its welcome.
    #[must_use]
    pub const fn server_patches(&self) -> bool {
        self.server_patches
    }

    /// Limits the versions the hello lists, best first.
    pub fn set_versions(&mut self, versions: Vec<u8>) {
        self.versions = versions;
    }

    /// The version the server chose, once welcomed.
    #[must_use]
    pub const fn version(&self) -> Option<u8> {
        self.version
    }

    /// Sets the chosen version, which the welcome otherwise sets: for a
    /// test or a server encoding frames as a client would.
    pub const fn set_version(&mut self, version: Option<u8>) {
        self.version = version;
    }

    /// The encodings the server accepts, from its welcome.
    #[must_use]
    pub fn accepted(&self) -> &[Encoding] {
        &self.accepted
    }

    fn envelope_version(&self, message: &Outbound) -> u8 {
        if matches!(message, Outbound::Hello(_)) {
            1
        } else {
            self.version.unwrap_or(1)
        }
    }

    /// The `patch` payload for `message`, if it is a timeline change this
    /// server takes as patches and a patch is the better way to send it.
    fn patch_payload(
        &self,
        message: &Outbound,
        version: u8,
    ) -> Result<Option<Box<RawValue>>, Error> {
        let Outbound::Timeline {
            snapshot,
            patches,
            base_generation: Some(base_generation),
        } = message
        else {
            return Ok(None);
        };
        if !self.server_patches
            || version < 2
            || patches.is_empty()
            || self.since_snapshot + 1 >= CHECKPOINT
            || patches.iter().any(|patch| patch.op == Op::ReplaceTimeline)
        {
            return Ok(None);
        }
        let encoded = otio_ext_sync::patch::to_json(patches);
        // A snapshot with no timeline is one `decode_client` read from a
        // patch, which can only go as one.
        if !snapshot.timeline.is_empty() && encoded.len() >= snapshot.timeline.len() {
            return Ok(None);
        }
        raw_value(&PatchPayload {
            sequence_id: snapshot.sequence_id.clone(),
            base_generation: *base_generation,
            generation: snapshot.generation,
            patches: raw(&encoded)?,
        })
        .map(Some)
    }

    fn check_order(&mut self, envelope: &Envelope<'_>) -> Result<(), Error> {
        let Some(order) = envelope.order else {
            return Err(Error::Protocol(format!(
                "a server `{}` frame has no order",
                envelope.kind
            )));
        };
        // Each connection counts from 1 with no gaps, so a first frame past
        // 1 means frames (perhaps the welcome) were lost.
        let expected = self.order.map_or(1, |last| last + 1);
        if order != expected {
            return Err(Error::Gap {
                expected,
                got: order,
            });
        }
        self.order = Some(order);
        Ok(())
    }
}

impl Profile for Furnace {
    fn reset(&mut self) {
        self.accepted.clear();
        self.version = None;
        self.seq = 0;
        self.order = None;
        self.server_patches = false;
        self.since_snapshot = 0;
    }

    fn encode(&mut self, message: &Outbound, now: i64) -> Result<Frame, Error> {
        let version = self.envelope_version(message);
        let v2_only = match message {
            Outbound::ReviewCommit(_) => true,
            Outbound::Other { kind, .. } => kind.starts_with("lan_"),
            _ => false,
        };
        if version < 2 && v2_only {
            return Err(Error::Unsupported(
                "this message needs Furnace protocol v2; this server chose v1".to_string(),
            ));
        }
        let (kind, payload) = match self.patch_payload(message, version)? {
            Some(payload) => ("patch".to_string(), payload),
            None => client_payload(message, &self.versions, &self.offered, self.receive_patches)?,
        };
        if matches!(message, Outbound::Timeline { .. }) {
            self.since_snapshot = if kind == "patch" {
                self.since_snapshot + 1
            } else {
                0
            };
        }
        self.seq += 1;
        let text = to_json(&ClientEnvelope {
            v: version,
            kind: &kind,
            seq: self.seq,
            ts: now,
            payload: &payload,
        })?;
        if matches!(message, Outbound::Hello(_)) {
            return Ok(Frame::text(text));
        }
        compression::encode(text, &self.accepted)
    }

    fn decode(&mut self, frame: &Frame) -> Result<Vec<Inbound>, Error> {
        let text = compression::decode_accepting(frame, &self.offered)?;
        let envelope: Envelope<'_> = from_json(&text)?;
        if envelope.v != 1 && envelope.v != 2 {
            return Err(Error::Protocol(format!(
                "unsupported envelope version {}",
                envelope.v
            )));
        }
        let mismatched = self
            .version
            .filter(|version| envelope.kind != "welcome" && envelope.v != *version);
        if let Some(version) = mismatched {
            return Err(Error::Protocol(format!(
                "a v{} frame on a v{version} connection",
                envelope.v
            )));
        }
        self.check_order(&envelope)?;
        if envelope.kind == "welcome" {
            if self.version.is_some() {
                return Err(Error::Protocol(
                    "a second welcome on one connection".to_string(),
                ));
            }
            if !self.versions.contains(&envelope.v) {
                return Err(Error::Protocol(format!(
                    "the server chose v{}, which this client did not offer",
                    envelope.v
                )));
            }
            self.version = Some(envelope.v);
            let extras: WelcomeExtras = from_json(envelope.payload.get())?;
            self.accepted = compression::parse_names(&extras.encodings.unwrap_or_default());
            self.server_patches = envelope.v == 2
                && extras
                    .server_capabilities
                    .iter()
                    .any(|capability| capability == PATCH_SERVER_CAPABILITY);
        }
        Ok(vec![server_message(&envelope)?])
    }
}

fn to_json<T: Serialize>(value: &T) -> Result<String, Error> {
    serde_json::to_string(value).map_err(|error| Error::Json(error.to_string()))
}

fn from_json<'a, T: Deserialize<'a>>(text: &'a str) -> Result<T, Error> {
    serde_json::from_str(text).map_err(|error| Error::Json(error.to_string()))
}

fn raw(text: &str) -> Result<Box<RawValue>, Error> {
    RawValue::from_string(text.to_string()).map_err(|error| Error::Json(error.to_string()))
}

fn raw_value<T: Serialize>(value: &T) -> Result<Box<RawValue>, Error> {
    raw(&to_json(value)?)
}

/// Writes `number` as JSON, an integer without a fraction, as a Go client
/// writes a whole frame or rate.
fn number(number: f64) -> Result<Box<RawValue>, Error> {
    if !number.is_finite() {
        return Err(Error::Json(format!("{number} is not a JSON number")));
    }
    #[allow(clippy::cast_possible_truncation)]
    if number.fract() == 0.0 && number.abs() < 9_007_199_254_740_992.0 {
        return raw(&format!("{}", number as i64));
    }
    raw(&format!("{number}"))
}

// --- Envelopes -------------------------------------------------------------

#[derive(Serialize)]
struct ClientEnvelope<'a> {
    v: u8,
    #[serde(rename = "type")]
    kind: &'a str,
    seq: u64,
    ts: i64,
    payload: &'a RawValue,
}

/// A server envelope, as Furnace's `wire.Envelope` is written.
#[derive(Serialize, Deserialize)]
pub(crate) struct Envelope<'a> {
    pub(crate) v: u8,
    #[serde(rename = "type", borrow)]
    pub(crate) kind: &'a str,
    #[serde(rename = "sessionId", default, skip_serializing_if = "Option::is_none")]
    pub(crate) session_id: Option<String>,
    #[serde(rename = "senderId", default, skip_serializing_if = "Option::is_none")]
    pub(crate) sender_id: Option<String>,
    #[serde(
        rename = "senderPresenceId",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub(crate) sender_presence_id: Option<String>,
    #[serde(default)]
    pub(crate) seq: u64,
    #[serde(default)]
    pub(crate) ts: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) order: Option<u64>,
    #[serde(borrow)]
    pub(crate) payload: &'a RawValue,
}

// --- Payloads --------------------------------------------------------------

fn is_empty(text: &str) -> bool {
    text.is_empty()
}

#[derive(Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub(crate) struct HelloPayload {
    pub(crate) supported_versions: Vec<u8>,
    pub(crate) token: String,
    pub(crate) display_name: String,
    pub(crate) client: ClientInfo,
    #[serde(default, skip_serializing_if = "is_empty")]
    pub(crate) identity_credential: String,
    #[serde(default, skip_serializing_if = "is_empty")]
    pub(crate) device_id: String,
    #[serde(default, skip_serializing_if = "is_empty")]
    pub(crate) client_kind: String,
    #[serde(default, skip_serializing_if = "is_empty")]
    pub(crate) form_factor: String,
    #[serde(default, skip_serializing_if = "is_empty")]
    pub(crate) device_display_name: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) capabilities: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) encodings: Vec<String>,
}

#[derive(Serialize, Deserialize, Default)]
pub(crate) struct ClientInfo {
    pub(crate) app: String,
    pub(crate) version: String,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SnapshotPayload {
    pub(crate) sequence_id: String,
    pub(crate) generation: i64,
    pub(crate) otio_json: Box<RawValue>,
}

impl SnapshotPayload {
    pub(crate) fn from_snapshot(snapshot: &Snapshot) -> Result<Self, Error> {
        Ok(Self {
            sequence_id: snapshot.sequence_id.clone(),
            generation: snapshot.generation,
            otio_json: raw(&snapshot.timeline)?,
        })
    }

    pub(crate) fn into_snapshot(self) -> Snapshot {
        Snapshot {
            sequence_id: self.sequence_id,
            generation: self.generation,
            timeline: self.otio_json.get().to_string(),
        }
    }
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PatchPayload {
    pub(crate) sequence_id: String,
    pub(crate) base_generation: i64,
    pub(crate) generation: i64,
    pub(crate) patches: Box<RawValue>,
}

impl PatchPayload {
    pub(crate) fn read_patches(&self) -> Result<Vec<otio_ext_sync::Patch>, Error> {
        otio_ext_sync::patch::from_json(self.patches.get())
            .map_err(|error| Error::Json(error.to_string()))
    }
}

#[derive(Serialize, Deserialize)]
pub(crate) struct PresencePayload {
    pub(crate) playhead: PlayheadPayload,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PlayheadPayload {
    pub(crate) sequence_id: String,
    pub(crate) frame: Box<RawValue>,
    pub(crate) playing: bool,
    pub(crate) rate: Box<RawValue>,
}

impl PlayheadPayload {
    pub(crate) fn from_playhead(playhead: &Playhead) -> Result<Self, Error> {
        Ok(Self {
            sequence_id: playhead.sequence_id.clone(),
            frame: number(playhead.frame)?,
            playing: playhead.playing,
            rate: number(playhead.rate)?,
        })
    }

    fn into_playhead(self) -> Result<Playhead, Error> {
        let parse = |raw: &RawValue| -> Result<f64, Error> {
            raw.get()
                .trim()
                .parse::<f64>()
                .map_err(|_| Error::Json(format!("{} is not a number", raw.get())))
        };
        Ok(Playhead {
            sequence_id: self.sequence_id,
            frame: parse(&self.frame)?,
            playing: self.playing,
            rate: parse(&self.rate)?,
        })
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Participant {
    pub(crate) id: String,
    pub(crate) display_name: String,
    pub(crate) role: String,
    pub(crate) joined_at: i64,
    pub(crate) connection_state: String,
    #[serde(default)]
    pub(crate) muted: bool,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Member {
    pub(crate) user_id: String,
    pub(crate) display_name: String,
    pub(crate) role: String,
    pub(crate) presence_ids: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Presence {
    pub(crate) presence_id: String,
    pub(crate) user_id: String,
    pub(crate) device_id: String,
    pub(crate) client_kind: String,
    pub(crate) form_factor: String,
    pub(crate) device_display_name: String,
    #[serde(default)]
    pub(crate) capabilities: Vec<String>,
    pub(crate) joined_at: i64,
    pub(crate) connection_state: String,
    #[serde(default)]
    pub(crate) muted: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct WelcomePayload {
    self_id: String,
    #[serde(default)]
    self_user_id: Option<String>,
    #[serde(default)]
    self_presence_id: Option<String>,
    role: String,
    session: SessionInfo,
    #[serde(default)]
    roster: Vec<Participant>,
    #[serde(default)]
    presences: Option<Vec<Presence>>,
    #[serde(default)]
    leader_id: String,
    #[serde(default)]
    leader_presence_id: Option<String>,
    #[serde(default)]
    pending_mutation_acknowledgements: Vec<AckPayload>,
    #[serde(default)]
    snapshot: Option<SnapshotPayload>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct WelcomeExtras {
    #[serde(default)]
    encodings: Option<Vec<String>>,
    #[serde(default)]
    server_capabilities: Vec<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SessionInfo {
    id: String,
    created_at: i64,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RosterPayload {
    event: String,
    participant: Participant,
    #[serde(default)]
    roster: Vec<Participant>,
    #[serde(default)]
    presence: Option<Presence>,
    #[serde(default)]
    presences: Option<Vec<Presence>>,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LeaderPayload {
    pub(crate) leader_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) leader_presence_id: Option<String>,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct MuteSetPayload {
    pub(crate) participant_id: String,
    pub(crate) muted: bool,
}

#[derive(Serialize, Deserialize)]
pub(crate) struct ReasonPayload {
    pub(crate) reason: String,
}

#[derive(Serialize, Deserialize)]
pub(crate) struct ErrorPayload {
    pub(crate) code: String,
    pub(crate) message: String,
    #[serde(default)]
    pub(crate) fatal: bool,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ReviewCommitPayload {
    pub(crate) client_mutation_id: String,
    pub(crate) anchor: Box<RawValue>,
    pub(crate) body_markdown: String,
    pub(crate) annotation_commands: Vec<Box<RawValue>>,
}

impl ReviewCommitPayload {
    pub(crate) fn from_commit(commit: &ReviewCommit) -> Result<Self, Error> {
        Ok(Self {
            client_mutation_id: commit.client_mutation_id.clone(),
            anchor: raw(&commit.anchor)?,
            body_markdown: commit.body_markdown.clone(),
            annotation_commands: commit
                .annotation_commands
                .iter()
                .map(|command| raw(command))
                .collect::<Result<_, _>>()?,
        })
    }

    pub(crate) fn into_commit(self) -> ReviewCommit {
        ReviewCommit {
            client_mutation_id: self.client_mutation_id,
            anchor: self.anchor.get().to_string(),
            body_markdown: self.body_markdown,
            annotation_commands: self
                .annotation_commands
                .into_iter()
                .map(|command| command.get().to_string())
                .collect(),
        }
    }
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ReviewRelayPayload {
    pub(crate) client_mutation_id: String,
    pub(crate) comment_id: String,
    pub(crate) author_user_id: String,
    pub(crate) author_presence_id: String,
    pub(crate) server_order: u64,
    pub(crate) revision: u64,
    pub(crate) created_at: i64,
    pub(crate) anchor: Box<RawValue>,
    pub(crate) body_markdown: String,
    pub(crate) annotation_commands: Vec<Box<RawValue>>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AckPayload {
    pub(crate) client_mutation_id: String,
    pub(crate) comment_id: String,
    pub(crate) server_order: u64,
    pub(crate) revision: u64,
}

impl AckPayload {
    fn into_ack(self) -> ReviewAck {
        ReviewAck {
            client_mutation_id: self.client_mutation_id,
            comment_id: self.comment_id,
            server_order: self.server_order,
            revision: self.revision,
        }
    }
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ReviewRejectedPayload {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) client_mutation_id: Option<String>,
    pub(crate) code: String,
    pub(crate) message: String,
}

// --- Client messages -------------------------------------------------------

/// The type and payload of the frame a client sends for `message`.
fn client_payload(
    message: &Outbound,
    versions: &[u8],
    offered: &[Encoding],
    receive_patches: bool,
) -> Result<(String, Box<RawValue>), Error> {
    let typed =
        |kind: &str, payload: Result<Box<RawValue>, Error>| Ok((kind.to_string(), payload?));
    match message {
        Outbound::Hello(config) => {
            let encodings = if offered
                .iter()
                .any(|encoding| *encoding != Encoding::Identity)
            {
                compression::names(offered)
            } else {
                Vec::new()
            };
            typed(
                "hello",
                raw_value(&hello_payload(config, versions, encodings, receive_patches)),
            )
        }
        Outbound::Timeline { snapshot, .. } => typed(
            "snapshot",
            raw_value(&SnapshotPayload::from_snapshot(snapshot)?),
        ),
        Outbound::Playhead(playhead) => typed(
            "presence",
            raw_value(&PresencePayload {
                playhead: PlayheadPayload::from_playhead(playhead)?,
            }),
        ),
        Outbound::ReviewCommit(commit) => typed(
            "review_commit",
            raw_value(&ReviewCommitPayload::from_commit(commit)?),
        ),
        Outbound::SetLeader(leader) => typed(
            "leader_set",
            raw_value(&LeaderPayload {
                leader_id: leader.clone(),
                leader_presence_id: None,
            }),
        ),
        Outbound::SetMuted { participant, muted } => typed(
            "mute_set",
            raw_value(&MuteSetPayload {
                participant_id: participant.clone(),
                muted: *muted,
            }),
        ),
        Outbound::Bye => typed("bye", raw("{}")),
        Outbound::Selection(_) => Err(Error::Unsupported(
            "Furnace does not relay selections yet".to_string(),
        )),
        Outbound::Other {
            channel,
            kind,
            payload,
        } => {
            if CLIENT_TYPES.contains(&kind.as_str()) && !PASSTHROUGH_TYPES.contains(&kind.as_str())
            {
                return Err(Error::Unsupported(format!(
                    "`{kind}` messages go through their own session call, not `send` (channel {})",
                    channel.name()
                )));
            }
            if !PASSTHROUGH_TYPES.contains(&kind.as_str()) {
                return Err(Error::Unsupported(format!(
                    "Furnace does not know `{kind}` messages (channel {})",
                    channel.name()
                )));
            }
            // `signal` goes on the signal channel and the rest, all host
            // only, on the control channel, whose lease the session checks.
            let expected = if kind == "signal" {
                Channel::Signal
            } else {
                Channel::Control
            };
            if *channel != expected {
                return Err(Error::Unsupported(format!(
                    "`{kind}` messages go on the {} channel, not {}",
                    expected.name(),
                    channel.name()
                )));
            }
            typed(kind, raw(payload))
        }
    }
}

fn hello_payload(
    config: &Config,
    versions: &[u8],
    encodings: Vec<String>,
    receive_patches: bool,
) -> HelloPayload {
    let identity = &config.identity;
    let v2 = versions.contains(&2);
    let mut capabilities = if v2 {
        identity.capabilities.clone()
    } else {
        Vec::new()
    };
    // The profile decides whether patches are received, whatever the
    // identity lists: Furnace sends patches to anyone whose hello names it.
    if !receive_patches {
        capabilities.retain(|capability| capability != PATCH_RECEIVE_CAPABILITY);
    } else if v2
        && !capabilities
            .iter()
            .any(|capability| capability == PATCH_RECEIVE_CAPABILITY)
    {
        capabilities.push(PATCH_RECEIVE_CAPABILITY.to_string());
    }
    let when_v2 = |value: &str| if v2 { value.to_string() } else { String::new() };
    HelloPayload {
        supported_versions: versions.to_vec(),
        token: config.token.clone(),
        display_name: identity.display_name.clone(),
        client: ClientInfo {
            app: identity.app.clone(),
            version: identity.app_version.clone(),
        },
        identity_credential: when_v2(&identity.credential),
        device_id: when_v2(&identity.device_id),
        client_kind: when_v2(&identity.client_kind),
        form_factor: when_v2(&identity.form_factor),
        device_display_name: when_v2(&identity.device_name),
        capabilities,
        encodings,
    }
}

// --- Server messages -------------------------------------------------------

fn sender(envelope: &Envelope<'_>) -> Option<String> {
    envelope
        .sender_presence_id
        .clone()
        .or_else(|| envelope.sender_id.clone())
        .filter(|sender| !sender.is_empty())
}

fn non_empty(value: String) -> Option<String> {
    (!value.is_empty()).then_some(value)
}

fn peer_from(participant: &Participant, presence: Option<&Presence>) -> Peer {
    Peer {
        id: participant.id.clone(),
        user_id: presence.map(|presence| presence.user_id.clone()),
        display_name: participant.display_name.clone(),
        role: Role::from_name(&participant.role),
        joined_at: participant.joined_at,
        connection_state: participant.connection_state.clone(),
        muted: participant.muted,
        client_kind: presence.map(|presence| presence.client_kind.clone()),
        form_factor: presence.map(|presence| presence.form_factor.clone()),
        device_name: presence.map(|presence| presence.device_display_name.clone()),
        capabilities: presence
            .map(|presence| presence.capabilities.clone())
            .unwrap_or_default(),
    }
}

fn peers_from(roster: &[Participant], presences: Option<&[Presence]>) -> Vec<Peer> {
    roster
        .iter()
        .map(|participant| {
            let presence = presences.and_then(|presences| {
                presences
                    .iter()
                    .find(|presence| presence.presence_id == participant.id)
            });
            peer_from(participant, presence)
        })
        .collect()
}

fn server_message(envelope: &Envelope<'_>) -> Result<Inbound, Error> {
    let payload = envelope.payload.get();
    Ok(match envelope.kind {
        "welcome" => {
            let welcome: WelcomePayload = from_json(payload)?;
            Inbound::Welcome(Welcome {
                session_id: welcome.session.id,
                created_at: welcome.session.created_at,
                self_id: welcome.self_presence_id.unwrap_or(welcome.self_id),
                self_user_id: welcome.self_user_id,
                role: Some(Role::from_name(&welcome.role)),
                peers: peers_from(&welcome.roster, welcome.presences.as_deref()),
                leader: non_empty(welcome.leader_presence_id.unwrap_or(welcome.leader_id)),
                snapshot: welcome.snapshot.map(SnapshotPayload::into_snapshot),
                acknowledgements: welcome
                    .pending_mutation_acknowledgements
                    .into_iter()
                    .map(AckPayload::into_ack)
                    .collect(),
                raw: payload.to_string(),
            })
        }
        "snapshot" => Inbound::Timeline {
            sender: sender(envelope),
            snapshot: from_json::<SnapshotPayload>(payload)?.into_snapshot(),
        },
        "patch" => {
            let patch: PatchPayload = from_json(payload)?;
            Inbound::Patches {
                sender: sender(envelope),
                patches: patch.read_patches()?,
                sequence_id: patch.sequence_id,
                base_generation: patch.base_generation,
                generation: patch.generation,
            }
        }
        "presence" => Inbound::Playhead {
            sender: sender(envelope).unwrap_or_default(),
            playhead: from_json::<PresencePayload>(payload)?
                .playhead
                .into_playhead()?,
        },
        "roster" => {
            let roster: RosterPayload = from_json(payload)?;
            let event = match roster.event.as_str() {
                "join" => RosterEvent::Join,
                "leave" => RosterEvent::Leave,
                "update" => RosterEvent::Update,
                other => {
                    return Err(Error::Protocol(format!("unknown roster event `{other}`")));
                }
            };
            Inbound::Roster {
                event,
                peer: peer_from(&roster.participant, roster.presence.as_ref()),
                peers: peers_from(&roster.roster, roster.presences.as_deref()),
            }
        }
        "leader" => {
            let leader: LeaderPayload = from_json(payload)?;
            Inbound::Leader {
                leader: non_empty(leader.leader_presence_id.unwrap_or(leader.leader_id)),
            }
        }
        "review_commit" => {
            let relay: ReviewRelayPayload = from_json(payload)?;
            Inbound::ReviewRelay(ReviewRelay {
                ack: ReviewAck {
                    client_mutation_id: relay.client_mutation_id.clone(),
                    comment_id: relay.comment_id,
                    server_order: relay.server_order,
                    revision: relay.revision,
                },
                commit: ReviewCommitPayload {
                    client_mutation_id: relay.client_mutation_id,
                    anchor: relay.anchor,
                    body_markdown: relay.body_markdown,
                    annotation_commands: relay.annotation_commands,
                }
                .into_commit(),
                author_user_id: relay.author_user_id,
                author_presence_id: relay.author_presence_id,
                created_at: relay.created_at,
            })
        }
        "review_committed" => {
            Inbound::ReviewCommitted(from_json::<AckPayload>(payload)?.into_ack())
        }
        "review_rejected" => {
            let rejected: ReviewRejectedPayload = from_json(payload)?;
            Inbound::ReviewRejected {
                client_mutation_id: rejected.client_mutation_id,
                code: rejected.code,
                message: rejected.message,
            }
        }
        "session_ended" => Inbound::Ended {
            reason: from_json::<ReasonPayload>(payload)?.reason,
        },
        "error" => {
            let error: ErrorPayload = from_json(payload)?;
            Inbound::Error {
                code: error.code,
                message: error.message,
                fatal: error.fatal,
            }
        }
        other => Inbound::Other {
            kind: other.to_string(),
            sender: sender(envelope),
            payload: payload.to_string(),
        },
    })
}

/// Reads a frame a client sent, as a server would: what the client meant,
/// and the envelope's version. For [`crate::loopback`] and for checking
/// this profile against Furnace's corpus.
///
/// # Errors
///
/// [`Error::Json`] or [`Error::Protocol`] if the frame is not a client
/// frame Furnace would accept.
pub fn decode_client(frame: &Frame) -> Result<(u8, Outbound), Error> {
    let text = compression::decode(frame)?;
    let envelope: Envelope<'_> = from_json(&text)?;
    if envelope.order.is_some() || envelope.session_id.is_some() || envelope.sender_id.is_some() {
        return Err(Error::Protocol(
            "client frames omit order, sessionId and senderId".to_string(),
        ));
    }
    let payload = envelope.payload.get();
    let message = match envelope.kind {
        "hello" => {
            let hello: HelloPayload = from_json(payload)?;
            Outbound::Hello(Config {
                token: hello.token,
                identity: Identity {
                    display_name: hello.display_name,
                    app: hello.client.app,
                    app_version: hello.client.version,
                    credential: hello.identity_credential,
                    device_id: hello.device_id,
                    client_kind: hello.client_kind,
                    form_factor: hello.form_factor,
                    device_name: hello.device_display_name,
                    capabilities: hello.capabilities,
                },
            })
        }
        "snapshot" => {
            let snapshot = from_json::<SnapshotPayload>(payload)?.into_snapshot();
            Outbound::Timeline {
                snapshot,
                patches: Vec::new(),
                base_generation: None,
            }
        }
        // A patch carries no timeline: the snapshot here has its sequence
        // and generation and an empty timeline, which the server fills from
        // its own copy.
        "patch" => {
            let patch: PatchPayload = from_json(payload)?;
            Outbound::Timeline {
                patches: patch.read_patches()?,
                snapshot: Snapshot {
                    sequence_id: patch.sequence_id,
                    generation: patch.generation,
                    timeline: String::new(),
                },
                base_generation: Some(patch.base_generation),
            }
        }
        "presence" => Outbound::Playhead(
            from_json::<PresencePayload>(payload)?
                .playhead
                .into_playhead()?,
        ),
        "review_commit" => {
            Outbound::ReviewCommit(from_json::<ReviewCommitPayload>(payload)?.into_commit())
        }
        "leader_set" => Outbound::SetLeader(from_json::<LeaderPayload>(payload)?.leader_id),
        "mute_set" => {
            let mute: MuteSetPayload = from_json(payload)?;
            Outbound::SetMuted {
                participant: mute.participant_id,
                muted: mute.muted,
            }
        }
        "bye" => Outbound::Bye,
        other if CLIENT_TYPES.contains(&other) => Outbound::Other {
            channel: if other == "signal" {
                Channel::Signal
            } else {
                Channel::Control
            },
            kind: other.to_string(),
            payload: payload.to_string(),
        },
        other => return Err(Error::Protocol(format!("unknown message type `{other}`"))),
    };
    Ok((envelope.v, message))
}

/// The versions and encodings a client's hello lists, for a server.
///
/// # Errors
///
/// [`Error::Json`] if `frame` is not a hello.
pub fn hello_offer(frame: &Frame) -> Result<(Vec<u8>, Vec<Encoding>), Error> {
    let text = compression::decode(frame)?;
    let envelope: Envelope<'_> = from_json(&text)?;
    let hello: HelloPayload = from_json(envelope.payload.get())?;
    Ok((
        hello.supported_versions,
        compression::parse_names(&hello.encodings),
    ))
}
