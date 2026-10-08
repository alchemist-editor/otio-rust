//! Media linkers and hooks, run as upstream's `Adapter` runs them.
//!
//! The registry is process-wide, as upstream's manifest is, so every test
//! here takes [`LOCK`] and starts from a fresh registry.

use std::sync::{Arc, Mutex, MutexGuard};

use otio_adapter::Error;
use otio_adapter::plugins::{
    self, HookScript, LinkerChoice, MediaLinker, PluginArguments, Registry,
};
use otio_core::schema::{ExternalReference, MediaReferenceData};
use otio_core::{Any, AnyDictionary, Document, Node, NodeId};

static LOCK: Mutex<()> = Mutex::new(());

/// Takes the lock and resets the registry.
fn fresh() -> MutexGuard<'static, ()> {
    let guard = LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    *plugins::registry() = Registry::new();
    guard
}

/// Set in the child process [`the_default_linker_is_the_one_the_environment_names`]
/// starts, since a test cannot change its own environment without `unsafe`.
const CHILD: &str = "OTIO_ADAPTER_PLUGINS_TEST_CHILD";

const TIMELINE: &str = r#"{
    "OTIO_SCHEMA": "Timeline.1",
    "name": "cut",
    "tracks": {
        "OTIO_SCHEMA": "Stack.1",
        "children": [{
            "OTIO_SCHEMA": "Track.1",
            "kind": "Video",
            "children": [
                {"OTIO_SCHEMA": "Clip.2", "name": "a", "media_references": {
                    "DEFAULT_MEDIA": {"OTIO_SCHEMA": "MissingReference.1"}
                }, "active_media_reference_key": "DEFAULT_MEDIA"},
                {"OTIO_SCHEMA": "Clip.2", "name": "b", "media_references": {
                    "DEFAULT_MEDIA": {"OTIO_SCHEMA": "MissingReference.1"}
                }, "active_media_reference_key": "DEFAULT_MEDIA"}
            ]
        }]
    }
}"#;

fn timeline() -> (Document, NodeId) {
    let document = otio_core::from_str(TIMELINE).expect("the timeline reads");
    let root = document.root().expect("it has a root");
    (document, root)
}

fn name_of(document: &Document, id: NodeId) -> String {
    document.try_get(id).expect("it is there").name().to_owned()
}

fn rename(document: &mut Document, id: NodeId, name: &str) {
    if let Some(base) = document.get_mut(id).and_then(Node::base_mut) {
        base.name = name.to_owned();
    }
}

/// A script appending `tag` to the name of what it is handed.
fn tagging(tag: &'static str) -> HookScript {
    HookScript::new(move |document, target, _| {
        let name = format!("{}+{tag}", name_of(document, target));
        rename(document, target, &name);
        Ok(target)
    })
}

/// A linker pointing each clip at `linked/<clip name><suffix>`, the suffix
/// coming from its arguments.
fn studio_linker() -> MediaLinker {
    MediaLinker::new(|document, clip, arguments| {
        let suffix = match arguments.get("suffix") {
            Some(Any::String(suffix)) => suffix.clone(),
            _ => String::new(),
        };
        let url = format!("linked/{}{suffix}", name_of(document, clip));
        Ok(Some(document.insert(Node::ExternalReference(
            ExternalReference {
                media: MediaReferenceData::default(),
                target_url: url,
            },
        ))))
    })
}

fn active_url(document: &Document, clip: NodeId) -> Option<String> {
    let Node::Clip(clip) = document.try_get(clip).ok()? else {
        return None;
    };
    let reference = clip
        .media_references
        .get(&clip.active_media_reference_key)?;
    match document.try_get(*reference).ok()? {
        Node::ExternalReference(reference) => Some(reference.target_url.clone()),
        _ => None,
    }
}

#[test]
fn the_adapter_hooks_are_declared_from_the_start() {
    let _lock = fresh();
    assert_eq!(
        plugins::registry().hook_names(),
        [
            "post_adapter_read",
            "post_media_linker",
            "pre_adapter_write",
            "post_adapter_write"
        ]
    );
}

#[test]
fn scripts_run_in_the_order_attached_each_on_what_the_last_returned() {
    let _lock = fresh();
    {
        let mut registry = plugins::registry();
        registry.register_hook_script("one", tagging("one"));
        registry.register_hook_script("two", tagging("two"));
        registry.attach_hook_script("custom", "two");
        registry.attach_hook_script("custom", "one");
    }
    let (mut document, root) = timeline();
    let result = plugins::run_hook("custom", &mut document, root, &AnyDictionary::new())
        .expect("the hook runs");
    assert_eq!(name_of(&document, result), "cut+two+one");

    plugins::registry().set_scripts_attached_to("custom", vec!["one".to_owned()]);
    let result = plugins::run_hook("custom", &mut document, root, &AnyDictionary::new())
        .expect("the hook runs");
    assert_eq!(name_of(&document, result), "cut+two+one+one");
}

#[test]
fn a_hook_can_hand_back_another_object() {
    let _lock = fresh();
    plugins::registry().register_hook_script(
        "replace",
        HookScript::new(|document, _, _| {
            let mut timeline = otio_core::schema::Timeline::default();
            timeline.base.name = "stand-in".to_owned();
            Ok(document.insert(Node::Timeline(timeline)))
        }),
    );
    plugins::registry().attach_hook_script("custom", "replace");
    let (mut document, root) = timeline();
    let result = plugins::run_hook("custom", &mut document, root, &AnyDictionary::new())
        .expect("the hook runs");
    assert_eq!(name_of(&document, result), "stand-in");
}

#[test]
fn running_what_is_not_there_fails_as_upstream_does() {
    let _lock = fresh();
    let (mut document, root) = timeline();
    let error = plugins::run_hook("nowhere", &mut document, root, &AnyDictionary::new())
        .expect_err("no such hook");
    assert!(matches!(error, Error::UnknownHook(hook) if hook == "nowhere"));

    plugins::registry().attach_hook_script("custom", "missing");
    let error = plugins::run_hook("custom", &mut document, root, &AnyDictionary::new())
        .expect_err("no such script");
    assert!(matches!(error, Error::UnknownHookScript(name) if name == "missing"));

    plugins::registry()
        .register_hook_script("failing", HookScript::new(|_, _, _| Err("no".to_owned())));
    plugins::registry().set_scripts_attached_to("custom", vec!["failing".to_owned()]);
    let error = plugins::run_hook("custom", &mut document, root, &AnyDictionary::new())
        .expect_err("the script fails");
    assert!(
        matches!(error, Error::Plugin { name, message } if name == "failing" && message == "no")
    );
}

#[test]
fn a_hook_may_use_the_registry_itself() {
    let _lock = fresh();
    plugins::registry().register_hook_script(
        "outer",
        HookScript::new(|document, target, arguments| {
            plugins::registry().register_hook_script("inner", tagging("inner"));
            plugins::registry().attach_hook_script("nested", "inner");
            plugins::run_hook("nested", document, target, arguments).map_err(|e| e.to_string())
        }),
    );
    plugins::registry().attach_hook_script("custom", "outer");
    let (mut document, root) = timeline();
    let result = plugins::run_hook("custom", &mut document, root, &AnyDictionary::new())
        .expect("nothing deadlocks");
    assert_eq!(name_of(&document, result), "cut+inner");
}

#[test]
fn a_script_sees_what_an_earlier_one_registered() {
    let _lock = fresh();
    // The second script is registered only by the first, which also
    // replaces the third.
    plugins::registry().register_hook_script(
        "first",
        HookScript::new(|_, target, _| {
            plugins::registry().register_hook_script("second", tagging("second"));
            plugins::registry().register_hook_script("third", tagging("new third"));
            Ok(target)
        }),
    );
    plugins::registry().register_hook_script("third", tagging("old third"));
    plugins::registry().set_scripts_attached_to(
        "custom",
        vec!["first".into(), "second".into(), "third".into()],
    );
    let (mut document, root) = timeline();
    let result =
        plugins::run_hook("custom", &mut document, root, &AnyDictionary::new()).expect("it runs");
    assert_eq!(name_of(&document, result), "cut+second+new third");
}

#[test]
fn a_named_linker_gives_every_clip_what_it_returns() {
    let _lock = fresh();
    plugins::registry().register_media_linker("studio", studio_linker());
    let (mut document, root) = timeline();
    let clips = document.find_clips(root).expect("clips");
    let before = document.len();
    let mut arguments = AnyDictionary::new();
    arguments.insert("suffix".to_owned(), Any::String(".mov".to_owned()));
    plugins::link_media(
        &mut document,
        root,
        &LinkerChoice::Named("studio".to_owned()),
        &arguments,
    )
    .expect("it links");
    assert_eq!(
        active_url(&document, clips[0]).as_deref(),
        Some("linked/a.mov")
    );
    assert_eq!(
        active_url(&document, clips[1]).as_deref(),
        Some("linked/b.mov")
    );
    // The references replaced are gone, not left behind.
    assert_eq!(document.len(), before);
}

#[test]
fn a_reference_another_clip_still_holds_is_kept() {
    let _lock = fresh();
    let (mut document, root) = timeline();
    let clips = document.find_clips(root).expect("clips");
    // Clip b shares clip a's reference.
    let shared = match document.try_get(clips[0]).expect("a") {
        Node::Clip(clip) => clip.media_references["DEFAULT_MEDIA"],
        _ => unreachable!(),
    };
    if let Ok(Node::Clip(clip)) = document.try_get_mut(clips[1]) {
        let own = clip
            .media_references
            .insert("DEFAULT_MEDIA".to_owned(), shared)
            .expect("b had one");
        document.remove(own);
    }
    // Relinks clip a only, and records whether what b holds is still there.
    let seen = Arc::new(Mutex::new(None));
    let record = Arc::clone(&seen);
    plugins::registry().register_media_linker(
        "first",
        MediaLinker::new(move |document, clip, _| {
            if name_of(document, clip) == "b" {
                *record.lock().expect("lock") = Some(document.contains(shared));
                return Ok(None);
            }
            Ok(Some(document.insert(Node::ExternalReference(
                ExternalReference::default(),
            ))))
        }),
    );
    plugins::link_media(
        &mut document,
        root,
        &LinkerChoice::Named("first".to_owned()),
        &AnyDictionary::new(),
    )
    .expect("it links");
    assert_eq!(*seen.lock().expect("lock"), Some(true));
    assert!(document.contains(shared));
}

#[test]
fn what_only_the_replaced_reference_held_goes_with_it() {
    let _lock = fresh();
    plugins::registry().register_media_linker("studio", studio_linker());
    let (mut document, root) = timeline();
    let clips = document.find_clips(root).expect("clips");
    // Clip a's reference holds a marker in its metadata, and clip b holds
    // one that a's reference also holds.
    let held = document.insert(Node::Marker(Default::default()));
    let shared = document.insert(Node::Marker(Default::default()));
    let reference = match document.try_get(clips[0]).expect("a") {
        Node::Clip(clip) => clip.media_references["DEFAULT_MEDIA"],
        _ => unreachable!(),
    };
    if let Some(base) = document.get_mut(reference).and_then(Node::base_mut) {
        base.metadata.insert("held".to_owned(), Any::Object(held));
        base.metadata
            .insert("shared".to_owned(), Any::Object(shared));
    }
    if let Some(base) = document.get_mut(clips[1]).and_then(Node::base_mut) {
        base.metadata
            .insert("shared".to_owned(), Any::Object(shared));
    }
    plugins::link_media(
        &mut document,
        root,
        &LinkerChoice::Named("studio".to_owned()),
        &AnyDictionary::new(),
    )
    .expect("it links");
    assert!(!document.contains(reference));
    assert!(!document.contains(held));
    assert!(document.contains(shared));
}

#[test]
fn objects_holding_only_each_other_go_with_the_reference() {
    let _lock = fresh();
    plugins::registry().register_media_linker("studio", studio_linker());
    let (mut document, root) = timeline();
    let clips = document.find_clips(root).expect("clips");
    // Clip a's reference and a marker hold each other, and nothing else
    // holds either.
    let marker = document.insert(Node::Marker(Default::default()));
    let reference = match document.try_get(clips[0]).expect("a") {
        Node::Clip(clip) => clip.media_references["DEFAULT_MEDIA"],
        _ => unreachable!(),
    };
    if let Some(base) = document.get_mut(reference).and_then(Node::base_mut) {
        base.metadata
            .insert("marker".to_owned(), Any::Object(marker));
    }
    if let Some(base) = document.get_mut(marker).and_then(Node::base_mut) {
        base.metadata
            .insert("reference".to_owned(), Any::Object(reference));
    }
    plugins::link_media(
        &mut document,
        root,
        &LinkerChoice::Named("studio".to_owned()),
        &AnyDictionary::new(),
    )
    .expect("it links");
    assert!(!document.contains(reference));
    assert!(!document.contains(marker));
}

#[test]
fn a_reference_holding_the_root_leaves_the_document_whole() {
    let _lock = fresh();
    plugins::registry().register_media_linker("studio", studio_linker());
    let (mut document, root) = timeline();
    let clips = document.find_clips(root).expect("clips");
    let reference = match document.try_get(clips[0]).expect("a") {
        Node::Clip(clip) => clip.media_references["DEFAULT_MEDIA"],
        _ => unreachable!(),
    };
    if let Some(base) = document.get_mut(reference).and_then(Node::base_mut) {
        base.metadata.insert("root".to_owned(), Any::Object(root));
    }
    plugins::link_media(
        &mut document,
        root,
        &LinkerChoice::Named("studio".to_owned()),
        &AnyDictionary::new(),
    )
    .expect("it links");
    assert!(!document.contains(reference));
    assert!(document.contains(root));
    assert_eq!(active_url(&document, clips[0]).as_deref(), Some("linked/a"));
    assert_eq!(active_url(&document, clips[1]).as_deref(), Some("linked/b"));
}

#[test]
fn what_is_being_linked_stays_though_it_is_not_yet_the_root() {
    let _lock = fresh();
    plugins::registry().register_media_linker("studio", studio_linker());
    let (mut document, root) = timeline();
    // As after a post_adapter_read hook hands back a new timeline, before
    // the document's root is set to it.
    document.set_root(None);
    let clips = document.find_clips(root).expect("clips");
    let reference = match document.try_get(clips[0]).expect("a") {
        Node::Clip(clip) => clip.media_references["DEFAULT_MEDIA"],
        _ => unreachable!(),
    };
    if let Some(base) = document.get_mut(reference).and_then(Node::base_mut) {
        base.metadata.insert("root".to_owned(), Any::Object(root));
    }
    plugins::link_media(
        &mut document,
        root,
        &LinkerChoice::Named("studio".to_owned()),
        &AnyDictionary::new(),
    )
    .expect("it links");
    assert!(!document.contains(reference));
    assert!(document.contains(root));
    assert_eq!(active_url(&document, clips[1]).as_deref(), Some("linked/b"));
}

#[test]
fn a_linker_returning_nothing_leaves_the_clip_alone() {
    let _lock = fresh();
    plugins::registry().register_media_linker("none", MediaLinker::new(|_, _, _| Ok(None)));
    let (mut document, root) = timeline();
    let json = otio_core::to_string(&document).expect("it writes");
    plugins::link_media(
        &mut document,
        root,
        &LinkerChoice::Named("none".to_owned()),
        &AnyDictionary::new(),
    )
    .expect("it links");
    assert_eq!(otio_core::to_string(&document).expect("it writes"), json);
}

#[test]
fn an_unknown_linker_is_refused_with_upstreams_message() {
    let _lock = fresh();
    plugins::registry().register_media_linker("studio", studio_linker());
    let (mut document, root) = timeline();
    let error = plugins::link_media(
        &mut document,
        root,
        &LinkerChoice::Named("elsewhere".to_owned()),
        &AnyDictionary::new(),
    )
    .expect_err("no such linker");
    assert_eq!(
        error.to_string(),
        "media linker not supported: elsewhere, available: ['studio']"
    );
}

#[test]
fn with_no_default_named_the_default_links_nothing() {
    let _lock = fresh();
    if std::env::var_os(plugins::DEFAULT_MEDIA_LINKER_VARIABLE).is_some() {
        return;
    }
    plugins::registry().register_media_linker("studio", studio_linker());
    let (mut document, root) = timeline();
    let clips = document.find_clips(root).expect("clips");
    for choice in [LinkerChoice::Default, LinkerChoice::Named(String::new())] {
        plugins::link_media(&mut document, root, &choice, &AnyDictionary::new())
            .expect("no default links nothing");
        assert_eq!(active_url(&document, clips[0]), None);
    }
}

#[test]
fn the_default_linker_is_the_one_the_environment_names() {
    let status = std::process::Command::new(std::env::current_exe().expect("the test binary"))
        .args([
            "--exact",
            "default_linker_in_a_child",
            "--test-threads",
            "1",
        ])
        .env(CHILD, "1")
        .env(plugins::DEFAULT_MEDIA_LINKER_VARIABLE, "studio")
        .status()
        .expect("the child runs");
    assert!(status.success());
}

/// The body of [`the_default_linker_is_the_one_the_environment_names`], run
/// in a process whose environment names `studio`.
#[test]
fn default_linker_in_a_child() {
    if std::env::var_os(CHILD).is_none() {
        return;
    }
    let _lock = fresh();
    plugins::registry().register_media_linker("studio", studio_linker());
    let (mut document, root) = timeline();
    let clips = document.find_clips(root).expect("clips");
    plugins::link_media(
        &mut document,
        root,
        &LinkerChoice::DoNotLink,
        &AnyDictionary::new(),
    )
    .expect("not linking links nothing");
    assert_eq!(active_url(&document, clips[0]), None);
    plugins::link_media(
        &mut document,
        root,
        &LinkerChoice::Default,
        &AnyDictionary::new(),
    )
    .expect("the default links");
    assert_eq!(active_url(&document, clips[0]).as_deref(), Some("linked/a"));
}

#[test]
fn after_read_runs_upstreams_sequence_with_upstreams_arguments() {
    let _lock = fresh();
    let seen = Arc::new(Mutex::new(Vec::new()));
    for hook in [plugins::POST_ADAPTER_READ, plugins::POST_MEDIA_LINKER] {
        let seen = Arc::clone(&seen);
        plugins::registry().register_hook_script(
            hook,
            HookScript::new(move |document, target, arguments| {
                let clip = document.find_clips(target).map_err(|e| e.to_string())?[0];
                seen.lock().unwrap().push((
                    hook,
                    arguments.keys().cloned().collect::<Vec<_>>(),
                    active_url(document, clip),
                ));
                Ok(target)
            }),
        );
        plugins::registry().attach_hook_script(hook, hook);
    }
    plugins::registry().register_media_linker("studio", studio_linker());

    let (mut document, _) = timeline();
    let mut arguments = PluginArguments {
        media_linker: LinkerChoice::Named("studio".to_owned()),
        ..PluginArguments::default()
    };
    arguments
        .hook_arguments
        .insert("mine".to_owned(), Any::Bool(true));
    arguments
        .media_linker_arguments
        .insert("suffix".to_owned(), Any::String(".mxf".to_owned()));
    plugins::after_read(&mut document, &arguments, AnyDictionary::new()).expect("it runs");

    let seen = seen.lock().unwrap();
    assert_eq!(
        *seen,
        [
            (
                plugins::POST_ADAPTER_READ,
                vec![
                    "adapter_arguments".to_owned(),
                    "media_linker_argument_map".to_owned(),
                    "mine".to_owned()
                ],
                None
            ),
            // Upstream hands post_media_linker the linker's arguments.
            (
                plugins::POST_MEDIA_LINKER,
                vec!["suffix".to_owned()],
                Some("linked/a.mxf".to_owned())
            ),
        ]
    );
}

#[test]
fn the_write_hooks_see_the_path_and_pre_write_chooses_what_is_written() {
    let _lock = fresh();
    plugins::registry().register_hook_script("pre", tagging("pre"));
    plugins::registry().attach_hook_script(plugins::PRE_ADAPTER_WRITE, "pre");
    let path = Arc::new(Mutex::new(None));
    let sink = Arc::clone(&path);
    plugins::registry().register_hook_script(
        "post",
        HookScript::new(move |_, target, arguments| {
            *sink.lock().unwrap() = arguments.get("_filepath").cloned();
            Ok(target)
        }),
    );
    plugins::registry().attach_hook_script(plugins::POST_ADAPTER_WRITE, "post");

    let (mut document, root) = timeline();
    let arguments = plugins::write_hook_arguments(
        &PluginArguments::default(),
        AnyDictionary::new(),
        Some("cut.otio"),
    );
    let written = plugins::before_write(&mut document, root, &arguments).expect("it runs");
    assert_eq!(name_of(&document, written), "cut+pre");
    plugins::after_write(&mut document, written, &arguments).expect("it runs");
    assert_eq!(
        *path.lock().unwrap(),
        Some(Any::String("cut.otio".to_owned()))
    );
}

#[test]
fn anything_to_run_says_whether_a_copy_is_needed() {
    let _lock = fresh();
    assert!(!plugins::anything_to_run(&LinkerChoice::Default));
    plugins::registry().register_media_linker("studio", studio_linker());
    assert!(plugins::anything_to_run(&LinkerChoice::Named(
        "studio".to_owned()
    )));
    assert!(!plugins::anything_to_run(&LinkerChoice::DoNotLink));
    plugins::registry().register_hook_script("pre", tagging("pre"));
    plugins::registry().attach_hook_script(plugins::PRE_ADAPTER_WRITE, "pre");
    assert!(plugins::anything_to_run(&LinkerChoice::DoNotLink));
}
