//! Live review: annotations merge as Alchemist's review design says, strokes
//! in progress read as ORI writes them, and a stroke drawn by one client
//! reaches another intact, over Furnace's ledger and over ORI's wire.

mod common;

use common::enable;
use opentime::RationalTime;
use otio_core::{Document, NodeId};
use otio_ext_ori_annotations::Error;
use otio_ext_ori_annotations::annotations::{self, Merged};
use otio_ext_ori_annotations::command::{self, Command, PaintStart, TextAnnotation, Vertices};
use otio_ext_ori_annotations::review::{self, Anchor, Partial};
use otio_ext_sync::session::{
    self, ApplyOptions, Config, Event, Identity, ReviewAck, ReviewCommit, ReviewRelay, Session,
};
use otio_ext_sync_transport::loopback::{ClientId, Room, pump};
use otio_ext_sync_transport::ori::{Options, Ori};
use otio_ext_sync_transport::{Connection, Frame, Furnace};

/// A timeline with one video track holding two clips, with GUIDs.
const TIMELINE: &str = r#"{"OTIO_SCHEMA":"Timeline.1","name":"Program","metadata":{"sync":{"guid":"main"}},
  "tracks":{"OTIO_SCHEMA":"Stack.1","name":"tracks","metadata":{"sync":{"guid":"main/stack"}},"children":[
    {"OTIO_SCHEMA":"Track.1","name":"V1","kind":"Video","metadata":{"sync":{"guid":"main/v1"}},"children":[
      {"OTIO_SCHEMA":"Clip.2","name":"Beach Run","metadata":{"sync":{"guid":"main/clip-1"}},
       "source_range":{"OTIO_SCHEMA":"TimeRange.1",
         "start_time":{"OTIO_SCHEMA":"RationalTime.1","value":0,"rate":24},
         "duration":{"OTIO_SCHEMA":"RationalTime.1","value":48,"rate":24}}},
      {"OTIO_SCHEMA":"Clip.2","name":"Sunset","metadata":{"sync":{"guid":"main/clip-2"}},
       "source_range":{"OTIO_SCHEMA":"TimeRange.1",
         "start_time":{"OTIO_SCHEMA":"RationalTime.1","value":0,"rate":24},
         "duration":{"OTIO_SCHEMA":"RationalTime.1","value":24,"rate":24}}}]}]}}"#;

fn timeline() -> (Document, NodeId) {
    let document = otio_core::from_str(TIMELINE).expect("reads");
    let root = document.root().expect("a root");
    (document, root)
}

fn empty() -> (Document, NodeId) {
    let document = otio_core::from_str(r#"{"OTIO_SCHEMA":"Timeline.1"}"#).expect("reads");
    let root = document.root().expect("a root");
    (document, root)
}

fn stroke(uuid: &str, points: &[(f64, f64)]) -> [Command; 3] {
    command::stroke(
        PaintStart {
            rgba: Some([1.0, 0.0, 0.0, 1.0]),
            timestamp: Some("2026-10-21T09:30:00".to_string()),
            ..PaintStart::new(uuid)
        },
        Vertices::with_size(points, 0.01),
    )
}

fn text(uuid: &str, body: &str) -> Command {
    Command::Text(TextAnnotation {
        text: Some(body.to_string()),
        ..TextAnnotation::new(uuid)
    })
}

/// An annotation clip for `main/clip-1` at frame 10.
fn annotation_clip(document: &mut Document, timeline: NodeId) -> NodeId {
    let track = annotations::ensure_track(document, timeline).expect("a track");
    annotations::ensure_clip(
        document,
        track,
        "main/clip-1",
        RationalTime::new(10.0, 24.0),
    )
    .expect("a clip")
}

fn shape(document: &Document, clip: NodeId) -> Vec<String> {
    annotations::commands(document, clip)
        .expect("reads")
        .into_iter()
        .map(|id| {
            format!(
                "{}:{}",
                command::schema_name_of(document, id).unwrap(),
                command::uuid_of(document, id).unwrap_or("-")
            )
        })
        .collect()
}

#[test]
fn merging_replaces_by_uuid_in_place_and_adds_the_rest() {
    enable();
    let (mut document, timeline) = timeline();
    let clip = annotation_clip(&mut document, timeline);
    let mut first = stroke("a", &[(0.0, 0.0), (0.1, 0.1)]).to_vec();
    first.push(text("t", "hello"));
    annotations::add_commands(&mut document, clip, &first).expect("adds");

    let mut incoming = stroke("a", &[(0.2, 0.2)]).to_vec();
    incoming.truncate(2);
    incoming.extend(stroke("b", &[(0.3, 0.3)]));
    let merged = annotations::merge_commands(&mut document, clip, &incoming).expect("merges");
    assert_eq!(
        merged,
        Merged {
            replaced: 1,
            added: 1
        }
    );
    assert_eq!(
        shape(&document, clip),
        [
            "PaintStart:a",
            "PaintPoint:a",
            "TextAnnotation:t",
            "PaintStart:b",
            "PaintPoint:b",
            "PaintEnd:b"
        ]
    );
}

#[test]
fn a_replacement_keeps_kinds_it_does_not_hold_and_an_empty_one_clears() {
    enable();
    let (mut document, timeline) = timeline();
    let clip = annotation_clip(&mut document, timeline);
    let mut first = vec![text("t", "keep me")];
    first.extend(stroke("a", &[(0.0, 0.0)]));
    annotations::add_commands(&mut document, clip, &first).expect("adds");

    let removed = annotations::replace_by_kind(&mut document, clip, &stroke("c", &[(0.5, 0.5)]))
        .expect("replaces");
    assert_eq!(removed, 3);
    assert_eq!(
        shape(&document, clip),
        [
            "TextAnnotation:t",
            "PaintStart:c",
            "PaintPoint:c",
            "PaintEnd:c"
        ]
    );

    let removed = annotations::replace_by_kind(&mut document, clip, &[]).expect("clears");
    assert_eq!(removed, 4);
    assert!(shape(&document, clip).is_empty());
}

#[test]
fn two_clips_for_one_frame_become_one() {
    enable();
    let (mut document, timeline) = timeline();
    let clip = annotation_clip(&mut document, timeline);
    annotations::add_commands(&mut document, clip, &stroke("a", &[(0.0, 0.0)])).expect("adds");
    // A second clip for the same media clip and frame, as two people
    // annotating at once would leave.
    let track = annotations::find_track(&document, timeline)
        .unwrap()
        .unwrap();
    let twin = annotations::ensure_clip(
        &mut document,
        track,
        "main/clip-1",
        RationalTime::new(11.0, 24.0),
    )
    .expect("a clip");
    if let Some(otio_core::Node::Clip(twin)) = document.get_mut(twin) {
        twin.item.source_range = Some(opentime::TimeRange::new(
            RationalTime::new(10.0, 24.0),
            RationalTime::new(1.0, 24.0),
        ));
    }
    annotations::set_commands(&mut document, twin, &stroke("b", &[(0.1, 0.1)])).expect("sets");

    assert_eq!(
        annotations::merge_duplicates(&mut document, timeline).unwrap(),
        1
    );
    let found = annotations::list(&document, timeline).expect("lists");
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].commands.len(), 6);
    assert_eq!(
        annotations::merge_duplicates(&mut document, timeline).unwrap(),
        0
    );
}

#[test]
fn partials_round_trip_and_read_every_one_ori_recorded() {
    enable();
    let partial = Partial {
        clip_guid: "main/clip-1".to_string(),
        frame: 10.0,
        fps: 24.0,
        commands: stroke("a", &[(0.0, 0.0), (0.1, 0.2)])[..2].to_vec(),
    };
    let json = partial.to_json().expect("writes");
    assert!(json.starts_with(
        r#"{"clip_guid":"main/clip-1","frame":10,"fps":24,"events":[{"OTIO_SCHEMA":"PaintStart.1""#
    ));
    assert_eq!(Partial::from_json(&json).expect("reads"), partial);

    let recordings = format!(
        "{}/../../sync/otio-ext-sync-transport/tests/fixtures/ori/recordings",
        env!("CARGO_MANIFEST_DIR")
    );
    let mut read = 0;
    for entry in std::fs::read_dir(recordings).expect("vendored") {
        let text = std::fs::read_to_string(entry.unwrap().path()).unwrap();
        for line in text.lines() {
            let record: serde_json::Value = serde_json::from_str(line).expect("JSON lines");
            let message = &record["payload"]["payload"];
            if message["command_schema"] != "Annotation.1"
                || message["command"]["event"] != "PARTIAL"
            {
                continue;
            }
            let payload = &message["command"]["payload"];
            let event = Event::Message {
                kind: review::PARTIAL_KIND.to_string(),
                sender: None,
                payload: payload.to_string(),
            };
            let partial = review::read_partial(&event)
                .expect("reads")
                .expect("is a partial");
            assert_eq!(partial.clip_guid, payload["clip_guid"].as_str().unwrap());
            assert_eq!(partial.frame, payload["frame"].as_f64().unwrap());
            assert_eq!(
                partial.commands.len(),
                payload["events"].as_array().unwrap().len()
            );
            read += 1;
        }
    }
    assert_eq!(read, 450, "every PARTIAL in ORI's recordings");
}

#[test]
fn every_annotation_ori_ends_with_reads_as_typed_commands() {
    enable();
    let expected = format!(
        "{}/../../sync/otio-ext-sync-transport/tests/fixtures/ori/expected",
        env!("CARGO_MANIFEST_DIR")
    );
    let mut clips = 0;
    for entry in std::fs::read_dir(expected).expect("vendored") {
        let text = std::fs::read_to_string(entry.unwrap().path()).unwrap();
        let state: serde_json::Value = serde_json::from_str(&text).expect("JSON");
        for timeline in state["timelines"].as_object().unwrap().values() {
            let document = otio_core::from_str(&timeline.to_string()).expect("reads");
            let root = document.root().unwrap();
            for annotation in annotations::list(&document, root).expect("lists") {
                for id in &annotation.commands {
                    let command = Command::read(&document, *id).expect("a typed command");
                    assert!(!matches!(command, Command::Other(_)), "{command:?}");
                }
                clips += 1;
            }
        }
    }
    assert!(clips >= 10, "only {clips} annotation clips");
}

// ── Furnace ───────────────────────────────────────────────────────────────

fn furnace(token: &str, name: &str, credential: &str, capabilities: &[&str]) -> Connection {
    let config = Config {
        token: token.to_string(),
        identity: Identity {
            display_name: name.to_string(),
            credential: credential.to_string(),
            capabilities: capabilities.iter().map(ToString::to_string).collect(),
            ..Identity::default()
        },
    };
    Connection::new(Session::new(config), Furnace::new())
}

fn joined(room: &mut Room, connection: &mut Connection, token: &str) -> ClientId {
    let id = room.open(token);
    connection.connect();
    pump(room, &mut [(id, connection)], 0).expect("joins");
    assert!(connection.session().is_joined());
    id
}

fn events<P: otio_ext_sync_transport::Profile>(connection: &mut Connection<P>) -> Vec<Event> {
    std::iter::from_fn(|| connection.poll_event()).collect()
}

fn apply(document: &mut Document, timeline: NodeId, events: &[Event]) {
    for event in events {
        session::handle(document, timeline, event, &ApplyOptions::default()).expect("applies");
    }
}

fn anchor(frame: f64) -> Anchor {
    Anchor {
        session_id: String::new(),
        sequence_id: "main".to_string(),
        captured_generation: 1,
        clip_guid: "main/clip-1".to_string(),
        source_time: RationalTime::new(frame, 24.0),
        captured_sequence_time: RationalTime::new(frame, 24.0),
        media_key: Some("clip-1".to_string()),
        image_size: Some((1920, 1080)),
        captured_at: 1_787_742_000_000,
    }
}

fn commands_json(document: &Document, timeline: NodeId) -> String {
    let found = annotations::list(document, timeline).expect("lists");
    assert_eq!(found.len(), 1, "one annotation");
    command::to_json(document, &found[0].commands, None).expect("writes")
}

#[test]
fn a_stroke_committed_by_a_reviewer_reaches_everyone_intact() {
    enable();
    let reviewer_capabilities = ["review.comment.create", "review.annotation.create"];
    let mut room = Room::new();
    let mut host = furnace(Room::HOST_TOKEN, "Ari", "ari", &["review.commit.receive"]);
    let host_id = joined(&mut room, &mut host, Room::HOST_TOKEN);
    let (mut host_document, host_timeline) = timeline();
    host.session_mut()
        .publish(&mut host_document, host_timeline, "main")
        .expect("publishes");
    pump(&mut room, &mut [(host_id, &mut host)], 0).expect("sent");

    let mut bea = furnace(Room::JOIN_TOKEN, "Bea", "bea", &reviewer_capabilities);
    let bea_id = joined(&mut room, &mut bea, Room::JOIN_TOKEN);
    let (mut bea_document, bea_timeline) = empty();
    apply(&mut bea_document, bea_timeline, &events(&mut bea));

    let mut bea = (bea_id, bea);

    // She commits it with a note; the host folds it in and publishes.
    let drawn = stroke("bea-1", &[(0.0, 0.0), (0.1, 0.1), (0.2, 0.0)]);
    let id = review::commit(
        bea.1.session_mut(),
        &bea_document,
        bea_timeline,
        &anchor(10.0),
        "Too dark here.",
        &drawn,
        "",
    )
    .expect("commits");
    pump(
        &mut room,
        &mut [(bea.0, &mut bea.1), (host_id, &mut host)],
        0,
    )
    .expect("relayed");
    let relay = events(&mut host)
        .into_iter()
        .find_map(|event| match event {
            Event::ReviewRelay(relay) => Some(relay),
            _ => None,
        })
        .expect("the host has the comment");
    assert_eq!(relay.commit.client_mutation_id, id);
    let folded = review::fold(&mut host_document, host_timeline, &relay).expect("folds");
    assert_eq!(
        folded.merged,
        Merged {
            replaced: 0,
            added: 1
        }
    );
    assert!(!folded.already);
    assert!(
        review::fold(&mut host_document, host_timeline, &relay)
            .expect("again")
            .already
    );
    host.session_mut()
        .publish(&mut host_document, host_timeline, "main")
        .expect("publishes");
    pump(
        &mut room,
        &mut [(host_id, &mut host), (bea.0, &mut bea.1)],
        0,
    )
    .expect("relayed");
    let seen = events(&mut bea.1);
    assert!(
        seen.iter().any(
            |event| matches!(event, Event::ReviewCommitted(ack) if ack.client_mutation_id == id)
        )
    );
    apply(&mut bea_document, bea_timeline, &seen);

    let mut scratch = Document::new();
    let ids: Vec<NodeId> = drawn
        .iter()
        .map(|command| command.insert(&mut scratch).unwrap())
        .collect();
    let expected = command::to_json(&scratch, &ids, None).unwrap();
    assert_eq!(commands_json(&host_document, host_timeline), expected);
    assert_eq!(commands_json(&bea_document, bea_timeline), expected);
    let found = annotations::list(&bea_document, bea_timeline).unwrap();
    assert_eq!(found[0].note.as_deref(), Some("Too dark here."));
    assert_eq!(found[0].status.as_deref(), Some("open"));
    let bea_user = relay.author_user_id.clone();
    assert_eq!(
        review::owner_of(&bea_document, found[0].holder, "bea-1").unwrap(),
        Some(bea_user.clone())
    );

    // Bea may replace her own stroke; Cy may add his but not replace hers.
    review::commit(
        bea.1.session_mut(),
        &bea_document,
        bea_timeline,
        &anchor(10.0),
        "",
        &stroke("bea-1", &[(0.3, 0.3)]),
        "",
    )
    .expect("her own");
    let mut cy = furnace(Room::JOIN_TOKEN, "Cy", "cy", &reviewer_capabilities);
    let _cy_id = joined(&mut room, &mut cy, Room::JOIN_TOKEN);
    let (mut cy_document, cy_timeline) = empty();
    apply(&mut cy_document, cy_timeline, &events(&mut cy));
    assert_eq!(
        review::commit(
            cy.session_mut(),
            &cy_document,
            cy_timeline,
            &anchor(10.0),
            "",
            &stroke("bea-1", &[]),
            "",
        )
        .unwrap_err(),
        Error::NotPermitted {
            uuid: "bea-1".to_string(),
            owner: Some(bea_user.clone()),
        }
    );
    review::commit(
        cy.session_mut(),
        &cy_document,
        cy_timeline,
        &anchor(10.0),
        "",
        &stroke("cy-1", &[(0.4, 0.4)]),
        "",
    )
    .expect("his own");

    // The host refuses the same, should a client skip the check.
    let forged = ReviewRelay {
        commit: ReviewCommit {
            client_mutation_id: "forged".to_string(),
            anchor: anchor(10.0).to_json(),
            body_markdown: String::new(),
            annotation_commands: vec![
                r#"{"OTIO_SCHEMA":"PaintStart.1","uuid":"bea-1"}"#.to_string(),
            ],
        },
        ack: ReviewAck {
            client_mutation_id: "forged".to_string(),
            comment_id: "comment_forged".to_string(),
            server_order: 9,
            revision: 1,
        },
        author_user_id: "someone-else".to_string(),
        author_presence_id: "p".to_string(),
        created_at: 0,
    };
    let before = commands_json(&host_document, host_timeline);
    assert!(matches!(
        review::fold(&mut host_document, host_timeline, &forged),
        Err(Error::NotPermitted { .. })
    ));
    assert_eq!(commands_json(&host_document, host_timeline), before);
}

#[test]
fn folds_furnaces_review_commit_fixture() {
    enable();
    let text = std::fs::read_to_string(format!(
        "{}/../../sync/otio-ext-sync-transport/tests/fixtures/furnace-conformance/v5/server-review-commit-v2.json",
        env!("CARGO_MANIFEST_DIR")
    ))
    .expect("vendored");
    let mut profile = Furnace::new();
    let inbound = otio_ext_sync_transport::Profile::decode(&mut profile, &Frame::text(text))
        .expect("decodes");
    let [session::Inbound::ReviewRelay(relay)] = inbound.as_slice() else {
        panic!("{inbound:?}")
    };
    let (mut document, timeline) = timeline();
    let folded = review::fold(&mut document, timeline, relay).expect("folds");
    let found = annotations::describe(&document, folded.clip).expect("an annotation");
    assert_eq!(found.frame, Some(RationalTime::new(1042.0, 24.0)));
    assert_eq!(found.commands.len(), 2);
    assert_eq!(found.note.as_deref(), Some("The eyeline feels late here."));
    assert_eq!(
        review::owner_of(&document, folded.clip, "stroke-conformance-1").unwrap(),
        Some("u_viewer_conformance".to_string())
    );
    let anchor = Anchor::from_json(&relay.commit.anchor).expect("reads");
    assert_eq!(anchor.captured_generation, 18);
    assert_eq!(anchor.image_size, Some((1920, 1080)));
    assert_eq!(Anchor::from_json(&anchor.to_json()).unwrap(), anchor);
}

// ── ORI's wire ────────────────────────────────────────────────────────────

/// Two ORI peers on one in-memory fanout exchange.
struct Exchange {
    peers: Vec<Connection<Ori>>,
    log: Vec<String>,
}

impl Exchange {
    fn add(&mut self, guid: &str) -> usize {
        let mut options = Options::new("review-session");
        options.peer_guid = guid.to_string();
        let mut connection = Connection::new(Session::new(Config::default()), Ori::new(options));
        connection.connect();
        self.peers.push(connection);
        self.peers.len() - 1
    }

    fn pump(&mut self, now: i64) {
        loop {
            let mut moved = false;
            for sender in 0..self.peers.len() {
                while let Some(frame) = self.peers[sender].poll_transmit(now).expect("sends") {
                    moved = true;
                    let envelope: serde_json::Value =
                        serde_json::from_slice(&frame.data).expect("JSON");
                    let command = &envelope["payload"];
                    self.log.push(format!(
                        "{}/{}",
                        command["command_schema"].as_str().unwrap_or_default(),
                        command["command"]["event"].as_str().unwrap_or_default()
                    ));
                    for receiver in &mut self.peers {
                        receiver.receive(&frame).expect("receives");
                    }
                }
            }
            if !moved {
                return;
            }
        }
    }
}

#[test]
fn strokes_travel_ori_s_wire_as_ori_sends_them() {
    enable();
    let mut exchange = Exchange {
        peers: Vec::new(),
        log: Vec::new(),
    };
    let a = exchange.add("peer-a");
    exchange.pump(0);
    exchange.pump(2_000);
    let (mut a_document, a_timeline) = timeline();
    exchange.peers[a]
        .session_mut()
        .publish(&mut a_document, a_timeline, "main")
        .expect("publishes");
    exchange.pump(2_100);
    let b = exchange.add("peer-b");
    exchange.pump(3_000);
    let (mut b_document, b_timeline) = empty();
    let seen = events(&mut exchange.peers[b]);
    apply(&mut b_document, b_timeline, &seen);
    let _ = events(&mut exchange.peers[a]);

    // A stroke in progress, as RV and xStudio send one.
    let partial = Partial {
        clip_guid: "main/clip-1".to_string(),
        frame: 10.0,
        fps: 24.0,
        commands: stroke("a-1", &[(0.0, 0.0)])[..2].to_vec(),
    };
    review::send_partial(exchange.peers[a].session_mut(), &partial).expect("sends");
    exchange.log.clear();
    exchange.pump(3_100);
    assert_eq!(exchange.log, ["Annotation.1/PARTIAL"]);
    let heard: Vec<Partial> = events(&mut exchange.peers[b])
        .iter()
        .filter_map(|event| review::read_partial(event).expect("reads"))
        .collect();
    assert_eq!(heard, [partial]);

    // The finished stroke, then an edit to it.
    let clip = annotation_clip(&mut a_document, a_timeline);
    annotations::add_commands(
        &mut a_document,
        clip,
        &stroke("a-1", &[(0.0, 0.0), (0.1, 0.1)]),
    )
    .expect("adds");
    exchange.peers[a]
        .session_mut()
        .publish(&mut a_document, a_timeline, "main")
        .expect("publishes");
    exchange.pump(3_200);
    annotations::replace_commands(&mut a_document, clip, "a-1", &stroke("a-1", &[(0.5, 0.5)]))
        .expect("replaces");
    exchange.log.clear();
    exchange.peers[a]
        .session_mut()
        .publish(&mut a_document, a_timeline, "main")
        .expect("publishes");
    exchange.pump(3_300);
    assert_eq!(
        exchange.log,
        ["OTIO_SESSION_1.0/REPLACE_ANNOTATION_COMMANDS"]
    );
    apply(&mut b_document, b_timeline, &events(&mut exchange.peers[b]));
    assert_eq!(
        commands_json(&b_document, b_timeline),
        commands_json(&a_document, a_timeline)
    );
}
