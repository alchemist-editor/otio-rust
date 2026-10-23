//! The reference effects linker, `otio_fx_registry`.
//!
//! The effects schema proposal this library prototypes (ADR 0013) has a
//! common vocabulary of effects, the `otio.fx.*` registry, and this linker
//! rewrites a host's own effects into it using the mapping table the
//! registry document carries. The document, [`REGISTRY_JSON`], is vendored
//! byte for byte from the proposal; a read can name another with the
//! `registry_path` argument.
//!
//! Each mapping names a registry `effect_id`, a `match` whose keys must all
//! hold, an optional `fidelity`, and an optional `parameter_map`:
//!
//! ```json
//! {
//!     "effect_id": "otio.fx.gaussian_blur",
//!     "match": {"effect_name": "Gaussian Blur", "metadata_key": "fcp_xml"},
//!     "fidelity": "approximate",
//!     "parameter_map": {"amount": {"from": "radius", "scale": 0.01}}
//! }
//! ```
//!
//! `effect_id`, `effect_name` and `host` match an effect whose field is
//! equal; `metadata_key` one whose metadata has that key. Any other key
//! never matches, so a newer table cannot match more than it means to.
//!
//! A linked effect keeps what it was in `metadata["otio.fx"]["linked_from"]`
//! and how faithful the mapping is in `metadata["otio.fx"]["fidelity"]`. Its
//! `host` and `host_version` are left alone.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

use otio_core::{Any, AnyDictionary, Document, Node, NodeId};

use crate::plugins::EffectsLinker;

/// The linker's name, as the built-in manifest lists it.
pub const LINKER_NAME: &str = "otio_fx_registry";

/// The prefix of every registry `effect_id`. An effect already carrying one
/// is left alone.
pub const REGISTRY_PREFIX: &str = "otio.fx.";

/// The argument naming a registry document to use in place of
/// [`REGISTRY_JSON`].
pub const REGISTRY_PATH_ARGUMENT: &str = "registry_path";

/// The registry document the linker uses unless told otherwise, vendored
/// byte for byte from the proposal. The Python package ships the same bytes
/// beside its `otio_fx_registry.py`, and a test checks they agree.
pub const REGISTRY_JSON: &str = include_str!("../data/otio_fx_registry.json");

/// The reference linker, ready to register.
#[must_use]
pub fn linker() -> EffectsLinker {
    EffectsLinker::new(|document, effect, _parent, arguments| {
        link_effect(document, effect, arguments)
    })
}

/// A parsed registry document, as plain values.
type Registry = Arc<AnyDictionary>;

/// Parses a registry document.
///
/// # Errors
///
/// Returns why, if `json` is not a JSON object.
pub fn parse_registry(json: &str) -> Result<AnyDictionary, String> {
    match otio_core::from_str_any(json) {
        Ok((_, Any::Dictionary(registry))) => Ok(registry),
        Ok(_) => Err("the registry is not a JSON object".to_owned()),
        Err(error) => Err(format!("the registry could not be read: {error}")),
    }
}

/// The registry at `path`, or the vendored one, each read once.
fn load(path: Option<&str>) -> Result<Registry, String> {
    static CACHE: OnceLock<Mutex<HashMap<String, Registry>>> = OnceLock::new();
    let key = path.unwrap_or_default().to_owned();
    let cache = CACHE.get_or_init(Mutex::default);
    if let Some(found) = cache
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .get(&key)
    {
        return Ok(found.clone());
    }
    let parsed = match path {
        None => parse_registry(REGISTRY_JSON)?,
        Some(path) => {
            let text = std::fs::read_to_string(path)
                .map_err(|error| format!("could not read the registry at {path}: {error}"))?;
            parse_registry(&text)?
        }
    };
    let parsed = Arc::new(parsed);
    cache
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .insert(key, parsed.clone());
    Ok(parsed)
}

fn list<'a>(dictionary: &'a AnyDictionary, key: &str) -> &'a [Any] {
    match dictionary.get(key) {
        Some(Any::Vector(items)) => items,
        _ => &[],
    }
}

fn text<'a>(dictionary: &'a AnyDictionary, key: &str) -> Option<&'a str> {
    dictionary.get(key).and_then(Any::as_str)
}

/// A number as a float, if it is one; a bool is not.
fn number(value: &Any) -> Option<f64> {
    match value {
        Any::Int(value) => Some(*value as f64),
        Any::UInt(value) => Some(*value as f64),
        Any::Double(value) => Some(*value),
        _ => None,
    }
}

/// Whether every key of `rule` holds for an effect.
fn matches(rule: &AnyDictionary, node: &Node) -> bool {
    let Some(effect) = node.effect() else {
        return false;
    };
    rule.iter().all(|(key, expected)| match key.as_str() {
        "metadata_key" => expected
            .as_str()
            .is_some_and(|key| effect.base.metadata.contains_key(key)),
        "effect_id" => expected.as_str() == Some(effect.effect_id.as_str()),
        "effect_name" => expected.as_str() == Some(effect.effect_name.as_str()),
        "host" => expected.as_str() == Some(effect.host.as_str()),
        _ => false,
    })
}

/// The new parameters: the entry's defaults, then what the mapping's
/// `parameter_map` takes from the effect's own.
fn mapped_parameters(
    entry: &AnyDictionary,
    mapping: &AnyDictionary,
    native: &AnyDictionary,
) -> AnyDictionary {
    let mut parameters = AnyDictionary::new();
    for parameter in list(entry, "parameters") {
        let Any::Dictionary(parameter) = parameter else {
            continue;
        };
        if let (Some(key), Some(default)) = (text(parameter, "key"), parameter.get("default")) {
            parameters.insert(key.to_owned(), default.clone());
        }
    }
    let Some(Any::Dictionary(map)) = mapping.get("parameter_map") else {
        return parameters;
    };
    for (key, rule) in map {
        let Any::Dictionary(rule) = rule else {
            continue;
        };
        let from = text(rule, "from").unwrap_or(key);
        let Some(value) = native.get(from) else {
            continue;
        };
        let value = match number(value) {
            Some(value_f64) => {
                let scale = rule.get("scale");
                let offset = rule.get("offset");
                // Python keeps integer arithmetic integral, so a whole
                // number scaled and offset by whole numbers stays one.
                match (value, scale, offset) {
                    (Any::Int(value), Some(Any::Int(scale)), Some(Any::Int(offset))) => value
                        .checked_mul(*scale)
                        .and_then(|scaled| scaled.checked_add(*offset))
                        .map_or_else(
                            || Any::Double(*value as f64 * *scale as f64 + *offset as f64),
                            Any::Int,
                        ),
                    _ => Any::Double(
                        value_f64 * scale.and_then(number).unwrap_or(1.0)
                            + offset.and_then(number).unwrap_or(0.0),
                    ),
                }
            }
            None => value.clone(),
        };
        parameters.insert(key.clone(), value);
    }
    parameters
}

/// What the linker does with one effect: see the module documentation.
///
/// Returns the same effect, changed, or nothing if it leaves it alone: a
/// time effect, an effect already in the registry, and an effect no mapping
/// matches.
///
/// # Errors
///
/// Returns why, if the registry cannot be read or `effect` is not an effect
/// in `document`.
pub fn link_effect(
    document: &mut Document,
    effect: NodeId,
    arguments: &AnyDictionary,
) -> Result<Option<NodeId>, String> {
    let node = document
        .try_get(effect)
        .map_err(|error| error.to_string())?;
    let Some(data) = node.effect() else {
        return Err(format!("a {} is not an effect", node.schema_name()));
    };
    if !matches!(node, Node::Effect(_)) || data.effect_id.starts_with(REGISTRY_PREFIX) {
        // Retimes stay out of the registry.
        return Ok(None);
    }
    let registry = load(arguments.get(REGISTRY_PATH_ARGUMENT).and_then(Any::as_str))?;
    let Some(mapping) = list(&registry, "mappings")
        .iter()
        .find_map(|mapping| match mapping {
            Any::Dictionary(mapping) => {
                let rule = match mapping.get("match") {
                    Some(Any::Dictionary(rule)) => rule.clone(),
                    _ => AnyDictionary::new(),
                };
                matches(&rule, node).then_some(mapping)
            }
            _ => None,
        })
    else {
        return Ok(None);
    };
    let Some(target) = text(mapping, "effect_id") else {
        return Ok(None);
    };
    let Some(entry) = list(&registry, "effects")
        .iter()
        .find_map(|entry| match entry {
            Any::Dictionary(entry) if text(entry, "effect_id") == Some(target) => Some(entry),
            _ => None,
        })
    else {
        return Ok(None);
    };

    let parameters = mapped_parameters(entry, mapping, &data.parameters);
    let mut linked_from = AnyDictionary::new();
    linked_from.insert("effect_id".into(), data.effect_id.clone().into());
    linked_from.insert("effect_version".into(), data.effect_version.clone().into());
    linked_from.insert(
        "parameters".into(),
        Any::Dictionary(data.parameters.clone()),
    );
    linked_from.insert("linker".into(), LINKER_NAME.into());
    let fidelity = text(mapping, "fidelity")
        .unwrap_or("approximate")
        .to_owned();
    let effect_id = target.to_owned();
    let effect_version = text(entry, "effect_version").unwrap_or_default().to_owned();
    let context = text(entry, "context").unwrap_or_default().to_owned();

    let data = document
        .try_get_mut(effect)
        .map_err(|error| error.to_string())?
        .effect_mut()
        .ok_or_else(|| "the effect went away".to_owned())?;
    let otio_fx = data
        .base
        .metadata
        .entry(otio_core::upgrade::OTIO_FX_KEY.to_owned())
        .or_insert_with(|| Any::Dictionary(AnyDictionary::new()));
    if !matches!(otio_fx, Any::Dictionary(_)) {
        *otio_fx = Any::Dictionary(AnyDictionary::new());
    }
    if let Any::Dictionary(otio_fx) = otio_fx {
        otio_fx.insert("linked_from".into(), Any::Dictionary(linked_from));
        otio_fx.insert("fidelity".into(), fidelity.into());
    }
    data.effect_id = effect_id;
    data.effect_version = effect_version;
    data.parameters = parameters;
    if data.context.is_empty() {
        data.context = context;
    }
    Ok(Some(effect))
}
