//! Patches: the operations that turn one version of a timeline into the next.
//!
//! A port of the patch model in Furnace's `internal/otiodiff` (svc-furnace,
//! in Go). Every node a patch names is addressed by its `metadata.sync.guid`.
//!
//! # On the wire
//!
//! [`to_json`] writes a patch list exactly as Go's `json.Marshal` writes
//! Furnace's `[]Patch`, so the two programs exchange identical bytes: the
//! members in the order `op`, `parentGuid`, `index`, `node`, `guid`, `path`,
//! `value`, `removed`, `timeline`, each left out when Go's `omitempty` would
//! leave it out, strings escaped as Go escapes them, and the JSON a patch
//! carries compacted. [`from_json`] reads what either side wrote.
//!
//! The JSON a patch carries (a node, a value, a timeline) is kept as the text
//! it was cut from, as Go keeps a `json.RawMessage`, and is never re-encoded:
//! a number is written with the digits it arrived with, so a frame count of
//! any size survives a round trip through a patch.

use std::fmt;

use crate::raw;

/// What a patch does.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Op {
    /// Inserts [`Patch::node`] into the children of [`Patch::parent_guid`]
    /// at [`Patch::index`]. A node already in the timeline with a GUID the
    /// inserted subtree also holds is taken out of its old place, so the
    /// insert adopts it.
    InsertChild,
    /// Moves [`Patch::guid`] into the children of [`Patch::parent_guid`] at
    /// [`Patch::index`].
    MoveChild,
    /// Removes [`Patch::guid`] from the children of [`Patch::parent_guid`].
    RemoveChild,
    /// Sets, or with [`Patch::removed`] removes, the property at
    /// [`Patch::path`] of [`Patch::guid`].
    SetProperty,
    /// Replaces the whole timeline with [`Patch::timeline`]. It is always the
    /// only patch in its list.
    ReplaceTimeline,
    /// An operation this version does not know. It reads and writes like the
    /// others, so it can be passed along, but [`crate::apply()`] refuses it.
    Other(String),
}

impl Op {
    /// The operation's name on the wire, such as `insert_child`.
    #[must_use]
    pub fn as_str(&self) -> &str {
        match self {
            Self::InsertChild => "insert_child",
            Self::MoveChild => "move_child",
            Self::RemoveChild => "remove_child",
            Self::SetProperty => "set_property",
            Self::ReplaceTimeline => "replace_timeline",
            Self::Other(name) => name,
        }
    }

    /// The operation with this name on the wire.
    #[must_use]
    pub fn from_name(name: &str) -> Self {
        match name {
            "insert_child" => Self::InsertChild,
            "move_child" => Self::MoveChild,
            "remove_child" => Self::RemoveChild,
            "set_property" => Self::SetProperty,
            "replace_timeline" => Self::ReplaceTimeline,
            other => Self::Other(other.to_owned()),
        }
    }
}

impl Default for Op {
    /// The empty operation, which is what a patch read from JSON without an
    /// `op` has, as in Go.
    fn default() -> Self {
        Self::Other(String::new())
    }
}

impl fmt::Display for Op {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// One patch operation.
///
/// It has one shape for every [`Op`], as Furnace's has; each operation uses
/// the fields its documentation names and leaves the rest empty. An empty
/// string, `None` and `false` are what Go's `omitempty` leaves out, so they
/// are also what a member missing from the JSON reads as.
///
/// [`node`](Self::node), [`value`](Self::value) and
/// [`timeline`](Self::timeline) hold JSON text, written as given apart from
/// being compacted. Each must be valid JSON, as every patch [`crate::diff()`]
/// makes and [`from_json`] reads is.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Patch {
    /// What the patch does.
    pub op: Op,
    /// The GUID of the parent a child is inserted into, moved into or
    /// removed from.
    pub parent_guid: String,
    /// Where among the parent's children a child is inserted or moved to.
    pub index: Option<i64>,
    /// The subtree an insert adds, as JSON.
    pub node: Option<String>,
    /// The GUID of the node a move, remove or property change acts on.
    pub guid: String,
    /// The property a [`Op::SetProperty`] sets: a JSON Pointer of one token,
    /// such as `/name`, or two under `metadata`, such as `/metadata/color`.
    pub path: String,
    /// The value a [`Op::SetProperty`] sets, as JSON. `null` is a value: it
    /// sets the property to null rather than removing it.
    pub value: Option<String>,
    /// Whether a [`Op::SetProperty`] removes its property rather than
    /// setting it.
    pub removed: bool,
    /// The timeline a [`Op::ReplaceTimeline`] puts in place, as JSON.
    pub timeline: Option<String>,
}

/// A JSON field that is present, as Go's `len(raw) != 0` decides.
pub(crate) fn raw_present(raw: Option<&String>) -> Option<&str> {
    raw.map(String::as_str).filter(|text| !text.is_empty())
}

impl Patch {
    /// Writes this patch as compact JSON, exactly as Furnace writes it.
    #[must_use]
    pub fn to_json(&self) -> String {
        let mut out = String::new();
        self.write(&mut out);
        out
    }

    /// Reads one patch from JSON, as Furnace reads one.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Decode`] if `text` is not JSON, or if a member has
    /// the wrong type.
    pub fn from_json(text: &str) -> Result<Self, Error> {
        if !raw::is_valid(text) {
            return Err(Error::Decode("the patch is not valid JSON".into()));
        }
        read_patch(raw::trim(text))
    }

    fn write(&self, out: &mut String) {
        out.push_str("{\"op\":");
        raw::push_string(out, self.op.as_str());
        if !self.parent_guid.is_empty() {
            out.push_str(",\"parentGuid\":");
            raw::push_string(out, &self.parent_guid);
        }
        if let Some(index) = self.index {
            out.push_str(",\"index\":");
            out.push_str(&index.to_string());
        }
        if let Some(node) = raw_present(self.node.as_ref()) {
            out.push_str(",\"node\":");
            raw::compact_into(out, node);
        }
        if !self.guid.is_empty() {
            out.push_str(",\"guid\":");
            raw::push_string(out, &self.guid);
        }
        if !self.path.is_empty() {
            out.push_str(",\"path\":");
            raw::push_string(out, &self.path);
        }
        if let Some(value) = raw_present(self.value.as_ref()) {
            out.push_str(",\"value\":");
            raw::compact_into(out, value);
        }
        if self.removed {
            out.push_str(",\"removed\":true");
        }
        if let Some(timeline) = raw_present(self.timeline.as_ref()) {
            out.push_str(",\"timeline\":");
            raw::compact_into(out, timeline);
        }
        out.push('}');
    }
}

/// Writes a patch list as compact JSON, exactly as Go's `json.Marshal`
/// writes Furnace's `[]Patch`.
#[must_use]
pub fn to_json(patches: &[Patch]) -> String {
    let mut out = String::from("[");
    for (i, patch) in patches.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        patch.write(&mut out);
    }
    out.push(']');
    out
}

/// Writes a patch list as indented JSON, exactly as Go's
/// `json.MarshalIndent(patches, "", "  ")` does, which is how Furnace's
/// golden files are written.
#[must_use]
pub fn to_json_pretty(patches: &[Patch]) -> String {
    raw::indent(&to_json(patches))
}

/// Reads a patch list from JSON, as Go's `json.Unmarshal` reads Furnace's
/// `[]Patch`.
///
/// Members are matched to fields as Go matches them, ignoring case, and
/// members no field has are ignored. `null` reads as an empty list, and a
/// `null` element as an empty patch.
///
/// # Errors
///
/// Returns [`Error::Decode`] if `text` is not JSON, is not an array, or has
/// a member of the wrong type, such as an `index` that is not an integer.
pub fn from_json(text: &str) -> Result<Vec<Patch>, Error> {
    if !raw::is_valid(text) {
        return Err(Error::Decode("the patch list is not valid JSON".into()));
    }
    let text = raw::trim(text);
    if text == "null" {
        return Ok(Vec::new());
    }
    let elements = raw::elements(text)
        .ok_or_else(|| Error::Decode("the patch list is not an array".into()))?;
    elements.into_iter().map(read_patch).collect()
}

/// The Go field a member names: an exact match, or else a match ignoring
/// case, as `encoding/json` decides.
fn field_for(key: &str) -> Option<&'static str> {
    const FIELDS: [&str; 9] = [
        "op",
        "parentGuid",
        "index",
        "node",
        "guid",
        "path",
        "value",
        "removed",
        "timeline",
    ];
    if let Some(exact) = FIELDS.iter().find(|field| **field == key) {
        return Some(exact);
    }
    let folded = raw::fold_name(key);
    FIELDS
        .iter()
        .find(|field| raw::fold_name(field) == folded)
        .copied()
}

fn read_patch(text: &str) -> Result<Patch, Error> {
    let mut patch = Patch::default();
    if text == "null" {
        return Ok(patch);
    }
    let members =
        raw::members(text).ok_or_else(|| Error::Decode("a patch is not an object".into()))?;
    for (key, value) in members {
        let Some(field) = field_for(&key) else {
            continue;
        };
        let mismatch = |wanted: &str| {
            Error::Decode(format!(
                "the patch member {} is not {wanted}",
                raw::quote(field)
            ))
        };
        let first = value.as_bytes()[0];
        match field {
            "op" | "parentGuid" | "guid" | "path" => {
                if value == "null" {
                    continue;
                }
                if first != b'"' {
                    return Err(mismatch("a string"));
                }
                let decoded = raw::decode_string(value);
                match field {
                    "op" => patch.op = Op::from_name(&decoded),
                    "parentGuid" => patch.parent_guid = decoded,
                    "guid" => patch.guid = decoded,
                    _ => patch.path = decoded,
                }
            }
            "index" => {
                if value == "null" {
                    patch.index = None;
                } else if first == b'-' || first.is_ascii_digit() {
                    // Go reads an `int` with `strconv.ParseInt`, so a fraction
                    // or an exponent is refused rather than rounded.
                    patch.index = Some(
                        value
                            .parse::<i64>()
                            .map_err(|_| mismatch("an integer index"))?,
                    );
                } else {
                    return Err(mismatch("a number"));
                }
            }
            "removed" => match value {
                "true" => patch.removed = true,
                "false" => patch.removed = false,
                "null" => {}
                _ => return Err(mismatch("a boolean")),
            },
            "node" => patch.node = Some(value.to_owned()),
            "value" => patch.value = Some(value.to_owned()),
            _ => patch.timeline = Some(value.to_owned()),
        }
    }
    Ok(patch)
}

/// The two versions of a timeline that are diffed: one snapshot as Furnace
/// sends it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Snapshot {
    /// Which sequence this is a version of.
    pub sequence_id: String,
    /// How many versions of the sequence came before this one. A later
    /// version has a larger generation.
    pub generation: i64,
    /// The timeline, as OTIO JSON.
    pub timeline: String,
}

/// Limits past which a diff gives up on patches and sends the whole timeline.
///
/// A field left at zero takes its default, as in Furnace.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Options {
    /// The most patches a diff may make: 500 when zero.
    pub max_ops: usize,
    /// The most patches a diff may make, as a fraction of the nodes in the
    /// larger of the two timelines: 0.4 when zero or negative.
    pub max_ops_fraction: f64,
}

/// The default for [`Options::max_ops`].
pub const DEFAULT_MAX_OPS: usize = 500;

/// The default for [`Options::max_ops_fraction`].
pub const DEFAULT_MAX_OPS_FRACTION: f64 = 0.4;

/// What a diff did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Stats {
    /// How many [`Op::InsertChild`] patches it made.
    pub insert_ops: usize,
    /// How many [`Op::MoveChild`] patches it made.
    pub move_ops: usize,
    /// How many [`Op::RemoveChild`] patches it made.
    pub remove_ops: usize,
    /// How many [`Op::SetProperty`] patches it made.
    pub set_property_ops: usize,
    /// How many patches it made in all, before any fallback.
    pub total_ops: usize,
    /// How many nodes the larger of the two timelines has.
    pub node_count: usize,
    /// Whether it gave up on patches and sent the whole timeline in one
    /// [`Op::ReplaceTimeline`].
    pub fell_back: bool,
    /// Why it fell back, in Furnace's words, such as `generation regression`.
    pub fallback_reason: String,
}

/// Why a diff, an apply or a read of patches failed.
///
/// A diff fails only when a timeline is not JSON at all; every other problem
/// makes it fall back to [`Op::ReplaceTimeline`] instead, and [`Stats`] says
/// why. The messages are Furnace's.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Error {
    /// Text that has to be JSON is not. It names the text, such as
    /// `old timeline`.
    InvalidJson(&'static str),
    /// A timeline holds something sync cannot address, such as a node with
    /// no GUID. It says what, in the words a fallback would give.
    Unaddressable(String),
    /// A [`Op::ReplaceTimeline`] patch came with other patches.
    ReplaceNotAlone,
    /// A [`Op::ReplaceTimeline`] patch has no timeline, or one that is not
    /// JSON.
    ReplaceInvalid,
    /// The timeline patches were applied to could not be read as a
    /// sync-addressable timeline.
    OldTimeline(String),
    /// One patch could not be applied.
    Patch {
        /// Where the patch is in its list, from zero.
        index: usize,
        /// The patch's operation.
        op: String,
        /// What was wrong with it.
        message: String,
    },
    /// Patches could not be read from JSON.
    Decode(String),
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidJson(what) => write!(formatter, "{what} is not valid JSON"),
            Self::Unaddressable(reason) => formatter.write_str(reason),
            Self::ReplaceNotAlone => formatter.write_str("replace_timeline must be the sole patch"),
            Self::ReplaceInvalid => {
                formatter.write_str("replace_timeline requires a valid timeline")
            }
            Self::OldTimeline(reason) => write!(formatter, "parse old timeline: {reason}"),
            Self::Patch { index, op, message } => {
                write!(formatter, "patch {index} ({op}): {message}")
            }
            Self::Decode(message) => write!(formatter, "cannot read patches: {message}"),
        }
    }
}

impl std::error::Error for Error {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_go_field_names_and_omits_empty_members() {
        let patch = Patch {
            op: Op::MoveChild,
            parent_guid: "a/b".into(),
            index: Some(0),
            guid: "c<d>".into(),
            ..Patch::default()
        };
        assert_eq!(
            patch.to_json(),
            r#"{"op":"move_child","parentGuid":"a/b","index":0,"guid":"c\u003cd\u003e"}"#
        );
        let removal = Patch {
            op: Op::SetProperty,
            guid: "g".into(),
            path: "/name".into(),
            removed: true,
            ..Patch::default()
        };
        assert_eq!(
            removal.to_json(),
            r#"{"op":"set_property","guid":"g","path":"/name","removed":true}"#
        );
        assert_eq!(to_json(&[]), "[]");
        assert_eq!(to_json_pretty(&[]), "[]");
    }

    #[test]
    fn reads_what_it_writes() {
        let patches = vec![
            Patch {
                op: Op::InsertChild,
                parent_guid: "t".into(),
                index: Some(2),
                node: Some(r#"{"b": 1, "a": 18446744073709551617}"#.into()),
                ..Patch::default()
            },
            Patch {
                op: Op::SetProperty,
                guid: "g".into(),
                path: "/metadata/x".into(),
                value: Some("null".into()),
                ..Patch::default()
            },
        ];
        let text = to_json(&patches);
        assert!(text.contains(r#""node":{"b":1,"a":18446744073709551617}"#));
        let read = from_json(&text).unwrap();
        assert_eq!(to_json(&read), text);
        assert_eq!(read[1].value.as_deref(), Some("null"));
    }

    #[test]
    fn reads_as_go_does() {
        let read =
            from_json(r#"[{"OP":"wat","Index":3,"removed":null,"extra":[1]}, null]"#).unwrap();
        assert_eq!(read[0].op, Op::Other("wat".into()));
        assert_eq!(read[0].index, Some(3));
        assert_eq!(read[1], Patch::default());
        assert!(from_json("null").unwrap().is_empty());
        assert!(from_json(r#"[{"index":1.5}]"#).is_err());
        assert!(from_json(r#"[{"op":1}]"#).is_err());
        assert!(from_json(r#"{"op":"x"}"#).is_err());
        assert!(from_json("not patches").is_err());
    }
}
