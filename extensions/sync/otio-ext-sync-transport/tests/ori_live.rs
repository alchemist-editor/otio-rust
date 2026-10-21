//! ORI sessions over a real RabbitMQ: Rust peers with each other, and with
//! ORI's own Python peer.
//!
//! Runs only when `OTIO_AMQP_URL` names a broker, such as
//! `amqp://guest:guest@localhost:5672/`; the tests with ORI's peer also need
//! `ORI_PYTHON`, the `python` directory of an ORIAnnotations checkout, with
//! `opentimelineio` and `pika` importable. CI sets both. Without them the
//! tests pass having done nothing.

#![cfg(feature = "amqp")]

use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde_json::{Value, json};

use otio_ext_sync::session::{Config, Identity, Session};
use otio_ext_sync_transport::amqp::Client;
use otio_ext_sync_transport::ori::Options;

const WAIT: Duration = Duration::from_secs(20);

/// A timeline with two video tracks, the first holding three clips.
fn timeline(names: [&str; 3], second_track: &[&str]) -> Value {
    let clip = |guid: &str, name: &str| {
        json!({"OTIO_SCHEMA": "Clip.2", "name": name, "metadata": {"sync": {"guid": guid}},
               "source_range": {"OTIO_SCHEMA": "TimeRange.1",
                 "start_time": {"OTIO_SCHEMA": "RationalTime.1", "value": 0.0, "rate": 24.0},
                 "duration": {"OTIO_SCHEMA": "RationalTime.1", "value": 24.0, "rate": 24.0}}})
    };
    let guids = ["c1", "c2", "c3"];
    let first: Vec<Value> = guids
        .iter()
        .zip(names)
        .filter(|(guid, _)| !second_track.contains(guid))
        .map(|(guid, name)| clip(guid, name))
        .collect();
    let second: Vec<Value> = guids
        .iter()
        .zip(names)
        .filter(|(guid, _)| second_track.contains(guid))
        .map(|(guid, name)| clip(guid, name))
        .collect();
    json!({"OTIO_SCHEMA": "Timeline.1", "name": "Program", "metadata": {"sync": {"guid": "main"}},
      "tracks": {"OTIO_SCHEMA": "Stack.1", "name": "tracks", "metadata": {"sync": {"guid": "stack"}}, "children": [
        {"OTIO_SCHEMA": "Track.1", "name": "V1", "kind": "Video", "metadata": {"sync": {"guid": "v1"}}, "children": first},
        {"OTIO_SCHEMA": "Track.1", "name": "V2", "kind": "Video", "metadata": {"sync": {"guid": "v2"}}, "children": second}]}})
}

fn broker() -> Option<String> {
    std::env::var("OTIO_AMQP_URL")
        .ok()
        .filter(|url| !url.is_empty())
}

fn ori_python() -> Option<String> {
    std::env::var("ORI_PYTHON")
        .ok()
        .filter(|path| !path.is_empty())
}

/// A session id no other run shares, since exchanges outlive their peers.
fn session_id(name: &str) -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    format!("otio-rust-{name}-{}-{nanos}", std::process::id())
}

fn connect(url: &str, session: &str, guid: &str) -> Client {
    connect_with(url, session, guid, |_| {})
}

fn connect_with(
    url: &str,
    session: &str,
    guid: &str,
    settings: impl FnOnce(&mut Options),
) -> Client {
    let mut options = Options::new(session);
    options.peer_guid = guid.to_string();
    settings(&mut options);
    let config = Config {
        token: String::new(),
        identity: Identity {
            display_name: guid.to_string(),
            app: "otio-rust".to_string(),
            ..Identity::default()
        },
    };
    Client::connect(url, Session::new(config), options).expect("connects to the broker")
}

/// Runs every client until `done` holds, failing after [`WAIT`].
fn pump(clients: &mut [&mut Client], what: &str, mut done: impl FnMut(&[&mut Client]) -> bool) {
    let deadline = Instant::now() + WAIT;
    while !done(clients) {
        assert!(Instant::now() < deadline, "timed out waiting until {what}");
        for client in clients.iter_mut() {
            while client
                .next_event(Duration::from_millis(10))
                .expect("runs")
                .is_some()
            {}
        }
    }
}

fn publish(client: &mut Client, timeline: &Value) {
    let mut document = otio_core::from_str(&timeline.to_string()).expect("the timeline reads");
    let root = document.root().expect("a root");
    let sequence = timeline["metadata"]["sync"]["guid"]
        .as_str()
        .expect("a guid")
        .to_string();
    client
        .session_mut()
        .publish(&mut document, root, &sequence)
        .expect("publishes");
}

/// A timeline as otio-rust writes it, every number a float.
fn canonical(timeline: &str) -> Value {
    let document = otio_core::from_str(timeline).expect("the timeline reads");
    numbers(serde_json::from_str(&otio_core::to_string(&document).expect("writes")).unwrap())
}

fn numbers(value: Value) -> Value {
    match value {
        Value::Number(number) => number
            .as_f64()
            .and_then(serde_json::Number::from_f64)
            .map_or(Value::Number(number), Value::Number),
        Value::Array(items) => Value::Array(items.into_iter().map(numbers).collect()),
        Value::Object(members) => Value::Object(
            members
                .into_iter()
                .map(|(key, value)| (key, numbers(value)))
                .collect(),
        ),
        other => other,
    }
}

fn ours(client: &Client, sequence: &str) -> Option<Value> {
    client
        .session()
        .timeline(sequence)
        .map(|snapshot| canonical(&snapshot.timeline))
}

/// ORI's own peer, in a child process.
struct OriPeer {
    child: Child,
    input: ChildStdin,
    output: BufReader<ChildStdout>,
}

impl OriPeer {
    fn start(ori_python: &str, url: &str, session: &str, guid: &str) -> Self {
        let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/ori/live_peer.py");
        let python = std::env::var("PYTHON").unwrap_or_else(|_| "python3".to_string());
        let mut child = Command::new(python)
            .arg(script)
            .args([ori_python, url, session, guid])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("ORI's peer starts");
        let input = child.stdin.take().unwrap();
        let output = BufReader::new(child.stdout.take().unwrap());
        Self {
            child,
            input,
            output,
        }
    }

    fn ask(&mut self, command: &Value) -> Value {
        writeln!(self.input, "{command}").expect("ORI's peer is listening");
        self.input.flush().unwrap();
        let mut line = String::new();
        self.output
            .read_line(&mut line)
            .expect("ORI's peer answers");
        assert!(!line.is_empty(), "ORI's peer exited");
        serde_json::from_str(&line).expect("JSON")
    }

    fn timeline(&mut self, sequence: &str) -> Option<Value> {
        let state = self.ask(&json!({"op": "state"}));
        state["timelines"].get(sequence).cloned().map(numbers)
    }
}

impl Drop for OriPeer {
    fn drop(&mut self) {
        let _ = writeln!(self.input, "{}", json!({"op": "quit"}));
        let _ = self.input.flush();
        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline {
            if matches!(self.child.try_wait(), Ok(Some(_))) {
                return;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        let _ = self.child.kill();
    }
}

/// Runs `client` until ORI's peer holds the same `sequence` it does.
fn converge(client: &mut Client, ori: &mut OriPeer, sequence: &str, what: &str) {
    let deadline = Instant::now() + WAIT;
    loop {
        while client
            .next_event(Duration::from_millis(100))
            .expect("runs")
            .is_some()
        {}
        let (rust, python) = (ours(client, sequence), ori.timeline(sequence));
        if rust.is_some() && rust == python {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "timed out waiting until {what}:\nours  {}\nORI's {}",
            rust.unwrap_or_default(),
            python.unwrap_or_default()
        );
    }
}

#[test]
fn rust_peers_converge_over_rabbitmq() {
    let Some(url) = broker() else {
        eprintln!("OTIO_AMQP_URL is not set; skipping");
        return;
    };
    let session = session_id("rust");
    let mut first = connect(&url, &session, "rust-first");
    pump(
        &mut [&mut first],
        "the first peer elects itself",
        |clients| clients[0].connection().profile().is_master(),
    );
    publish(&mut first, &timeline(["One", "Two", "Three"], &[]));

    let mut second = connect(&url, &session, "rust-second");
    pump(
        &mut [&mut first, &mut second],
        "the second peer joins",
        |clients| clients[1].session().is_joined() && ours(clients[1], "main").is_some(),
    );
    assert_eq!(second.connection().profile().master(), Some("rust-first"));
    assert_eq!(ours(&second, "main"), ours(&first, "main"));

    publish(&mut first, &timeline(["One", "Second", "Three"], &["c3"]));
    pump(
        &mut [&mut first, &mut second],
        "the second peer has the edit",
        |clients| ours(clients[1], "main") == ours(clients[0], "main"),
    );
    publish(
        &mut second,
        &timeline(["First", "Second", "Three"], &["c3"]),
    );
    pump(
        &mut [&mut first, &mut second],
        "the first peer has the edit",
        |clients| ours(clients[0], "main") == ours(clients[1], "main"),
    );
    assert_eq!(
        ours(&first, "main"),
        Some(canonical(
            &timeline(["First", "Second", "Three"], &["c3"]).to_string()
        ))
    );
    second.close().expect("leaves");
    first.close().expect("leaves");
}

#[test]
fn compressed_messages_cross_rabbitmq() {
    let Some(url) = broker() else {
        eprintln!("OTIO_AMQP_URL is not set; skipping");
        return;
    };
    let compress = |options: &mut Options| {
        options.compress_snapshots = true;
        options.compress_messages = true;
    };
    let session = session_id("compressed");
    let mut first = connect_with(&url, &session, "rust-first", compress);
    pump(
        &mut [&mut first],
        "the first peer elects itself",
        |clients| clients[0].connection().profile().is_master(),
    );
    // Notes well over the megabyte past which both compress.
    let mut large = timeline(["One", "Two", "Three"], &[]);
    large["metadata"]["notes"] = Value::from("frame by frame. ".repeat(100_000));
    publish(&mut first, &large);

    let mut second = connect_with(&url, &session, "rust-second", compress);
    pump(
        &mut [&mut first, &mut second],
        "the second peer takes the snapshot",
        |clients| ours(clients[1], "main").is_some(),
    );
    large["metadata"]["notes"] = Value::from("shot by shot. ".repeat(100_000));
    publish(&mut first, &large);
    pump(
        &mut [&mut first, &mut second],
        "the second peer takes the edit",
        |clients| ours(clients[1], "main") == ours(clients[0], "main"),
    );
    second.close().expect("leaves");
    first.close().expect("leaves");
}

#[test]
fn a_rust_peer_joins_oris_peer() {
    let (Some(url), Some(ori_python)) = (broker(), ori_python()) else {
        eprintln!("OTIO_AMQP_URL or ORI_PYTHON is not set; skipping");
        return;
    };
    let session = session_id("ori-master");
    let mut ori = OriPeer::start(&ori_python, &url, &session, "ori-peer");
    let deadline = Instant::now() + WAIT;
    while !ori.ask(&json!({"op": "status"}))["is_master"]
        .as_bool()
        .unwrap_or(false)
    {
        assert!(Instant::now() < deadline, "ORI's peer never elected itself");
        std::thread::sleep(Duration::from_millis(200));
    }
    // As otio-rust writes it, since upstream's reader wants every field.
    let loaded = canonical(&timeline(["One", "Two", "Three"], &[]).to_string());
    assert_eq!(
        ori.ask(&json!({"op": "load", "timeline": loaded}))["sent"],
        "SENT"
    );

    let mut rust = connect(&url, &session, "rust-peer");
    converge(
        &mut rust,
        &mut ori,
        "main",
        "the Rust peer takes ORI's timeline",
    );
    assert_eq!(rust.connection().profile().master(), Some("ori-peer"));

    // Ours, carried as ORI's SET_PROPERTY, MOVE_CHILD, REMOVE_CHILD and
    // INSERT_CHILD.
    publish(&mut rust, &timeline(["One", "Second", "Three"], &["c3"]));
    converge(&mut rust, &mut ori, "main", "ORI's peer takes our edit");
    assert_eq!(
        ori.timeline("main")
            .map(|timeline| timeline["tracks"]["children"][1]["children"][0]["name"].clone()),
        Some(json!("Three"))
    );

    // Theirs.
    ori.ask(&json!({"op": "set", "target": "c1", "path": "name", "value": "First"}));
    ori.ask(&json!({"op": "set", "target": "c2", "path": "metadata/review/status", "value": "approved"}));
    converge(&mut rust, &mut ori, "main", "we take ORI's edits");
    let edited = ours(&rust, "main").unwrap();
    assert_eq!(
        edited["tracks"]["children"][0]["children"][0]["name"],
        "First"
    );
    assert_eq!(
        edited["tracks"]["children"][0]["children"][1]["metadata"]["review"]["status"],
        "approved"
    );

    ori.ask(&json!({"op": "playback", "timeline": "main",
                    "state": {"playing": false, "current_time": {"value": 12.0, "rate": 24.0}}}));
    pump(&mut [&mut rust], "we take ORI's playback", |clients| {
        clients[0]
            .connection()
            .profile()
            .playback_state()
            .is_some_and(|state| {
                state
                    .get("current_time")
                    .is_some_and(|time| time["value"] == 12.0)
            })
    });
    rust.close().expect("leaves");
}

#[test]
fn oris_peer_joins_a_rust_peer() {
    let (Some(url), Some(ori_python)) = (broker(), ori_python()) else {
        eprintln!("OTIO_AMQP_URL or ORI_PYTHON is not set; skipping");
        return;
    };
    let session = session_id("rust-master");
    let mut rust = connect(&url, &session, "rust-peer");
    pump(&mut [&mut rust], "the Rust peer elects itself", |clients| {
        clients[0].connection().profile().is_master()
    });
    publish(&mut rust, &timeline(["One", "Two", "Three"], &[]));

    let mut ori = OriPeer::start(&ori_python, &url, &session, "ori-peer");
    converge(&mut rust, &mut ori, "main", "ORI's peer takes our timeline");
    assert_eq!(ori.ask(&json!({"op": "status"}))["master"], "rust-peer");

    publish(&mut rust, &timeline(["One", "Two", "Three"], &["c1"]));
    converge(&mut rust, &mut ori, "main", "ORI's peer takes our move");
    ori.ask(&json!({"op": "set", "target": "c3", "path": "name", "value": "Last"}));
    converge(&mut rust, &mut ori, "main", "we take ORI's rename");
    assert_eq!(
        ours(&rust, "main").unwrap()["tracks"]["children"][0]["children"][1]["name"],
        "Last"
    );
    rust.close().expect("leaves");
}
