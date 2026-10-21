//! Timelines as JSON values: finding objects by GUID and changing them the
//! way ORI's patcher does, or the way this crate's patches do.

use serde_json::{Map, Value};

use otio_ext_sync::patch::{Op, Patch};

/// The GUID an OTIO object carries in `metadata.sync.guid`, as ORI and sync
/// both keep it.
pub(super) fn guid_of(node: &Value) -> Option<&str> {
    node.get("metadata")?.get("sync")?.get("guid")?.as_str()
}

/// Whether `node`'s schema is `name`, at any version: `Track` matches
/// `Track.1`.
pub(super) fn is_schema(node: &Value, name: &str) -> bool {
    node.get("OTIO_SCHEMA")
        .and_then(Value::as_str)
        .and_then(|schema| schema.split_once('.'))
        .is_some_and(|(schema, _)| schema == name)
}

/// Where an object is in a timeline: a JSON Pointer to it, and to the
/// object whose `children` hold it with its place there, unless it is the
/// timeline or the timeline's stack.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Location {
    pub(super) pointer: String,
    pub(super) parent: Option<(String, usize)>,
}

/// Finds the object whose GUID is `guid` in the timeline `root`, walking it
/// as ORI does: the timeline, its stack, and the children of compositions.
pub(super) fn locate(root: &Value, guid: &str) -> Option<Location> {
    let mut stack = vec![(root, String::new(), None::<(String, usize)>)];
    while let Some((node, pointer, parent)) = stack.pop() {
        if guid_of(node) == Some(guid) {
            return Some(Location { pointer, parent });
        }
        if let Some(tracks) = node.get("tracks").filter(|tracks| tracks.is_object()) {
            stack.push((tracks, format!("{pointer}/tracks"), None));
        }
        if let Some(children) = node.get("children").and_then(Value::as_array) {
            for (index, child) in children.iter().enumerate().rev() {
                stack.push((
                    child,
                    format!("{pointer}/children/{index}"),
                    Some((pointer.clone(), index)),
                ));
            }
        }
    }
    None
}

/// The children of the object at `pointer`, if it has any to hold.
fn children_mut<'a>(root: &'a mut Value, pointer: &str) -> Option<&'a mut Vec<Value>> {
    root.pointer_mut(pointer)?
        .get_mut("children")?
        .as_array_mut()
}

/// Where `child` is among the children of the object at `pointer`.
fn child_index(root: &Value, pointer: &str, child: &str) -> Option<usize> {
    root.pointer(pointer)?
        .get("children")?
        .as_array()?
        .iter()
        .position(|item| guid_of(item) == Some(child))
}

/// Python's `list.insert`: a negative index counts from the end and an index
/// past either end is clamped to it.
fn python_insert(children: &mut Vec<Value>, index: i64, value: Value) {
    let len = i64::try_from(children.len()).unwrap_or(i64::MAX);
    let at = if index < 0 {
        (len + index).max(0)
    } else {
        index.min(len)
    };
    children.insert(usize::try_from(at).unwrap_or(0), value);
}

/// Gives `node` a fresh GUID if it has none, as ORI's patcher does with an
/// inserted child.
fn ensure_guid(node: &mut Value) {
    if guid_of(node).is_some() {
        return;
    }
    let Some(object) = node.as_object_mut() else {
        return;
    };
    let metadata = object
        .entry("metadata")
        .or_insert_with(|| Value::Object(Map::new()));
    if !metadata.is_object() {
        *metadata = Value::Object(Map::new());
    }
    let sync = metadata
        .as_object_mut()
        .expect("made an object above")
        .entry("sync")
        .or_insert_with(|| Value::Object(Map::new()));
    if !sync.is_object() {
        *sync = Value::Object(Map::new());
    }
    sync.as_object_mut().expect("made an object above").insert(
        "guid".to_string(),
        Value::String(otio_ext_sync::guid::new_guid()),
    );
}

/// Python's truth: what `if value:` accepts.
fn truthy(value: Option<&Value>) -> bool {
    match value {
        None | Some(Value::Null) => false,
        Some(Value::Bool(flag)) => *flag,
        Some(Value::Number(number)) => number.as_f64().is_some_and(|number| number != 0.0),
        Some(Value::String(text)) => !text.is_empty(),
        Some(Value::Array(items)) => !items.is_empty(),
        Some(Value::Object(members)) => !members.is_empty(),
    }
}

/// The frame a clip's `source_range` starts at, truncated as Python's
/// `int()` does.
fn start_frame(node: &Value) -> Option<i64> {
    let value = node
        .get("source_range")?
        .get("start_time")?
        .get("value")?
        .as_f64()?;
    #[allow(clippy::cast_possible_truncation)]
    Some(value.trunc() as i64)
}

/// ORI's annotation merge: an annotation clip inserted into a track that
/// already holds one for the same clip and frame adds its commands to that
/// one instead. Returns whether it merged.
fn merge_annotation(root: &mut Value, parent: &str, child: &Value) -> bool {
    let Some(parent_node) = root.pointer(parent) else {
        return false;
    };
    if !is_schema(parent_node, "Track") {
        return false;
    }
    let metadata = child.get("metadata");
    let incoming = metadata.and_then(|metadata| metadata.get("annotation_commands"));
    let clip_guid = metadata.and_then(|metadata| metadata.get("clip_guid"));
    if !truthy(incoming) || !truthy(clip_guid) {
        return false;
    }
    let Some(frame) = child
        .get("source_range")
        .filter(|range| !range.is_null())
        .and_then(|_| start_frame(child))
    else {
        return false;
    };
    let Some(existing) = parent_node
        .get("children")
        .and_then(Value::as_array)
        .and_then(|children| {
            children.iter().position(|item| {
                is_schema(item, "Clip")
                    && item.get("metadata").and_then(|m| m.get("clip_guid")) == clip_guid
                    && start_frame(item) == Some(frame)
            })
        })
    else {
        return false;
    };
    let pointer = format!("{parent}/children/{existing}/metadata/annotation_commands");
    let Some(Value::Array(commands)) = root.pointer_mut(&pointer) else {
        return false;
    };
    if let Some(Value::Array(incoming)) = incoming {
        commands.extend(incoming.iter().cloned());
    }
    true
}

/// One of ORI's `OTIO_SESSION_1.0` mutations.
pub(super) enum OriEdit<'a> {
    SetProperty {
        target: &'a str,
        path: &'a str,
        value: Value,
    },
    InsertChild {
        parent: &'a str,
        index: i64,
        child: Value,
    },
    MoveChild {
        parent: &'a str,
        child: &'a str,
        to_index: i64,
    },
    RemoveChild {
        parent: &'a str,
        child: &'a str,
    },
    ReplaceAnnotationCommands {
        clip: &'a str,
        commands: Value,
    },
}

impl OriEdit<'_> {
    /// The GUID the edit has to find to do anything.
    pub(super) fn anchor(&self) -> &str {
        match self {
            Self::SetProperty { target, .. } => target,
            Self::InsertChild { parent, .. }
            | Self::MoveChild { parent, .. }
            | Self::RemoveChild { parent, .. } => parent,
            Self::ReplaceAnnotationCommands { clip, .. } => clip,
        }
    }
}

/// Applies `edit` to the timeline `root` as ORI's patcher does, including
/// what it leaves alone: an edit whose objects are not there changes
/// nothing. Returns whether the timeline changed.
pub(super) fn apply_ori(root: &mut Value, edit: OriEdit<'_>) -> bool {
    let Some(anchor) = locate(root, edit.anchor()) else {
        return false;
    };
    let pointer = anchor.pointer;
    match edit {
        OriEdit::SetProperty { path, value, .. } => {
            let Some(node) = root.pointer_mut(&pointer).and_then(Value::as_object_mut) else {
                return false;
            };
            if let Some(rest) = path.strip_prefix("metadata/") {
                let parts: Vec<&str> = rest.split('/').collect();
                let metadata = node
                    .entry("metadata")
                    .or_insert_with(|| Value::Object(Map::new()));
                let mut current = metadata;
                for part in &parts[..parts.len() - 1] {
                    let Some(object) = current.as_object_mut() else {
                        return false;
                    };
                    current = object
                        .entry((*part).to_string())
                        .or_insert_with(|| Value::Object(Map::new()));
                }
                let Some(object) = current.as_object_mut() else {
                    return false;
                };
                object.insert(parts[parts.len() - 1].to_string(), value);
            } else {
                node.insert(path.to_string(), value);
            }
            true
        }
        OriEdit::InsertChild {
            index, mut child, ..
        } => {
            if merge_annotation(root, &pointer, &child) {
                ensure_guid(&mut child);
                return true;
            }
            let Some(children) = children_mut(root, &pointer) else {
                return false;
            };
            ensure_guid(&mut child);
            if index == -1 {
                children.push(child);
            } else {
                python_insert(children, index, child);
            }
            true
        }
        OriEdit::MoveChild {
            child, to_index, ..
        } => {
            let Some(current) = child_index(root, &pointer, child) else {
                return false;
            };
            let children = children_mut(root, &pointer).expect("found the child in it");
            let moving = children.remove(current);
            python_insert(children, to_index, moving);
            true
        }
        OriEdit::RemoveChild { child, .. } => {
            let Some(current) = child_index(root, &pointer, child) else {
                return false;
            };
            children_mut(root, &pointer)
                .expect("found the child in it")
                .remove(current);
            true
        }
        OriEdit::ReplaceAnnotationCommands { commands, .. } => {
            let Some(node) = root.pointer_mut(&pointer).and_then(Value::as_object_mut) else {
                return false;
            };
            let metadata = node
                .entry("metadata")
                .or_insert_with(|| Value::Object(Map::new()));
            let Some(metadata) = metadata.as_object_mut() else {
                return false;
            };
            metadata.insert("annotation_commands".to_string(), commands);
            true
        }
    }
}

/// Whether `node` holds children of its own: a composition that is not
/// empty, whose descendants ORI would not index after inserting it.
pub(super) fn has_descendants(node: &Value) -> bool {
    node.get("children")
        .and_then(Value::as_array)
        .is_some_and(|children| !children.is_empty())
        || node.get("tracks").is_some_and(Value::is_object)
}

/// Reads one JSON Pointer token: `~1` is `/` and `~0` is `~`.
pub(super) fn unescape(token: &str) -> String {
    token.replace("~1", "/").replace("~0", "~")
}

/// Applies one of this crate's patches to `root` as [`otio_ext_sync::apply`]
/// would, so a profile can follow a list of them. Returns `None` if the
/// patch does not fit.
pub(super) fn apply_patch(root: &mut Value, patch: &Patch) -> Option<()> {
    match &patch.op {
        Op::InsertChild => {
            let parent = locate(root, &patch.parent_guid)?.pointer;
            let node: Value = serde_json::from_str(patch.node.as_deref()?).ok()?;
            let children = children_mut(root, &parent)?;
            let index = usize::try_from(patch.index?).ok()?;
            if index > children.len() {
                return None;
            }
            children.insert(index, node);
            Some(())
        }
        Op::MoveChild => {
            let destination = locate(root, &patch.parent_guid)?.pointer;
            let moving = locate(root, &patch.guid)?;
            let (source, from) = moving.parent?;
            let len = root
                .pointer(&destination)?
                .get("children")?
                .as_array()?
                .len();
            let mut index = usize::try_from(patch.index?).ok()?;
            if index > len {
                return None;
            }
            if source == destination && index == len {
                index -= 1;
            }
            let node = children_mut(root, &source)?.remove(from);
            // Taking the node out may have shifted the destination's own
            // place, so it is found again.
            let destination = locate(root, &patch.parent_guid)?.pointer;
            let children = children_mut(root, &destination)?;
            if index > children.len() {
                return None;
            }
            children.insert(index, node);
            Some(())
        }
        Op::RemoveChild => {
            let parent = locate(root, &patch.parent_guid)?.pointer;
            let index = child_index(root, &parent, &patch.guid)?;
            children_mut(root, &parent)?.remove(index);
            Some(())
        }
        Op::SetProperty => {
            let pointer = locate(root, &patch.guid)?.pointer;
            let node = root.pointer_mut(&pointer)?.as_object_mut()?;
            let tokens: Vec<String> = patch
                .path
                .strip_prefix('/')?
                .split('/')
                .map(unescape)
                .collect();
            let (holder, key) = match tokens.as_slice() {
                [key] => (node, key.clone()),
                [metadata, key] if metadata == "metadata" => (
                    node.entry("metadata")
                        .or_insert_with(|| Value::Object(Map::new()))
                        .as_object_mut()?,
                    key.clone(),
                ),
                _ => return None,
            };
            if patch.removed {
                holder.remove(&key);
            } else {
                holder.insert(key, serde_json::from_str(patch.value.as_deref()?).ok()?);
            }
            Some(())
        }
        Op::ReplaceTimeline => {
            *root = serde_json::from_str(patch.timeline.as_deref()?).ok()?;
            Some(())
        }
        Op::Other(_) => None,
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn clip(guid: &str) -> Value {
        json!({"OTIO_SCHEMA": "Clip.2", "name": guid, "metadata": {"sync": {"guid": guid}}})
    }

    fn timeline() -> Value {
        json!({
            "OTIO_SCHEMA": "Timeline.1",
            "metadata": {"sync": {"guid": "tl"}},
            "tracks": {
                "OTIO_SCHEMA": "Stack.1",
                "metadata": {"sync": {"guid": "stack"}},
                "children": [
                    {"OTIO_SCHEMA": "Track.1", "metadata": {"sync": {"guid": "v1"}},
                     "children": [clip("a"), clip("b"), clip("c")]},
                    {"OTIO_SCHEMA": "Track.1", "metadata": {"sync": {"guid": "v2"}},
                     "children": []}
                ]
            }
        })
    }

    fn names(root: &Value, track: usize) -> Vec<String> {
        root["tracks"]["children"][track]["children"]
            .as_array()
            .unwrap()
            .iter()
            .map(|clip| clip["name"].as_str().unwrap().to_string())
            .collect()
    }

    #[test]
    fn locates_through_the_stack() {
        let root = timeline();
        assert_eq!(locate(&root, "tl").unwrap().pointer, "");
        assert_eq!(locate(&root, "stack").unwrap().pointer, "/tracks");
        let b = locate(&root, "b").unwrap();
        assert_eq!(b.pointer, "/tracks/children/0/children/1");
        assert_eq!(b.parent, Some(("/tracks/children/0".to_string(), 1)));
        assert!(locate(&root, "missing").is_none());
    }

    #[test]
    fn moves_as_python_lists_do() {
        let mut root = timeline();
        let edit = OriEdit::MoveChild {
            parent: "v1",
            child: "a",
            to_index: 99,
        };
        assert!(apply_ori(&mut root, edit));
        assert_eq!(names(&root, 0), ["b", "c", "a"]);
        let edit = OriEdit::MoveChild {
            parent: "v2",
            child: "a",
            to_index: 0,
        };
        assert!(
            !apply_ori(&mut root, edit),
            "ORI moves only within a parent"
        );
    }

    #[test]
    fn sets_nested_metadata_and_appends() {
        let mut root = timeline();
        assert!(apply_ori(
            &mut root,
            OriEdit::SetProperty {
                target: "b",
                path: "metadata/a/b",
                value: json!(3),
            }
        ));
        assert_eq!(
            root.pointer("/tracks/children/0/children/1/metadata/a/b"),
            Some(&json!(3))
        );
        assert!(apply_ori(
            &mut root,
            OriEdit::InsertChild {
                parent: "v2",
                index: -1,
                child: json!({"OTIO_SCHEMA": "Gap.1", "name": "g"}),
            }
        ));
        let gap = &root["tracks"]["children"][1]["children"][0];
        assert!(guid_of(gap).is_some(), "an inserted child gets a GUID");
    }

    #[test]
    fn merges_annotation_clips() {
        let mut root = timeline();
        let annotation = |command: i64| {
            json!({"OTIO_SCHEMA": "Clip.2", "name": "ann",
                   "source_range": {"start_time": {"value": 12.0, "rate": 24.0}},
                   "metadata": {"clip_guid": "a", "annotation_commands": [command],
                                "sync": {"guid": format!("ann{command}")}}})
        };
        for command in [1, 2] {
            assert!(apply_ori(
                &mut root,
                OriEdit::InsertChild {
                    parent: "v2",
                    index: -1,
                    child: annotation(command),
                }
            ));
        }
        let track = root["tracks"]["children"][1]["children"]
            .as_array()
            .unwrap();
        assert_eq!(track.len(), 1);
        assert_eq!(track[0]["metadata"]["annotation_commands"], json!([1, 2]));
    }

    #[test]
    fn follows_this_crates_patches() {
        let mut root = timeline();
        let patch = Patch {
            op: Op::MoveChild,
            parent_guid: "v2".to_string(),
            guid: "a".to_string(),
            index: Some(0),
            ..Patch::default()
        };
        apply_patch(&mut root, &patch).unwrap();
        assert_eq!(names(&root, 0), ["b", "c"]);
        assert_eq!(names(&root, 1), ["a"]);
        let patch = Patch {
            op: Op::MoveChild,
            parent_guid: "v1".to_string(),
            guid: "b".to_string(),
            index: Some(2),
            ..Patch::default()
        };
        apply_patch(&mut root, &patch).unwrap();
        assert_eq!(names(&root, 0), ["c", "b"]);
    }
}
