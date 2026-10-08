//! Media linkers and hooks: upstream's two plugin points, as plain functions.
//!
//! Upstream OpenTimelineIO lets a studio change what an adapter reads and
//! writes without changing the adapter. A *media linker* is handed each clip
//! a read produced and returns the media reference the clip should point at:
//! a proxy on the local network, say, in place of the path the file names. A
//! *hook script* is handed the whole timeline at a named point — after a read,
//! after linking, before and after a write, or at a point an adapter declares
//! for itself — and returns the timeline to go on with.
//!
//! Upstream finds both through Python plugin manifests. Here they are
//! functions registered under a name, so that every language reaches them the
//! same way; the Python bindings keep upstream's manifests on top.
//!
//! ```text
//!  read ──► post_adapter_read ──► media linker, per clip ──► post_media_linker
//!  pre_adapter_write ──► write ──► post_adapter_write
//! ```
//!
//! [`after_read`], [`before_write`] and [`after_write`] run those sequences as
//! upstream's `Adapter.read_from_file` and `Adapter.write_to_file` run them,
//! quirks included: `post_media_linker` is handed the media linker's
//! arguments rather than the hook arguments.
//!
//! The registry is process-wide, as upstream's active manifest is, behind
//! [`registry`]. Nothing is called while it is locked, so a hook may register
//! another, or run one.

use std::collections::HashSet;
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};

use otio_core::{Any, AnyDictionary, Document, Node, NodeId};

use crate::{Error, Result};

/// Runs on what an adapter read, before media is linked.
pub const POST_ADAPTER_READ: &str = "post_adapter_read";
/// Runs on what an adapter read, after media is linked.
pub const POST_MEDIA_LINKER: &str = "post_media_linker";
/// Runs on what is about to be written; what it returns is written.
pub const PRE_ADAPTER_WRITE: &str = "pre_adapter_write";
/// Runs on what was written, once it has been.
pub const POST_ADAPTER_WRITE: &str = "post_adapter_write";

/// The four hooks every adapter runs, in the order upstream's built-in
/// manifest declares them.
pub const ADAPTER_HOOKS: [&str; 4] = [
    POST_ADAPTER_READ,
    POST_MEDIA_LINKER,
    PRE_ADAPTER_WRITE,
    POST_ADAPTER_WRITE,
];

/// The environment variable naming the media linker a read uses when the
/// caller names none, as upstream's `OTIO_DEFAULT_MEDIA_LINKER`.
pub const DEFAULT_MEDIA_LINKER_VARIABLE: &str = "OTIO_DEFAULT_MEDIA_LINKER";

type HookFn = dyn Fn(&mut Document, NodeId, &AnyDictionary) -> std::result::Result<NodeId, String>
    + Send
    + Sync;

type LinkerFn = dyn Fn(&mut Document, NodeId, &AnyDictionary) -> std::result::Result<Option<NodeId>, String>
    + Send
    + Sync;

/// A hook script: upstream's `hook_function(in_timeline, argument_map)`.
///
/// It is handed the document, the object the hook runs on, and the hook's
/// arguments, and returns the object to go on with: the same one changed, or
/// another it put in the document. A message it returns stops whatever ran
/// the hook with [`Error::Plugin`].
#[derive(Clone)]
pub struct HookScript(Arc<HookFn>);

impl HookScript {
    /// A hook script running `script`.
    pub fn new(
        script: impl Fn(&mut Document, NodeId, &AnyDictionary) -> std::result::Result<NodeId, String>
        + Send
        + Sync
        + 'static,
    ) -> Self {
        Self(Arc::new(script))
    }

    /// Runs the script.
    ///
    /// # Errors
    ///
    /// Returns the script's own message if it fails.
    pub fn run(
        &self,
        document: &mut Document,
        target: NodeId,
        arguments: &AnyDictionary,
    ) -> std::result::Result<NodeId, String> {
        (self.0)(document, target, arguments)
    }
}

impl std::fmt::Debug for HookScript {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("HookScript").finish_non_exhaustive()
    }
}

/// A media linker: upstream's `link_media_reference(in_clip, argument_map)`.
///
/// It is handed the document, one clip, and the linker's arguments, and
/// returns a media reference it put in the document for the clip to use in
/// place of its active one, or nothing to leave the clip as it is.
#[derive(Clone)]
pub struct MediaLinker(Arc<LinkerFn>);

impl MediaLinker {
    /// A media linker running `linker`.
    pub fn new(
        linker: impl Fn(
            &mut Document,
            NodeId,
            &AnyDictionary,
        ) -> std::result::Result<Option<NodeId>, String>
        + Send
        + Sync
        + 'static,
    ) -> Self {
        Self(Arc::new(linker))
    }

    /// Runs the linker on one clip.
    ///
    /// # Errors
    ///
    /// Returns the linker's own message if it fails.
    pub fn link(
        &self,
        document: &mut Document,
        clip: NodeId,
        arguments: &AnyDictionary,
    ) -> std::result::Result<Option<NodeId>, String> {
        (self.0)(document, clip, arguments)
    }
}

impl std::fmt::Debug for MediaLinker {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("MediaLinker").finish_non_exhaustive()
    }
}

/// Which media linker a read uses: upstream's `media_linker_name`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum LinkerChoice {
    /// The one `OTIO_DEFAULT_MEDIA_LINKER` names, or none if it names none:
    /// upstream's `MediaLinkingPolicy.ForceDefaultLinker`, its default.
    #[default]
    Default,
    /// None: upstream's `MediaLinkingPolicy.DoNotLinkMedia`.
    DoNotLink,
    /// The one registered under this name. An empty name is
    /// [`LinkerChoice::Default`], as upstream treats it.
    Named(String),
}

/// The linkers, hook scripts and hooks a process knows: upstream's active
/// manifest, less the adapters and schemas, which are compiled in here.
///
/// Names keep the order they were first registered in, as a manifest's
/// lists do, and registering a name again replaces what it names in place.
#[derive(Debug, Clone)]
pub struct Registry {
    media_linkers: Vec<(String, MediaLinker)>,
    hook_scripts: Vec<(String, HookScript)>,
    hooks: Vec<(String, Vec<String>)>,
}

impl Default for Registry {
    fn default() -> Self {
        Self::new()
    }
}

impl Registry {
    /// A registry with no linkers or scripts, and the four adapter hooks
    /// declared, each with nothing attached.
    #[must_use]
    pub fn new() -> Self {
        Self {
            media_linkers: Vec::new(),
            hook_scripts: Vec::new(),
            hooks: ADAPTER_HOOKS
                .iter()
                .map(|hook| ((*hook).to_owned(), Vec::new()))
                .collect(),
        }
    }

    /// Registers `linker` under `name`, replacing any registered already,
    /// and returns the one it replaced.
    ///
    /// A caller whose linkers release something when dropped drops what
    /// this returns once the registry is unlocked, so that what the release
    /// does may lock it again.
    pub fn register_media_linker(
        &mut self,
        name: impl Into<String>,
        linker: MediaLinker,
    ) -> Option<MediaLinker> {
        upsert(&mut self.media_linkers, name.into(), linker)
    }

    /// Forgets the linker registered under `name`, returning whether there
    /// was one.
    pub fn remove_media_linker(&mut self, name: &str) -> bool {
        self.take_media_linker(name).is_some()
    }

    /// Forgets the linker registered under `name`, returning it, to be
    /// dropped once the registry is unlocked.
    pub fn take_media_linker(&mut self, name: &str) -> Option<MediaLinker> {
        take(&mut self.media_linkers, name)
    }

    /// The linker registered under `name`.
    #[must_use]
    pub fn media_linker(&self, name: &str) -> Option<&MediaLinker> {
        find(&self.media_linkers, name)
    }

    /// The names of the registered linkers: upstream's
    /// `available_media_linker_names()`.
    #[must_use]
    pub fn media_linker_names(&self) -> Vec<String> {
        names(&self.media_linkers)
    }

    /// Registers `script` under `name`, replacing any registered already.
    ///
    /// Registering a script does not attach it to any hook. Returns the
    /// script it replaced, as [`Registry::register_media_linker`] does.
    pub fn register_hook_script(
        &mut self,
        name: impl Into<String>,
        script: HookScript,
    ) -> Option<HookScript> {
        upsert(&mut self.hook_scripts, name.into(), script)
    }

    /// Forgets the script registered under `name`, returning whether there
    /// was one. It stays attached wherever it was; running such a hook
    /// fails with [`Error::UnknownHookScript`], as upstream's does.
    pub fn remove_hook_script(&mut self, name: &str) -> bool {
        self.take_hook_script(name).is_some()
    }

    /// Forgets the script registered under `name`, returning it, to be
    /// dropped once the registry is unlocked.
    pub fn take_hook_script(&mut self, name: &str) -> Option<HookScript> {
        take(&mut self.hook_scripts, name)
    }

    /// The script registered under `name`.
    #[must_use]
    pub fn hook_script(&self, name: &str) -> Option<&HookScript> {
        find(&self.hook_scripts, name)
    }

    /// The names of the registered scripts: upstream's
    /// `available_hookscript_names()`.
    #[must_use]
    pub fn hook_script_names(&self) -> Vec<String> {
        names(&self.hook_scripts)
    }

    /// Declares a hook, with nothing attached, unless it is declared already.
    ///
    /// A hook has to be declared to be run, as upstream's has to be in a
    /// manifest's `hooks`.
    pub fn declare_hook(&mut self, hook: impl Into<String>) {
        let hook = hook.into();
        if !self.hooks.iter().any(|(name, _)| *name == hook) {
            self.hooks.push((hook, Vec::new()));
        }
    }

    /// The declared hooks: upstream's `hooks.names()`.
    #[must_use]
    pub fn hook_names(&self) -> Vec<String> {
        self.hooks.iter().map(|(name, _)| name.clone()).collect()
    }

    /// Whether `hook` is declared.
    #[must_use]
    pub fn has_hook(&self, hook: &str) -> bool {
        self.hooks.iter().any(|(name, _)| name == hook)
    }

    /// The scripts attached to `hook`, in the order they run, or nothing if
    /// it is not declared: upstream's `scripts_attached_to()`.
    #[must_use]
    pub fn scripts_attached_to(&self, hook: &str) -> Option<&[String]> {
        self.hooks
            .iter()
            .find(|(name, _)| name == hook)
            .map(|(_, scripts)| scripts.as_slice())
    }

    /// Attaches the script named `script` to `hook`, after those attached
    /// already, declaring the hook if it is not.
    pub fn attach_hook_script(&mut self, hook: &str, script: impl Into<String>) {
        self.declare_hook(hook);
        if let Some((_, scripts)) = self.hooks.iter_mut().find(|(name, _)| name == hook) {
            scripts.push(script.into());
        }
    }

    /// Replaces what is attached to `hook`, declaring it if it is not. This
    /// is how scripts are reordered or detached, as upstream's editable list
    /// is.
    pub fn set_scripts_attached_to(&mut self, hook: &str, scripts: Vec<String>) {
        self.declare_hook(hook);
        if let Some((_, attached)) = self.hooks.iter_mut().find(|(name, _)| name == hook) {
            *attached = scripts;
        }
    }

    /// The names of the scripts to run for `hook`, copied so that they can be
    /// run once the registry is no longer borrowed.
    fn scripts_for(&self, hook: &str) -> Result<Vec<String>> {
        self.scripts_attached_to(hook)
            .map(<[String]>::to_vec)
            .ok_or_else(|| Error::UnknownHook(hook.to_owned()))
    }

    /// The linker `choice` names, if it names one, looked up now so that it
    /// can be run once the registry is no longer borrowed.
    fn linker_for(&self, choice: &LinkerChoice) -> Result<Option<(String, MediaLinker)>> {
        let name = match choice {
            LinkerChoice::DoNotLink => return Ok(None),
            LinkerChoice::Named(name) if !name.is_empty() => name.clone(),
            LinkerChoice::Default | LinkerChoice::Named(_) => {
                match std::env::var(DEFAULT_MEDIA_LINKER_VARIABLE) {
                    Ok(name) if !name.is_empty() => name,
                    _ => return Ok(None),
                }
            }
        };
        match self.media_linker(&name) {
            Some(linker) => Ok(Some((name, linker.clone()))),
            None => Err(Error::UnknownMediaLinker {
                name,
                available: self.media_linker_names(),
            }),
        }
    }
}

fn upsert<T>(entries: &mut Vec<(String, T)>, name: String, value: T) -> Option<T> {
    match entries.iter_mut().find(|(found, _)| *found == name) {
        Some((_, slot)) => Some(std::mem::replace(slot, value)),
        None => {
            entries.push((name, value));
            None
        }
    }
}

fn take<T>(entries: &mut Vec<(String, T)>, name: &str) -> Option<T> {
    let index = entries.iter().position(|(found, _)| found == name)?;
    Some(entries.remove(index).1)
}

fn find<'a, T>(entries: &'a [(String, T)], name: &str) -> Option<&'a T> {
    entries
        .iter()
        .find(|(found, _)| found == name)
        .map(|(_, value)| value)
}

fn names<T>(entries: &[(String, T)]) -> Vec<String> {
    entries.iter().map(|(name, _)| name.clone()).collect()
}

/// The process's registry, which [`run_hook`], [`link_media`] and the rest
/// consult.
///
/// Lock it to register or look up; do not hold the lock across a call that
/// runs a hook or linker, since those lock it themselves.
pub fn registry() -> MutexGuard<'static, Registry> {
    static REGISTRY: OnceLock<Mutex<Registry>> = OnceLock::new();
    REGISTRY
        .get_or_init(|| Mutex::new(Registry::new()))
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Runs every script attached to `hook` on `target`, in order, each on what
/// the one before returned: upstream's `hooks.run`.
///
/// Returns what the last script returned, or `target` if none is attached.
///
/// # Errors
///
/// Returns [`Error::UnknownHook`] if `hook` is not declared,
/// [`Error::UnknownHookScript`] if an attached script is not registered,
/// and [`Error::Plugin`] if a script fails.
pub fn run_hook(
    hook: &str,
    document: &mut Document,
    target: NodeId,
    arguments: &AnyDictionary,
) -> Result<NodeId> {
    let names = registry().scripts_for(hook)?;
    let mut current = target;
    for name in names {
        // Looked up only now, as upstream's are, so that what an earlier
        // script registers or removes holds for the ones after it.
        let script = registry()
            .hook_script(&name)
            .cloned()
            .ok_or_else(|| Error::UnknownHookScript(name.clone()))?;
        current = script
            .run(document, current, arguments)
            .map_err(|message| Error::Plugin { name, message })?;
        document.try_get(current)?;
    }
    Ok(current)
}

/// Runs the linker `choice` names on every clip under `root`, giving each
/// clip the media reference the linker returns in place of its active one:
/// upstream's `_with_linked_media_references`.
///
/// Only a timeline, composition, collection or clip has clips to link, as
/// only those have upstream's `find_clips`, a clip's finding just itself;
/// anything else is left alone.
///
/// # Errors
///
/// Returns [`Error::UnknownMediaLinker`] if `choice` names a linker that is
/// not registered, [`Error::Plugin`] if the linker fails, and
/// [`Error::Core`] if what it returns is not a media reference in the
/// document.
pub fn link_media(
    document: &mut Document,
    root: NodeId,
    choice: &LinkerChoice,
    arguments: &AnyDictionary,
) -> Result<()> {
    let Some((name, linker)) = registry().linker_for(choice)? else {
        return Ok(());
    };
    let node = document.try_get(root)?;
    if !matches!(node, Node::Timeline(_) | Node::Clip(_)) && node.children().is_none() {
        return Ok(());
    }
    for clip in document.find_clips(root)? {
        let linked = linker
            .link(document, clip, arguments)
            .map_err(|message| Error::Plugin {
                name: name.clone(),
                message,
            })?;
        if let Some(reference) = linked {
            set_active_media_reference(document, clip, reference, &name, root)?;
        }
    }
    Ok(())
}

/// Points a clip's active media reference at `reference`, dropping the one
/// it replaces. `root` is what is being linked, which stays whatever the
/// replaced reference held.
fn set_active_media_reference(
    document: &mut Document,
    clip: NodeId,
    reference: NodeId,
    linker: &str,
    root: NodeId,
) -> Result<()> {
    if document.try_get(reference)?.media().is_none() {
        return Err(Error::Plugin {
            name: linker.to_owned(),
            message: format!(
                "the media linker returned a {}, not a media reference",
                document.try_get(reference)?.schema_name()
            ),
        });
    }
    let Node::Clip(found) = document.try_get_mut(clip)? else {
        return Ok(());
    };
    let key = found.active_media_reference_key.clone();
    let previous = found.media_references.insert(key, reference);
    // The reference replaced goes with it, unless something else still holds
    // it, as another clip sharing it does.
    if let Some(previous) = previous.filter(|previous| *previous != reference) {
        discard(document, previous, root);
    }
    Ok(())
}

/// Removes `id` and whatever it owns, through its metadata or a generator's
/// parameters, except what something outside all that still holds, and what
/// that in turn owns.
///
/// Ownership is decided for the whole group at once rather than object by
/// object, so objects that hold each other, and nothing else holds, go too.
fn discard(document: &mut Document, id: NodeId, root: NodeId) {
    // Everything `id` owns, directly or not.
    let mut group = HashSet::from([id]);
    let mut pending = vec![id];
    while let Some(next) = pending.pop() {
        if let Some(node) = document.get(next) {
            node.visit_owned(&mut |object| {
                if group.insert(object) {
                    pending.push(object);
                }
            });
        }
    }
    // What something outside the group holds stays, with what it owns. The
    // document holds its root, and the caller what it is linking, which a
    // hook may have made since the document's root was set.
    let mut kept: Vec<NodeId> = [document.root(), Some(root)]
        .into_iter()
        .flatten()
        .filter(|kept| group.contains(kept))
        .collect();
    for (owner, node) in document.iter() {
        if group.contains(&owner) {
            // A parent is an owner too, though `visit_owned` leaves it out.
            if node.parent().is_some_and(|parent| !group.contains(&parent)) {
                kept.push(owner);
            }
            continue;
        }
        node.visit_owned(&mut |object| {
            if group.contains(&object) {
                kept.push(object);
            }
        });
    }
    let mut retained = Vec::new();
    while let Some(next) = kept.pop() {
        if group.remove(&next) {
            retained.push(next);
            if let Some(node) = document.get(next) {
                node.visit_owned(&mut |object| kept.push(object));
            }
        }
    }
    // What stays no longer sits in a composition that goes.
    for object in retained {
        if let Some(node) = document.get_mut(object) {
            if node.parent().is_some_and(|parent| group.contains(&parent)) {
                node.set_parent(None);
            }
        }
    }
    for object in group {
        document.remove(object);
    }
}

/// What a read or write hands its linker and hooks: upstream's
/// `media_linker_name`, `media_linker_argument_map` and
/// `hook_function_argument_map`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PluginArguments {
    /// The media linker a read runs.
    pub media_linker: LinkerChoice,
    /// What the media linker, and the `post_media_linker` hook, are handed.
    pub media_linker_arguments: AnyDictionary,
    /// What the other hooks are handed, with the read's or write's own
    /// arguments added.
    pub hook_arguments: AnyDictionary,
}

/// Runs what upstream runs once an adapter has read: `post_adapter_read`,
/// the media linker, then `post_media_linker`, leaving the document's root
/// at what the last of them returned.
///
/// `adapter_arguments` are the adapter's own options, which the hooks are
/// handed as `adapter_arguments`, as upstream's are; and the hooks are
/// handed the linker's arguments as `media_linker_argument_map`.
///
/// A document with no root is left alone.
///
/// # Errors
///
/// As [`run_hook`] and [`link_media`].
pub fn after_read(
    document: &mut Document,
    arguments: &PluginArguments,
    adapter_arguments: AnyDictionary,
) -> Result<()> {
    let Some(root) = document.root() else {
        return Ok(());
    };
    let mut hook_arguments = arguments.hook_arguments.clone();
    hook_arguments.insert(
        "adapter_arguments".to_owned(),
        Any::Dictionary(adapter_arguments),
    );
    hook_arguments.insert(
        "media_linker_argument_map".to_owned(),
        Any::Dictionary(arguments.media_linker_arguments.clone()),
    );
    let root = run_hook(POST_ADAPTER_READ, document, root, &hook_arguments)?;
    link_media(
        document,
        root,
        &arguments.media_linker,
        &arguments.media_linker_arguments,
    )?;
    // Upstream hands this hook the linker's arguments, not the hook ones.
    let root = run_hook(
        POST_MEDIA_LINKER,
        document,
        root,
        &arguments.media_linker_arguments,
    )?;
    document.set_root(Some(root));
    Ok(())
}

/// The arguments the write hooks are handed: the caller's, with the
/// adapter's options as `adapter_arguments` and, writing to a file, its path
/// as `_filepath`, as upstream's are.
#[must_use]
pub fn write_hook_arguments(
    arguments: &PluginArguments,
    adapter_arguments: AnyDictionary,
    path: Option<&str>,
) -> AnyDictionary {
    let mut hook_arguments = arguments.hook_arguments.clone();
    hook_arguments.insert(
        "adapter_arguments".to_owned(),
        Any::Dictionary(adapter_arguments),
    );
    if let Some(path) = path {
        hook_arguments.insert("_filepath".to_owned(), Any::String(path.to_owned()));
    }
    hook_arguments
}

/// Runs `pre_adapter_write` on what is about to be written, returning what
/// to write instead. Build `hook_arguments` with [`write_hook_arguments`].
///
/// # Errors
///
/// As [`run_hook`].
pub fn before_write(
    document: &mut Document,
    target: NodeId,
    hook_arguments: &AnyDictionary,
) -> Result<NodeId> {
    run_hook(PRE_ADAPTER_WRITE, document, target, hook_arguments)
}

/// Runs `post_adapter_write` on what was written. What it returns is
/// ignored, as upstream ignores it, though what it changed stays changed.
///
/// # Errors
///
/// As [`run_hook`].
pub fn after_write(
    document: &mut Document,
    target: NodeId,
    hook_arguments: &AnyDictionary,
) -> Result<()> {
    run_hook(POST_ADAPTER_WRITE, document, target, hook_arguments).map(|_| ())
}

/// Whether running the read or write sequence could change anything: some
/// script is attached to one of the adapter hooks, or a linker would run.
///
/// A caller holding a document it may not change can skip copying it when
/// this is false.
#[must_use]
pub fn anything_to_run(choice: &LinkerChoice) -> bool {
    let registry = registry();
    let hooked = ADAPTER_HOOKS.iter().any(|hook| {
        registry
            .scripts_attached_to(hook)
            .is_some_and(|scripts| !scripts.is_empty())
    });
    hooked || !matches!(registry.linker_for(choice), Ok(None))
}
