//! A session's settings, events and state as JSON, for the C interface and
//! every binding over it.
//!
//! Each language takes these apart with its own JSON library, so the shapes
//! here are the interface, and they keep to the spelling Furnace uses:
//! camelCase fields, and a snapshot as `sequenceId`, `generation` and
//! `otioJson`. An event is an object whose `type` says which:
//!
//! | `type` | Fields |
//! | --- | --- |
//! | `joined` | `role`, `selfId`, `rejoined` |
//! | `timeline` | `sequenceId`, `generation`, `sender`, `patches`, `snapshot`, `joining` |
//! | `timeline_removed` | `sequenceId`, `sender` |
//! | `playhead` | `sender`, `playhead` (`sequenceId`, `frame`, `playing`, `rate`) |
//! | `selection` | `sender`, `selection` (`sequenceId`, `guids`) |
//! | `roster` | `event` (`join`, `update` or `leave`), `peer` |
//! | `leader` | `leader` |
//! | `review_relay` | `commit`, `ack`, `authorUserId`, `authorPresenceId`, `createdAt` |
//! | `review_committed` | `clientMutationId`, `commentId`, `serverOrder`, `revision` |
//! | `review_rejected` | `clientMutationId`, `code`, `message` |
//! | `error` | `code`, `message`, `fatal` |
//! | `ended` | `reason` |
//! | `message` | `kind`, `sender`, `payload` |

use serde::{Deserialize, Serialize};
use serde_json::value::RawValue;

use otio_ext_sync::Snapshot;
use otio_ext_sync::session::{
    Config, Event, Identity, Peer, Playhead, ReviewAck, ReviewCommit, ReviewRelay, Role,
    RosterEvent, Selection, Session, TimelineChange,
};

use crate::Error;
use crate::compression::Encoding;

/// What a program sets up a session with: the session's [`Config`] and the
/// encodings it accepts.
#[derive(Debug, Clone)]
pub struct Settings {
    /// How the session joins.
    pub config: Config,
    /// The encodings to accept for frames the server sends: every one
    /// unless the settings list them.
    pub encodings: Vec<Encoding>,
}

#[derive(Deserialize, Default)]
#[serde(default, rename_all = "camelCase", deny_unknown_fields)]
struct SettingsJson {
    token: String,
    identity: Option<IdentityJson>,
    encodings: Option<Vec<String>>,
}

#[derive(Deserialize, Default)]
#[serde(default, rename_all = "camelCase", deny_unknown_fields)]
struct IdentityJson {
    display_name: Option<String>,
    app: Option<String>,
    app_version: Option<String>,
    credential: Option<String>,
    device_id: Option<String>,
    client_kind: Option<String>,
    form_factor: Option<String>,
    device_name: Option<String>,
    capabilities: Option<Vec<String>>,
}

/// Reads a session's settings: an object with `token`, `identity` and
/// `encodings`, every one of them optional.
///
/// `identity` holds `displayName`, `app`, `appVersion`, `credential`,
/// `deviceId`, `clientKind`, `formFactor`, `deviceName` and `capabilities`;
/// any left out keep [`Identity::default`]'s value. `encodings` lists `br`,
/// `gzip` and `identity`, and an empty list turns compression off.
///
/// # Errors
///
/// [`Error::Json`] for anything else, including a field this does not know,
/// so that a misspelt one is not silently ignored.
pub fn settings_from_json(text: &str) -> Result<Settings, Error> {
    let read: SettingsJson = parse(text)?;
    let mut identity = Identity::default();
    if let Some(given) = read.identity {
        let fields = [
            (given.display_name, &mut identity.display_name),
            (given.app, &mut identity.app),
            (given.app_version, &mut identity.app_version),
            (given.credential, &mut identity.credential),
            (given.device_id, &mut identity.device_id),
            (given.client_kind, &mut identity.client_kind),
            (given.form_factor, &mut identity.form_factor),
            (given.device_name, &mut identity.device_name),
        ];
        for (value, field) in fields {
            if let Some(value) = value {
                *field = value;
            }
        }
        if let Some(capabilities) = given.capabilities {
            identity.capabilities = capabilities;
        }
    }
    let encodings = match read.encodings {
        None => Encoding::ALL.to_vec(),
        Some(names) => names
            .iter()
            .map(|name| {
                Encoding::from_name(name)
                    .ok_or_else(|| Error::Json(format!("unknown encoding {name:?}")))
            })
            .collect::<Result<_, _>>()?,
    };
    Ok(Settings {
        config: Config {
            token: read.token,
            identity,
        },
        encodings,
    })
}

fn parse<'a, T: Deserialize<'a>>(text: &'a str) -> Result<T, Error> {
    serde_json::from_str(text).map_err(|error| Error::Json(error.to_string()))
}

fn raw(text: String) -> Box<RawValue> {
    RawValue::from_string(text).unwrap_or_else(|_| null())
}

fn null() -> Box<RawValue> {
    RawValue::from_string("null".to_string()).expect("null is JSON")
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PlayheadJson {
    sequence_id: String,
    frame: f64,
    playing: bool,
    rate: f64,
}

impl From<&Playhead> for PlayheadJson {
    fn from(playhead: &Playhead) -> Self {
        Self {
            sequence_id: playhead.sequence_id.clone(),
            frame: playhead.frame,
            playing: playhead.playing,
            rate: playhead.rate,
        }
    }
}

impl From<PlayheadJson> for Playhead {
    fn from(playhead: PlayheadJson) -> Self {
        Self {
            sequence_id: playhead.sequence_id,
            frame: playhead.frame,
            playing: playhead.playing,
            rate: playhead.rate,
        }
    }
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SelectionJson {
    sequence_id: String,
    guids: Vec<String>,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PeerJson {
    id: String,
    user_id: Option<String>,
    display_name: String,
    role: String,
    joined_at: i64,
    connection_state: String,
    muted: bool,
    client_kind: Option<String>,
    form_factor: Option<String>,
    device_name: Option<String>,
    capabilities: Vec<String>,
}

impl From<&Peer> for PeerJson {
    fn from(peer: &Peer) -> Self {
        Self {
            id: peer.id.clone(),
            user_id: peer.user_id.clone(),
            display_name: peer.display_name.clone(),
            role: peer.role.as_str().to_string(),
            joined_at: peer.joined_at,
            connection_state: peer.connection_state.clone(),
            muted: peer.muted,
            client_kind: peer.client_kind.clone(),
            form_factor: peer.form_factor.clone(),
            device_name: peer.device_name.clone(),
            capabilities: peer.capabilities.clone(),
        }
    }
}

impl From<PeerJson> for Peer {
    fn from(peer: PeerJson) -> Self {
        Self {
            id: peer.id,
            user_id: peer.user_id,
            display_name: peer.display_name,
            role: Role::from_name(&peer.role),
            joined_at: peer.joined_at,
            connection_state: peer.connection_state,
            muted: peer.muted,
            client_kind: peer.client_kind,
            form_factor: peer.form_factor,
            device_name: peer.device_name,
            capabilities: peer.capabilities,
        }
    }
}

#[derive(Serialize, Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
struct CommitJson {
    client_mutation_id: String,
    anchor: Option<Box<RawValue>>,
    body_markdown: String,
    annotation_commands: Vec<Box<RawValue>>,
}

impl From<&ReviewCommit> for CommitJson {
    fn from(commit: &ReviewCommit) -> Self {
        Self {
            client_mutation_id: commit.client_mutation_id.clone(),
            anchor: Some(raw(commit.anchor.clone())),
            body_markdown: commit.body_markdown.clone(),
            annotation_commands: commit
                .annotation_commands
                .iter()
                .map(|command| raw(command.clone()))
                .collect(),
        }
    }
}

impl From<CommitJson> for ReviewCommit {
    fn from(commit: CommitJson) -> Self {
        Self {
            client_mutation_id: commit.client_mutation_id,
            anchor: commit
                .anchor
                .map_or_else(|| "null".to_string(), |anchor| anchor.get().to_string()),
            body_markdown: commit.body_markdown,
            annotation_commands: commit
                .annotation_commands
                .iter()
                .map(|command| command.get().to_string())
                .collect(),
        }
    }
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AckJson {
    client_mutation_id: String,
    comment_id: String,
    server_order: u64,
    revision: u64,
}

impl From<&ReviewAck> for AckJson {
    fn from(ack: &ReviewAck) -> Self {
        Self {
            client_mutation_id: ack.client_mutation_id.clone(),
            comment_id: ack.comment_id.clone(),
            server_order: ack.server_order,
            revision: ack.revision,
        }
    }
}

impl From<AckJson> for ReviewAck {
    fn from(ack: AckJson) -> Self {
        Self {
            client_mutation_id: ack.client_mutation_id,
            comment_id: ack.comment_id,
            server_order: ack.server_order,
            revision: ack.revision,
        }
    }
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum EventJson {
    #[serde(rename_all = "camelCase")]
    Joined {
        role: String,
        self_id: String,
        rejoined: bool,
    },
    #[serde(rename_all = "camelCase")]
    Timeline {
        sequence_id: String,
        generation: i64,
        sender: Option<String>,
        patches: Box<RawValue>,
        snapshot: Box<RawValue>,
        joining: bool,
    },
    Playhead {
        sender: String,
        playhead: PlayheadJson,
    },
    Selection {
        sender: String,
        selection: SelectionJson,
    },
    Roster {
        event: String,
        peer: PeerJson,
    },
    Leader {
        leader: Option<String>,
    },
    #[serde(rename_all = "camelCase")]
    ReviewRelay {
        commit: CommitJson,
        ack: AckJson,
        author_user_id: String,
        author_presence_id: String,
        created_at: i64,
    },
    #[serde(rename_all = "camelCase")]
    ReviewCommitted {
        client_mutation_id: String,
        comment_id: String,
        server_order: u64,
        revision: u64,
    },
    #[serde(rename_all = "camelCase")]
    ReviewRejected {
        client_mutation_id: Option<String>,
        code: String,
        message: String,
    },
    Error {
        code: String,
        message: String,
        fatal: bool,
    },
    #[serde(rename_all = "camelCase")]
    TimelineRemoved {
        sequence_id: String,
        sender: Option<String>,
    },
    Ended {
        reason: String,
    },
    Message {
        kind: String,
        sender: Option<String>,
        payload: Box<RawValue>,
    },
}

#[derive(Deserialize)]
struct Tag {
    #[serde(rename = "type")]
    kind: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct TimelineIn {
    sequence_id: String,
    generation: i64,
    sender: Option<String>,
    patches: Box<RawValue>,
    snapshot: Box<RawValue>,
    joining: bool,
}

#[derive(Deserialize)]
struct MessageIn {
    kind: String,
    sender: Option<String>,
    payload: Box<RawValue>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RelayIn {
    commit: CommitJson,
    ack: AckJson,
    author_user_id: String,
    author_presence_id: String,
    created_at: i64,
}

/// `event` as JSON; see the module documentation for its shape.
#[must_use]
pub fn event_to_json(event: &Event) -> String {
    let json = match event {
        Event::Joined {
            role,
            self_id,
            rejoined,
        } => EventJson::Joined {
            role: role.as_str().to_string(),
            self_id: self_id.clone(),
            rejoined: *rejoined,
        },
        Event::Timeline(change) => EventJson::Timeline {
            sequence_id: change.sequence_id.clone(),
            generation: change.generation,
            sender: change.sender.clone(),
            patches: raw(otio_ext_sync::patch::to_json(&change.patches)),
            snapshot: raw(change.snapshot.to_json()),
            joining: change.joining,
        },
        Event::Playhead { sender, playhead } => EventJson::Playhead {
            sender: sender.clone(),
            playhead: playhead.into(),
        },
        Event::Selection { sender, selection } => EventJson::Selection {
            sender: sender.clone(),
            selection: SelectionJson {
                sequence_id: selection.sequence_id.clone(),
                guids: selection.guids.clone(),
            },
        },
        Event::Roster { event, peer } => EventJson::Roster {
            event: event.as_str().to_string(),
            peer: peer.into(),
        },
        Event::Leader { leader } => EventJson::Leader {
            leader: leader.clone(),
        },
        Event::ReviewRelay(relay) => EventJson::ReviewRelay {
            commit: (&relay.commit).into(),
            ack: (&relay.ack).into(),
            author_user_id: relay.author_user_id.clone(),
            author_presence_id: relay.author_presence_id.clone(),
            created_at: relay.created_at,
        },
        Event::ReviewCommitted(ack) => EventJson::ReviewCommitted {
            client_mutation_id: ack.client_mutation_id.clone(),
            comment_id: ack.comment_id.clone(),
            server_order: ack.server_order,
            revision: ack.revision,
        },
        Event::ReviewRejected {
            client_mutation_id,
            code,
            message,
        } => EventJson::ReviewRejected {
            client_mutation_id: client_mutation_id.clone(),
            code: code.clone(),
            message: message.clone(),
        },
        Event::Error {
            code,
            message,
            fatal,
        } => EventJson::Error {
            code: code.clone(),
            message: message.clone(),
            fatal: *fatal,
        },
        Event::TimelineRemoved {
            sequence_id,
            sender,
        } => EventJson::TimelineRemoved {
            sequence_id: sequence_id.clone(),
            sender: sender.clone(),
        },
        Event::Ended { reason } => EventJson::Ended {
            reason: reason.clone(),
        },
        Event::Message {
            kind,
            sender,
            payload,
        } => EventJson::Message {
            kind: kind.clone(),
            sender: sender.clone(),
            payload: raw(payload.clone()),
        },
    };
    serde_json::to_string(&json).expect("an event always serialises")
}

/// The event `text` holds, as [`event_to_json`] wrote it.
///
/// # Errors
///
/// [`Error::Json`] if it is not such an event.
pub fn event_from_json(text: &str) -> Result<Event, Error> {
    // serde reads a tagged enum through a buffer that cannot hold a
    // `RawValue`, so the events carrying JSON of their own are read by hand.
    match parse::<Tag>(text)?.kind.as_str() {
        "timeline" => {
            let read: TimelineIn = parse(text)?;
            return Ok(Event::Timeline(TimelineChange {
                sequence_id: read.sequence_id,
                generation: read.generation,
                sender: read.sender,
                patches: otio_ext_sync::patch::from_json(read.patches.get())
                    .map_err(|error| Error::Json(error.to_string()))?,
                snapshot: Snapshot::from_json(read.snapshot.get())
                    .map_err(|error| Error::Json(error.to_string()))?,
                joining: read.joining,
            }));
        }
        "message" => {
            let read: MessageIn = parse(text)?;
            return Ok(Event::Message {
                kind: read.kind,
                sender: read.sender,
                payload: read.payload.get().to_string(),
            });
        }
        "review_relay" => {
            let read: RelayIn = parse(text)?;
            return Ok(Event::ReviewRelay(ReviewRelay {
                commit: read.commit.into(),
                ack: read.ack.into(),
                author_user_id: read.author_user_id,
                author_presence_id: read.author_presence_id,
                created_at: read.created_at,
            }));
        }
        _ => {}
    }
    let event = match parse::<EventJson>(text)? {
        EventJson::Joined {
            role,
            self_id,
            rejoined,
        } => Event::Joined {
            role: Role::from_name(&role),
            self_id,
            rejoined,
        },
        EventJson::Playhead { sender, playhead } => Event::Playhead {
            sender,
            playhead: playhead.into(),
        },
        EventJson::Selection { sender, selection } => Event::Selection {
            sender,
            selection: Selection {
                sequence_id: selection.sequence_id,
                guids: selection.guids,
            },
        },
        EventJson::Roster { event, peer } => Event::Roster {
            event: match event.as_str() {
                "join" => RosterEvent::Join,
                "leave" => RosterEvent::Leave,
                "update" => RosterEvent::Update,
                other => return Err(Error::Json(format!("unknown roster event {other:?}"))),
            },
            peer: peer.into(),
        },
        EventJson::Leader { leader } => Event::Leader { leader },
        EventJson::ReviewCommitted {
            client_mutation_id,
            comment_id,
            server_order,
            revision,
        } => Event::ReviewCommitted(ReviewAck {
            client_mutation_id,
            comment_id,
            server_order,
            revision,
        }),
        EventJson::ReviewRejected {
            client_mutation_id,
            code,
            message,
        } => Event::ReviewRejected {
            client_mutation_id,
            code,
            message,
        },
        EventJson::Error {
            code,
            message,
            fatal,
        } => Event::Error {
            code,
            message,
            fatal,
        },
        EventJson::TimelineRemoved {
            sequence_id,
            sender,
        } => Event::TimelineRemoved {
            sequence_id,
            sender,
        },
        EventJson::Ended { reason } => Event::Ended { reason },
        EventJson::Timeline { .. } | EventJson::ReviewRelay { .. } | EventJson::Message { .. } => {
            unreachable!("read above")
        }
    };
    Ok(event)
}

/// A review commit from JSON: `clientMutationId` (made up if left out),
/// `anchor`, `bodyMarkdown` and `annotationCommands`.
///
/// # Errors
///
/// [`Error::Json`] if it is not such an object.
pub fn review_from_json(text: &str) -> Result<ReviewCommit, Error> {
    Ok(parse::<CommitJson>(text)?.into())
}

/// `text` as a JSON string.
#[must_use]
pub fn quote(text: &str) -> String {
    serde_json::to_string(text).expect("a string always serialises")
}

/// A list of GUIDs from a JSON list of strings.
///
/// # Errors
///
/// [`Error::Json`] if it is not such a list.
pub fn guids_from_json(text: &str) -> Result<Vec<String>, Error> {
    parse(text)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct StateJson<'a> {
    joined: bool,
    ended: bool,
    session_id: Option<&'a str>,
    self_id: Option<&'a str>,
    self_user_id: Option<&'a str>,
    role: Option<&'a str>,
    leader: Option<&'a str>,
    peers: Vec<PeerJson>,
    active_sequence: Option<&'a str>,
    sequences: Vec<&'a str>,
    pending_reviews: Vec<CommitJson>,
    version: Option<u8>,
    encodings: Vec<&'static str>,
}

/// Where `session` stands, as JSON: `joined`, `ended`, `sessionId`,
/// `selfId`, `selfUserId`, `role`, `leader`, `peers`, `activeSequence`,
/// `sequences` and `pendingReviews`, with the wire's `version` and the
/// `encodings` the server accepts.
#[must_use]
pub fn state_to_json(session: &Session, version: Option<u8>, encodings: &[Encoding]) -> String {
    let state = StateJson {
        joined: session.is_joined(),
        ended: session.has_ended(),
        session_id: session.session_id(),
        self_id: session.self_id(),
        self_user_id: session.self_user_id(),
        role: session.role().map(Role::as_str),
        leader: session.leader(),
        peers: session.peers().iter().map(PeerJson::from).collect(),
        active_sequence: session.active_sequence(),
        sequences: session.sequences().collect(),
        pending_reviews: session
            .pending_reviews()
            .iter()
            .map(CommitJson::from)
            .collect(),
        version,
        encodings: encodings.iter().map(|encoding| encoding.name()).collect(),
    };
    serde_json::to_string(&state).expect("a state always serialises")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_fill_in_what_they_leave_out() {
        let settings = settings_from_json(
            r#"{"token":"t","identity":{"displayName":"Ada","capabilities":["review.comment.create"]},"encodings":["gzip"]}"#,
        )
        .unwrap();
        assert_eq!(settings.config.token, "t");
        assert_eq!(settings.config.identity.display_name, "Ada");
        assert_eq!(settings.config.identity.app, "otio-rust");
        assert_eq!(
            settings.config.identity.capabilities,
            ["review.comment.create"]
        );
        assert_eq!(settings.encodings, [Encoding::Gzip]);
        assert_eq!(settings_from_json("{}").unwrap().encodings, Encoding::ALL);
    }

    #[test]
    fn settings_refuse_what_they_do_not_know() {
        assert!(settings_from_json(r#"{"tokn":"t"}"#).is_err());
        assert!(settings_from_json(r#"{"encodings":["zstd"]}"#).is_err());
        assert!(settings_from_json("3").is_err());
    }

    #[test]
    fn events_survive_json() {
        let events = [
            Event::Joined {
                role: Role::Host,
                self_id: "p_1".into(),
                rejoined: true,
            },
            Event::Playhead {
                sender: "p_2".into(),
                playhead: Playhead {
                    sequence_id: "seq".into(),
                    frame: 12.0,
                    playing: true,
                    rate: 1.0,
                },
            },
            Event::Selection {
                sender: "p_2".into(),
                selection: Selection {
                    sequence_id: "seq".into(),
                    guids: vec!["a".into()],
                },
            },
            Event::Leader { leader: None },
            Event::ReviewCommitted(ReviewAck {
                client_mutation_id: "m".into(),
                comment_id: "c".into(),
                server_order: 3,
                revision: 1,
            }),
            Event::ReviewRelay(ReviewRelay {
                commit: ReviewCommit {
                    client_mutation_id: "m".into(),
                    anchor: r#"{"frame":3}"#.into(),
                    body_markdown: "Hi".into(),
                    annotation_commands: vec![r#"{"cmd":"x"}"#.into()],
                },
                ..ReviewRelay::default()
            }),
            Event::ReviewRejected {
                client_mutation_id: None,
                code: "no".into(),
                message: "No.".into(),
            },
            Event::Error {
                code: "bad".into(),
                message: "Bad.".into(),
                fatal: true,
            },
            Event::Ended {
                reason: "host_left".into(),
            },
            Event::Message {
                kind: "signal".into(),
                sender: Some("p_2".into()),
                payload: r#"{"to":"p_1"}"#.into(),
            },
        ];
        for event in events {
            let json = event_to_json(&event);
            assert_eq!(event_from_json(&json).unwrap(), event, "{json}");
        }
    }

    #[test]
    fn a_review_commit_reads_from_json() {
        let commit = review_from_json(
            r#"{"anchor":{"frame":3},"bodyMarkdown":"Hi","annotationCommands":[{"a":1}]}"#,
        )
        .unwrap();
        assert_eq!(commit.client_mutation_id, "");
        assert_eq!(commit.anchor, r#"{"frame":3}"#);
        assert_eq!(commit.annotation_commands, [r#"{"a":1}"#]);
    }
}
