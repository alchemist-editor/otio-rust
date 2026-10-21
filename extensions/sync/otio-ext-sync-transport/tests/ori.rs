//! Peers on ORI Sync's wire, through an in-memory fanout exchange.

mod common;

use otio_ext_sync::session::{Config, Event, Identity, Playhead, Role, RosterEvent, Session};
use otio_ext_sync_transport::ori::{self, Options, Ori};
use otio_ext_sync_transport::{Connection, Error, Frame};

/// A timeline with two video tracks, the first holding three clips.
const TWO_TRACKS: &str = r#"{"OTIO_SCHEMA":"Timeline.1","name":"Program","metadata":{"sync":{"guid":"main"}},
  "tracks":{"OTIO_SCHEMA":"Stack.1","name":"tracks","metadata":{"sync":{"guid":"main/stack"}},"children":[
    {"OTIO_SCHEMA":"Track.1","name":"V1","kind":"Video","metadata":{"sync":{"guid":"main/v1"}},"children":[
      CLIP_1, CLIP_2, CLIP_3]},
    {"OTIO_SCHEMA":"Track.1","name":"V2","kind":"Video","metadata":{"sync":{"guid":"main/v2"}},"children":[]}]}}"#;

fn clip(guid: &str, name: &str) -> String {
    format!(
        r#"{{"OTIO_SCHEMA":"Clip.2","name":"{name}","metadata":{{"sync":{{"guid":"{guid}"}}}},
       "source_range":{{"OTIO_SCHEMA":"TimeRange.1",
         "start_time":{{"OTIO_SCHEMA":"RationalTime.1","value":0,"rate":24}},
         "duration":{{"OTIO_SCHEMA":"RationalTime.1","value":24,"rate":24}}}}}}"#
    )
}

/// [`TWO_TRACKS`] with the clips `v1` and `v2` name, in order.
fn timeline(v1: &[&str], v2: &[&str]) -> String {
    let clips = |guids: &[&str]| {
        guids
            .iter()
            .map(|guid| clip(guid, &guid.to_uppercase()))
            .collect::<Vec<_>>()
            .join(",")
    };
    TWO_TRACKS
        .replace("CLIP_1, CLIP_2, CLIP_3", &clips(v1))
        .replace(
            r#""children":[]}]}}"#,
            &format!(r#""children":[{}]}}]}}}}"#, clips(v2)),
        )
}

/// One ORI session's exchange: every frame goes to every peer, the sender
/// included, as a RabbitMQ fanout exchange delivers it.
struct Exchange {
    peers: Vec<Connection<Ori>>,
    /// Who sent each frame and what it was, as `schema/event`.
    log: Vec<(usize, String)>,
    /// Peers that hear nothing and say nothing, as if their process hung.
    silent: Vec<bool>,
}

impl Exchange {
    fn new() -> Self {
        Self {
            peers: Vec::new(),
            log: Vec::new(),
            silent: Vec::new(),
        }
    }

    fn add(&mut self, guid: &str, options: impl FnOnce(&mut Options)) -> usize {
        let mut settings = Options::new("test-session");
        settings.peer_guid = guid.to_string();
        options(&mut settings);
        let config = Config {
            token: String::new(),
            identity: Identity {
                display_name: guid.to_string(),
                app: "otio-rust".to_string(),
                ..Identity::default()
            },
        };
        let mut connection = Connection::new(Session::new(config), Ori::new(settings));
        connection.connect();
        self.peers.push(connection);
        self.silent.push(false);
        self.peers.len() - 1
    }

    /// Moves frames until nobody has anything to say at `now`.
    fn pump(&mut self, now: i64) {
        loop {
            let mut moved = false;
            for sender in 0..self.peers.len() {
                if self.silent[sender] {
                    continue;
                }
                while let Some(frame) = self.peers[sender].poll_transmit(now).expect("sends") {
                    moved = true;
                    self.log.push((sender, describe(&frame)));
                    for receiver in 0..self.peers.len() {
                        if !self.silent[receiver] {
                            self.peers[receiver].receive(&frame).expect("receives");
                        }
                    }
                }
            }
            if !moved {
                return;
            }
        }
    }

    fn sent(&self, peer: usize) -> Vec<&str> {
        self.log
            .iter()
            .filter(|(sender, _)| *sender == peer)
            .map(|(_, kind)| kind.as_str())
            .collect()
    }

    fn events(&mut self, peer: usize) -> Vec<Event> {
        std::iter::from_fn(|| self.peers[peer].poll_event()).collect()
    }

    fn publish(&mut self, peer: usize, sequence: &str, json: &str) {
        let mut document = otio_core::from_str(json).expect("the timeline reads");
        let root = document.root().expect("a root");
        self.peers[peer]
            .session_mut()
            .publish(&mut document, root, sequence)
            .expect("publishes");
    }

    fn timeline(&self, peer: usize, sequence: &str) -> String {
        self.peers[peer]
            .session()
            .timeline(sequence)
            .expect("has the sequence")
            .timeline
            .clone()
    }

    fn same(&self, sequence: &str) {
        let first = self.timeline(0, sequence);
        for peer in 1..self.peers.len() {
            if self.silent[peer] {
                continue;
            }
            assert!(
                otio_ext_sync::json_eq(&first, &self.timeline(peer, sequence)).unwrap(),
                "peer {peer} differs on {sequence}:\n{first}\n{}",
                self.timeline(peer, sequence)
            );
        }
    }
}

/// A frame's `schema/event`.
fn describe(frame: &Frame) -> String {
    let text = if frame.binary {
        let mut out = Vec::new();
        std::io::Read::read_to_end(
            &mut flate2::read::ZlibDecoder::new(frame.data.as_slice()),
            &mut out,
        )
        .expect("zlib");
        String::from_utf8(out).expect("UTF-8")
    } else {
        frame.as_text().expect("text").to_string()
    };
    let envelope: serde_json::Value = serde_json::from_str(&text).expect("JSON");
    format!(
        "{}/{}",
        envelope["payload"]["command_schema"].as_str().unwrap(),
        envelope["payload"]["command"]["event"].as_str().unwrap()
    )
}

/// Two peers in one session: the first elected itself and published
/// [`TWO_TRACKS`] as `main`, the second joined.
fn two_peers() -> Exchange {
    let mut exchange = Exchange::new();
    let a = exchange.add("peer-a", |_| {});
    exchange.pump(0);
    exchange.pump(2_000);
    exchange.publish(a, "main", &timeline(&["c1", "c2", "c3"], &[]));
    exchange.pump(2_100);
    exchange.add("peer-b", |_| {});
    exchange.pump(3_000);
    exchange.events(0);
    exchange.events(1);
    exchange.log.clear();
    exchange
}

#[test]
fn a_lone_peer_elects_itself_after_two_seconds() {
    let mut exchange = Exchange::new();
    let a = exchange.add("peer-a", |_| {});
    exchange.pump(0);
    assert_eq!(
        exchange.sent(a),
        ["LiveSession.1/WHO_IS_MASTER", "LiveSession.1/PEER_ANNOUNCE"]
    );
    exchange.pump(1_999);
    assert!(!exchange.peers[a].session().is_joined());
    exchange.pump(2_000);
    assert!(exchange.peers[a].session().is_joined());
    assert!(exchange.peers[a].profile().is_master());
    assert_eq!(exchange.sent(a)[2], "LiveSession.1/I_AM_MASTER");
    assert!(matches!(
        exchange.events(a).as_slice(),
        [Event::Joined {
            role: Role::Driver,
            rejoined: false,
            ..
        }]
    ));
}

#[test]
fn a_joiner_takes_the_masters_timelines() {
    let mut exchange = Exchange::new();
    let a = exchange.add("peer-a", |_| {});
    exchange.pump(0);
    exchange.pump(2_000);
    exchange.publish(a, "main", &timeline(&["c1", "c2", "c3"], &[]));
    exchange.pump(2_100);
    assert_eq!(exchange.sent(a).last(), Some(&"TIMELINE_1.0/ADD_TIMELINE"));
    exchange.events(a);

    let b = exchange.add("peer-b", |_| {});
    exchange.pump(3_000);
    assert_eq!(
        exchange.sent(b),
        [
            "LiveSession.1/WHO_IS_MASTER",
            "LiveSession.1/PEER_ANNOUNCE",
            "LiveSession.1/STATE_REQUEST"
        ]
    );
    assert!(
        exchange
            .sent(a)
            .ends_with(&["LiveSession.1/I_AM_MASTER", "LiveSession.1/STATE_SNAPSHOT"])
    );
    assert!(exchange.peers[b].session().is_joined());
    assert_eq!(exchange.peers[b].profile().master(), Some("peer-a"));
    assert!(!exchange.peers[b].profile().is_master());
    exchange.same("main");
    let seen = exchange.events(b);
    assert!(matches!(&seen[0], Event::Timeline(change) if change.joining));
    assert!(matches!(
        seen.last(),
        Some(Event::Joined {
            role: Role::Driver,
            ..
        })
    ));
    let roster = exchange.events(a);
    assert!(roster.iter().any(|event| matches!(
        event,
        Event::Roster { event: RosterEvent::Join, peer } if peer.id == "peer-b"
    )));
    assert_eq!(exchange.peers[b].session().peers().len(), 2);
}

#[test]
fn edits_travel_as_ori_edits() {
    let mut exchange = two_peers();
    let (a, b) = (0, 1);

    // A rename is a property.
    exchange.publish(
        a,
        "main",
        &timeline(&["c1", "c2", "c3"], &[]).replace("\"C2\"", "\"Two\""),
    );
    exchange.pump(4_000);
    assert_eq!(exchange.sent(a), ["OTIO_SESSION_1.0/SET_PROPERTY"]);
    exchange.same("main");

    // Any driver may edit: a reorder within a track is a move.
    let mut log = exchange.log.len();
    exchange.publish(
        b,
        "main",
        &timeline(&["c3", "c1", "c2"], &[]).replace("\"C2\"", "\"Two\""),
    );
    exchange.pump(4_100);
    assert!(
        exchange.log[log..]
            .iter()
            .all(|(_, kind)| kind == "OTIO_SESSION_1.0/MOVE_CHILD")
    );
    exchange.same("main");

    // ORI moves only within a parent: to another track is a removal and an
    // insert.
    log = exchange.log.len();
    exchange.publish(
        a,
        "main",
        &timeline(&["c3", "c2"], &["c1"]).replace("\"C2\"", "\"Two\""),
    );
    exchange.pump(4_200);
    let kinds: Vec<&str> = exchange.log[log..]
        .iter()
        .map(|(_, kind)| kind.as_str())
        .collect();
    assert!(
        kinds.ends_with(&[
            "OTIO_SESSION_1.0/REMOVE_CHILD",
            "OTIO_SESSION_1.0/INSERT_CHILD"
        ]),
        "{kinds:?}"
    );
    assert!(
        kinds
            .iter()
            .all(|kind| kind.starts_with("OTIO_SESSION_1.0/"))
    );
    exchange.same("main");

    // A track with clips in it would reach ORI's peers with its clips
    // unindexed, so it goes as the whole timeline.
    log = exchange.log.len();
    let with_track = timeline(&["c3", "c2"], &["c1"]).replace("\"C2\"", "\"Two\"").replace(
        r#"]}]}}"#,
        &format!(
            r#"]}},{{"OTIO_SCHEMA":"Track.1","name":"V3","kind":"Video","metadata":{{"sync":{{"guid":"main/v3"}}}},"children":[{}]}}]}}}}"#,
            clip("c4", "C4")
        ),
    );
    exchange.publish(b, "main", &with_track);
    exchange.pump(4_300);
    let kinds: Vec<&str> = exchange.log[log..]
        .iter()
        .map(|(_, kind)| kind.as_str())
        .collect();
    assert_eq!(kinds, ["TIMELINE_1.0/REPLACE_TIMELINE"]);
    exchange.same("main");

    let seen = exchange.events(a);
    assert!(
        seen.iter()
            .all(|event| matches!(event, Event::Timeline(change) if !change.patches.is_empty()))
    );
}

#[test]
fn playback_moves_the_playhead_and_switches_timelines() {
    let mut exchange = two_peers();
    let (a, b) = (0, 1);
    exchange.publish(
        a,
        "second",
        &timeline(&["s1"], &[]).replace("\"main", "\"second"),
    );
    exchange.pump(4_000);
    // Another timeline arrives without becoming the active one.
    assert_eq!(exchange.peers[b].session().active_sequence(), Some("main"));
    exchange.same("second");
    exchange.events(b);

    exchange.peers[a]
        .session_mut()
        .set_playhead(Playhead {
            sequence_id: "second".to_string(),
            frame: 12.0,
            playing: true,
            rate: 24.0,
        })
        .expect("sends");
    exchange.pump(4_100);
    assert_eq!(exchange.sent(a).last(), Some(&"PLAYBACK_SETTINGS_1.0/SET"));
    let seen = exchange.events(b);
    assert!(matches!(
        &seen[0],
        Event::Playhead { sender, playhead }
            if sender == "peer-a" && playhead.frame == 12.0 && playhead.playing && playhead.rate == 24.0
    ));
    assert!(
        matches!(&seen[1], Event::Timeline(change) if change.sequence_id == "second" && change.patches.is_empty())
    );
    assert_eq!(
        exchange.peers[b].session().active_sequence(),
        Some("second")
    );
}

#[test]
fn a_silent_master_is_replaced() {
    let mut exchange = two_peers();
    exchange.silent[0] = true;
    let c = exchange.add("peer-c", |_| {});
    exchange.pump(10_000);
    // B saw the question go unanswered, and C found no master.
    exchange.pump(12_001);
    assert!(exchange.peers[1].profile().is_master());
    assert!(exchange.peers[c].session().is_joined());
    // B forgets A once it has been silent long enough.
    exchange.pump(30_000);
    let seen = exchange.events(1);
    assert!(seen.iter().any(|event| matches!(
        event,
        Event::Roster { event: RosterEvent::Leave, peer } if peer.id == "peer-a"
    )));
}

#[test]
fn a_reconnecting_peer_joins_again() {
    let mut exchange = two_peers();
    exchange.publish(0, "main", &timeline(&["c2", "c1", "c3"], &[]));
    exchange.peers[1].disconnected();
    exchange.silent[1] = true;
    exchange.pump(5_000);
    exchange.silent[1] = false;
    exchange.peers[1].connect();
    exchange.pump(6_000);
    exchange.same("main");
    assert!(
        exchange
            .events(1)
            .iter()
            .any(|event| matches!(event, Event::Joined { rejoined: true, .. }))
    );
}

#[test]
fn large_snapshots_and_messages_compress_when_asked() {
    let mut exchange = Exchange::new();
    let a = exchange.add("peer-a", |options| {
        options.compress_snapshots = true;
        options.compress_messages = true;
    });
    exchange.pump(0);
    exchange.pump(2_000);
    let names: Vec<String> = (0..3_000).map(|index| format!("c{index:04}")).collect();
    let guids: Vec<&str> = names.iter().map(String::as_str).collect();
    let big = timeline(&guids, &[]).replace(
        "\"value\":24",
        &format!("\"value\":24,\"note\":\"{}\"", "x".repeat(300)),
    );
    assert!(big.len() > ori::COMPRESS_OVER);
    exchange.publish(a, "main", &big);
    exchange.pump(2_100);
    let b = exchange.add("peer-b", |_| {});
    exchange.pump(3_000);
    assert!(exchange.peers[b].session().is_joined());
    exchange.same("main");
}

#[test]
fn messages_from_other_sessions_and_itself_are_ignored() {
    let mut exchange = two_peers();
    let stray = Frame::text(
        r#"{"session":"another","source_guid":"x","payload":{"command_schema":"TIMELINE_1.0","command":{"event":"REMOVE_TIMELINE","payload":{"timeline_guid":"main"}}}}"#,
    );
    exchange.peers[1].receive(&stray).expect("receives");
    exchange.pump(4_000);
    assert!(exchange.peers[1].session().timeline("main").is_some());
    assert!(matches!(
        exchange.peers[1].receive(&Frame::text("not json")),
        Err(Error::Json(_))
    ));
}
