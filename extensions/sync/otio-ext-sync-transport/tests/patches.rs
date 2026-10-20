//! A host's changes travelling as patches through the in-memory Furnace
//! room, which applies them to its copy as Furnace does.

mod common;

use otio_ext_sync::session::{Config, Event, Identity, Inbound, Outbound, Role, Session};
use otio_ext_sync::{Snapshot, patch};
use otio_ext_sync_transport::loopback::{ClientId, Room, pump};
use otio_ext_sync_transport::{Connection, Error, Frame, Furnace, Profile, furnace};
use serde_json::Value;

fn connection(token: &str, name: &str, versions: &[u8], receive_patches: bool) -> Connection {
    let config = Config {
        token: token.to_string(),
        identity: Identity {
            display_name: name.to_string(),
            ..Identity::default()
        },
    };
    let mut profile = Furnace::with_encodings(Vec::new());
    profile.set_versions(versions.to_vec());
    profile.set_receive_patches(receive_patches);
    Connection::new(Session::new(config), profile)
}

fn joined(room: &mut Room, connection: &mut Connection, token: &str) -> ClientId {
    let id = room.open(token);
    connection.connect();
    pump(room, &mut [(id, connection)], 0).expect("joins");
    assert!(connection.session().is_joined());
    id
}

/// Sends what `host` has queued to the room, and returns each frame's type.
fn send_all(room: &mut Room, host: &mut Connection, id: ClientId) -> Vec<String> {
    let mut kinds = Vec::new();
    while let Some(frame) = host.poll_transmit(0).expect("encodes") {
        let value: Value = serde_json::from_str(frame.as_text().expect("text")).expect("JSON");
        kinds.push(value["type"].as_str().expect("a type").to_string());
        room.deliver(id, &frame).expect("the room takes it");
    }
    kinds
}

/// The types of the frames the room queued for `id`, which it then takes.
fn received(room: &mut Room, id: ClientId, connection: &mut Connection) -> Vec<String> {
    let mut kinds = Vec::new();
    while let Some(frame) = room.take(id) {
        let value: Value = serde_json::from_str(frame.as_text().expect("text")).expect("JSON");
        kinds.push(value["type"].as_str().expect("a type").to_string());
        connection.receive(&frame).expect("applies");
    }
    kinds
}

fn timeline_of(connection: &Connection) -> &Snapshot {
    connection.session().timeline("main").expect("a timeline")
}

#[test]
fn patches_reach_receivers_and_snapshots_reach_everyone_else() {
    let mut room = Room::with_encodings(Vec::new());
    let mut host = connection(Room::HOST_TOKEN, "Ari", &[2, 1], true);
    let host_id = joined(&mut room, &mut host, Room::HOST_TOKEN);
    assert!(host.profile().server_patches());
    let mut receiver = connection(Room::JOIN_TOKEN, "Bea", &[2, 1], true);
    let receiver_id = joined(&mut room, &mut receiver, Room::JOIN_TOKEN);
    let mut declines = connection(Room::JOIN_TOKEN, "Cy", &[2, 1], false);
    let declines_id = joined(&mut room, &mut declines, Room::JOIN_TOKEN);
    let mut v1 = connection(Room::JOIN_TOKEN, "Dee", &[1], true);
    let v1_id = joined(&mut room, &mut v1, Room::JOIN_TOKEN);
    for (id, client) in [
        (host_id, &mut host),
        (receiver_id, &mut receiver),
        (declines_id, &mut declines),
        (v1_id, &mut v1),
    ] {
        received(&mut room, id, client);
    }

    let (mut document, timeline) = common::host_document();
    host.session_mut()
        .publish(&mut document, timeline, "main")
        .expect("publishes");
    assert_eq!(send_all(&mut room, &mut host, host_id), ["snapshot"]);
    common::rename(&mut document, timeline, "main/clip-2", "Sunset (alt)");
    host.session_mut()
        .publish(&mut document, timeline, "main")
        .expect("publishes");
    assert_eq!(send_all(&mut room, &mut host, host_id), ["patch"]);
    assert!(room.take(host_id).is_none(), "the sender gets nothing back");

    let expected = timeline_of(&host).clone();
    assert_eq!(room.snapshot().map(|s| s.generation), Some(2));
    assert!(
        otio_ext_sync::json_eq(&room.snapshot().unwrap().timeline, &expected.timeline).unwrap()
    );
    for (id, client, kinds) in [
        (receiver_id, &mut receiver, ["snapshot", "patch"]),
        (declines_id, &mut declines, ["snapshot", "snapshot"]),
        (v1_id, &mut v1, ["snapshot", "snapshot"]),
    ] {
        assert_eq!(received(&mut room, id, client), kinds);
        let theirs = timeline_of(client);
        assert_eq!(theirs.generation, 2);
        assert!(otio_ext_sync::json_eq(&theirs.timeline, &expected.timeline).unwrap());
        let changes: Vec<_> = std::iter::from_fn(|| client.poll_event())
            .filter_map(|event| match event {
                Event::Timeline(change) => Some(change),
                _ => None,
            })
            .collect();
        assert_eq!(changes.len(), 2);
        assert_eq!(changes[1].patches.len(), 1, "a rename, either way");
    }
}

#[test]
fn an_older_furnace_gets_snapshots() {
    let mut room = Room::with_encodings(Vec::new()).without_patches();
    let mut host = connection(Room::HOST_TOKEN, "Ari", &[2, 1], true);
    let host_id = joined(&mut room, &mut host, Room::HOST_TOKEN);
    assert!(!host.profile().server_patches());
    let (mut document, timeline) = common::host_document();
    for name in ["One", "Two", "Three"] {
        common::rename(&mut document, timeline, "main/clip-1", name);
        host.session_mut()
            .publish(&mut document, timeline, "main")
            .expect("publishes");
        assert_eq!(send_all(&mut room, &mut host, host_id), ["snapshot"]);
    }
}

#[test]
fn the_host_sends_a_snapshot_every_checkpoint() {
    let mut room = Room::with_encodings(Vec::new());
    let mut host = connection(Room::HOST_TOKEN, "Ari", &[2, 1], true);
    let host_id = joined(&mut room, &mut host, Room::HOST_TOKEN);
    let mut viewer = connection(Room::JOIN_TOKEN, "Bea", &[2, 1], true);
    let viewer_id = joined(&mut room, &mut viewer, Room::JOIN_TOKEN);
    let (mut document, timeline) = common::host_document();
    let mut kinds = Vec::new();
    let checkpoint = usize::try_from(furnace::CHECKPOINT).unwrap();
    for change in 0..=(2 * checkpoint) {
        common::rename(
            &mut document,
            timeline,
            "main/clip-1",
            &format!("Take {change}"),
        );
        host.session_mut()
            .publish(&mut document, timeline, "main")
            .expect("publishes");
        kinds.extend(send_all(&mut room, &mut host, host_id));
    }
    let snapshots: Vec<usize> = kinds
        .iter()
        .enumerate()
        .filter(|(_, kind)| *kind == "snapshot")
        .map(|(index, _)| index)
        .collect();
    assert_eq!(snapshots, [0, checkpoint, 2 * checkpoint]);
    received(&mut room, viewer_id, &mut viewer);
    assert!(
        otio_ext_sync::json_eq(&timeline_of(&viewer).timeline, &timeline_of(&host).timeline)
            .unwrap()
    );
}

#[test]
fn a_refused_patch_makes_the_host_send_the_whole_timeline() {
    let mut room = Room::with_encodings(Vec::new());
    let mut host = connection(Room::HOST_TOKEN, "Ari", &[2, 1], true);
    let host_id = joined(&mut room, &mut host, Room::HOST_TOKEN);
    let (mut document, timeline) = common::host_document();
    host.session_mut()
        .publish(&mut document, timeline, "main")
        .expect("publishes");
    send_all(&mut room, &mut host, host_id);

    // The room's copy moves on without the host (it restarted with an
    // older timeline, say), so the host's next patch is stale.
    let mut older = room.snapshot().unwrap().clone();
    older.generation = 0;
    let frame = {
        let mut profile = Furnace::with_encodings(Vec::new());
        profile.set_version(Some(2));
        profile
            .encode(
                &Outbound::Timeline {
                    patches: Vec::new(),
                    snapshot: older,
                    base_generation: None,
                },
                0,
            )
            .unwrap()
    };
    room.deliver(host_id, &frame).expect("the room takes it");

    common::rename(&mut document, timeline, "main/clip-1", "Renamed");
    host.session_mut()
        .publish(&mut document, timeline, "main")
        .expect("publishes");
    assert_eq!(send_all(&mut room, &mut host, host_id), ["patch"]);
    let seen = received(&mut room, host_id, &mut host);
    assert_eq!(seen, ["error"]);
    assert!(std::iter::from_fn(|| host.poll_event()).any(
        |event| matches!(event, Event::Error { code, fatal: false, .. } if code == "stale_patch")
    ));
    assert_eq!(send_all(&mut room, &mut host, host_id), ["snapshot"]);
    let stored = room.snapshot().unwrap();
    assert_eq!(stored.generation, 2);
    assert!(otio_ext_sync::json_eq(&stored.timeline, &timeline_of(&host).timeline).unwrap());
}

#[test]
fn patches_that_do_not_fit_ask_for_a_reconnect() {
    let mut room = Room::with_encodings(Vec::new());
    let mut host = connection(Room::HOST_TOKEN, "Ari", &[2, 1], true);
    let host_id = joined(&mut room, &mut host, Room::HOST_TOKEN);
    let mut viewer = connection(Room::JOIN_TOKEN, "Bea", &[2, 1], true);
    let viewer_id = joined(&mut room, &mut viewer, Room::JOIN_TOKEN);
    received(&mut room, viewer_id, &mut viewer);
    let (mut document, timeline) = common::host_document();
    host.session_mut()
        .publish(&mut document, timeline, "main")
        .expect("publishes");
    send_all(&mut room, &mut host, host_id);
    received(&mut room, viewer_id, &mut viewer);

    // A patch from a generation this viewer never had.
    let mut session = Session::new(Config::default());
    session.connect();
    let welcome = viewer.session().self_id().map(str::to_string);
    assert!(welcome.is_some());
    let stray = Inbound::Patches {
        sender: None,
        sequence_id: "main".to_string(),
        base_generation: 5,
        generation: 6,
        patches: patch::from_json("[]").unwrap(),
    };
    let mut viewer_session = std::mem::replace(viewer.session_mut(), session);
    viewer_session.receive(stray);
    match viewer_session.take_fault() {
        Some(otio_ext_sync::session::Error::OutOfSync(message)) => {
            assert!(message.contains("generation 5"), "{message}");
        }
        other => panic!("{other:?}"),
    }
    assert!(viewer_session.take_fault().is_none(), "taking it clears it");
    assert_eq!(
        viewer_session.timeline("main").map(|s| s.generation),
        Some(1)
    );
    assert_eq!(viewer_session.role(), Some(&Role::Viewer));
}

#[test]
fn connection_receive_reports_patches_that_do_not_fit() {
    let mut viewer = connection(Room::JOIN_TOKEN, "Bea", &[2, 1], true);
    viewer.connect();
    let _ = viewer.poll_transmit(0);
    let welcome = r#"{"v":2,"type":"welcome","sessionId":"s","seq":0,"ts":0,"order":1,"payload":{"selfId":"p_2","role":"viewer","session":{"id":"s","createdAt":0},"roster":[],"leaderId":"","serverCapabilities":["timeline.patch"]}}"#;
    viewer.receive(&Frame::text(welcome)).expect("welcomed");
    let stray = r#"{"v":2,"type":"patch","sessionId":"s","senderId":"p_1","seq":3,"ts":0,"order":2,"payload":{"sequenceId":"main","baseGeneration":1,"generation":2,"patches":[]}}"#;
    match viewer.receive(&Frame::text(stray)) {
        Err(Error::Session(otio_ext_sync::session::Error::OutOfSync(_))) => {}
        other => panic!("{other:?}"),
    }
}
