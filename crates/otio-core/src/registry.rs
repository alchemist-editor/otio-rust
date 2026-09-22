//! Which schemas exist, at which versions, and how to move an object between
//! versions.
//!
//! Upstream keeps a process-wide `TypeRegistry`: every schema it can read,
//! the newest version of each, and per-version functions that upgrade an
//! object read from an older file or downgrade one being written for an
//! older release. This is that registry. It starts out holding the schemas
//! built into this library, with upstream's built-in upgrade and downgrade
//! functions, and a program may add to it at run time:
//!
//! - [`register_type`] defines a schema of the program's own. Its objects are
//!   read as [`DynamicObject`](crate::schema::DynamicObject)s rather than as
//!   [`UnknownSchema`](crate::schema::UnknownSchema)s, which is what upstream's
//!   Python `register_type` amounts to in its C++.
//! - [`register_upgrade_function`] and [`register_downgrade_function`] add a
//!   step to a schema's version ladder.
//!
//! # Functions work on field maps
//!
//! An upgrade or downgrade function is handed an object's fields as an
//! [`AnyDictionary`] and changes them in place, as upstream's do. The map is
//! self-contained: objects nested inside it are dictionaries carrying their
//! own `OTIO_SCHEMA` key rather than handles into a document, so a function
//! needs no document to read or build one. Two things differ by direction,
//! both as upstream has them:
//!
//! - An upgrade sees the fields without the `OTIO_SCHEMA` and `OTIO_REF_ID`
//!   keys, and sees times, ranges, colours and the other value types as
//!   values.
//! - A downgrade sees the whole object as it would be written, `OTIO_SCHEMA`
//!   included, with value types as dictionaries too; the reader sets
//!   `OTIO_SCHEMA` afterwards, so a function may drop it.
//!
//! Upstream hands an upgrade function nested objects already read, as
//! objects; here they are still dictionaries, and they are upgraded after
//! the object holding them rather than before.
//!
//! # Registration is first come, first served
//!
//! As upstream's, registering a schema or a function a second time does
//! nothing and returns `false`: the first registration stays.

use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, LazyLock, PoisonError, RwLock, RwLockReadGuard, RwLockWriteGuard};

use crate::arena::Document;
use crate::error::{Error, Result};
use crate::value::{Any, AnyDictionary};

/// A map from schema name to schema version.
///
/// Upstream calls this `schema_version_map`; a writer takes one to say which
/// version of each schema to write.
pub type SchemaVersionMap = BTreeMap<String, u32>;

/// A map from a release label, such as `"0.17.0"`, to the schema versions
/// that release wrote.
pub type LabelToSchemaVersionMap = BTreeMap<String, SchemaVersionMap>;

/// An upgrade or downgrade function: it changes an object's fields in place.
///
/// Returning an error stops the read or write it is part of; see
/// [`Error::VersionFunctionFailed`].
pub type VersionFunction = Arc<dyn Fn(&mut AnyDictionary) -> Result<()> + Send + Sync>;

/// What a schema registered at run time derives from.
///
/// Upstream lets a program define a schema by subclassing either of its two
/// root classes, and the choice decides whether objects of it carry a name
/// and metadata of their own.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DynamicBase {
    /// Upstream's `SerializableObject`: every field is a dynamic field.
    SerializableObject,
    /// Upstream's `SerializableObjectWithMetadata`: a name and metadata, and
    /// dynamic fields beside them.
    SerializableObjectWithMetadata,
}

/// How objects of a registered schema are held once read.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum SchemaKind {
    /// One of the schemas built into this library, read into its own
    /// [`Node`](crate::Node) variant.
    BuiltIn,
    /// A schema registered at run time, read as a
    /// [`DynamicObject`](crate::schema::DynamicObject).
    Dynamic(DynamicBase),
}

/// Everything the registry knows about one schema.
struct Record {
    version: u32,
    kind: SchemaKind,
    /// Keyed by the version each one upgrades *to*.
    upgrades: BTreeMap<u32, VersionFunction>,
    /// Keyed by the version each one downgrades *from*.
    downgrades: BTreeMap<u32, VersionFunction>,
}

impl Record {
    fn new(version: u32, kind: SchemaKind) -> Self {
        Self {
            version,
            kind,
            upgrades: BTreeMap::new(),
            downgrades: BTreeMap::new(),
        }
    }
}

struct Registry {
    records: HashMap<String, Record>,
    /// Old names upstream still reads, each standing for a current schema.
    aliases: HashMap<String, String>,
}

/// The schemas built into this library, at the versions it writes.
///
/// `UnknownSchema` is here because upstream registers it: it is what an
/// object of any schema nobody registered is read as.
const BUILT_IN: [(&str, u32); 23] = [
    ("Clip", 2),
    ("Composable", 1),
    ("Composition", 1),
    ("Effect", 1),
    ("ExternalReference", 1),
    ("FreezeFrame", 1),
    ("Gap", 1),
    ("GeneratorReference", 1),
    ("ImageSequenceReference", 1),
    ("Item", 1),
    ("LinearTimeWarp", 1),
    ("Marker", 3),
    ("MediaReference", 1),
    ("MissingReference", 1),
    ("SerializableCollection", 1),
    ("SerializableObject", 1),
    ("SerializableObjectWithMetadata", 1),
    ("Stack", 1),
    ("TimeEffect", 1),
    ("Timeline", 1),
    ("Track", 1),
    ("Transition", 1),
    ("UnknownSchema", 1),
];

/// Names older releases wrote, and the schema each is now read as.
const ALIASES: [(&str, &str); 3] = [
    ("Filler", "Gap"),
    ("SerializeableCollection", "SerializableCollection"),
    ("Sequence", "Track"),
];

impl Registry {
    fn with_built_ins() -> Self {
        let mut records = HashMap::new();
        for (name, version) in BUILT_IN {
            records.insert(name.to_string(), Record::new(version, SchemaKind::BuiltIn));
        }
        let aliases = ALIASES
            .iter()
            .map(|(old, new)| ((*old).to_string(), (*new).to_string()))
            .collect();
        let mut registry = Self { records, aliases };

        let built_in: [(&str, u32, VersionFunction); 3] = [
            ("Marker", 2, Arc::new(crate::upgrade::marker_1_to_2)),
            ("Marker", 3, Arc::new(crate::upgrade::marker_2_to_3)),
            ("Clip", 2, Arc::new(crate::upgrade::clip_1_to_2)),
        ];
        for (name, version, function) in built_in {
            if let Some(record) = registry.record_mut(name) {
                record.upgrades.insert(version, function);
            }
        }
        let built_in: [(&str, u32, VersionFunction); 2] = [
            ("Marker", 3, Arc::new(crate::upgrade::marker_3_to_2)),
            ("Clip", 2, Arc::new(crate::upgrade::clip_2_to_1)),
        ];
        for (name, version, function) in built_in {
            if let Some(record) = registry.record_mut(name) {
                record.downgrades.insert(version, function);
            }
        }
        registry
    }

    fn canonical<'a>(&'a self, name: &'a str) -> &'a str {
        self.aliases.get(name).map_or(name, String::as_str)
    }

    fn record(&self, name: &str) -> Option<&Record> {
        self.records.get(self.canonical(name))
    }

    fn record_mut(&mut self, name: &str) -> Option<&mut Record> {
        let name = self.canonical(name).to_string();
        self.records.get_mut(&name)
    }
}

static REGISTRY: LazyLock<RwLock<Registry>> =
    LazyLock::new(|| RwLock::new(Registry::with_built_ins()));

/// Takes the registry for reading.
///
/// No user function ever runs with the lock held — each is cloned out first —
/// so a poisoned lock can only mean a panic inside this module's own
/// bookkeeping, which leaves nothing half-done. Carrying on is safe.
fn read() -> RwLockReadGuard<'static, Registry> {
    REGISTRY.read().unwrap_or_else(PoisonError::into_inner)
}

fn write() -> RwLockWriteGuard<'static, Registry> {
    REGISTRY.write().unwrap_or_else(PoisonError::into_inner)
}

/// Registers a schema defined at run time.
///
/// Objects of it are read as [`DynamicObject`](crate::schema::DynamicObject)s
/// from then on. Returns `false`, and changes nothing, if the name is already
/// registered — including as one of the built-in schemas or their aliases.
pub fn register_type(schema_name: &str, schema_version: u32, base: DynamicBase) -> bool {
    let mut registry = write();
    if registry.record(schema_name).is_some() {
        return false;
    }
    registry.records.insert(
        schema_name.to_string(),
        Record::new(schema_version, SchemaKind::Dynamic(base)),
    );
    true
}

/// Registers the function that upgrades `schema_name` to
/// `version_to_upgrade_to` from the version before it.
///
/// Reading an object older than the registered version runs, in order, every
/// upgrade function from the object's own version up to the registered one;
/// a function keyed at the object's own version runs too, as upstream's do.
/// Returns `false` if the schema is not registered or already has a function
/// for that version.
pub fn register_upgrade_function(
    schema_name: &str,
    version_to_upgrade_to: u32,
    function: VersionFunction,
) -> bool {
    let mut registry = write();
    let Some(record) = registry.record_mut(schema_name) else {
        return false;
    };
    if record.upgrades.contains_key(&version_to_upgrade_to) {
        return false;
    }
    record.upgrades.insert(version_to_upgrade_to, function);
    true
}

/// Registers the function that downgrades `schema_name` from
/// `version_to_downgrade_from` to the version before it.
///
/// Returns `false` if the schema is not registered or already has a function
/// for that version.
pub fn register_downgrade_function(
    schema_name: &str,
    version_to_downgrade_from: u32,
    function: VersionFunction,
) -> bool {
    let mut registry = write();
    let Some(record) = registry.record_mut(schema_name) else {
        return false;
    };
    if record.downgrades.contains_key(&version_to_downgrade_from) {
        return false;
    }
    record
        .downgrades
        .insert(version_to_downgrade_from, function);
    true
}

/// Every registered schema and the version it is written at.
///
/// Aliases are not listed separately, as upstream's are not.
#[must_use]
pub fn type_version_map() -> SchemaVersionMap {
    read()
        .records
        .iter()
        .map(|(name, record)| (name.clone(), record.version))
        .collect()
}

/// The version `schema_name` is written at, if it is registered.
///
/// An alias answers for the schema it stands for.
#[must_use]
pub fn schema_version(schema_name: &str) -> Option<u32> {
    read().record(schema_name).map(|record| record.version)
}

/// How objects of `schema_name` are held, if it is registered.
#[must_use]
pub fn schema_kind(schema_name: &str) -> Option<SchemaKind> {
    read().record(schema_name).map(|record| record.kind)
}

/// The name `schema_name` is read as: itself, or the schema an old alias
/// stands for.
#[must_use]
pub fn canonical_schema_name(schema_name: &str) -> String {
    read().canonical(schema_name).to_string()
}

/// What the reader needs to know about a schema, looked up once.
pub(crate) struct Found {
    pub(crate) version: u32,
    pub(crate) kind: SchemaKind,
}

pub(crate) fn find(schema_name: &str) -> Option<Found> {
    read().record(schema_name).map(|record| Found {
        version: record.version,
        kind: record.kind,
    })
}

/// The upgrade functions to run on an object of `schema_name` read at
/// `from`, in order.
///
/// Upstream runs every function keyed between the object's version and the
/// registered one, both ends included.
pub(crate) fn upgrades(schema_name: &str, from: u32) -> Vec<VersionFunction> {
    let registry = read();
    let Some(record) = registry.record(schema_name) else {
        return Vec::new();
    };
    record
        .upgrades
        .range(from..=record.version)
        .map(|(_, function)| Arc::clone(function))
        .collect()
}

/// The downgrade functions that take `schema_name` from `from` down to `to`,
/// in the order they run.
///
/// # Errors
///
/// [`Error::NoDowngradeFunction`] if a step is missing, naming the version
/// that could not be left.
pub(crate) fn downgrades(schema_name: &str, from: u32, to: u32) -> Result<Vec<VersionFunction>> {
    let registry = read();
    let record = registry.record(schema_name);
    let mut result = Vec::new();
    let mut version = from;
    while version > to {
        let function = record
            .and_then(|record| record.downgrades.get(&version))
            .ok_or_else(|| Error::NoDowngradeFunction {
                schema: schema_name.to_string(),
                from: version,
                to,
            })?;
        result.push(Arc::clone(function));
        version -= 1;
    }
    Ok(result)
}

/// Builds an object of `schema_name` at `schema_version` from `data`, the
/// way reading one from a file would.
///
/// The object is upgraded to the registered version first; a schema nobody
/// registered becomes an [`UnknownSchema`](crate::schema::UnknownSchema), as
/// it would in a file. Objects held in `data` are resolved against
/// `document` and copied, and the result is the root of a document of its
/// own.
///
/// # Errors
///
/// [`Error::UnsupportedSchemaVersion`] if `schema_version` is newer than the
/// registered one, and whatever reading `data` as that schema produces.
pub fn instance_from_schema(
    document: &Document,
    schema_name: &str,
    schema_version: u32,
    data: &AnyDictionary,
) -> Result<Document> {
    let mut object = data.clone();
    object.insert(
        "OTIO_SCHEMA".to_string(),
        Any::String(format!("{schema_name}.{schema_version}")),
    );
    let options = crate::serialize::WriteOptions {
        indent: None,
        ..crate::serialize::WriteOptions::default()
    };
    let text = crate::serialize::to_string_with(document, &Any::Dictionary(object), &options)?;
    crate::deserialize::from_str_unlocated(&text)
}

/// The schema versions each release of upstream OpenTimelineIO wrote.
///
/// Upstream compiles this in as `CORE_VERSION_MAP`, and a writer can be told
/// to target one of its labels so that an older release can read the file.
/// Copied from upstream at 0.19.
#[must_use]
pub fn release_to_schema_version_map() -> LabelToSchemaVersionMap {
    // Every release wrote these at version 1, apart from the three below.
    const COMMON: [&str; 26] = [
        "Adapter",
        "Composable",
        "Composition",
        "Effect",
        "ExternalReference",
        "FreezeFrame",
        "Gap",
        "GeneratorReference",
        "HookScript",
        "ImageSequenceReference",
        "Item",
        "LinearTimeWarp",
        "MediaLinker",
        "MediaReference",
        "MissingReference",
        "PluginManifest",
        "SchemaDef",
        "SerializableCollection",
        "SerializableObject",
        "SerializableObjectWithMetadata",
        "Stack",
        "TimeEffect",
        "Timeline",
        "Track",
        "Transition",
        "UnknownSchema",
    ];
    // Each release's label, its `Clip` and `Marker` versions, and whether
    // it registered the `Test` schema upstream's own suite uses.
    const RELEASES: [(&str, u32, u32, bool); 7] = [
        ("0.14.0", 1, 2, false),
        ("0.15.0", 2, 2, true),
        ("0.16.0", 2, 2, true),
        ("0.17.0", 2, 2, true),
        ("0.18.0", 2, 2, true),
        ("0.18.1", 2, 2, true),
        ("0.19.0.dev1", 2, 3, true),
    ];

    RELEASES
        .iter()
        .map(|&(label, clip, marker, test)| {
            let mut versions: SchemaVersionMap =
                COMMON.iter().map(|name| ((*name).to_string(), 1)).collect();
            versions.insert("Clip".to_string(), clip);
            versions.insert("Marker".to_string(), marker);
            if test {
                versions.insert("Test".to_string(), 1);
            }
            (label.to_string(), versions)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn built_in_schemas_are_registered_at_the_versions_written() {
        let versions = type_version_map();
        assert_eq!(versions.get("Clip"), Some(&2));
        assert_eq!(versions.get("Marker"), Some(&3));
        assert_eq!(versions.get("UnknownSchema"), Some(&1));
        // An alias answers, but is not listed as a schema of its own.
        assert_eq!(schema_version("Filler"), Some(1));
        assert!(!versions.contains_key("Filler"));
        assert_eq!(canonical_schema_name("Sequence"), "Track");
    }

    #[test]
    fn the_first_registration_stays() {
        assert!(register_type(
            "RegistryTestFirst",
            3,
            DynamicBase::SerializableObject
        ));
        assert!(!register_type(
            "RegistryTestFirst",
            5,
            DynamicBase::SerializableObjectWithMetadata
        ));
        assert_eq!(schema_version("RegistryTestFirst"), Some(3));
        assert_eq!(
            schema_kind("RegistryTestFirst"),
            Some(SchemaKind::Dynamic(DynamicBase::SerializableObject))
        );

        // Nor can a built-in schema or an alias be taken over.
        assert!(!register_type("Clip", 9, DynamicBase::SerializableObject));
        assert!(!register_type("Filler", 9, DynamicBase::SerializableObject));
        assert_eq!(schema_kind("Clip"), Some(SchemaKind::BuiltIn));
    }

    #[test]
    fn functions_need_a_registered_schema_and_a_free_slot() {
        let noop: VersionFunction = Arc::new(|_| Ok(()));
        assert!(!register_upgrade_function(
            "RegistryTestNobody",
            2,
            noop.clone()
        ));
        // Upstream registers the step from `Clip.1` itself.
        assert!(!register_upgrade_function("Clip", 2, noop.clone()));
        assert!(!register_downgrade_function("Marker", 3, noop));
    }

    #[test]
    fn a_missing_downgrade_step_names_the_version_it_stopped_at() {
        register_type("RegistryTestLadder", 3, DynamicBase::SerializableObject);
        register_downgrade_function("RegistryTestLadder", 3, Arc::new(|_| Ok(())));
        assert_eq!(
            downgrades("RegistryTestLadder", 3, 1).err(),
            Some(Error::NoDowngradeFunction {
                schema: "RegistryTestLadder".to_string(),
                from: 2,
                to: 1,
            })
        );
        assert_eq!(
            downgrades("RegistryTestLadder", 3, 2).map(|f| f.len()),
            Ok(1)
        );
    }

    #[test]
    fn the_release_map_matches_upstreams() {
        let releases = release_to_schema_version_map();
        assert_eq!(releases.len(), 7);
        assert_eq!(releases["0.14.0"]["Clip"], 1);
        assert!(!releases["0.14.0"].contains_key("Test"));
        assert_eq!(releases["0.15.0"]["Clip"], 2);
        assert_eq!(releases["0.18.1"]["Marker"], 2);
        assert_eq!(releases["0.19.0.dev1"]["Marker"], 3);
        assert_eq!(releases["0.19.0.dev1"].len(), 29);
        assert_eq!(releases["0.14.0"].len(), 28);
    }
}
