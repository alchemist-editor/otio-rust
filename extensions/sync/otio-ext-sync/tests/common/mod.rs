//! Timelines for the tests, built as Furnace's Go tests build theirs.

#![allow(dead_code)]

use std::collections::BTreeMap;

use otio_ext_sync::{Op, Patch};

/// A JSON value, written as Go's `json.Marshal` writes a `map[string]any`:
/// object members in byte order of their names.
#[derive(Debug, Clone)]
pub enum J {
    Bool(bool),
    Int(i64),
    Str(String),
    Array(Vec<J>),
    Object(BTreeMap<String, J>),
}

impl J {
    pub fn object<const N: usize>(members: [(&str, J); N]) -> Self {
        Self::Object(
            members
                .into_iter()
                .map(|(key, value)| (key.to_owned(), value))
                .collect(),
        )
    }

    pub fn str(value: impl Into<String>) -> Self {
        Self::Str(value.into())
    }

    pub fn members(&mut self) -> &mut BTreeMap<String, J> {
        match self {
            Self::Object(members) => members,
            _ => panic!("not an object"),
        }
    }

    pub fn to_json(&self) -> String {
        let mut out = String::new();
        self.write(&mut out);
        out
    }

    fn write(&self, out: &mut String) {
        match self {
            Self::Bool(value) => out.push_str(if *value { "true" } else { "false" }),
            Self::Int(value) => out.push_str(&value.to_string()),
            Self::Str(value) => out.push_str(&otio_core::json::escape(value)),
            Self::Array(values) => {
                out.push('[');
                for (i, value) in values.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    value.write(out);
                }
                out.push(']');
            }
            Self::Object(members) => {
                out.push('{');
                for (i, (key, value)) in members.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    out.push_str(&otio_core::json::escape(key));
                    out.push(':');
                    value.write(out);
                }
                out.push('}');
            }
        }
    }
}

/// A node with a schema and a sync GUID, plus `fields`.
pub fn synthetic_node<const N: usize>(schema: &str, guid: &str, fields: [(&str, J); N]) -> J {
    let mut node = J::object([
        ("OTIO_SCHEMA", J::str(schema)),
        (
            "metadata",
            J::object([("sync", J::object([("guid", J::str(guid))]))]),
        ),
    ]);
    for (key, value) in fields {
        node.members().insert(key.to_owned(), value);
    }
    node
}

#[derive(Debug, Clone, Default)]
pub struct SyntheticTrack {
    pub id: i64,
    pub clips: Vec<i64>,
}

/// The state Furnace's property test edits: tracks of clips, with names and
/// metadata namespaces.
#[derive(Debug, Clone, Default)]
pub struct SyntheticState {
    pub tracks: Vec<SyntheticTrack>,
    pub names: BTreeMap<i64, String>,
    pub metadata: BTreeMap<i64, BTreeMap<String, J>>,
    pub next_track_id: i64,
    pub next_clip_id: i64,
    pub leading_gap_on: bool,
    pub inter_gaps_on: bool,
}

/// Renders a state as a timeline, optionally with a problem injected:
/// `guidless` appends a Transition, `duplicate` repeats a clip.
pub fn render_synthetic_timeline(
    state: &SyntheticState,
    sequence_id: &str,
    injection: &str,
) -> String {
    let mut track_nodes = Vec::new();
    for track in &state.tracks {
        let mut children = Vec::new();
        if state.leading_gap_on && !track.clips.is_empty() {
            children.push(synthetic_node(
                "Gap.1",
                &format!("gap-head:{sequence_id}/trk-{}", track.id),
                [("name", J::str("head gap"))],
            ));
        }
        for (i, &id) in track.clips.iter().enumerate() {
            let mut metadata = J::object([(
                "sync",
                J::object([("guid", J::str(format!("{sequence_id}/clip-{id}")))]),
            )]);
            for (key, value) in state.metadata.get(&id).into_iter().flatten() {
                metadata.members().insert(key.clone(), value.clone());
            }
            children.push(J::object([
                ("OTIO_SCHEMA", J::str("Clip.2")),
                ("metadata", metadata),
                (
                    "name",
                    J::str(state.names.get(&id).cloned().unwrap_or_default()),
                ),
                (
                    "source_range",
                    J::object([
                        (
                            "duration",
                            J::object([
                                ("rate", J::Int(24)),
                                ("value", J::Int(id * 1_000_000_000_000 + 48)),
                            ]),
                        ),
                        (
                            "start_time",
                            J::object([
                                ("rate", J::Int(24)),
                                ("value", J::Int(id * 1_000_000_000_000)),
                            ]),
                        ),
                    ]),
                ),
            ]));
            if state.inter_gaps_on && i + 1 < track.clips.len() {
                children.push(synthetic_node(
                    "Gap.1",
                    &format!("gap-after:{sequence_id}/clip-{id}"),
                    [("name", J::str("between gap"))],
                ));
            }
        }
        track_nodes.push(synthetic_node(
            "Track.1",
            &format!("{sequence_id}/trk-{}", track.id),
            [
                ("children", J::Array(children)),
                ("name", J::str(format!("track-{}", track.id))),
            ],
        ));
    }
    if track_nodes.is_empty() {
        track_nodes.push(synthetic_node(
            "Track.1",
            &format!("{sequence_id}/trk-1"),
            [("children", J::Array(Vec::new()))],
        ));
    }
    let first_children = match track_nodes[0].members().get_mut("children") {
        Some(J::Array(children)) => children,
        _ => unreachable!(),
    };
    if injection == "guidless" {
        first_children.push(J::object([
            ("OTIO_SCHEMA", J::str("Transition.1")),
            ("name", J::str("crossfade")),
        ]));
    }
    if injection == "duplicate" {
        let existing = first_children.iter().find(|child| match child {
            J::Object(members) => {
                matches!(members.get("OTIO_SCHEMA"), Some(J::Str(schema)) if schema.starts_with("Clip."))
            }
            _ => false,
        });
        let duplicate = match existing {
            Some(clip) => clip.clone(),
            None => {
                let clip = synthetic_node(
                    "Clip.2",
                    &format!("{sequence_id}/clip-999999"),
                    [("name", J::str("duplicate"))],
                );
                first_children.push(clip.clone());
                clip
            }
        };
        first_children.push(duplicate);
    }
    synthetic_node(
        "Timeline.1",
        sequence_id,
        [
            ("name", J::str(sequence_id)),
            (
                "tracks",
                synthetic_node(
                    "Stack.1",
                    &format!("{sequence_id}/stack"),
                    [("children", J::Array(track_nodes))],
                ),
            ),
        ],
    )
    .to_json()
}

/// One track of bare clips in `order`, written as Furnace's benchmarks write
/// it.
pub fn render_benchmark_timeline(order: &[i64]) -> String {
    let mut out = String::from(
        r#"{"OTIO_SCHEMA":"Timeline.1","metadata":{"sync":{"guid":"main"}},"tracks":{"OTIO_SCHEMA":"Stack.1","metadata":{"sync":{"guid":"main/stack"}},"children":[{"OTIO_SCHEMA":"Track.1","metadata":{"sync":{"guid":"main/trk-1"}},"children":["#,
    );
    for (i, id) in order.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        out.push_str(r#"{"OTIO_SCHEMA":"Clip.2","metadata":{"sync":{"guid":"main/clip-"#);
        out.push_str(&id.to_string());
        out.push_str(r#""}}}"#);
    }
    out.push_str("]}]}}");
    out
}

pub fn count_ops(patches: &[Patch], op: &Op) -> usize {
    patches.iter().filter(|patch| patch.op == *op).count()
}

/// SplitMix64: a small, seeded, deterministic generator, standing in for the
/// `rapid` draws of Furnace's property tests.
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        Self(seed)
    }

    pub fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// A number from `low` to `high`, both included.
    pub fn range(&mut self, low: usize, high: usize) -> usize {
        low + (self.next_u64() % (high - low + 1) as u64) as usize
    }

    pub fn bool(&mut self) -> bool {
        self.next_u64() & 1 == 1
    }
}
