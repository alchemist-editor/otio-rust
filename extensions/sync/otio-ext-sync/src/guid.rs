//! Identity: every synced object carries a GUID in `metadata.sync.guid`.
//!
//! A [`NodeId`] names an object in one document in one process, and does not
//! survive a write and a read, so peers cannot use it to say which object a
//! patch is about. ORI Sync and Alchemist's Furnace both name objects by
//! `metadata.sync.guid`, so sync does too ([ADR 0011]). It is ordinary
//! metadata: a file carrying it opens anywhere OpenTimelineIO does.
//!
//! The objects sync addresses are the timeline and every composable under
//! it: its stack, the tracks and stacks in that, and their items. Markers,
//! effects and media references travel inside the item that holds them.
//!
//! [ADR 0011]: https://github.com/alchemist-editor/otio-rust/blob/main/docs/adr/0011-timeline-sync.md

use std::collections::hash_map::RandomState;
use std::collections::{HashMap, HashSet};
use std::fmt;
use std::hash::{BuildHasher, Hasher};
use std::sync::atomic::{AtomicU64, Ordering};

use otio_core::schema::Node;
use otio_core::{Any, AnyDictionary, Document, NodeId};

/// The metadata namespace sync keeps its fields in.
pub const NAMESPACE: &str = "sync";

/// The field in that namespace holding an object's GUID.
pub const KEY: &str = "guid";

/// Returns the GUID `id` carries, if it carries one.
#[must_use]
pub fn guid(document: &Document, id: NodeId) -> Option<&str> {
    let base = document.get(id)?.base()?;
    match base.metadata.get(NAMESPACE)? {
        Any::Dictionary(sync) => match sync.get(KEY)? {
            Any::String(guid) if !guid.is_empty() => Some(guid),
            _ => None,
        },
        _ => None,
    }
}

/// Gives `id` the GUID `guid`, replacing any it had.
///
/// Anything else in its `sync` namespace is kept. A `sync` entry that is not
/// a dictionary is replaced by one.
///
/// # Errors
///
/// [`otio_core::Error::StaleHandle`] if `id` is not in the document. An
/// object with no metadata of its own, which no object sync addresses is, is
/// left alone.
pub fn set_guid(
    document: &mut Document,
    id: NodeId,
    guid: impl Into<String>,
) -> otio_core::Result<()> {
    let Some(base) = document.try_get_mut(id)?.base_mut() else {
        return Ok(());
    };
    let namespace = base
        .metadata
        .entry(NAMESPACE.to_string())
        .or_insert_with(|| Any::Dictionary(AnyDictionary::new()));
    if !matches!(namespace, Any::Dictionary(_)) {
        *namespace = Any::Dictionary(AnyDictionary::new());
    }
    if let Any::Dictionary(sync) = namespace {
        sync.insert(KEY.to_string(), Any::String(guid.into()));
    }
    Ok(())
}

/// Makes a new GUID: 128 random bits, written as a version 4 UUID.
///
/// The bits come from the standard library's per-process random hash keys
/// and a counter, which is unique enough to name objects but is not meant
/// for secrets.
#[must_use]
pub fn new_guid() -> String {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let count = COUNTER.fetch_add(1, Ordering::Relaxed);
    let half = |salt: u64| {
        let mut hasher = RandomState::new().build_hasher();
        hasher.write_u64(count);
        hasher.write_u64(salt);
        hasher.finish()
    };
    let mut bytes = [0_u8; 16];
    bytes[..8].copy_from_slice(&half(0).to_le_bytes());
    bytes[8..].copy_from_slice(&half(1).to_le_bytes());
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    let hex: String = bytes.iter().map(|byte| format!("{byte:02x}")).collect();
    format!(
        "{}-{}-{}-{}-{}",
        &hex[0..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..32]
    )
}

/// Returns the objects sync addresses under `root`, parents before their
/// children: `root` itself, a timeline's stack, and every composable in it.
#[must_use]
pub fn synced_objects(document: &Document, root: NodeId) -> Vec<NodeId> {
    let mut found = Vec::new();
    let mut pending = vec![root];
    while let Some(id) = pending.pop() {
        let Some(node) = document.get(id) else {
            continue;
        };
        found.push(id);
        match node {
            Node::Timeline(timeline) => pending.extend(timeline.tracks),
            node => {
                if let Some(children) = node.children() {
                    pending.extend(children.iter().rev());
                }
            }
        }
    }
    found
}

/// Gives every object sync addresses under `root` a GUID, if it lacks one or
/// shares one with an object met earlier, and returns the objects it gave
/// one to.
///
/// A shared GUID usually means an object was copied with its metadata; the
/// first met, parent before child and in child order, keeps it.
///
/// # Errors
///
/// [`otio_core::Error::StaleHandle`] if `root` is not in the document.
pub fn stamp(document: &mut Document, root: NodeId) -> otio_core::Result<Vec<NodeId>> {
    document.try_get(root)?;
    let mut seen = HashSet::new();
    let mut stamped = Vec::new();
    for id in synced_objects(document, root) {
        let fresh = match guid(document, id) {
            Some(existing) => !seen.insert(existing.to_string()),
            None => true,
        };
        if fresh && document.get(id).and_then(Node::base).is_some() {
            let new = new_guid();
            seen.insert(new.clone());
            set_guid(document, id, new)?;
            stamped.push(id);
        }
    }
    Ok(stamped)
}

/// Two objects under one root claiming one GUID; see [`GuidIndex::build`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DuplicateGuid {
    /// The GUID.
    pub guid: String,
    /// The object that claimed it first.
    pub first: NodeId,
    /// The object that claimed it again.
    pub second: NodeId,
}

impl fmt::Display for DuplicateGuid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "two objects carry the GUID '{}'", self.guid)
    }
}

impl std::error::Error for DuplicateGuid {}

/// Finds an object by its GUID.
#[derive(Debug, Clone, Default)]
pub struct GuidIndex {
    objects: HashMap<String, NodeId>,
}

impl GuidIndex {
    /// Indexes every object sync addresses under `root` that carries a GUID.
    ///
    /// # Errors
    ///
    /// [`DuplicateGuid`] if two carry the same one; [`stamp`] first to
    /// rule that out.
    pub fn build(document: &Document, root: NodeId) -> Result<Self, DuplicateGuid> {
        let mut objects = HashMap::new();
        for id in synced_objects(document, root) {
            let Some(found) = guid(document, id) else {
                continue;
            };
            if let Some(first) = objects.insert(found.to_string(), id) {
                return Err(DuplicateGuid {
                    guid: found.to_string(),
                    first,
                    second: id,
                });
            }
        }
        Ok(Self { objects })
    }

    /// The object carrying `guid`, if one does.
    #[must_use]
    pub fn get(&self, guid: &str) -> Option<NodeId> {
        self.objects.get(guid).copied()
    }

    /// Records that `id` carries `guid`, returning the object that did
    /// before, if another did.
    pub fn insert(&mut self, guid: impl Into<String>, id: NodeId) -> Option<NodeId> {
        self.objects.insert(guid.into(), id)
    }

    /// Forgets `guid`, returning the object that carried it.
    pub fn remove(&mut self, guid: &str) -> Option<NodeId> {
        self.objects.remove(guid)
    }

    /// How many objects are indexed.
    #[must_use]
    pub fn len(&self) -> usize {
        self.objects.len()
    }

    /// Whether nothing is indexed.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.objects.is_empty()
    }
}
