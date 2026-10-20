//! Applying patches to a timeline.
//!
//! A port of `Apply` from Furnace's `internal/otiodiff` (svc-furnace, in Go),
//! with its checks and error messages, so a patch list one side accepts the
//! other accepts too.
//!
//! The result is compared with the timeline the patches were made from as
//! JSON values, not as bytes: see [`crate::json_eq`]. Each structural node
//! is written with its members in byte order of their names, and everything
//! is compacted, as Go's encoder writes a `map[string]json.RawMessage`.

use crate::patch::{Error, Op, Patch, raw_present};
use crate::raw::{self, quote};
use crate::tree::{Arena, checked_index, has_children, is_value_property, metadata_namespaces};

/// Applies `patches`, in order, to `old_timeline`, and returns the resulting
/// timeline as JSON.
///
/// A [`Op::ReplaceTimeline`] must be the only patch in its list, and its
/// timeline is returned exactly as it is.
///
/// # Errors
///
/// Returns [`Error::InvalidJson`] if `old_timeline` is not JSON,
/// [`Error::ReplaceNotAlone`] or [`Error::ReplaceInvalid`] for a misused
/// replace, [`Error::OldTimeline`] if the timeline is not one sync can
/// address, and [`Error::Patch`] for the first patch that cannot be applied.
pub fn apply(old_timeline: &str, patches: &[Patch]) -> Result<String, Error> {
    if !raw::is_valid(old_timeline) {
        return Err(Error::InvalidJson("old timeline"));
    }
    if patches.len() > 1 && patches.iter().any(|patch| patch.op == Op::ReplaceTimeline) {
        return Err(Error::ReplaceNotAlone);
    }
    if let [patch] = patches {
        if patch.op == Op::ReplaceTimeline {
            return match raw_present(patch.timeline.as_ref()) {
                Some(timeline) if raw::is_valid(timeline) => Ok(timeline.to_owned()),
                _ => Err(Error::ReplaceInvalid),
            };
        }
    }

    let mut tree =
        Arena::parse(old_timeline, true).map_err(|problem| Error::OldTimeline(problem.0))?;
    for (index, patch) in patches.iter().enumerate() {
        apply_one(&mut tree, patch).map_err(|message| Error::Patch {
            index,
            op: patch.op.as_str().to_owned(),
            message,
        })?;
    }
    Ok(tree.marshal(tree.root))
}

fn apply_one(tree: &mut Arena, patch: &Patch) -> Result<(), String> {
    let parent_missing = || format!("parent guid {} does not exist", quote(&patch.parent_guid));
    let guid_missing = || format!("guid {} does not exist", quote(&patch.guid));
    match &patch.op {
        Op::InsertChild => {
            let (Some(index), Some(node)) = (patch.index, raw_present(patch.node.as_ref())) else {
                return Err("insert_child requires parentGuid, index, and node".into());
            };
            if patch.parent_guid.is_empty() {
                return Err("insert_child requires parentGuid, index, and node".into());
            }
            let parent = *tree
                .by_guid
                .get(&patch.parent_guid)
                .ok_or_else(parent_missing)?;
            if !has_children(&tree.nodes[parent].kind) {
                return Err(format!(
                    "parent {} has no insertable children array",
                    quote(&patch.parent_guid)
                ));
            }
            let len = tree.nodes[parent].children.len();
            if checked_index(index, len).is_none() {
                return Err(format!("index {index} out of range 0..{len}"));
            }
            if !raw::is_valid(node) {
                return Err("node is not valid JSON".into());
            }
            let inserted = tree
                .parse_detached(node)
                .map_err(|problem| format!("parse inserted node: {}", problem.0))?;
            if tree.nodes[inserted].kind == "Timeline" {
                return Err("cannot insert a Timeline as a child".into());
            }
            tree.adopt_and_insert(parent, index, inserted)
        }

        Op::MoveChild => {
            let Some(index) = patch.index else {
                return Err("move_child requires parentGuid, guid, and index".into());
            };
            if patch.parent_guid.is_empty() || patch.guid.is_empty() {
                return Err("move_child requires parentGuid, guid, and index".into());
            }
            let dest = *tree
                .by_guid
                .get(&patch.parent_guid)
                .ok_or_else(parent_missing)?;
            let moving = *tree.by_guid.get(&patch.guid).ok_or_else(guid_missing)?;
            if !has_children(&tree.nodes[dest].kind) {
                return Err(format!(
                    "parent {} has no movable children array",
                    quote(&patch.parent_guid)
                ));
            }
            if tree.nodes[moving].parent.is_none() {
                return Err("cannot move the timeline root".into());
            }
            let len = tree.nodes[dest].children.len();
            if checked_index(index, len).is_none() {
                return Err(format!("index {index} out of range 0..{len}"));
            }
            let mut destination = index;
            if tree.nodes[moving].parent == Some(dest) && checked_index(index, len) == Some(len) {
                // The public range is defined against the destination before
                // the source is detached, so len means "move to end" for a
                // same-parent move even though the post-detach insertion
                // index is len - 1.
                destination -= 1;
            }
            tree.move_node(moving, dest, destination)?;
            tree.reindex();
            Ok(())
        }

        Op::RemoveChild => {
            if patch.parent_guid.is_empty() || patch.guid.is_empty() {
                return Err("remove_child requires parentGuid and guid".into());
            }
            let parent = *tree
                .by_guid
                .get(&patch.parent_guid)
                .ok_or_else(parent_missing)?;
            let removing = *tree.by_guid.get(&patch.guid).ok_or_else(guid_missing)?;
            if tree.nodes[removing].parent != Some(parent) {
                return Err(format!(
                    "guid {} is not a child of parent {}",
                    quote(&patch.guid),
                    quote(&patch.parent_guid)
                ));
            }
            tree.detach(removing);
            tree.reindex();
            Ok(())
        }

        Op::SetProperty => {
            if patch.guid.is_empty() || patch.path.is_empty() {
                return Err("set_property requires guid and path".into());
            }
            let target = *tree.by_guid.get(&patch.guid).ok_or_else(guid_missing)?;
            let value = raw_present(patch.value.as_ref());
            if patch.removed {
                if value.is_some() {
                    return Err("removed set_property must omit value".into());
                }
            } else if !value.is_some_and(raw::is_valid) {
                return Err("set_property requires a valid value unless removed".into());
            }
            let segments = parse_property_path(&patch.path)?;
            let node = &mut tree.nodes[target];
            if let [key] = segments.as_slice() {
                if !is_value_property(&node.kind, key) || key == "metadata" {
                    return Err(format!(
                        "path {} targets structural bookkeeping",
                        quote(&patch.path)
                    ));
                }
                match value {
                    Some(value) => node.fields.insert(key.clone(), value.to_owned()),
                    None => node.fields.remove(key),
                };
                return Ok(());
            }
            let key = &segments[1];
            if key == "sync" {
                return Err("/metadata/sync cannot be patched".into());
            }
            let current = node.fields.get("metadata").cloned();
            let mut metadata = metadata_namespaces(current.as_deref())?;
            match value {
                Some(value) => metadata.insert(key.clone(), value),
                None => metadata.remove(key.as_str()),
            };
            let mut encoded = String::from("{");
            for (i, (name, value)) in metadata.iter().enumerate() {
                if i > 0 {
                    encoded.push(',');
                }
                raw::push_string(&mut encoded, name);
                encoded.push(':');
                raw::compact_into(&mut encoded, value);
            }
            encoded.push('}');
            node.fields.insert("metadata".into(), encoded);
            Ok(())
        }

        Op::ReplaceTimeline => Err("replace_timeline must be the sole patch".into()),
        Op::Other(name) => Err(format!("unknown patch op {}", quote(name))),
    }
}

/// Splits a property path into its decoded JSON Pointer tokens: one, or two
/// with `metadata` first.
fn parse_property_path(path: &str) -> Result<Vec<String>, String> {
    let Some(rest) = path.strip_prefix('/') else {
        return Err(format!("invalid property path {}", quote(path)));
    };
    let raw_segments: Vec<&str> = rest.split('/').collect();
    if raw_segments.len() != 1 && raw_segments.len() != 2 {
        return Err(format!("unsupported property path {}", quote(path)));
    }
    let mut segments = Vec::with_capacity(raw_segments.len());
    for segment in raw_segments {
        match unescape_pointer_token(segment) {
            Some(decoded) if !decoded.is_empty() => segments.push(decoded),
            _ => return Err(format!("invalid property path {}", quote(path))),
        }
    }
    if segments.len() == 2 && segments[0] != "metadata" {
        return Err(format!("unsupported property path {}", quote(path)));
    }
    Ok(segments)
}

fn unescape_pointer_token(token: &str) -> Option<String> {
    let mut out = String::with_capacity(token.len());
    let mut chars = token.chars();
    while let Some(c) = chars.next() {
        if c != '~' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('0') => out.push('~'),
            Some('1') => out.push('/'),
            _ => return None,
        }
    }
    Some(out)
}
