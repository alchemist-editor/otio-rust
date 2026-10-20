//! Joins a real Furnace: a host and a viewer over WebSockets.
//!
//! Runs only when `FURNACE_URL` names a running furnaced, such as
//! `http://127.0.0.1:8080`; CI builds one from svc-furnace and sets it.
//! Without it the test passes having done nothing.

#![cfg(feature = "websocket")]

mod common;

use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::{Duration, Instant};

use otio_ext_sync::session::{
    self, ApplyOptions, Config, Event, Handled, Identity, Playhead, ReviewCommit, Role, Session,
};
use otio_ext_sync::{Op, Patch};
use otio_ext_sync_transport::websocket::Client;

/// What `POST /v1/sessions` returns.
struct Created {
    ws_url: String,
    host_token: String,
    join_token: String,
}

/// Creates a session on the furnaced at `base`, over plain HTTP.
fn create_session(base: &str) -> Created {
    let address = base.trim_start_matches("http://").trim_end_matches('/');
    let mut stream = TcpStream::connect(address).expect("furnaced is listening");
    write!(
        stream,
        "POST /v1/sessions HTTP/1.1\r\nHost: {address}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
    )
    .expect("the request is sent");
    let mut response = String::new();
    stream.read_to_string(&mut response).expect("a response");
    let body = response.split("\r\n\r\n").nth(1).expect("a body");
    let json: serde_json::Value = serde_json::from_str(body.trim()).expect("JSON");
    let field = |name: &str| json[name].as_str().expect(name).to_string();
    Created {
        ws_url: field("wsUrl"),
        host_token: field("hostToken"),
        join_token: field("joinToken"),
    }
}

fn config(token: &str, name: &str, capabilities: &[&str]) -> Config {
    Config {
        token: token.to_string(),
        identity: Identity {
            display_name: name.to_string(),
            device_name: format!("{name}'s test device"),
            capabilities: capabilities.iter().map(ToString::to_string).collect(),
            ..Identity::default()
        },
    }
}

/// Reads events until `found` picks one, failing after ten seconds.
fn wait_for<T>(client: &mut Client, mut found: impl FnMut(&Event) -> Option<T>) -> (T, Vec<Event>) {
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut seen = Vec::new();
    while Instant::now() < deadline {
        if let Some(event) = client
            .next_event(Duration::from_millis(200))
            .expect("the socket works")
        {
            let picked = found(&event);
            seen.push(event);
            if let Some(picked) = picked {
                return (picked, seen);
            }
        }
    }
    panic!("timed out; saw {seen:#?}");
}

#[test]
fn a_viewer_follows_the_host_through_furnace() {
    let Ok(base) = std::env::var("FURNACE_URL") else {
        eprintln!("FURNACE_URL is not set; skipping the live Furnace test");
        return;
    };
    common::enable_sync();
    let created = create_session(&base);

    // The host joins and publishes its timeline.
    let host_config = config(&created.host_token, "Ari", &["review.commit.receive"]);
    let mut host =
        Client::connect(&created.ws_url, Session::new(host_config)).expect("host connects");
    let (role, _) = wait_for(&mut host, |event| match event {
        Event::Joined { role, .. } => Some(role.clone()),
        _ => None,
    });
    assert_eq!(role, Role::Host);
    let (mut host_document, host_timeline) = common::host_document();
    let patches = host
        .session_mut()
        .publish(&mut host_document, host_timeline, "main")
        .expect("the host may publish");
    assert_eq!(patches.len(), 1);
    assert_eq!(patches[0].op, Op::ReplaceTimeline);
    host.flush().expect("the snapshot is sent");

    // The viewer joins to the host's timeline.
    let viewer_config = config(&created.join_token, "Bea", &["review.comment.create"]);
    let mut viewer =
        Client::connect(&created.ws_url, Session::new(viewer_config)).expect("viewer connects");
    let (mut viewer_document, viewer_timeline) = common::viewer_document();
    let options = ApplyOptions::default();
    let (_, seen) = wait_for(&mut viewer, |event| {
        matches!(event, Event::Joined { .. }).then_some(())
    });
    let joining = seen
        .iter()
        .find_map(|event| match event {
            Event::Timeline(change) if change.joining => Some(change.clone()),
            _ => None,
        })
        .expect("the join brings the timeline");
    assert_eq!(joining.generation, 1);
    for event in &seen {
        session::handle(&mut viewer_document, viewer_timeline, event, &options).expect("applies");
    }
    assert_eq!(
        common::name_of(&viewer_document, viewer_timeline, "main/clip-1"),
        "Beach Run"
    );
    let clip = otio_ext_sync::guid::GuidIndex::build(&viewer_document, viewer_timeline)
        .expect("unique")
        .get("main/clip-1")
        .expect("the clip arrived");

    // The host renames a clip; the viewer gets a patch, applied in place.
    common::rename(
        &mut host_document,
        host_timeline,
        "main/clip-1",
        "Beach Run v2",
    );
    let patches = host
        .session_mut()
        .publish(&mut host_document, host_timeline, "main")
        .expect("publishes");
    assert_eq!(
        otio_ext_sync::patch::to_json(&patches),
        r#"[{"op":"set_property","guid":"main/clip-1","path":"/name","value":"Beach Run v2"}]"#
    );
    host.flush().expect("sent");
    let (change, _) = wait_for(&mut viewer, |event| match event {
        Event::Timeline(change) => Some(change.clone()),
        _ => None,
    });
    assert_eq!(change.generation, 2);
    assert_eq!(change.patches, patches);
    let handled = session::handle(
        &mut viewer_document,
        viewer_timeline,
        &Event::Timeline(change),
        &options,
    )
    .expect("applies");
    assert_eq!(handled, Handled::Applied);
    assert_eq!(
        viewer_document
            .get(clip)
            .and_then(otio_core::Node::base)
            .expect("same handle")
            .name,
        "Beach Run v2"
    );

    // The viewer moves its playhead; the host sees it.
    let playhead = Playhead {
        sequence_id: "main".to_string(),
        frame: 101.0,
        playing: true,
        rate: 23.976,
    };
    viewer
        .session_mut()
        .set_playhead(playhead.clone())
        .expect("anyone may");
    viewer.flush().expect("sent");
    let (seen_playhead, _) = wait_for(&mut host, |event| match event {
        Event::Playhead { playhead, .. } => Some(playhead.clone()),
        _ => None,
    });
    assert_eq!(seen_playhead, playhead);

    // The viewer commits a review comment; Furnace relays it to the host.
    let anchor = format!(
        r#"{{"sessionId":"{}","sequenceId":"main","capturedGeneration":2,"clipGuid":"main/clip-1",
           "sourceTime":{{"value":12,"rate":24}},"capturedSequenceTime":{{"value":12,"rate":24}},
           "mediaFingerprint":{{"mediaKey":"clip-1"}},"capturedAt":1787742000000}}"#,
        viewer.session().session_id().expect("joined")
    );
    let id = viewer
        .session_mut()
        .commit_review(ReviewCommit {
            anchor,
            body_markdown: "The eyeline feels late here.".to_string(),
            ..ReviewCommit::default()
        })
        .expect("may comment");
    viewer.flush().expect("sent");
    let (ack, _) = wait_for(&mut viewer, |event| match event {
        Event::ReviewCommitted(ack) => Some(ack.clone()),
        Event::ReviewRejected { code, message, .. } => panic!("rejected: {code}: {message}"),
        _ => None,
    });
    assert_eq!(ack.client_mutation_id, id);
    assert!(viewer.session().pending_reviews().is_empty());
    let (relay, _) = wait_for(&mut host, |event| match event {
        Event::ReviewRelay(relay) => Some(relay.clone()),
        _ => None,
    });
    assert_eq!(relay.ack, ack);
    assert_eq!(relay.commit.body_markdown, "The eyeline feels late here.");

    // A viewer may not change the timeline: refused before it reaches Furnace.
    assert_eq!(
        viewer
            .session_mut()
            .publish(&mut viewer_document, viewer_timeline, "main")
            .unwrap_err(),
        session::Error::NoLease(session::Channel::Timeline)
    );

    viewer.close().expect("leaves");
    let (left, _) = wait_for(&mut host, |event| match event {
        Event::Roster { event, peer } if *event == session::RosterEvent::Leave => {
            Some(peer.clone())
        }
        _ => None,
    });
    assert_eq!(left.display_name, "Bea");
    host.close().expect("leaves");
    let _ = Patch::default();
}
