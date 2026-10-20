//! Sessions against each other through the in-memory Furnace room.

mod common;

use otio_ext_sync::session::{
    self, ApplyOptions, Channel, Config, Event, Handled, Identity, Playhead, ReviewCommit, Role,
    RosterEvent, Session,
};
use otio_ext_sync::{Op, patch};
use otio_ext_sync_transport::compression::Encoding;
use otio_ext_sync_transport::loopback::{ClientId, Room, pump};
use otio_ext_sync_transport::{Connection, Error, Frame, Furnace, Profile};

fn connection(token: &str, name: &str, capabilities: &[&str]) -> Connection {
    let config = Config {
        token: token.to_string(),
        identity: Identity {
            display_name: name.to_string(),
            capabilities: capabilities.iter().map(ToString::to_string).collect(),
            ..Identity::default()
        },
    };
    Connection::new(Session::new(config), Furnace::new())
}

fn events(connection: &mut Connection) -> Vec<Event> {
    std::iter::from_fn(|| connection.poll_event()).collect()
}

fn joined(room: &mut Room, connection: &mut Connection, token: &str) -> ClientId {
    let id = room.open(token);
    connection.connect();
    pump(room, &mut [(id, connection)], 0).expect("joins");
    assert!(connection.session().is_joined());
    id
}

/// Applies every event to the document, as a program's loop would.
fn apply_all(
    document: &mut otio_core::Document,
    timeline: otio_core::NodeId,
    events: &[Event],
) -> Vec<Handled> {
    events
        .iter()
        .map(|event| {
            session::handle(document, timeline, event, &ApplyOptions::default()).expect("applies")
        })
        .collect()
}

#[test]
fn viewers_follow_the_host_with_patches_and_keep_their_handles() {
    let mut room = Room::new();
    let mut host = connection(Room::HOST_TOKEN, "Ari", &[]);
    let host_id = joined(&mut room, &mut host, Room::HOST_TOKEN);
    let (mut host_document, host_timeline) = common::host_document();
    host.session_mut()
        .publish(&mut host_document, host_timeline, "main")
        .expect("publishes");
    pump(&mut room, &mut [(host_id, &mut host)], 0).expect("sent");
    assert_eq!(room.snapshot().map(|s| s.generation), Some(1));

    let mut viewer = connection(Room::JOIN_TOKEN, "Bea", &[]);
    let viewer_id = joined(&mut room, &mut viewer, Room::JOIN_TOKEN);
    let seen = events(&mut viewer);
    assert!(
        matches!(&seen[0], Event::Timeline(change) if change.joining && change.patches[0].op == Op::ReplaceTimeline)
    );
    assert!(matches!(
        &seen[1],
        Event::Joined {
            role: Role::Viewer,
            rejoined: false,
            ..
        }
    ));
    let (mut document, timeline) = common::viewer_document();
    assert_eq!(
        apply_all(&mut document, timeline, &seen),
        [Handled::Applied, Handled::Joined]
    );
    let clip = otio_ext_sync::guid::GuidIndex::build(&document, timeline)
        .expect("unique")
        .get("main/clip-2")
        .expect("arrived");

    // The host sees the viewer join.
    pump(&mut room, &mut [(host_id, &mut host)], 0).expect("relayed");
    let host_seen = events(&mut host);
    assert!(host_seen.iter().any(|event| matches!(event, Event::Roster { event: RosterEvent::Join, peer } if peer.display_name == "Bea")));
    assert_eq!(host.session().peers().len(), 2);

    common::rename(
        &mut host_document,
        host_timeline,
        "main/clip-2",
        "Sunset (alt)",
    );
    let patches = host
        .session_mut()
        .publish(&mut host_document, host_timeline, "main")
        .expect("publishes");
    pump(
        &mut room,
        &mut [(host_id, &mut host), (viewer_id, &mut viewer)],
        0,
    )
    .expect("relayed");
    let seen = events(&mut viewer);
    let [Event::Timeline(change)] = seen.as_slice() else {
        panic!("{seen:?}")
    };
    assert_eq!(change.patches, patches);
    assert_eq!(change.generation, 2);
    assert_eq!(change.sender.as_deref(), host.session().self_id());
    assert_eq!(
        apply_all(&mut document, timeline, &seen),
        [Handled::Applied]
    );
    assert_eq!(
        document
            .get(clip)
            .and_then(otio_core::Node::base)
            .expect("kept")
            .name,
        "Sunset (alt)"
    );

    // Publishing what is already sent sends nothing.
    assert!(
        host.session_mut()
            .publish(&mut host_document, host_timeline, "main")
            .expect("ok")
            .is_empty()
    );
    assert!(host.poll_transmit(0).expect("ok").is_none());
}

#[test]
fn large_frames_are_compressed_only_when_both_sides_list_an_encoding() {
    // Enough clips that a snapshot is well past the threshold.
    let (mut host_document, host_timeline) = common::host_document();
    for index in 0..40 {
        let clip = otio_core::from_str(&format!(
            r#"{{"OTIO_SCHEMA":"Clip.2","name":"Shot {index}","metadata":{{"sync":{{"guid":"main/shot-{index}"}}}}}}"#
        ))
        .expect("reads");
        let clip_root = clip.root().expect("a root");
        let arrived = host_document.absorb(clip);
        let v1 = otio_ext_sync::guid::GuidIndex::build(&host_document, host_timeline)
            .expect("unique")
            .get("main/v1")
            .expect("the track");
        host_document
            .append_child(v1, arrived[&clip_root])
            .expect("appends");
    }

    for (room_encodings, expect_binary) in [(Encoding::ALL.to_vec(), true), (Vec::new(), false)] {
        let mut room = Room::with_encodings(room_encodings);
        let mut host = connection(Room::HOST_TOKEN, "Ari", &[]);
        let host_id = joined(&mut room, &mut host, Room::HOST_TOKEN);
        assert_eq!(
            host.profile().accepted().contains(&Encoding::Brotli),
            expect_binary
        );
        let mut document = host_document.clone();
        host.session_mut()
            .publish(&mut document, host_timeline, "main")
            .expect("publishes");
        let frame = host.poll_transmit(0).expect("encodes").expect("a snapshot");
        assert_eq!(frame.binary, expect_binary);
        if expect_binary {
            assert_eq!(frame.data[0], Encoding::Brotli.tag());
        }
        room.deliver(host_id, &frame).expect("the room reads it");

        // The viewer gets the snapshot in its welcome, then a compressed
        // relay of the next edit if it listed an encoding.
        let mut viewer = connection(Room::JOIN_TOKEN, "Bea", &[]);
        let viewer_id = joined(&mut room, &mut viewer, Room::JOIN_TOKEN);
        let _ = events(&mut viewer);
        common::rename(&mut document, host_timeline, "main/shot-3", "Shot three");
        host.session_mut()
            .publish(&mut document, host_timeline, "main")
            .expect("publishes");
        pump(&mut room, &mut [(host_id, &mut host)], 0).expect("sent");
        let relayed: Vec<Frame> = std::iter::from_fn(|| room.take(viewer_id)).collect();
        assert_eq!(relayed.len(), 1);
        assert_eq!(relayed[0].binary, expect_binary);
        viewer.receive(&relayed[0]).expect("expands");
        let seen = events(&mut viewer);
        let [Event::Timeline(change)] = seen.as_slice() else {
            panic!("{seen:?}")
        };
        assert_eq!(
            patch::to_json(&change.patches),
            r#"[{"op":"set_property","guid":"main/shot-3","path":"/name","value":"Shot three"}]"#
        );
    }
}

#[test]
fn roles_and_capabilities_are_enforced_before_sending() {
    let mut room = Room::new();
    let mut host = connection(Room::HOST_TOKEN, "Ari", &[]);
    joined(&mut room, &mut host, Room::HOST_TOKEN);
    let mut viewer = connection(Room::JOIN_TOKEN, "Bea", &[]);
    joined(&mut room, &mut viewer, Room::JOIN_TOKEN);
    let (mut document, timeline) = common::host_document();

    let session = viewer.session_mut();
    assert!(!session.may_send(&Channel::Timeline));
    assert_eq!(
        session
            .publish(&mut document, timeline, "main")
            .unwrap_err(),
        session::Error::NoLease(Channel::Timeline)
    );
    assert_eq!(
        session.commit_review(ReviewCommit::default()).unwrap_err(),
        session::Error::NoLease(Channel::Review)
    );
    assert!(session.may_send(&Channel::Playback));

    let mut idle = Session::new(Config::default());
    assert_eq!(
        idle.set_playhead(Playhead::default()).unwrap_err(),
        session::Error::NotJoined
    );
}

#[test]
fn a_playhead_not_yet_sent_is_replaced_by_the_newest() {
    let mut room = Room::new();
    let mut host = connection(Room::HOST_TOKEN, "Ari", &[]);
    let host_id = joined(&mut room, &mut host, Room::HOST_TOKEN);
    let mut viewer = connection(Room::JOIN_TOKEN, "Bea", &[]);
    let viewer_id = joined(&mut room, &mut viewer, Room::JOIN_TOKEN);
    let _ = events(&mut host);

    for frame in [10.0, 11.0, 12.5] {
        viewer
            .session_mut()
            .set_playhead(Playhead {
                sequence_id: "main".to_string(),
                frame,
                playing: true,
                rate: 24.0,
            })
            .expect("may");
    }
    pump(
        &mut room,
        &mut [(viewer_id, &mut viewer), (host_id, &mut host)],
        0,
    )
    .expect("relayed");
    let playheads: Vec<f64> = events(&mut host)
        .into_iter()
        .filter_map(|event| match event {
            Event::Playhead { playhead, .. } => Some(playhead.frame),
            _ => None,
        })
        .collect();
    assert_eq!(playheads, [12.5]);
}

#[test]
fn review_commits_are_acknowledged_once_across_a_reconnect() {
    let mut room = Room::new();
    let mut viewer = connection(Room::JOIN_TOKEN, "Bea", &["review.comment.create"]);
    let viewer_id = joined(&mut room, &mut viewer, Room::JOIN_TOKEN);
    let _ = events(&mut viewer);
    let commit = ReviewCommit {
        client_mutation_id: "mutation_1".to_string(),
        anchor: r#"{"sequenceId":"main","clipGuid":"main/clip-1"}"#.to_string(),
        body_markdown: "Trim the head.".to_string(),
        annotation_commands: vec![r#"{"OTIO_SCHEMA":"PaintStart.1","uuid":"s1"}"#.to_string()],
    };

    // No host can take it yet: refused, and dropped from the ledger.
    viewer
        .session_mut()
        .commit_review(commit.clone())
        .expect("may");
    pump(&mut room, &mut [(viewer_id, &mut viewer)], 0).expect("sent");
    let seen = events(&mut viewer);
    assert!(
        matches!(&seen[..], [Event::ReviewRejected { code, .. }] if code == "host_unavailable")
    );
    assert!(viewer.session().pending_reviews().is_empty());

    let mut host = connection(Room::HOST_TOKEN, "Ari", &["review.commit.receive"]);
    let host_id = joined(&mut room, &mut host, Room::HOST_TOKEN);
    let _ = events(&mut host);

    // Sent, but the connection drops before the acknowledgement arrives.
    viewer
        .session_mut()
        .commit_review(commit.clone())
        .expect("may");
    let frame = viewer.poll_transmit(0).expect("ok").expect("the commit");
    room.deliver(viewer_id, &frame).expect("accepted");
    room.close(viewer_id);
    viewer.disconnected();
    assert_eq!(viewer.session().pending_reviews().len(), 1);

    // Joining again: the welcome carries the acknowledgement, so the commit
    // is not sent again and the host gets it once.
    let viewer_id = joined(&mut room, &mut viewer, Room::JOIN_TOKEN);
    let seen = events(&mut viewer);
    assert!(seen.iter().any(|event| matches!(event, Event::ReviewCommitted(ack) if ack.client_mutation_id == "mutation_1")));
    assert!(
        seen.iter()
            .any(|event| matches!(event, Event::Joined { rejoined: true, .. }))
    );
    assert!(viewer.session().pending_reviews().is_empty());
    pump(
        &mut room,
        &mut [(viewer_id, &mut viewer), (host_id, &mut host)],
        0,
    )
    .expect("ok");
    let relays = events(&mut host)
        .into_iter()
        .filter(|event| matches!(event, Event::ReviewRelay(_)))
        .count();
    assert_eq!(relays, 1);

    // Reusing the id for something else is refused locally while waiting,
    // and by the server once acknowledged.
    let mut changed = commit.clone();
    changed.body_markdown = "Something else.".to_string();
    viewer
        .session_mut()
        .commit_review(changed.clone())
        .expect("not waiting");
    assert_eq!(
        viewer
            .session_mut()
            .commit_review(ReviewCommit {
                body_markdown: "Third.".into(),
                ..changed
            })
            .unwrap_err(),
        session::Error::MutationConflict("mutation_1".to_string())
    );
    pump(&mut room, &mut [(viewer_id, &mut viewer)], 0).expect("ok");
    let seen = events(&mut viewer);
    assert!(
        matches!(&seen[..], [Event::ReviewRejected { code, .. }] if code == "mutation_conflict")
    );
}

#[test]
fn a_viewer_that_missed_edits_rejoins_with_patches() {
    let mut room = Room::new();
    let mut host = connection(Room::HOST_TOKEN, "Ari", &[]);
    let host_id = joined(&mut room, &mut host, Room::HOST_TOKEN);
    let (mut host_document, host_timeline) = common::host_document();
    host.session_mut()
        .publish(&mut host_document, host_timeline, "main")
        .expect("publishes");
    pump(&mut room, &mut [(host_id, &mut host)], 0).expect("sent");

    let mut viewer = connection(Room::JOIN_TOKEN, "Bea", &[]);
    let viewer_id = joined(&mut room, &mut viewer, Room::JOIN_TOKEN);
    let (mut document, timeline) = common::viewer_document();
    apply_all(&mut document, timeline, &events(&mut viewer));
    let clip = otio_ext_sync::guid::GuidIndex::build(&document, timeline)
        .expect("ok")
        .get("main/clip-1")
        .expect("there");

    room.close(viewer_id);
    viewer.disconnected();
    common::rename(
        &mut host_document,
        host_timeline,
        "main/clip-1",
        "Renamed while away",
    );
    host.session_mut()
        .publish(&mut host_document, host_timeline, "main")
        .expect("publishes");
    pump(&mut room, &mut [(host_id, &mut host)], 0).expect("sent");

    joined(&mut room, &mut viewer, Room::JOIN_TOKEN);
    let seen = events(&mut viewer);
    let Event::Timeline(change) = &seen[0] else {
        panic!("{seen:?}")
    };
    assert!(change.joining);
    assert_eq!(
        change.patches[0].op,
        Op::SetProperty,
        "a patch, not a replacement"
    );
    assert_eq!(
        apply_all(&mut document, timeline, &seen),
        [Handled::Applied, Handled::Joined]
    );
    assert_eq!(
        document
            .get(clip)
            .and_then(otio_core::Node::base)
            .expect("kept")
            .name,
        "Renamed while away"
    );
}

#[test]
fn a_host_that_reconnects_carries_on_from_its_generation() {
    let mut room = Room::new();
    let mut host = connection(Room::HOST_TOKEN, "Ari", &[]);
    let host_id = joined(&mut room, &mut host, Room::HOST_TOKEN);
    let mut viewer = connection(Room::JOIN_TOKEN, "Bea", &[]);
    let viewer_id = joined(&mut room, &mut viewer, Room::JOIN_TOKEN);
    let (mut document, timeline) = common::host_document();
    host.session_mut()
        .publish(&mut document, timeline, "main")
        .expect("publishes");
    pump(
        &mut room,
        &mut [(host_id, &mut host), (viewer_id, &mut viewer)],
        0,
    )
    .expect("ok");
    let _ = events(&mut viewer);

    room.close(host_id);
    host.disconnected();
    pump(&mut room, &mut [(viewer_id, &mut viewer)], 0).expect("ok");
    let seen = events(&mut viewer);
    assert!(seen.iter().any(|event| matches!(event, Event::Roster { event: RosterEvent::Update, peer } if peer.connection_state == "reconnecting")));
    assert!(
        seen.iter()
            .any(|event| matches!(event, Event::Leader { leader: None }))
    );
    assert_eq!(viewer.session().leader(), None);

    let host_id = joined(&mut room, &mut host, Room::HOST_TOKEN);
    let seen = events(&mut host);
    assert!(
        !seen.iter().any(|event| matches!(event, Event::Timeline(_))),
        "the host keeps its own timeline"
    );
    assert!(matches!(
        seen.last(),
        Some(Event::Joined {
            role: Role::Host,
            rejoined: true,
            ..
        })
    ));
    common::rename(&mut document, timeline, "main", "Program v2");
    host.session_mut()
        .publish(&mut document, timeline, "main")
        .expect("publishes");
    pump(
        &mut room,
        &mut [(host_id, &mut host), (viewer_id, &mut viewer)],
        0,
    )
    .expect("ok");
    assert_eq!(room.snapshot().map(|s| s.generation), Some(2));
    let seen = events(&mut viewer);
    assert!(
        seen.iter()
            .any(|event| matches!(event, Event::Timeline(change) if change.generation == 2))
    );
    assert_eq!(viewer.session().leader(), host.session().self_id());
}

#[test]
fn a_gap_in_the_servers_frames_is_reported() {
    let mut profile = Furnace::new();
    let frame = |order: u64| {
        Frame::text(format!(
            r#"{{"v":1,"type":"leader","sessionId":"s","seq":0,"ts":0,"order":{order},"payload":{{"leaderId":"p"}}}}"#
        ))
    };
    profile
        .decode(&frame(4))
        .expect("the first frame sets the count");
    profile.decode(&frame(5)).expect("the next one follows");
    assert_eq!(
        profile.decode(&frame(7)).unwrap_err(),
        Error::Gap {
            expected: 6,
            got: 7
        }
    );
}

#[test]
fn selections_are_refused_until_furnace_relays_them() {
    let mut room = Room::new();
    let mut viewer = connection(Room::JOIN_TOKEN, "Bea", &[]);
    joined(&mut room, &mut viewer, Room::JOIN_TOKEN);
    viewer
        .session_mut()
        .set_selection(session::Selection::default())
        .expect("queued");
    assert!(matches!(
        viewer.poll_transmit(0),
        Err(Error::Unsupported(_))
    ));
    assert!(viewer.poll_transmit(0).expect("the rest goes on").is_none());
}

#[test]
fn a_host_whose_last_edit_was_lost_sends_it_again_on_reconnect() {
    let mut room = Room::new();
    let mut host = connection(Room::HOST_TOKEN, "Ari", &[]);
    let host_id = joined(&mut room, &mut host, Room::HOST_TOKEN);
    let mut viewer = connection(Room::JOIN_TOKEN, "Bea", &[]);
    let viewer_id = joined(&mut room, &mut viewer, Room::JOIN_TOKEN);
    let (mut document, timeline) = common::host_document();
    host.session_mut()
        .publish(&mut document, timeline, "main")
        .expect("publishes");
    pump(
        &mut room,
        &mut [(host_id, &mut host), (viewer_id, &mut viewer)],
        0,
    )
    .expect("ok");
    let _ = events(&mut viewer);

    // Generation 2 is queued, then the connection drops before it goes.
    common::rename(&mut document, timeline, "main", "Program v2");
    host.session_mut()
        .publish(&mut document, timeline, "main")
        .expect("publishes");
    room.close(host_id);
    host.disconnected();
    pump(&mut room, &mut [(viewer_id, &mut viewer)], 0).expect("ok");
    let _ = events(&mut viewer);
    assert_eq!(room.snapshot().map(|s| s.generation), Some(1));

    let host_id = joined(&mut room, &mut host, Room::HOST_TOKEN);
    pump(
        &mut room,
        &mut [(host_id, &mut host), (viewer_id, &mut viewer)],
        0,
    )
    .expect("ok");
    assert_eq!(room.snapshot().map(|s| s.generation), Some(2));
    let seen = events(&mut viewer);
    assert!(
        seen.iter()
            .any(|event| matches!(event, Event::Timeline(change) if change.generation == 2))
    );
}

#[test]
fn messages_with_their_own_call_are_refused_through_send() {
    let mut room = Room::new();
    let mut viewer = connection(Room::JOIN_TOKEN, "Bea", &[]);
    joined(&mut room, &mut viewer, Room::JOIN_TOKEN);
    for kind in ["snapshot", "hello", "review_commit", "presence"] {
        viewer
            .session_mut()
            .send(Channel::Control, kind, "{}")
            .expect("queued");
        assert!(
            matches!(viewer.poll_transmit(0), Err(Error::Unsupported(_))),
            "{kind}"
        );
    }
    viewer
        .session_mut()
        .send(Channel::Signal, "signal", r#"{"kind":"ping"}"#)
        .expect("queued");
    assert!(viewer.poll_transmit(0).expect("sends").is_some());
}

#[test]
fn review_commits_are_refused_on_protocol_v1() {
    let mut profile = Furnace::new();
    profile.set_version(Some(1));
    assert!(matches!(
        profile.encode(&session::Outbound::ReviewCommit(ReviewCommit::default()), 0),
        Err(Error::Unsupported(_))
    ));
}
