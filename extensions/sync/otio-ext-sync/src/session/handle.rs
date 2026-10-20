//! Acting on a session's events in a program's document.

use otio_adapter::plugins::{self, LinkerChoice};
use otio_core::{Any, AnyDictionary, Document, NodeId};

use super::{
    Error, Event, SYNC_AFTER_APPLY, SYNC_BEFORE_APPLY, SYNC_JOINED, TimelineChange, hook_arguments,
    plugin_message, replace_patch,
};
use crate::guid::GuidIndex;
use crate::patch::{Op, Patch};
use crate::raw;

/// How [`handle`] links media on the clips a change inserts.
#[derive(Debug, Clone, Default)]
pub struct ApplyOptions {
    /// Which media linker to run: the default one unless told otherwise, as
    /// reading a file does.
    pub linker: LinkerChoice,
    /// The arguments the linker gets.
    pub linker_arguments: AnyDictionary,
}

/// What [`handle`] did with an event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Handled {
    /// It applied a timeline change's patches.
    Applied,
    /// The patches did not fit this copy of the timeline, so it put the
    /// whole new timeline in place instead. Handles to the timeline and to
    /// objects whose GUIDs survived still work.
    Replaced,
    /// A `sync_before_apply` script refused the change, with its message.
    /// The timeline is as it was.
    Rejected(String),
    /// It ran `sync_joined`.
    Joined,
    /// The event does not concern the document.
    Ignored,
}

/// Acts on `event` in `document`, whose `timeline` is the session's copy:
/// applies a [`Event::Timeline`] in place and runs `sync_joined` for an
/// [`Event::Joined`]. Every other event is left to the program.
///
/// Applying a change runs `sync_before_apply`, whose scripts may refuse it,
/// then applies its patches so handles survive
/// ([`apply_to`](crate::document::apply_to)), falling back to the whole
/// snapshot if the patches do not fit, then runs the media linker on the
/// clips it inserted, then `sync_after_apply`. The hooks get `channel`,
/// `sequence_id`, `generation`, `patches` (as JSON) and `sender`.
///
/// A program with several timelines passes the one whose sequence the
/// change names.
///
/// # Errors
///
/// [`Error::Patch`] if neither the patches nor the snapshot can be applied,
/// and [`Error::Plugin`] if the media linker or a script after the change
/// fails.
pub fn handle(
    document: &mut Document,
    timeline: NodeId,
    event: &Event,
    options: &ApplyOptions,
) -> Result<Handled, Error> {
    match event {
        Event::Timeline(change) => apply_change(document, timeline, change, options),
        Event::Joined {
            role,
            self_id,
            rejoined,
        } => {
            if plugins::registry().has_hook(SYNC_JOINED) {
                let arguments = AnyDictionary::from([
                    ("role".to_string(), Any::String(role.as_str().to_string())),
                    ("self_id".to_string(), Any::String(self_id.clone())),
                    ("rejoined".to_string(), Any::Bool(*rejoined)),
                ]);
                plugins::run_hook(SYNC_JOINED, document, timeline, &arguments)
                    .map_err(|error| Error::Plugin(plugin_message(&error)))?;
            }
            Ok(Handled::Joined)
        }
        _ => Ok(Handled::Ignored),
    }
}

fn apply_change(
    document: &mut Document,
    timeline: NodeId,
    change: &TimelineChange,
    options: &ApplyOptions,
) -> Result<Handled, Error> {
    let arguments = hook_arguments(
        &super::Channel::Timeline,
        &change.snapshot,
        change.sender.as_deref(),
        &change.patches,
    );
    if plugins::registry().has_hook(SYNC_BEFORE_APPLY) {
        if let Err(error) = plugins::run_hook(SYNC_BEFORE_APPLY, document, timeline, &arguments) {
            return Ok(Handled::Rejected(plugin_message(&error)));
        }
    }

    let (handled, applied) = match crate::document::apply_to(document, timeline, &change.patches) {
        Ok(()) => (Handled::Applied, change.patches.clone()),
        Err(_) => {
            let replace = vec![replace_patch(&change.snapshot)];
            crate::document::apply_to(document, timeline, &replace)?;
            (Handled::Replaced, replace)
        }
    };

    if plugins::anything_to_run(&options.linker) {
        for root in inserted(document, timeline, &applied) {
            plugins::link_media(document, root, &options.linker, &options.linker_arguments)
                .map_err(|error| Error::Plugin(plugin_message(&error)))?;
        }
    }

    if plugins::registry().has_hook(SYNC_AFTER_APPLY) {
        plugins::run_hook(SYNC_AFTER_APPLY, document, timeline, &arguments)
            .map_err(|error| Error::Plugin(plugin_message(&error)))?;
    }
    Ok(handled)
}

/// The objects `patches` put in the timeline: each inserted subtree, or the
/// whole timeline when it was replaced.
fn inserted(document: &Document, timeline: NodeId, patches: &[Patch]) -> Vec<NodeId> {
    if patches.iter().any(|patch| patch.op == Op::ReplaceTimeline) {
        return vec![timeline];
    }
    let Ok(index) = GuidIndex::build(document, timeline) else {
        return vec![timeline];
    };
    patches
        .iter()
        .filter(|patch| patch.op == Op::InsertChild)
        .filter_map(|patch| patch.node.as_deref().and_then(node_guid))
        .filter_map(|guid| index.get(&guid))
        .collect()
}

/// The `metadata.sync.guid` of the object the JSON `node` describes.
fn node_guid(node: &str) -> Option<String> {
    let members = raw::member_map(node)?;
    let metadata = raw::member_map(members.get("metadata")?)?;
    let sync = raw::member_map(metadata.get(crate::guid::NAMESPACE)?)?;
    let guid = raw::trim(sync.get(crate::guid::KEY)?);
    guid.starts_with('"').then(|| raw::decode_string(guid))
}
