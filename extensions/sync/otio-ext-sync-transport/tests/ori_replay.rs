//! ORI's recordings, replayed into the ORI profile, end where ORI's own
//! headless peer ends.
//!
//! `fixtures/ori/recordings` are ORI's `sync_test` recordings, and
//! `fixtures/ori/expected` is what `fixtures/ori/reference.py` makes ORI's
//! `SyncManager` of each: its timelines, active timeline, playback state
//! and display state. Each recording is replayed here as ORI's player
//! replays it: the player answers the joiner as master, sends the first
//! recorded `STATE_SNAPSHOT` to the joiner, then every message that is not a
//! `LiveSession.1` one, in order. CI checks the expected files against ORI
//! itself when it can check ORI out.

use std::path::{Path, PathBuf};

use serde_json::Value;

use otio_ext_sync::session::{Config, Session};
use otio_ext_sync_transport::ori::{Options, Ori};
use otio_ext_sync_transport::{Connection, Frame};

const SELF: &str = "otio-rust-replay";
const PLAYER: &str = "ori-recording-player";
const SESSION: &str = "replay";

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/ori")
}

/// The first snapshot and the replayed messages, as ORI's player reads them.
fn load(path: &Path) -> (Value, Vec<Value>) {
    let text = std::fs::read_to_string(path).expect("reads the recording");
    let mut snapshot = None;
    let mut events = Vec::new();
    for line in text.lines().filter(|line| !line.trim().is_empty()) {
        let record: Value = serde_json::from_str(line).expect("a JSON line");
        let envelope = record["payload"].clone();
        if envelope["payload"]["command_schema"] == "LiveSession.1" {
            if envelope["payload"]["command"]["event"] == "STATE_SNAPSHOT" && snapshot.is_none() {
                snapshot = Some(envelope);
            }
            continue;
        }
        events.push(envelope);
    }
    (snapshot.expect("a snapshot"), events)
}

fn from_player(mut envelope: Value) -> Frame {
    envelope["source_guid"] = Value::from(PLAYER);
    envelope["session"] = Value::from(SESSION);
    Frame::text(envelope.to_string())
}

fn drain(connection: &mut Connection<Ori>) {
    while connection.poll_transmit(0).expect("sends").is_some() {}
}

/// A timeline as otio-rust reads and writes it, as the reference has it as
/// upstream OpenTimelineIO reads and writes it.
fn canonical(timeline: &str) -> Value {
    let document = otio_core::from_str(timeline).expect("the timeline reads");
    numbers(serde_json::from_str(&otio_core::to_string(&document).expect("writes")).unwrap())
}

/// `value` with every number as a float, since Python writes `24` where
/// Rust writes `24.0`.
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

/// Where `ours` and `theirs` first differ, as a JSON Pointer with both
/// values, if they do.
fn first_difference(ours: &Value, theirs: &Value, at: String) -> Option<String> {
    match (ours, theirs) {
        (Value::Object(a), Value::Object(b)) => {
            let keys: std::collections::BTreeSet<&String> = a.keys().chain(b.keys()).collect();
            keys.into_iter().find_map(|key| {
                first_difference(
                    a.get(key).unwrap_or(&Value::Null),
                    b.get(key).unwrap_or(&Value::Null),
                    format!("{at}/{key}"),
                )
                .or_else(|| {
                    (a.contains_key(key) != b.contains_key(key))
                        .then(|| format!("{at}/{key}: present in only one"))
                })
            })
        }
        (Value::Array(a), Value::Array(b)) if a.len() == b.len() => a
            .iter()
            .zip(b)
            .enumerate()
            .find_map(|(index, (a, b))| first_difference(a, b, format!("{at}/{index}"))),
        _ if ours == theirs => None,
        // Upstream OpenTimelineIO's JSON reader can land a float one unit in
        // the last place from the text, where otio-rust's reads it exactly.
        (Value::Number(a), Value::Number(b))
            if a.as_f64().zip(b.as_f64()).is_some_and(|(a, b)| {
                (a - b).abs() <= 4.0 * f64::EPSILON * a.abs().max(b.abs())
            }) =>
        {
            None
        }
        _ => {
            let short = |value: &Value| value.to_string().chars().take(300).collect::<String>();
            Some(format!(
                "{at}: ours {}, ORI's {}",
                short(ours),
                short(theirs)
            ))
        }
    }
}

fn without_timestamp(state: Option<Value>) -> Value {
    match state {
        Some(Value::Object(mut members)) => {
            members.remove("sync_timestamp");
            numbers(Value::Object(members))
        }
        Some(other) => numbers(other),
        None => Value::Null,
    }
}

/// Replays `recording`, returning what differs from ORI's end state.
fn replay(recording: &Path) -> Vec<String> {
    let name = recording
        .file_stem()
        .unwrap()
        .to_string_lossy()
        .into_owned();
    let expected: Value = serde_json::from_str(
        &std::fs::read_to_string(fixtures().join("expected").join(format!("{name}.json")))
            .expect("an expected end state"),
    )
    .expect("JSON");
    let (mut snapshot, events) = load(recording);

    let mut options = Options::new(SESSION);
    options.peer_guid = SELF.to_string();
    let mut connection = Connection::new(Session::new(Config::default()), Ori::new(options));
    connection.connect();
    drain(&mut connection);
    let master = serde_json::json!({
        "payload": {"command_schema": "LiveSession.1",
                    "command": {"event": "I_AM_MASTER", "payload": {"master_guid": PLAYER}}},
        "schema": "SYNC_REVIEW_1.0",
    });
    connection.receive(&from_player(master)).expect("receives");
    drain(&mut connection);
    snapshot["payload"]["command"]["payload"]["target_guid"] = Value::from(SELF);
    connection
        .receive(&from_player(snapshot))
        .expect("receives");
    drain(&mut connection);
    assert!(connection.session().is_joined(), "{name}: joined");
    for event in events {
        connection.receive(&from_player(event)).expect("receives");
        drain(&mut connection);
    }

    let mut differences = Vec::new();
    let session = connection.session();
    let theirs = expected["timelines"].as_object().expect("timelines");
    let ours: Vec<&str> = session.sequences().collect();
    let their_guids: Vec<&str> = theirs.keys().map(String::as_str).collect();
    if ours != their_guids {
        differences.push(format!("timelines {ours:?}, ORI has {their_guids:?}"));
    }
    for (guid, timeline) in theirs {
        if let Some(snapshot) = session.timeline(guid) {
            let ours = canonical(&snapshot.timeline);
            let theirs = numbers(timeline.clone());
            if let Some(difference) = first_difference(&ours, &theirs, String::new()) {
                differences.push(format!("timeline {guid} differs at {difference}"));
            }
        }
    }
    if let Some(active) = expected["active_timeline_guid"].as_str() {
        if session.active_sequence() != Some(active) {
            differences.push(format!(
                "active timeline {:?}, ORI's {active}",
                session.active_sequence()
            ));
        }
    }
    let profile = connection.profile();
    let playback = without_timestamp(profile.playback_state().cloned().map(Value::Object));
    if playback != numbers(expected["playback_state"].clone()) {
        differences.push(format!(
            "playback {playback}, ORI's {}",
            expected["playback_state"]
        ));
    }
    let display = without_timestamp(profile.display_state().cloned());
    if display != numbers(expected["display_state"].clone()) {
        differences.push(format!(
            "display {display}, ORI's {}",
            expected["display_state"]
        ));
    }
    differences
        .into_iter()
        .map(|difference| format!("{name}: {difference}"))
        .collect()
}

#[test]
fn recordings_replay_to_oris_end_state() {
    let mut recordings: Vec<PathBuf> = std::fs::read_dir(fixtures().join("recordings"))
        .expect("the recordings")
        .map(|entry| entry.unwrap().path())
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension == "jsonl")
        })
        .collect();
    recordings.sort();
    assert!(recordings.len() >= 20, "the recordings are vendored");
    let differences: Vec<String> = recordings.iter().flat_map(|path| replay(path)).collect();
    assert!(differences.is_empty(), "{}", differences.join("\n\n"));
}
