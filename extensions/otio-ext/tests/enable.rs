//! Installing and enabling extensions.
//!
//! The registry is process-wide and tests in one file share a process, so
//! every test uses ids no other test touches.

use std::sync::atomic::{AtomicUsize, Ordering};

use otio_core::Node;
use otio_core::json::Value;
use otio_core::registry::DynamicBase;
use otio_ext::{Error, Extension};

struct Plain {
    id: &'static str,
    depends_on: &'static [&'static str],
    schemas: &'static [&'static str],
    calls: AtomicUsize,
}

impl Plain {
    const fn new(
        id: &'static str,
        depends_on: &'static [&'static str],
        schemas: &'static [&'static str],
    ) -> Self {
        Self {
            id,
            depends_on,
            schemas,
            calls: AtomicUsize::new(0),
        }
    }
}

impl Extension for Plain {
    fn id(&self) -> &'static str {
        self.id
    }
    fn version(&self) -> &'static str {
        "1.0.0"
    }
    fn summary(&self) -> &'static str {
        "A test extension."
    }
    fn depends_on(&self) -> &'static [&'static str] {
        self.depends_on
    }
    fn schemas(&self) -> &'static [&'static str] {
        self.schemas
    }
    fn register(&self) -> Result<(), String> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        for schema in self.schemas {
            otio_core::registry::register_type(
                schema,
                1,
                DynamicBase::SerializableObjectWithMetadata,
            );
        }
        Ok(())
    }
}

#[test]
fn enabling_runs_dependencies_first_and_only_once() {
    static BASE: Plain = Plain::new("order-base", &[], &[]);
    static TOP: Plain = Plain::new("order-top", &["order-base"], &[]);
    otio_ext::install(&TOP).unwrap();
    otio_ext::install(&BASE).unwrap();
    assert!(!otio_ext::is_enabled("order-top"));

    otio_ext::enable("order-top").unwrap();
    otio_ext::enable("order-top").unwrap();
    otio_ext::enable("order-base").unwrap();

    let enabled = otio_ext::enabled();
    let base = enabled.iter().position(|id| *id == "order-base").unwrap();
    let top = enabled.iter().position(|id| *id == "order-top").unwrap();
    assert!(base < top);
    assert_eq!(BASE.calls.load(Ordering::SeqCst), 1);
    assert_eq!(TOP.calls.load(Ordering::SeqCst), 1);
}

#[test]
fn an_unknown_extension_is_refused_by_name() {
    let error = otio_ext::enable("no-such-extension").unwrap_err();
    assert_eq!(error, Error::Unknown("no-such-extension".to_string()));
    assert!(error.to_string().contains("'no-such-extension'"));
}

#[test]
fn a_missing_dependency_is_refused_and_nothing_is_enabled() {
    static ORPHAN: Plain = Plain::new("orphan", &["orphan-parent"], &[]);
    otio_ext::install(&ORPHAN).unwrap();
    let error = otio_ext::enable("orphan").unwrap_err();
    assert_eq!(error, Error::Unknown("orphan-parent".to_string()));
    assert!(!otio_ext::is_enabled("orphan"));
    assert_eq!(ORPHAN.calls.load(Ordering::SeqCst), 0);
}

#[test]
fn a_dependency_cycle_is_refused() {
    static LEFT: Plain = Plain::new("cycle-left", &["cycle-right"], &[]);
    static RIGHT: Plain = Plain::new("cycle-right", &["cycle-left"], &[]);
    otio_ext::install(&LEFT).unwrap();
    otio_ext::install(&RIGHT).unwrap();
    let error = otio_ext::enable("cycle-left").unwrap_err();
    assert_eq!(
        error,
        Error::Cycle(vec![
            "cycle-left".to_string(),
            "cycle-right".to_string(),
            "cycle-left".to_string()
        ])
    );
}

#[test]
fn installing_twice_is_harmless_but_a_second_extension_under_one_id_is_not() {
    static FIRST: Plain = Plain::new("twice", &[], &[]);
    static IMPOSTOR: Plain = Plain::new("twice", &[], &[]);
    otio_ext::install(&FIRST).unwrap();
    otio_ext::install(&FIRST).unwrap();
    assert_eq!(
        otio_ext::install(&IMPOSTOR).unwrap_err(),
        Error::Conflict("twice".to_string())
    );
}

#[test]
fn a_schema_belongs_to_one_extension() {
    static OWNER: Plain = Plain::new("claim-owner", &[], &["ClaimedSchema"]);
    static RIVAL: Plain = Plain::new("claim-rival", &[], &["ClaimedSchema"]);
    otio_ext::install(&OWNER).unwrap();
    assert_eq!(otio_ext::owner_of("ClaimedSchema"), Some("claim-owner"));
    assert!(matches!(
        otio_ext::install(&RIVAL),
        Err(Error::SchemaClaimed { .. })
    ));
    assert!(!otio_ext::available().contains(&"claim-rival"));
}

#[test]
fn an_extension_schema_round_trips_unchanged_while_off_and_reads_as_its_own_once_on() {
    static NOTES: Plain = Plain::new("notes", &[], &["ExtNote"]);
    otio_ext::install(&NOTES).unwrap();

    let file = r#"{
    "OTIO_SCHEMA": "ExtNote.1",
    "metadata": {},
    "name": "a note",
    "points": [1, 2, 3],
    "text": "hello"
}"#;

    let off = otio_core::from_str(file).unwrap();
    assert!(matches!(
        off.get(off.root().unwrap()),
        Some(Node::Unknown(_))
    ));
    let rewritten = otio_core::to_string_pretty(&off, 4).unwrap();
    assert_eq!(
        otio_core::json::parse(&rewritten).unwrap(),
        otio_core::json::parse(file).unwrap()
    );

    otio_ext::enable("notes").unwrap();
    let on = otio_core::from_str(file).unwrap();
    assert!(matches!(on.get(on.root().unwrap()), Some(Node::Dynamic(_))));
    // Upstream writes a registered schema's own fields before its name and
    // metadata, so only the order of the keys may change.
    let rewritten = otio_core::to_string_pretty(&on, 4).unwrap();
    assert_eq!(
        sorted(otio_core::json::parse(&rewritten).unwrap()),
        sorted(otio_core::json::parse(file).unwrap())
    );
}

fn sorted(value: Value) -> Value {
    match value {
        Value::Object(mut entries) => {
            entries.sort_by(|left, right| left.0.cmp(&right.0));
            Value::Object(
                entries
                    .into_iter()
                    .map(|(key, value)| (key, sorted(value)))
                    .collect(),
            )
        }
        Value::Array(items) => Value::Array(items.into_iter().map(sorted).collect()),
        other => other,
    }
}
