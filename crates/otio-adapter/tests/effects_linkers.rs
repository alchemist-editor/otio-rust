//! Effects linkers, the `post_effects_linker` hook and the reference linker
//! `otio_fx_registry`: the effects schema prototype (ADR 0013).
//!
//! The registry is process-wide, so every test here takes [`LOCK`] and
//! starts from a fresh registry with the built-ins in it.

use std::sync::{Arc, Mutex, MutexGuard};

use otio_adapter::plugins::{
    self, EffectsLinker, HookScript, LinkerChoice, PluginArguments, Registry,
};
use otio_adapter::{Error, otio_fx_registry};
use otio_core::schema::{EffectData, Node};
use otio_core::{Any, AnyDictionary, Document, NodeId};

static LOCK: Mutex<()> = Mutex::new(());

fn fresh() -> MutexGuard<'static, ()> {
    let guard = LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    *plugins::registry() = Registry::with_built_ins();
    guard
}

/// Set in the child process [`the_default_is_the_one_the_environment_names`]
/// starts.
const CHILD: &str = "OTIO_ADAPTER_EFFECTS_TEST_CHILD";

/// A track carrying an effect, holding a clip with three effects, a
/// transition with one, and a second clip.
const TIMELINE: &str = r#"{
    "OTIO_SCHEMA": "Timeline.1",
    "name": "tl",
    "tracks": {
        "OTIO_SCHEMA": "Stack.1",
        "children": [{
            "OTIO_SCHEMA": "Track.1",
            "name": "V1",
            "kind": "Video",
            "effects": [{"OTIO_SCHEMA": "Effect.2", "effect_name": "TrackGlow"}],
            "children": [
                {"OTIO_SCHEMA": "Clip.2", "name": "shot_1", "effects": [
                    {"OTIO_SCHEMA": "Effect.2", "effect_name": "Blur"},
                    {"OTIO_SCHEMA": "Effect.2", "effect_name": "Leave Me"},
                    {"OTIO_SCHEMA": "LinearTimeWarp.1", "effect_name": "", "time_scalar": 2.0}
                ]},
                {"OTIO_SCHEMA": "Transition.2", "name": "trx",
                 "transition_type": "Custom_Transition",
                 "effect": {"OTIO_SCHEMA": "Effect.2", "effect_name": "Wipe"}},
                {"OTIO_SCHEMA": "Clip.2", "name": "shot_2"}
            ]
        }]
    }
}"#;

fn timeline() -> (Document, NodeId) {
    let document = otio_core::from_str(TIMELINE).expect("the timeline reads");
    let root = document.root().expect("it has a root");
    (document, root)
}

fn effect(document: &Document, id: NodeId) -> &EffectData {
    document.try_get(id).unwrap().effect().expect("an effect")
}

/// The track, the clip and the transition of [`TIMELINE`].
fn holders(document: &Document, root: NodeId) -> (NodeId, NodeId, NodeId) {
    let Node::Timeline(timeline) = document.try_get(root).unwrap() else {
        panic!("a timeline");
    };
    let stack = timeline.tracks.unwrap();
    let track = document.try_get(stack).unwrap().children().unwrap()[0];
    let children = document.try_get(track).unwrap().children().unwrap();
    (track, children[0], children[1])
}

fn effects_of(document: &Document, item: NodeId) -> Vec<NodeId> {
    document
        .try_get(item)
        .unwrap()
        .item()
        .unwrap()
        .effects
        .clone()
}

fn transition_effect(document: &Document, transition: NodeId) -> Option<NodeId> {
    match document.try_get(transition).unwrap() {
        Node::Transition(transition) => transition.effect,
        _ => panic!("a transition"),
    }
}

/// The spec's example linker: a new effect `test.fx.<name>` for every
/// effect but `Leave Me` and the time effects, its context from what holds
/// it, its metadata naming that and carrying the arguments.
fn example_linker() -> EffectsLinker {
    EffectsLinker::new(|document, effect, parent, arguments| {
        let node = document
            .try_get(effect)
            .map_err(|error| error.to_string())?;
        let data = node.effect().ok_or("not an effect")?;
        if data.effect_name == "Leave Me" || !matches!(node, Node::Effect(_)) {
            return Ok(None);
        }
        let mut linked = EffectData::new();
        linked.effect_name = data.effect_name.clone();
        linked.effect_id = format!("test.fx.{}", data.effect_name.to_lowercase());
        let parent = parent.ok_or("no parent")?;
        let parent = document
            .try_get(parent)
            .map_err(|error| error.to_string())?;
        linked.context = if matches!(parent, Node::Transition(_)) {
            "transition"
        } else {
            "filter"
        }
        .to_owned();
        linked
            .base
            .metadata
            .insert("linked_on".into(), parent.name().into());
        linked.base.metadata.extend(arguments.clone());
        Ok(Some(document.insert(Node::Effect(linked))))
    })
}

fn named(name: &str) -> LinkerChoice {
    LinkerChoice::Named(name.to_owned())
}

#[test]
fn the_reference_linker_is_built_in_but_not_the_default() {
    let _lock = fresh();
    assert_eq!(
        plugins::registry().effects_linker_names(),
        [plugins::OTIO_FX_REGISTRY]
    );
    assert!(Registry::new().effects_linker_names().is_empty());
    if std::env::var_os(plugins::DEFAULT_EFFECTS_LINKER_VARIABLE).is_some() {
        return;
    }
    let (mut document, root) = timeline();
    let before = otio_core::to_string(&document).unwrap();
    plugins::link_effects(
        &mut document,
        root,
        &LinkerChoice::Default,
        &AnyDictionary::new(),
    )
    .expect("no default links nothing");
    assert_eq!(otio_core::to_string(&document).unwrap(), before);
}

#[test]
fn a_named_linker_visits_tracks_clips_and_transitions() {
    let _lock = fresh();
    plugins::registry().register_effects_linker("example", example_linker());
    let (mut document, root) = timeline();
    let (track, clip, transition) = holders(&document, root);
    let originals = effects_of(&document, clip);
    let mut arguments = AnyDictionary::new();
    arguments.insert("site".into(), "test".into());

    plugins::link_effects(&mut document, root, &named("example"), &arguments).expect("links");

    let track_effect = effect(&document, effects_of(&document, track)[0]);
    assert_eq!(track_effect.effect_id, "test.fx.trackglow");
    assert_eq!(track_effect.base.metadata["linked_on"], Any::from("V1"));

    let clip_effects = effects_of(&document, clip);
    assert_eq!(clip_effects.len(), 3);
    let blur = effect(&document, clip_effects[0]);
    assert_eq!(blur.effect_id, "test.fx.blur");
    assert_eq!(blur.context, "filter");
    assert_eq!(blur.base.metadata["site"], Any::from("test"));
    // The replaced effect went; the ones kept are the very same objects.
    assert!(!document.contains(originals[0]));
    assert_eq!(clip_effects[1..], originals[1..]);
    assert_eq!(effect(&document, clip_effects[1]).effect_id, "");
    assert!(matches!(
        document.try_get(clip_effects[2]).unwrap(),
        Node::LinearTimeWarp { .. }
    ));

    let wipe = effect(&document, transition_effect(&document, transition).unwrap());
    assert_eq!(wipe.effect_id, "test.fx.wipe");
    assert_eq!(wipe.context, "transition");
}

#[test]
fn do_not_link_runs_nothing() {
    let _lock = fresh();
    plugins::registry().register_effects_linker("example", example_linker());
    let (mut document, root) = timeline();
    let before = otio_core::to_string(&document).unwrap();
    plugins::link_effects(
        &mut document,
        root,
        &LinkerChoice::DoNotLink,
        &AnyDictionary::new(),
    )
    .unwrap();
    assert_eq!(otio_core::to_string(&document).unwrap(), before);
}

#[test]
fn an_unknown_linker_is_refused() {
    let _lock = fresh();
    let (mut document, root) = timeline();
    let error = plugins::link_effects(
        &mut document,
        root,
        &named("elsewhere"),
        &AnyDictionary::new(),
    )
    .unwrap_err();
    assert!(matches!(error, Error::UnknownEffectsLinker { .. }));
    assert_eq!(
        error.to_string(),
        "effects linker not supported: elsewhere, available: ['otio_fx_registry']"
    );
}

#[test]
fn a_linker_returning_what_is_not_an_effect_fails() {
    let _lock = fresh();
    plugins::registry()
        .register_effects_linker("broken", EffectsLinker::new(|_, _, parent, _| Ok(parent)));
    let (mut document, root) = timeline();
    let error = plugins::link_effects(&mut document, root, &named("broken"), &AnyDictionary::new())
        .unwrap_err();
    assert_eq!(
        error.to_string(),
        "broken: the effects linker returned a Track, not an effect"
    );
}

#[test]
fn a_bare_clip_is_linked_too() {
    let _lock = fresh();
    plugins::registry().register_effects_linker("example", example_linker());
    let mut document = otio_core::from_str(
        r#"{"OTIO_SCHEMA": "Clip.2", "name": "solo",
            "effects": [{"OTIO_SCHEMA": "Effect.2", "effect_name": "Blur"}]}"#,
    )
    .unwrap();
    let root = document.root().unwrap();
    plugins::link_effects(
        &mut document,
        root,
        &named("example"),
        &AnyDictionary::new(),
    )
    .unwrap();
    let linked = effects_of(&document, root)[0];
    assert_eq!(effect(&document, linked).effect_id, "test.fx.blur");
}

#[test]
fn post_effects_linker_runs_after_the_linker_with_its_arguments() {
    let _lock = fresh();
    let seen = Arc::new(Mutex::new(Vec::new()));
    let record = seen.clone();
    {
        let mut registry = plugins::registry();
        registry.register_effects_linker("example", example_linker());
        registry.register_hook_script(
            "record",
            HookScript::new(move |document, target, arguments| {
                let mut keys: Vec<String> = arguments.keys().cloned().collect();
                keys.sort();
                let (_, clip, _) = holders(document, target);
                let first = effects_of(document, clip)[0];
                record
                    .lock()
                    .unwrap()
                    .push((keys, effect(document, first).effect_id.clone()));
                Ok(target)
            }),
        );
        registry.attach_hook_script(plugins::POST_EFFECTS_LINKER, "record");
    }
    let (mut document, _) = timeline();
    let mut arguments = PluginArguments {
        effects_linker: named("example"),
        ..PluginArguments::default()
    };
    arguments
        .effects_linker_arguments
        .insert("site".into(), "test".into());
    plugins::after_read(&mut document, &arguments, AnyDictionary::new()).expect("it runs");
    assert_eq!(
        *seen.lock().unwrap(),
        [(vec!["site".to_owned()], "test.fx.blur".to_owned())]
    );
}

#[test]
fn anything_to_run_counts_the_effects_linker_and_its_hook() {
    let _lock = fresh();
    if std::env::var_os(plugins::DEFAULT_EFFECTS_LINKER_VARIABLE).is_some()
        || std::env::var_os(plugins::DEFAULT_MEDIA_LINKER_VARIABLE).is_some()
    {
        return;
    }
    assert!(!plugins::anything_to_run_after_read_with(
        &LinkerChoice::Default,
        &LinkerChoice::Default
    ));
    assert!(plugins::anything_to_run_after_read_with(
        &LinkerChoice::Default,
        &named(plugins::OTIO_FX_REGISTRY)
    ));
    plugins::registry().attach_hook_script(plugins::POST_EFFECTS_LINKER, "nothing");
    assert!(plugins::anything_to_run_after_read(&LinkerChoice::Default));
}

#[test]
fn the_default_is_the_one_the_environment_names() {
    let status = std::process::Command::new(std::env::current_exe().expect("the test binary"))
        .args(["--exact", "default_in_a_child", "--test-threads", "1"])
        .env(CHILD, "1")
        .env(plugins::DEFAULT_EFFECTS_LINKER_VARIABLE, "example")
        .status()
        .expect("the child runs");
    assert!(status.success());
}

/// The body of [`the_default_is_the_one_the_environment_names`].
#[test]
fn default_in_a_child() {
    if std::env::var_os(CHILD).is_none() {
        return;
    }
    let _lock = fresh();
    plugins::registry().register_effects_linker("example", example_linker());
    let (mut document, root) = timeline();
    let (_, clip, _) = holders(&document, root);
    plugins::link_effects(
        &mut document,
        root,
        &LinkerChoice::DoNotLink,
        &AnyDictionary::new(),
    )
    .unwrap();
    assert_eq!(
        effect(&document, effects_of(&document, clip)[0]).effect_id,
        ""
    );
    let arguments = PluginArguments::default();
    plugins::after_read(&mut document, &arguments, AnyDictionary::new()).unwrap();
    assert_eq!(
        effect(&document, effects_of(&document, clip)[0]).effect_id,
        "test.fx.blur"
    );
}

// --- The reference linker -------------------------------------------------

/// Runs the reference linker on one effect read from `json`, returning the
/// document, the effect, and whether the linker took it.
fn reference(json: &str, arguments: &AnyDictionary) -> (Document, NodeId, bool) {
    let mut document = otio_core::from_str(json).expect("the effect reads");
    let id = document.root().unwrap();
    let linker = plugins::registry()
        .effects_linker(plugins::OTIO_FX_REGISTRY)
        .cloned()
        .expect("built in");
    let linked = linker
        .link(&mut document, id, None, arguments)
        .expect("it runs");
    assert!(
        linked.is_none_or(|linked| linked == id),
        "it changes the effect in place"
    );
    (document, id, linked.is_some())
}

/// A registry file holding the vendored registry with `mappings` in place
/// of its own, and the arguments naming it.
fn custom_registry(name: &str, mappings: &str) -> (std::path::PathBuf, AnyDictionary) {
    let mut registry = otio_fx_registry::parse_registry(otio_fx_registry::REGISTRY_JSON).unwrap();
    let (_, mappings) = otio_core::from_str_any(mappings).unwrap();
    registry.insert("mappings".into(), mappings);
    let path = std::env::temp_dir().join(format!(
        "otio-fx-registry-{}-{name}.json",
        std::process::id()
    ));
    std::fs::write(
        &path,
        otio_core::to_string_any_pretty(&Document::new(), &Any::Dictionary(registry), 4).unwrap(),
    )
    .unwrap();
    let mut arguments = AnyDictionary::new();
    arguments.insert(
        "registry_path".into(),
        path.to_string_lossy().into_owned().into(),
    );
    (path, arguments)
}

#[test]
fn the_vendored_registry_is_the_one_python_ships() {
    let python =
        include_str!("../../otio-python/python/opentimelineio/adapters/otio_fx_registry.json");
    assert_eq!(otio_fx_registry::REGISTRY_JSON, python);
    let registry = otio_fx_registry::parse_registry(otio_fx_registry::REGISTRY_JSON).unwrap();
    let Any::Vector(effects) = &registry["effects"] else {
        panic!("a list of effects");
    };
    assert_eq!(effects.len(), 17);
}

#[test]
fn the_reference_linker_maps_by_name() {
    let _lock = fresh();
    let (document, id, linked) = reference(
        r#"{"OTIO_SCHEMA": "Effect.2", "effect_name": "Gaussian Blur",
            "host": "Premiere Pro", "parameters": {"Blurriness": 30.0}}"#,
        &AnyDictionary::new(),
    );
    assert!(linked);
    let data = effect(&document, id);
    assert_eq!(data.effect_id, "otio.fx.gaussian_blur");
    assert_eq!(data.effect_version, "1.0");
    assert_eq!(data.context, "filter");
    assert_eq!(data.host, "Premiere Pro");
    assert_eq!(data.parameters["amount"], Any::Double(0.0));
    assert_eq!(data.parameters["edge_mode"], Any::from("transparent"));
    assert_eq!(data.parameters["horizontal"], Any::Bool(true));
    assert!(!data.parameters.contains_key("Blurriness"));

    let Any::Dictionary(otio_fx) = &data.base.metadata["otio.fx"] else {
        panic!("otio.fx is a dictionary");
    };
    assert_eq!(otio_fx["fidelity"], Any::from("approximate"));
    let Any::Dictionary(from) = &otio_fx["linked_from"] else {
        panic!("linked_from is a dictionary");
    };
    assert_eq!(from["effect_id"], Any::from(""));
    assert_eq!(from["effect_version"], Any::from(""));
    assert_eq!(from["linker"], Any::from("otio_fx_registry"));
    let Any::Dictionary(parameters) = &from["parameters"] else {
        panic!("the original parameters");
    };
    assert_eq!(parameters["Blurriness"], Any::Double(30.0));
}

#[test]
fn a_transition_keeps_a_context_it_had() {
    let _lock = fresh();
    let (document, id, linked) = reference(
        r#"{"OTIO_SCHEMA": "Effect.2", "effect_name": "Cross Dissolve", "context": "filter"}"#,
        &AnyDictionary::new(),
    );
    assert!(linked);
    let data = effect(&document, id);
    assert_eq!(data.effect_id, "otio.fx.dissolve");
    assert_eq!(data.context, "filter");
    assert_eq!(data.parameters["reverse"], Any::Bool(false));
}

#[test]
fn the_reference_linker_maps_parameters_and_needs_its_metadata_key() {
    let _lock = fresh();
    let (path, arguments) = custom_registry(
        "parameters",
        r#"[{
            "effect_id": "otio.fx.gaussian_blur",
            "match": {"effect_name": "Blur", "metadata_key": "fcp_xml"},
            "fidelity": "exact",
            "parameter_map": {
                "amount": {"from": "radius", "scale": 0.01, "offset": 0.5},
                "horizontal": {"from": "h"},
                "edge_mode": {"from": "edges"}
            }
        }]"#,
    );

    // Without the metadata key, nothing matches.
    let (document, id, linked) = reference(
        r#"{"OTIO_SCHEMA": "Effect.2", "effect_name": "Blur"}"#,
        &arguments,
    );
    assert!(!linked);
    assert_eq!(effect(&document, id).effect_id, "");

    let (document, id, linked) = reference(
        r#"{"OTIO_SCHEMA": "Effect.2", "effect_name": "Blur",
            "metadata": {"fcp_xml": {"effectid": "blur"}},
            "parameters": {"radius": 50, "h": false, "edges": "repeat"}}"#,
        &arguments,
    );
    std::fs::remove_file(path).ok();
    assert!(linked);
    let data = effect(&document, id);
    assert_eq!(data.effect_id, "otio.fx.gaussian_blur");
    let Any::Double(amount) = data.parameters["amount"] else {
        panic!("a scaled number is a float");
    };
    assert!((amount - 1.0).abs() < 1e-12);
    // A bool is copied, not scaled.
    assert_eq!(data.parameters["horizontal"], Any::Bool(false));
    assert_eq!(data.parameters["vertical"], Any::Bool(true));
    assert_eq!(data.parameters["edge_mode"], Any::from("repeat"));
    let Any::Dictionary(otio_fx) = &data.base.metadata["otio.fx"] else {
        panic!("otio.fx is a dictionary");
    };
    assert_eq!(otio_fx["fidelity"], Any::from("exact"));
    let Any::Dictionary(fcp) = &data.base.metadata["fcp_xml"] else {
        panic!("the native metadata stays");
    };
    assert_eq!(fcp["effectid"], Any::from("blur"));
}

#[test]
fn an_unknown_match_key_never_matches() {
    let _lock = fresh();
    let (path, arguments) = custom_registry(
        "unknown",
        r#"[{"effect_id": "otio.fx.crop",
             "match": {"effect_name": "Crop", "plugin_vendor": "Acme"}}]"#,
    );
    let (_, _, linked) = reference(
        r#"{"OTIO_SCHEMA": "Effect.2", "effect_name": "Crop"}"#,
        &arguments,
    );
    std::fs::remove_file(path).ok();
    assert!(!linked);
}

#[test]
fn the_reference_linker_skips_time_effects_registry_effects_and_strangers() {
    let _lock = fresh();
    let (_, _, linked) = reference(
        r#"{"OTIO_SCHEMA": "LinearTimeWarp.1", "effect_name": "Gaussian Blur",
            "time_scalar": 2.0}"#,
        &AnyDictionary::new(),
    );
    assert!(!linked);
    let (document, id, linked) = reference(
        r#"{"OTIO_SCHEMA": "Effect.2", "effect_name": "Gaussian Blur",
            "effect_id": "otio.fx.gaussian_blur", "parameters": {"amount": 0.3}}"#,
        &AnyDictionary::new(),
    );
    assert!(!linked);
    assert_eq!(effect(&document, id).parameters["amount"], Any::Double(0.3));
    let (_, _, linked) = reference(
        r#"{"OTIO_SCHEMA": "Effect.2", "effect_name": "Lens Flare"}"#,
        &AnyDictionary::new(),
    );
    assert!(!linked);
}

#[test]
fn the_reference_linker_runs_in_the_read_sequence() {
    let _lock = fresh();
    let (mut document, root) = timeline();
    let (_, clip, transition) = holders(&document, root);
    let blur = effects_of(&document, clip)[0];
    let wipe = transition_effect(&document, transition).unwrap();
    if let Some(data) = document.get_mut(blur).and_then(Node::effect_mut) {
        data.effect_name = "Gaussian Blur".into();
    }
    if let Some(data) = document.get_mut(wipe).and_then(Node::effect_mut) {
        data.effect_name = "Cross Dissolve".into();
    }
    let arguments = PluginArguments {
        effects_linker: named(plugins::OTIO_FX_REGISTRY),
        ..PluginArguments::default()
    };
    plugins::after_read(&mut document, &arguments, AnyDictionary::new()).unwrap();
    assert_eq!(effect(&document, blur).effect_id, "otio.fx.gaussian_blur");
    assert_eq!(effect(&document, wipe).effect_id, "otio.fx.dissolve");
    assert_eq!(effect(&document, wipe).context, "transition");
}
