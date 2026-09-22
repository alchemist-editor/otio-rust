//! The OTIO schema ladder, which the C ABI deliberately flattens.
//!
//! Every object in a document is an `OtioNode` to C, because C has no better
//! idea to offer. `OtioNodeKind` says which schema a handle points at, but
//! nothing in the ABI says that a `Clip` is an `Item` and that an `Item` is a
//! `Composable` — and a language with types needs that to put
//! `source_range` on both `Clip` and `Track` without saying so twice.
//!
//! So it is declared here, once, and checked against `OtioNodeKind`: a kind
//! the enum has and this table does not is an error, and so is the reverse.
//! Adding a schema to the core therefore cannot reach the SDKs until someone
//! has said where it sits.

use crate::model::{Docs, Schema};
use crate::scan::{ScanError, Scanned};

/// A row of the table: the `OtioNodeKind` variant, its parent, and whether a
/// file can hold an object of exactly this schema.
struct Rung {
    kind: &'static str,
    parent: Option<&'static str>,
    concrete: bool,
}

/// The ladder, from the root down.
///
/// `concrete` is `false` only for `UnknownSchema` and `Other`, which name a
/// situation rather than a schema. Every upstream base class is `true`,
/// because upstream registers each as a schema in its own right and its own
/// tests construct them directly.
const LADDER: &[Rung] = &[
    Rung {
        kind: "SerializableObject",
        parent: None,
        concrete: true,
    },
    Rung {
        kind: "SerializableObjectWithMetadata",
        parent: Some("SerializableObject"),
        concrete: true,
    },
    Rung {
        kind: "Composable",
        parent: Some("SerializableObjectWithMetadata"),
        concrete: true,
    },
    Rung {
        kind: "Item",
        parent: Some("Composable"),
        concrete: true,
    },
    Rung {
        kind: "Transition",
        parent: Some("Composable"),
        concrete: true,
    },
    Rung {
        kind: "Composition",
        parent: Some("Item"),
        concrete: true,
    },
    Rung {
        kind: "Track",
        parent: Some("Composition"),
        concrete: true,
    },
    Rung {
        kind: "Stack",
        parent: Some("Composition"),
        concrete: true,
    },
    Rung {
        kind: "Clip",
        parent: Some("Item"),
        concrete: true,
    },
    Rung {
        kind: "Gap",
        parent: Some("Item"),
        concrete: true,
    },
    Rung {
        kind: "Timeline",
        parent: Some("SerializableObjectWithMetadata"),
        concrete: true,
    },
    Rung {
        kind: "Marker",
        parent: Some("SerializableObjectWithMetadata"),
        concrete: true,
    },
    Rung {
        kind: "SerializableCollection",
        parent: Some("SerializableObjectWithMetadata"),
        concrete: true,
    },
    Rung {
        kind: "Effect",
        parent: Some("SerializableObjectWithMetadata"),
        concrete: true,
    },
    Rung {
        kind: "TimeEffect",
        parent: Some("Effect"),
        concrete: true,
    },
    Rung {
        kind: "LinearTimeWarp",
        parent: Some("TimeEffect"),
        concrete: true,
    },
    Rung {
        kind: "FreezeFrame",
        parent: Some("LinearTimeWarp"),
        concrete: true,
    },
    Rung {
        kind: "MediaReference",
        parent: Some("SerializableObjectWithMetadata"),
        concrete: true,
    },
    Rung {
        kind: "ExternalReference",
        parent: Some("MediaReference"),
        concrete: true,
    },
    Rung {
        kind: "MissingReference",
        parent: Some("MediaReference"),
        concrete: true,
    },
    Rung {
        kind: "GeneratorReference",
        parent: Some("MediaReference"),
        concrete: true,
    },
    Rung {
        kind: "ImageSequenceReference",
        parent: Some("MediaReference"),
        concrete: true,
    },
    Rung {
        kind: "UnknownSchema",
        parent: Some("SerializableObject"),
        concrete: false,
    },
    Rung {
        kind: "Other",
        parent: Some("SerializableObject"),
        concrete: false,
    },
];

/// Builds the schema ladder, checking it against `OtioNodeKind`.
///
/// # Errors
///
/// Fails if the enum and the table disagree about which schemas exist, which
/// is what happens when the core grows one and nobody says where it goes.
pub fn ladder(node_kind: &crate::model::Enum) -> Scanned<Vec<Schema>> {
    let declared: Vec<&str> = LADDER.iter().map(|rung| rung.kind).collect();

    let missing: Vec<&str> = node_kind
        .variants
        .iter()
        .map(|variant| variant.name.as_str())
        .filter(|name| !declared.contains(name))
        .collect();
    if !missing.is_empty() {
        return Err(ScanError {
            location: "crates/otio-sdk-model/src/schema.rs".to_string(),
            message: format!(
                "`OtioNodeKind` has these kinds, and the schema ladder does not say where they \
                 sit: {missing:?}. Add a rung for each, then the SDKs will carry them."
            ),
        });
    }

    let extra: Vec<&str> = declared
        .iter()
        .copied()
        .filter(|name| {
            !node_kind
                .variants
                .iter()
                .any(|variant| variant.name == *name)
        })
        .collect();
    if !extra.is_empty() {
        return Err(ScanError {
            location: "crates/otio-sdk-model/src/schema.rs".to_string(),
            message: format!(
                "the schema ladder has rungs `OtioNodeKind` no longer knows about: {extra:?}"
            ),
        });
    }

    Ok(LADDER
        .iter()
        .map(|rung| {
            let docs = node_kind
                .variants
                .iter()
                .find(|variant| variant.name == rung.kind)
                .map_or_else(Docs::default, |variant| variant.docs.clone());
            Schema {
                name: rung.kind.to_string(),
                kind: rung.kind.to_string(),
                parent: rung.parent.map(str::to_string),
                concrete: rung.concrete,
                docs,
            }
        })
        .collect())
}

/// The schema one derives from, if any.
///
/// Answered from the table rather than from a built ladder, so the collision
/// check can use it while the ladder is still being assembled.
#[must_use]
pub fn parent_of(schema: &str) -> Option<&'static str> {
    LADDER
        .iter()
        .find(|rung| rung.kind == schema)
        .and_then(|rung| rung.parent)
}
