//! The framework otio-rust's extensions are built on.
//!
//! An extension is work that lives in this repository, is built against the
//! same arena and reaches every language through the same C ABI and SDK
//! generator, but is **not part of OpenTimelineIO**: timeline sync, ORI
//! Annotations, and whatever follows. [ADR 0010] sets out the rules; this
//! crate is the part every extension shares.
//!
//! # Off unless a program turns it on
//!
//! An extension does nothing until [`enable`] is called with its id. Until
//! then its schemas are not registered, so a file holding them reads them as
//! `UnknownSchema` and writes them back unchanged, exactly as upstream
//! OpenTimelineIO treats a schema whose schemadef plugin is not loaded.
//!
//! ```text
//!  install(&SYNC) ──► available, does nothing
//!  enable("sync") ──► dependencies first, then SYNC.register()
//!                     schemas and hooks now in the core's registries
//! ```
//!
//! Enabling is process-wide, like the core's schema registry, and cannot be
//! undone: a schema registered stays registered. Enabling an extension that
//! is already on does nothing and succeeds.
//!
//! # Installing
//!
//! A program makes an extension available with [`install`], usually through
//! the extension crate's own `install()` function. The C ABI installs every
//! extension compiled into `libotio` before it answers anything, so a C
//! caller only ever calls [`enable`].
//!
//! [ADR 0010]: https://github.com/alchemist-editor/otio-rust/blob/main/docs/adr/0010-extensions.md

use std::collections::BTreeMap;
use std::fmt;
use std::sync::{Mutex, MutexGuard, OnceLock};

/// One extension: what it is called, what it needs, and how it plugs in.
///
/// An implementation is a unit struct held in a `static`, so that
/// [`install`] can keep a `&'static` to it.
pub trait Extension: Sync {
    /// The stable id a program enables it by, such as `sync`. Lower case
    /// ASCII letters, digits and `-`.
    fn id(&self) -> &'static str;

    /// The extension's own version, which is independent of the library's.
    fn version(&self) -> &'static str;

    /// What it does, in a sentence.
    fn summary(&self) -> &'static str;

    /// The ids of the extensions it is built on. Enabling it enables these
    /// first, in the order given.
    fn depends_on(&self) -> &'static [&'static str] {
        &[]
    }

    /// The schema names it registers, such as `PaintStart`, so a schema can
    /// be traced back to the extension that brought it.
    fn schemas(&self) -> &'static [&'static str] {
        &[]
    }

    /// Adds its schemas, upgrade and downgrade functions and hooks to the
    /// core's registries. Called once, by [`enable`], after every
    /// dependency is on.
    ///
    /// # Errors
    ///
    /// A sentence saying what could not be registered.
    fn register(&self) -> Result<(), String>;
}

/// Why an extension could not be installed or enabled.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// No installed extension has this id.
    Unknown(String),
    /// Two different extensions were installed under one id.
    Conflict(String),
    /// Two extensions both claim a schema name.
    SchemaClaimed {
        /// The schema both claim.
        schema: String,
        /// The extension that claimed it first.
        owner: String,
        /// The extension that claimed it second.
        claimant: String,
    },
    /// An extension depends, directly or not, on itself.
    Cycle(Vec<String>),
    /// An extension's `register` failed.
    Register {
        /// The extension whose registration failed.
        id: String,
        /// What it said.
        message: String,
    },
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unknown(id) => {
                let available = available().join(", ");
                if available.is_empty() {
                    write!(formatter, "no extension '{id}' is installed, and none are")
                } else {
                    write!(
                        formatter,
                        "no extension '{id}' is installed; installed extensions are {available}"
                    )
                }
            }
            Self::Conflict(id) => write!(
                formatter,
                "two different extensions are installed under the id '{id}'"
            ),
            Self::SchemaClaimed {
                schema,
                owner,
                claimant,
            } => write!(
                formatter,
                "extension '{claimant}' claims schema '{schema}', which extension '{owner}' \
                 already owns"
            ),
            Self::Cycle(chain) => write!(
                formatter,
                "extensions depend on each other in a cycle: {}",
                chain.join(" -> ")
            ),
            Self::Register { id, message } => {
                write!(formatter, "extension '{id}' could not register: {message}")
            }
        }
    }
}

impl std::error::Error for Error {}

#[derive(Default)]
struct State {
    installed: BTreeMap<&'static str, &'static dyn Extension>,
    /// Ids in the order they were enabled.
    enabled: Vec<&'static str>,
    /// Which extension each claimed schema belongs to.
    owners: BTreeMap<&'static str, &'static str>,
}

fn state() -> MutexGuard<'static, State> {
    static STATE: OnceLock<Mutex<State>> = OnceLock::new();
    STATE
        .get_or_init(Mutex::default)
        .lock()
        // A panic while the state was held left it as it was before the
        // panicking call made any change, since every change is a single
        // insert or push; going on is safe.
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Makes an extension available to [`enable`]. It does nothing yet.
///
/// Installing the same extension twice does nothing.
///
/// # Errors
///
/// [`Error::Conflict`] if a different extension is installed under the same
/// id, or [`Error::SchemaClaimed`] if another installed extension claims one
/// of its schemas.
pub fn install(extension: &'static dyn Extension) -> Result<(), Error> {
    let mut state = state();
    let id = extension.id();
    if let Some(existing) = state.installed.get(id) {
        if same(*existing, extension) {
            return Ok(());
        }
        return Err(Error::Conflict(id.to_string()));
    }
    for schema in extension.schemas() {
        if let Some(owner) = state.owners.get(schema) {
            return Err(Error::SchemaClaimed {
                schema: (*schema).to_string(),
                owner: (*owner).to_string(),
                claimant: id.to_string(),
            });
        }
    }
    for schema in extension.schemas() {
        state.owners.insert(schema, id);
    }
    state.installed.insert(id, extension);
    Ok(())
}

fn same(left: &dyn Extension, right: &dyn Extension) -> bool {
    std::ptr::addr_eq(left as *const dyn Extension, right as *const dyn Extension)
}

/// Turns an installed extension on, and everything it depends on first.
///
/// Enabling one that is already on does nothing and succeeds.
///
/// # Errors
///
/// [`Error::Unknown`] if it, or something it depends on, is not installed;
/// [`Error::Cycle`] if the dependencies loop; [`Error::Register`] if a
/// registration fails. Extensions enabled before the failure stay on.
pub fn enable(id: &str) -> Result<(), Error> {
    // Work out the order first, holding the lock, then register each
    // without it, so a `register` may itself ask what is enabled.
    let order = {
        let state = state();
        let mut order = Vec::new();
        let mut path = Vec::new();
        plan(&state, id, &mut path, &mut order)?;
        order
    };
    for extension in order {
        if is_enabled(extension.id()) {
            continue;
        }
        extension.register().map_err(|message| Error::Register {
            id: extension.id().to_string(),
            message,
        })?;
        let mut state = state();
        if !state.enabled.contains(&extension.id()) {
            state.enabled.push(extension.id());
        }
    }
    Ok(())
}

/// Puts `id` and its dependencies into `order`, dependencies first.
fn plan(
    state: &State,
    id: &str,
    path: &mut Vec<&'static str>,
    order: &mut Vec<&'static dyn Extension>,
) -> Result<(), Error> {
    let extension = *state
        .installed
        .get(id)
        .ok_or_else(|| Error::Unknown(id.to_string()))?;
    if path.contains(&extension.id()) {
        let mut chain: Vec<String> = path.iter().map(|step| (*step).to_string()).collect();
        chain.push(extension.id().to_string());
        return Err(Error::Cycle(chain));
    }
    if order.iter().any(|done| done.id() == extension.id()) {
        return Ok(());
    }
    path.push(extension.id());
    for dependency in extension.depends_on() {
        plan(state, dependency, path, order)?;
    }
    path.pop();
    order.push(extension);
    Ok(())
}

/// Whether an extension is on.
#[must_use]
pub fn is_enabled(id: &str) -> bool {
    state().enabled.contains(&id)
}

/// The ids of the extensions that are on, in the order they were enabled.
#[must_use]
pub fn enabled() -> Vec<&'static str> {
    state().enabled.clone()
}

/// The ids of every installed extension, on or not, in id order.
#[must_use]
pub fn available() -> Vec<&'static str> {
    state().installed.keys().copied().collect()
}

/// An installed extension, by id.
#[must_use]
pub fn get(id: &str) -> Option<&'static dyn Extension> {
    state().installed.get(id).copied()
}

/// The id of the installed extension that claims a schema, if any.
#[must_use]
pub fn owner_of(schema_name: &str) -> Option<&'static str> {
    state().owners.get(schema_name).copied()
}
