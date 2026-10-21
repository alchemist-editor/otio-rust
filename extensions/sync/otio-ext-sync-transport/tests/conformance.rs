//! Furnace's conformance corpus: every frame it says a client sends or a
//! server sends, read and written by this crate's Furnace profile. See
//! `fixtures/furnace-conformance/README.md`.

use std::path::{Path, PathBuf};

use serde_json::Value;

use otio_ext_sync::json_eq;
use otio_ext_sync::session::{Inbound, Outbound, Role, RosterEvent};
use otio_ext_sync_transport::{Frame, Furnace, Profile, furnace};

fn corpus() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/furnace-conformance")
}

/// Every fixture, as (revision directory, file name, text).
fn fixtures(prefix: &str) -> Vec<(String, String, String)> {
    let mut found = Vec::new();
    for revision in ["v1", "v2", "v3", "v4", "v5"] {
        let directory = corpus().join(revision);
        let mut names: Vec<String> = std::fs::read_dir(&directory)
            .expect("the corpus is vendored")
            .map(|entry| {
                entry
                    .expect("an entry")
                    .file_name()
                    .to_string_lossy()
                    .into_owned()
            })
            .filter(|name| name.starts_with(prefix) && name.ends_with(".json"))
            .collect();
        names.sort();
        for name in names {
            let text = std::fs::read_to_string(directory.join(&name)).expect("reads");
            found.push((revision.to_string(), name, text));
        }
    }
    found
}

fn without_seq(mut value: Value) -> Value {
    let object = value.as_object_mut().expect("an envelope is an object");
    assert!(
        object.remove("seq").is_some_and(|seq| seq.is_u64()),
        "seq is a number"
    );
    value
}

#[test]
fn the_corpus_is_revision_5() {
    let version = std::fs::read_to_string(corpus().join("VERSION")).expect("VERSION");
    assert_eq!(version.trim(), "5");
}

#[test]
fn every_client_frame_is_written_as_furnace_reads_it() {
    let fixtures = fixtures("client-");
    assert_eq!(fixtures.len(), 8 + 12 + 15 + 17 + 19);
    for (revision, name, text) in fixtures {
        let label = format!("{revision}/{name}");
        let frame = Frame::text(text.clone());
        let (version, message) =
            furnace::decode_client(&frame).unwrap_or_else(|e| panic!("{label}: {e}"));
        let fixture: Value = serde_json::from_str(&text).expect("JSON");

        let mut profile = Furnace::with_encodings(Vec::new());
        // The profile asks for patches in every v2 hello; the fixtures
        // that do not ask stand for clients that do not.
        profile.set_receive_patches(text.contains(furnace::PATCH_RECEIVE_CAPABILITY));
        if matches!(message, Outbound::Hello(_)) {
            let (versions, _) = furnace::hello_offer(&frame).expect("a hello");
            profile.set_versions(versions);
        } else {
            profile.set_version(Some(version));
            profile.set_server_patches(true);
        }
        let ts = fixture["ts"].as_i64().expect("ts");
        let encoded = profile
            .encode(&message, ts)
            .unwrap_or_else(|e| panic!("{label}: {e}"));
        let encoded: Value = serde_json::from_str(encoded.as_text().expect("text")).expect("JSON");
        assert_eq!(without_seq(encoded), without_seq(fixture), "{label}");
    }
}

#[test]
fn every_server_frame_reads_as_what_it_says() {
    let fixtures = fixtures("server-");
    assert_eq!(fixtures.len(), 15 + 16 + 21 + 25 + 29);
    for (revision, name, text) in fixtures {
        let label = format!("{revision}/{name}");
        let fixture: Value = serde_json::from_str(&text).expect("JSON");
        let payload = &fixture["payload"];
        let mut profile = Furnace::new();
        let mut messages = profile
            .decode(&Frame::text(text.clone()))
            .unwrap_or_else(|e| panic!("{label}: {e}"));
        assert_eq!(messages.len(), 1, "{label}");
        let message = messages.remove(0);
        let sender = fixture["senderPresenceId"]
            .as_str()
            .or_else(|| fixture["senderId"].as_str())
            .map(str::to_string);
        let string = |value: &Value| value.as_str().unwrap_or_default().to_string();
        let json = |value: &Value| value.to_string();

        match (fixture["type"].as_str().expect("a type"), message) {
            ("welcome", Inbound::Welcome(welcome)) => {
                let self_id = payload["selfPresenceId"]
                    .as_str()
                    .or(payload["selfId"].as_str());
                assert_eq!(Some(welcome.self_id.as_str()), self_id, "{label}");
                assert_eq!(
                    welcome.self_user_id.as_deref(),
                    payload["selfUserId"].as_str(),
                    "{label}"
                );
                assert_eq!(
                    welcome.role,
                    Some(Role::from_name(&string(&payload["role"]))),
                    "{label}"
                );
                assert_eq!(
                    welcome.session_id,
                    string(&payload["session"]["id"]),
                    "{label}"
                );
                assert_eq!(
                    Some(welcome.created_at),
                    payload["session"]["createdAt"].as_i64()
                );
                check_peers(&label, &welcome.peers, payload);
                let leader = payload["leaderPresenceId"]
                    .as_str()
                    .or(payload["leaderId"].as_str());
                assert_eq!(
                    welcome.leader.as_deref(),
                    leader.filter(|l| !l.is_empty()),
                    "{label}"
                );
                match (&welcome.snapshot, payload.get("snapshot")) {
                    (Some(snapshot), Some(expected)) => {
                        assert_eq!(
                            snapshot.sequence_id,
                            string(&expected["sequenceId"]),
                            "{label}"
                        );
                        assert_eq!(Some(snapshot.generation), expected["generation"].as_i64());
                        assert!(json_eq(&snapshot.timeline, &json(&expected["otioJson"])).unwrap());
                    }
                    (None, None) => {}
                    other => panic!("{label}: snapshot {other:?}"),
                }
                let acks = payload["pendingMutationAcknowledgements"]
                    .as_array()
                    .cloned()
                    .unwrap_or_default();
                assert_eq!(welcome.acknowledgements.len(), acks.len(), "{label}");
                for (ack, expected) in welcome.acknowledgements.iter().zip(&acks) {
                    assert_eq!(
                        ack.client_mutation_id,
                        string(&expected["clientMutationId"])
                    );
                    assert_eq!(ack.comment_id, string(&expected["commentId"]));
                    assert_eq!(Some(ack.server_order), expected["serverOrder"].as_u64());
                }
                assert!(json_eq(&welcome.raw, &json(payload)).unwrap(), "{label}");
            }
            (
                "snapshot",
                Inbound::Timeline {
                    sender: from,
                    snapshot,
                    ..
                },
            ) => {
                assert_eq!(from, sender, "{label}");
                assert_eq!(snapshot.sequence_id, string(&payload["sequenceId"]));
                assert_eq!(Some(snapshot.generation), payload["generation"].as_i64());
                assert!(
                    json_eq(&snapshot.timeline, &json(&payload["otioJson"])).unwrap(),
                    "{label}"
                );
            }
            (
                "patch",
                Inbound::Patches {
                    sender: from,
                    sequence_id,
                    base_generation,
                    generation,
                    patches,
                    ..
                },
            ) => {
                assert_eq!(from, sender, "{label}");
                assert_eq!(sequence_id, string(&payload["sequenceId"]));
                assert_eq!(Some(base_generation), payload["baseGeneration"].as_i64());
                assert_eq!(Some(generation), payload["generation"].as_i64());
                assert!(
                    json_eq(
                        &otio_ext_sync::patch::to_json(&patches),
                        &json(&payload["patches"])
                    )
                    .unwrap(),
                    "{label}"
                );
            }
            (
                "presence",
                Inbound::Playhead {
                    sender: from,
                    playhead,
                },
            ) => {
                assert_eq!(Some(from), sender, "{label}");
                let expected = &payload["playhead"];
                assert_eq!(playhead.sequence_id, string(&expected["sequenceId"]));
                assert_eq!(Some(playhead.frame), expected["frame"].as_f64());
                assert_eq!(Some(playhead.playing), expected["playing"].as_bool());
                assert_eq!(Some(playhead.rate), expected["rate"].as_f64());
            }
            ("roster", Inbound::Roster { event, peer, peers }) => {
                assert_eq!(event.as_str(), string(&payload["event"]), "{label}");
                assert_eq!(peer.id, string(&payload["participant"]["id"]), "{label}");
                assert_eq!(Some(peer.muted), payload["participant"]["muted"].as_bool());
                if event == RosterEvent::Join {
                    assert!(peers.iter().any(|p| p.id == peer.id), "{label}");
                }
                check_peers(&label, &peers, payload);
            }
            ("leader", Inbound::Leader { leader }) => {
                let expected = payload["leaderPresenceId"]
                    .as_str()
                    .or(payload["leaderId"].as_str());
                assert_eq!(
                    leader.as_deref(),
                    expected.filter(|l| !l.is_empty()),
                    "{label}"
                );
            }
            ("review_commit", Inbound::ReviewRelay(relay)) => {
                assert_eq!(
                    relay.ack.client_mutation_id,
                    string(&payload["clientMutationId"])
                );
                assert_eq!(relay.ack.comment_id, string(&payload["commentId"]));
                assert_eq!(
                    Some(relay.ack.server_order),
                    payload["serverOrder"].as_u64()
                );
                assert_eq!(Some(relay.ack.revision), payload["revision"].as_u64());
                assert_eq!(relay.author_user_id, string(&payload["authorUserId"]));
                assert_eq!(
                    relay.author_presence_id,
                    string(&payload["authorPresenceId"])
                );
                assert_eq!(Some(relay.created_at), payload["createdAt"].as_i64());
                assert!(json_eq(&relay.commit.anchor, &json(&payload["anchor"])).unwrap());
                assert_eq!(relay.commit.body_markdown, string(&payload["bodyMarkdown"]));
                let commands = payload["annotationCommands"].as_array().expect("commands");
                assert_eq!(relay.commit.annotation_commands.len(), commands.len());
                for (command, expected) in relay.commit.annotation_commands.iter().zip(commands) {
                    assert!(json_eq(command, &json(expected)).unwrap(), "{label}");
                }
            }
            ("review_committed", Inbound::ReviewCommitted(ack)) => {
                assert_eq!(ack.client_mutation_id, string(&payload["clientMutationId"]));
                assert_eq!(ack.comment_id, string(&payload["commentId"]));
                assert_eq!(Some(ack.server_order), payload["serverOrder"].as_u64());
                assert_eq!(Some(ack.revision), payload["revision"].as_u64());
            }
            (
                "review_rejected",
                Inbound::ReviewRejected {
                    client_mutation_id,
                    code,
                    message,
                },
            ) => {
                assert_eq!(
                    client_mutation_id.as_deref(),
                    payload["clientMutationId"].as_str()
                );
                assert_eq!(code, string(&payload["code"]));
                assert_eq!(message, string(&payload["message"]));
            }
            ("session_ended", Inbound::Ended { reason }) => {
                assert_eq!(reason, string(&payload["reason"]), "{label}");
            }
            (
                "error",
                Inbound::Error {
                    code,
                    message,
                    fatal,
                },
            ) => {
                assert_eq!(code, string(&payload["code"]), "{label}");
                assert_eq!(message, string(&payload["message"]), "{label}");
                assert_eq!(Some(fatal), payload["fatal"].as_bool(), "{label}");
            }
            (
                kind,
                Inbound::Other {
                    kind: other,
                    sender: from,
                    payload: text,
                },
            ) if matches!(
                kind,
                "bin"
                    | "signal"
                    | "thumb_added"
                    | "waveform_added"
                    | "lan_mode"
                    | "lan_request"
                    | "lan_resolved"
            ) =>
            {
                assert_eq!(other, kind, "{label}");
                assert_eq!(from, sender, "{label}");
                assert!(json_eq(&text, &json(payload)).unwrap(), "{label}");
            }
            (kind, message) => panic!("{label}: a `{kind}` frame read as {message:?}"),
        }
    }
}

/// Checks decoded peers against a welcome or roster payload.
fn check_peers(label: &str, peers: &[otio_ext_sync::session::Peer], payload: &Value) {
    let roster = payload["roster"].as_array().expect("a roster");
    assert_eq!(peers.len(), roster.len(), "{label}");
    let presences = payload.get("presences").and_then(Value::as_array);
    for (peer, expected) in peers.iter().zip(roster) {
        assert_eq!(
            peer.id,
            expected["id"].as_str().unwrap_or_default(),
            "{label}"
        );
        assert_eq!(
            peer.display_name,
            expected["displayName"].as_str().unwrap_or_default()
        );
        assert_eq!(
            peer.role.as_str(),
            expected["role"].as_str().unwrap_or_default()
        );
        assert_eq!(Some(peer.joined_at), expected["joinedAt"].as_i64());
        assert_eq!(
            peer.connection_state,
            expected["connectionState"].as_str().unwrap_or_default()
        );
        assert_eq!(Some(peer.muted), expected["muted"].as_bool());
        let presence = presences
            .and_then(|presences| presences.iter().find(|p| p["presenceId"] == expected["id"]));
        match presence {
            Some(presence) => {
                assert_eq!(
                    peer.user_id.as_deref(),
                    presence["userId"].as_str(),
                    "{label}"
                );
                assert_eq!(peer.client_kind.as_deref(), presence["clientKind"].as_str());
                assert_eq!(peer.form_factor.as_deref(), presence["formFactor"].as_str());
                assert_eq!(
                    peer.device_name.as_deref(),
                    presence["deviceDisplayName"].as_str()
                );
                let capabilities: Vec<&str> = presence["capabilities"]
                    .as_array()
                    .map(|list| list.iter().filter_map(Value::as_str).collect())
                    .unwrap_or_default();
                assert_eq!(peer.capabilities, capabilities, "{label}");
            }
            None => assert_eq!(peer.user_id, None, "{label}"),
        }
    }
}

fn fixture(revision: &str, name: &str) -> String {
    std::fs::read_to_string(corpus().join(revision).join(name)).expect("the corpus is vendored")
}

/// Furnace computes the snapshot it rebuilds from a patch with its own
/// `otiodiff.Apply`; this crate's apply must reach the same timeline.
#[test]
fn applying_the_corpus_patch_reaches_furnaces_rebuilt_snapshot() {
    let read = |name: &str| -> Value { serde_json::from_str(&fixture("v5", name)).expect("JSON") };
    let base = read("client-snapshot-with-clips.json");
    let patch = read("client-patch-v2.json");
    let rebuilt = read("server-snapshot-patched-v2.json");
    assert_eq!(
        patch["payload"]["baseGeneration"],
        base["payload"]["generation"]
    );
    let patches =
        otio_ext_sync::patch::from_json(&patch["payload"]["patches"].to_string()).expect("patches");
    let applied =
        otio_ext_sync::apply(&base["payload"]["otioJson"].to_string(), &patches).expect("applies");
    assert!(json_eq(&applied, &rebuilt["payload"]["otioJson"].to_string()).unwrap());
    assert_eq!(
        rebuilt["payload"]["generation"],
        patch["payload"]["generation"]
    );
}

/// A viewer that joined to the corpus welcome and then got the corpus
/// patch holds the timeline Furnace rebuilt.
#[test]
fn a_session_applies_the_corpus_patch() {
    use otio_ext_sync::session::{Config, Event, Session};

    let decode = |name: &str| {
        let mut profile = Furnace::new();
        profile
            .decode(&Frame::text(fixture("v5", name)))
            .expect("decodes")
            .remove(0)
    };
    let mut session = Session::new(Config::default());
    session.connect();
    session.receive(decode("server-welcome-v2.json"));
    session.receive(decode("server-patch-v2.json"));
    assert!(session.take_fault().is_none());
    let rebuilt: Value =
        serde_json::from_str(&fixture("v5", "server-snapshot-patched-v2.json")).expect("JSON");
    let timeline = session.timeline("main").expect("a timeline");
    assert_eq!(timeline.generation, 8);
    assert!(
        json_eq(
            &timeline.timeline,
            &rebuilt["payload"]["otioJson"].to_string()
        )
        .unwrap()
    );
    let changes: Vec<_> = std::iter::from_fn(|| session.poll_event())
        .filter_map(|event| match event {
            Event::Timeline(change) => Some(change),
            _ => None,
        })
        .collect();
    assert_eq!(changes.len(), 2, "the join, then the patch");
    assert_eq!(changes[1].patches.len(), 4);
    assert!(!changes[1].joining);
}

/// The corpus welcome lists `timeline.patch`, so the profile sends a host's
/// change as a `patch`; the v4 welcome does not, so it sends a snapshot.
#[test]
fn the_welcome_decides_whether_patches_are_sent() {
    for (revision, expected) in [("v4", "snapshot"), ("v5", "patch")] {
        let mut profile = Furnace::new();
        profile
            .decode(&Frame::text(fixture(
                revision,
                "server-welcome-host-v2.json",
            )))
            .expect("decodes");
        let patch: Value =
            serde_json::from_str(&fixture("v5", "server-snapshot-patched-v2.json")).expect("JSON");
        let message = Outbound::Timeline {
            snapshot: otio_ext_sync::Snapshot {
                sequence_id: "main".to_string(),
                generation: 8,
                timeline: patch["payload"]["otioJson"].to_string(),
            },
            patches: otio_ext_sync::patch::from_json(
                r#"[{"op":"set_property","guid":"main/clip-1002","path":"/name","value":"x"}]"#,
            )
            .unwrap(),
            base_generation: Some(7),
            previous: None,
        };
        let frame = profile.encode(&message, 0).expect("encodes");
        let sent: Value = serde_json::from_str(frame.as_text().expect("text")).expect("JSON");
        assert_eq!(sent["type"], expected, "{revision}");
    }
}
