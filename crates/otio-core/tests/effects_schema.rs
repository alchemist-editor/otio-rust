//! `Effect.2` and `Transition.2`, the effects schema this library prototypes
//! from an unratified upstream proposal (ADR 0013): their baselines, and the
//! downgrade and upgrade between versions 2 and 1.

use otio_core::registry::SchemaVersionMap;
use otio_core::schema::{EffectData, Node, Transition};
use otio_core::{Any, AnyDictionary, Document, NodeId, WriteOptions};

fn targets(entries: &[(&str, u32)]) -> WriteOptions {
    WriteOptions {
        schema_version_targets: entries
            .iter()
            .map(|(name, version)| ((*name).to_string(), *version))
            .collect::<SchemaVersionMap>(),
        ..WriteOptions::default()
    }
}

fn written(document: &Document, options: &WriteOptions) -> String {
    let root = document.root().expect("a root");
    otio_core::to_string_with(document, &Any::Object(root), options).expect("writes")
}

fn alone(node: Node) -> (Document, NodeId) {
    let mut document = Document::new();
    let id = document.insert(node);
    document.set_root(Some(id));
    (document, id)
}

fn dictionary(entries: &[(&str, Any)]) -> AnyDictionary {
    entries
        .iter()
        .map(|(key, value)| ((*key).to_string(), value.clone()))
        .collect()
}

/// A fully populated `Effect.2`.
fn full_effect() -> EffectData {
    let mut effect = EffectData::new();
    effect.base.name = "blur".into();
    effect.effect_name = "Gaussian Blur".into();
    effect.effect_id = "otio.fx.gaussian_blur".into();
    effect.effect_version = "1.0".into();
    effect.host = "Premiere Pro".into();
    effect.host_version = "25.1".into();
    effect.context = "filter".into();
    effect.parameters = dictionary(&[("amount", Any::Double(0.5))]);
    effect
}

#[test]
fn an_empty_effect_is_written_as_the_effect_2_baseline() {
    let (document, _) = alone(Node::Effect(EffectData::new()));
    assert_eq!(
        written(&document, &WriteOptions::default()),
        r#"{
    "OTIO_SCHEMA": "Effect.2",
    "metadata": {},
    "name": "",
    "effect_name": "",
    "enabled": true,
    "effect_id": "",
    "effect_version": "",
    "host": "",
    "host_version": "",
    "context": "",
    "parameters": {}
}
"#
    );
}

#[test]
fn an_empty_transition_is_written_as_the_transition_2_baseline() {
    let (document, _) = alone(Node::Transition(Transition {
        enabled: true,
        ..Transition::default()
    }));
    assert_eq!(
        written(&document, &WriteOptions::default()),
        r#"{
    "OTIO_SCHEMA": "Transition.2",
    "metadata": {},
    "name": "",
    "in_offset": {
        "OTIO_SCHEMA": "RationalTime.1",
        "rate": 1.0,
        "value": 0.0
    },
    "out_offset": {
        "OTIO_SCHEMA": "RationalTime.1",
        "rate": 1.0,
        "value": 0.0
    },
    "transition_type": "",
    "enabled": true,
    "effect": null
}
"#
    );
}

#[test]
fn the_time_effects_stay_at_version_1_and_carry_the_new_fields() {
    let (document, _) = alone(Node::LinearTimeWarp {
        effect: EffectData {
            effect_name: "LinearTimeWarp".into(),
            ..EffectData::new()
        },
        time_scalar: 2.0,
    });
    let text = written(&document, &WriteOptions::default());
    assert!(text.contains("\"OTIO_SCHEMA\": \"LinearTimeWarp.1\""));
    assert!(text.contains(
        "\"enabled\": true,\n    \"effect_id\": \"\",\n    \"effect_version\": \"\",\n    \
         \"host\": \"\",\n    \"host_version\": \"\",\n    \"context\": \"\",\n    \
         \"parameters\": {},\n    \"time_scalar\": 2.0"
    ));
    for schema in ["TimeEffect", "FreezeFrame"] {
        let text = format!(r#"{{"OTIO_SCHEMA": "{schema}.1", "effect_name": "", "host": "x"}}"#);
        let document = otio_core::from_str(&text).unwrap();
        let root = document.root().unwrap();
        assert_eq!(document.try_get(root).unwrap().effect().unwrap().host, "x");
        assert!(written(&document, &WriteOptions::default()).contains(&format!("{schema}.1")));
    }
}

#[test]
fn every_field_round_trips() {
    let mut document = Document::new();
    let effect = document.insert(Node::Effect(full_effect()));
    let transition = document.insert(Node::Transition(Transition {
        transition_type: "SMPTE_Dissolve".into(),
        enabled: true,
        effect: Some(effect),
        ..Transition::default()
    }));
    document.set_root(Some(transition));
    let text = written(&document, &WriteOptions::default());
    let read = otio_core::from_str(&text).unwrap();
    assert_eq!(written(&read, &WriteOptions::default()), text);
    let Node::Transition(transition) = read.try_get(read.root().unwrap()).unwrap() else {
        panic!("a transition");
    };
    let effect = read.try_get(transition.effect.unwrap()).unwrap();
    assert_eq!(effect.effect().unwrap(), &full_effect());
}

/// `metadata["otio.fx"]` of what `text` holds at its top level.
fn otio_fx(text: &str) -> Option<otio_core::json::Value> {
    let value = otio_core::json::parse(text).unwrap();
    let object = value.as_object().unwrap();
    let metadata = object
        .iter()
        .find(|(key, _)| key == "metadata")?
        .1
        .as_object()?;
    metadata
        .iter()
        .find(|(key, _)| key == "otio.fx")
        .map(|(_, value)| value.clone())
}

#[test]
fn an_effect_goes_down_to_version_1_and_back_without_loss() {
    let mut effect = full_effect();
    effect.base.metadata.insert(
        "otio.fx".into(),
        Any::Dictionary(dictionary(&[("fidelity", "approximate".into())])),
    );
    effect
        .base
        .metadata
        .insert("studio".into(), Any::from("kept"));
    let (document, _) = alone(Node::Effect(effect.clone()));

    let down = written(&document, &targets(&[("Effect", 1)]));
    assert!(down.contains("\"OTIO_SCHEMA\": \"Effect.1\""));
    for key in [
        "effect_id",
        "effect_version",
        "host",
        "host_version",
        "context",
        "parameters",
    ] {
        assert!(
            !down.contains(&format!("\n    \"{key}\"")),
            "{key} left the top level"
        );
    }
    let stashed = otio_fx(&down).expect("the fields are kept in otio.fx");
    let stashed = stashed.as_object().unwrap();
    let keys: Vec<&str> = stashed.iter().map(|(key, _)| key.as_str()).collect();
    for key in [
        "fidelity",
        "effect_id",
        "effect_version",
        "host",
        "host_version",
        "context",
        "parameters",
    ] {
        assert!(keys.contains(&key), "otio.fx keeps {key}");
    }

    let up = otio_core::from_str(&down).unwrap();
    let node = up.try_get(up.root().unwrap()).unwrap();
    assert_eq!(node.effect().unwrap(), &effect);
}

#[test]
fn an_empty_effect_goes_down_with_nothing_added() {
    let (document, _) = alone(Node::Effect(EffectData {
        effect_name: "Blur".into(),
        ..EffectData::new()
    }));
    let down = written(&document, &targets(&[("Effect", 1)]));
    // A downgraded object is written from a dictionary, so in key order, as
    // upstream writes it.
    assert_eq!(
        down,
        r#"{
    "OTIO_SCHEMA": "Effect.1",
    "effect_name": "Blur",
    "enabled": true,
    "metadata": {},
    "name": ""
}
"#
    );
    let up = otio_core::from_str(&down).unwrap();
    assert_eq!(
        written(&up, &WriteOptions::default()),
        written(&document, &WriteOptions::default())
    );
}

#[test]
fn only_what_holds_something_is_stashed_and_otio_fx_goes_when_emptied() {
    let (document, _) = alone(Node::Effect(EffectData {
        host: "Resolve".into(),
        ..EffectData::new()
    }));
    let down = written(&document, &targets(&[("Effect", 1)]));
    let stashed = otio_fx(&down).unwrap();
    let keys: Vec<&str> = stashed
        .as_object()
        .unwrap()
        .iter()
        .map(|(key, _)| key.as_str())
        .collect();
    assert_eq!(keys, ["host"]);

    let up = otio_core::from_str(&down).unwrap();
    let node = up.try_get(up.root().unwrap()).unwrap();
    assert_eq!(node.effect().unwrap().host, "Resolve");
    assert!(node.base().unwrap().metadata.is_empty());
}

#[test]
fn a_transition_goes_down_with_its_effect_in_otio_fx_and_back() {
    let mut document = Document::new();
    let mut effect = full_effect();
    effect.base.metadata.insert(
        "otio.fx".into(),
        Any::Dictionary(dictionary(&[("fidelity", "exact".into())])),
    );
    let effect_id = document.insert(Node::Effect(effect.clone()));
    let mut transition = Transition {
        transition_type: "SMPTE_Dissolve".into(),
        enabled: true,
        effect: Some(effect_id),
        ..Transition::default()
    };
    transition.base.metadata.insert(
        "otio.fx".into(),
        Any::Dictionary(dictionary(&[("fidelity", "approximate".into())])),
    );
    let root = document.insert(Node::Transition(transition));
    document.set_root(Some(root));

    // Both downgraded, as the 0.18.1 release map would have it: the effect
    // first, so the transition keeps an `Effect.1`.
    let releases = otio_core::registry::release_to_schema_version_map();
    let options = WriteOptions {
        schema_version_targets: releases["0.18.1"].clone(),
        ..WriteOptions::default()
    };
    let down = written(&document, &options);
    assert!(down.contains("\"OTIO_SCHEMA\": \"Transition.1\""));
    assert!(down.contains("\"OTIO_SCHEMA\": \"Effect.1\""));
    assert!(!down.contains("\"effect\": null"));
    let stashed = otio_fx(&down).unwrap();
    let keys: Vec<&str> = stashed
        .as_object()
        .unwrap()
        .iter()
        .map(|(key, _)| key.as_str())
        .collect();
    assert!(keys.contains(&"fidelity") && keys.contains(&"effect"));

    let up = otio_core::from_str(&down).unwrap();
    let Node::Transition(read) = up.try_get(up.root().unwrap()).unwrap() else {
        panic!("a transition");
    };
    assert_eq!(
        read.base.metadata["otio.fx"],
        Any::Dictionary(dictionary(&[("fidelity", "approximate".into())]))
    );
    let read_effect = up.try_get(read.effect.unwrap()).unwrap();
    assert!(matches!(read_effect, Node::Effect(_)));
    assert_eq!(read_effect.effect().unwrap(), &effect);
}

#[test]
fn a_transition_without_an_effect_goes_down_with_nothing_added() {
    let (document, _) = alone(Node::Transition(Transition {
        enabled: true,
        ..Transition::default()
    }));
    let down = written(&document, &targets(&[("Transition", 1)]));
    assert!(down.contains("\"OTIO_SCHEMA\": \"Transition.1\""));
    assert!(!down.contains("effect"));
    assert!(otio_fx(&down).is_none());
}

#[test]
fn a_version_1_transition_reads_with_no_effect() {
    let document = otio_core::from_str(
        r#"{"OTIO_SCHEMA": "Transition.1", "transition_type": "SMPTE_Dissolve",
            "metadata": {"otio.fx": {"fidelity": "exact"}}}"#,
    )
    .unwrap();
    let Node::Transition(transition) = document.try_get(document.root().unwrap()).unwrap() else {
        panic!("a transition");
    };
    assert_eq!(transition.effect, None);
    // What is not one of the moved fields stays where it was.
    assert!(transition.base.metadata.contains_key("otio.fx"));
}

#[test]
fn a_transition_holding_something_else_is_refused() {
    let error = otio_core::from_str(
        r#"{"OTIO_SCHEMA": "Transition.2", "effect": {"OTIO_SCHEMA": "Clip.2"}}"#,
    );
    assert!(error.is_err());
}

#[test]
fn a_transitions_effect_is_owned_copied_and_removed_with_it() {
    let mut document = Document::new();
    let effect = document.insert(Node::Effect(full_effect()));
    let transition = document.insert(Node::Transition(Transition {
        effect: Some(effect),
        ..Transition::default()
    }));
    assert_eq!(document.owner_of(effect), Some(transition));

    let copy = document.deep_clone(transition).unwrap();
    let Node::Transition(copied) = document.try_get(copy).unwrap() else {
        panic!("a transition");
    };
    let copied_effect = copied.effect.unwrap();
    assert_ne!(copied_effect, effect);
    assert_eq!(
        document.try_get(copied_effect).unwrap().effect().unwrap(),
        &full_effect()
    );

    document.remove_recursive(transition).unwrap();
    assert!(!document.contains(effect));
    assert!(document.contains(copied_effect));
}
