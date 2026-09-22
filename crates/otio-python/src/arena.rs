//! How a Python object refers to a node in a document.
//!
//! # The problem
//!
//! `otio-core` keeps every object in a `Document` and names it with a
//! `NodeId`. Upstream's Python API has no document: a `Clip("a")` exists on
//! its own and is appended to a track later, and `track[0] is track[0]` is
//! true because a C++ object has exactly one Python wrapper.
//!
//! # What this does about it
//!
//! Each freshly built object gets a [`Shared`] document of its own, holding
//! just that object and whatever hangs off it. A Python wrapper is a
//! `(Shared, NodeId)` pair, so several wrappers can name the same node in the
//! same document without copying anything.
//!
//! The alternative — one document per interpreter — was rejected. It never
//! gets smaller, so every object anyone builds stays alive until the process
//! exits, and two unrelated timelines end up sharing a pool where a bug in
//! one can reach the other. A document per object means an object you throw
//! away takes its storage with it.
//!
//! # Putting one object inside another
//!
//! `item.markers.append(marker)` has to bring the marker into the item's
//! document, because a handle means nothing outside the arena it came from.
//! [`Shared::absorb`] moves it, and leaves behind a forwarding note: the
//! marker's old document becomes a `Moved` entry naming its new home and the
//! map from old handles to new ones. Every wrapper already handed out for
//! anything in that document keeps working, because each one resolves its
//! handle through that chain before touching it.
//!
//! The alternative was to update each wrapper's handle in place. That works
//! for the object being appended and fails for everything below it: a clip's
//! media reference, a marker's metadata, anything a caller happens to be
//! holding. Forwarding costs a hash lookup per call and cannot go stale.
//!
//! Identity is kept by [`Shared::wrapper_for`], which caches a weak reference
//! to each node's Python wrapper and hands the same one back next time. The
//! reference is weak on purpose: a strong one would make a cycle that runs
//! from the wrapper through the document's cache back to the wrapper, and
//! Python's collector cannot see through Rust to break it.
//!
//! Every borrow of the document is taken for one call and dropped before
//! returning to Python. Holding one across a call back into Python would
//! deadlock the moment that code touched the same document.
//!
//! # How long an object lives
//!
//! Upstream's objects are reference counted, and its Python wrappers take
//! part: an object inside a composition keeps its Python wrapper alive, so
//! an attribute set on `track[0]` is still there the next time `track[0]` is
//! asked for, and an object nothing holds any more is freed at once, taking
//! with it whatever it owned that Python does not also hold. Its own tests
//! pin both halves — deleting a track leaves a clip still held in Python
//! with no parent, and a clip popped from a collection and dropped is gone.
//!
//! Here that is done in three parts.
//!
//! - **Roots.** Each document records which of its objects nothing owns:
//!   what a constructor built, what a copy or a read produced, what was
//!   taken out of a composition. When the wrapper of a root goes away, the
//!   root is freed, and so is everything under it that no live wrapper
//!   holds; anything under it that Python still holds is cut loose as a root
//!   of its own. [`Shared::released`] does the same for an object taken out
//!   of a composition with no wrapper to hold it.
//! - **Retaining.** An object that something owns keeps its wrapper alive, so
//!   that the wrapper — its attributes, its Python subclass — outlives the
//!   Python code that last held it. The strong references live in a
//!   [`Keeper`], one per document, which every wrapper of that document holds
//!   in turn. That makes a cycle, wrapper to keeper to wrapper, but one made
//!   entirely of Python objects that report their references, so Python's
//!   collector can see it and break it once nothing else holds any of them.
//!   The document itself only holds the keeper weakly.
//! - **Identity tokens.** A wrapper being freed cannot be looked up through a
//!   weak reference — on Python 3.11 that would bring it back from the dead
//!   — so each wrapper carries a token and the cache records it, and a
//!   wrapper going away recognises its own entry by the token alone.

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, TryLockError};

use otio_core::{Any, Document, NodeId};

use pyo3::exceptions::PyRuntimeError;
use pyo3::prelude::*;
use pyo3::types::{PyWeakrefMethods, PyWeakrefReference};
use pyo3::{Py, PyAny, PyTraverseError, PyVisit};

/// The wrapper handed out for a node, as the document remembers it.
struct Cached {
    /// The wrapper, weakly: see the module documentation.
    reference: Py<PyWeakrefReference>,
    /// The wrapper's [`Registration::token`].
    token: u64,
}

/// A document, plus the Python wrappers handed out for its nodes.
#[derive(Default)]
struct Live {
    document: Document,
    /// The wrapper for each node, keyed by node.
    wrappers: HashMap<NodeId, Cached>,
    /// The nodes nothing owns; see "How long an object lives".
    roots: HashSet<NodeId>,
    /// This document's keeper, weakly, with the keeper's token so that a
    /// keeper being freed can tell whether this is still its entry.
    keeper: Option<(u64, Py<PyWeakrefReference>)>,
}

/// Hands out the tokens wrappers and keepers are recognised by.
fn next_token() -> u64 {
    static NEXT: AtomicU64 = AtomicU64::new(1);
    NEXT.fetch_add(1, Ordering::Relaxed)
}

/// What a wrapper carries so that its document can keep track of it.
///
/// Every class in `objects` holds one of these beside its handle.
pub struct Registration {
    /// Names this wrapper in its document's cache.
    token: u64,
    /// The keeper of the document the wrapper was registered in. Holding it
    /// keeps alive the wrappers that document retains.
    ///
    /// A mutex around an option rather than a `OnceLock`: Python's collector
    /// can visit an object after it is allocated and before PyO3 has written
    /// its contents, when they are all zero bytes, and zero bytes are an
    /// unlocked mutex around `None` but a `OnceLock` that claims to be set.
    keeper: Mutex<Option<Py<Keeper>>>,
}

impl Registration {
    /// A registration for a wrapper not yet registered anywhere.
    #[must_use]
    pub fn new() -> Self {
        Self {
            token: next_token(),
            keeper: Mutex::new(None),
        }
    }

    /// The token this wrapper is recognised by.
    #[must_use]
    pub const fn token(&self) -> u64 {
        self.token
    }

    /// Reports the keeper to Python's collector.
    ///
    /// # Errors
    ///
    /// Whatever the collector's visitor reports.
    pub fn traverse(&self, visit: &PyVisit<'_>) -> Result<(), PyTraverseError> {
        // Never wait here: the collector cannot. The lock is only ever taken
        // for a moment, with the interpreter attached, so it is free whenever
        // the collector runs.
        if let Ok(keeper) = self.keeper.try_lock() {
            if let Some(keeper) = keeper.as_ref() {
                visit.call(keeper)?;
            }
        }
        Ok(())
    }

    /// Lets go of the keeper, for Python's collector.
    pub fn clear(&mut self) {
        let keeper = match self.keeper.get_mut() {
            Ok(keeper) => keeper.take(),
            Err(poisoned) => poisoned.into_inner().take(),
        };
        drop(keeper);
    }
}

impl Default for Registration {
    fn default() -> Self {
        Self::new()
    }
}

/// Holds the wrappers of the objects a document owns, strongly.
///
/// See "How long an object lives" in the module documentation. When one
/// document is absorbed into another, its keeper hands what it retains to
/// the other's and then points at it, so that wrappers still holding the old
/// keeper keep the new one alive too.
#[pyclass(module = "opentimelineio._otio", weakref)]
pub struct Keeper {
    token: u64,
    /// The document this keeper serves, as it was when the keeper was made.
    home: Shared,
    retained: HashMap<NodeId, Py<PyAny>>,
    forward: Option<Py<Keeper>>,
}

#[pymethods]
impl Keeper {
    fn __traverse__(&self, visit: PyVisit<'_>) -> Result<(), PyTraverseError> {
        // Python's collector can visit an object between its allocation and
        // PyO3 writing its contents, when every byte is zero. No keeper is
        // ever given token zero, so that is how such an object is told apart.
        if self.token == 0 {
            return Ok(());
        }
        for wrapper in self.retained.values() {
            visit.call(wrapper)?;
        }
        if let Some(forward) = &self.forward {
            visit.call(forward)?;
        }
        Ok(())
    }

    fn __clear__(&mut self) {
        self.retained.clear();
        self.forward = None;
    }
}

impl Drop for Keeper {
    // Unhook from the document before the retained wrappers go, so that
    // nothing freed along with them finds a keeper halfway through being
    // freed.
    fn drop(&mut self) {
        let token = self.token;
        let _ = self.home.try_with_live(|live| {
            if live.keeper.as_ref().is_some_and(|(held, _)| *held == token) {
                live.keeper = None;
            }
            Ok(())
        });
    }
}

/// What a [`Shared`] holds: either the document itself, or a note saying
/// where its contents went.
enum Inner {
    Live(Live),
    Moved {
        into: Shared,
        translation: HashMap<NodeId, NodeId>,
    },
}

impl Default for Inner {
    fn default() -> Self {
        Self::Live(Live::default())
    }
}

/// A document shared by every Python object that lives in it.
#[derive(Clone, Default)]
pub struct Shared(Arc<Mutex<Inner>>);

impl Shared {
    /// Builds an empty document.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns whether two handles name the same document.
    ///
    /// Compares where each one currently lives, so a document that has been
    /// absorbed is the same document as the one that absorbed it.
    ///
    /// # Errors
    ///
    /// A `RuntimeError` if either document is poisoned.
    pub fn is(&self, other: &Self) -> PyResult<bool> {
        Ok(Arc::ptr_eq(&self.resolve()?.0, &other.resolve()?.0))
    }

    /// Takes the lock, turning a poisoned mutex into a Python exception.
    ///
    /// The lock is only ever held for the length of one call, so it is never
    /// contended in practice; poisoning means an earlier call panicked, and
    /// the document may be half-edited, so the honest thing is to say so.
    fn lock(&self) -> PyResult<MutexGuard<'_, Inner>> {
        self.0.lock().map_err(|_| {
            PyRuntimeError::new_err("the document was left inconsistent by an earlier error")
        })
    }

    /// Follows the forwarding chain to the document that holds the objects.
    ///
    /// # Errors
    ///
    /// A `RuntimeError` if a document along the way is poisoned.
    pub fn resolve(&self) -> PyResult<Self> {
        let mut here = self.clone();
        loop {
            let next = match &*here.lock()? {
                Inner::Live(_) => return Ok(here.clone()),
                Inner::Moved { into, .. } => into.clone(),
            };
            here = next;
        }
    }

    /// Follows the forwarding chain, translating `id` at each step.
    ///
    /// # Errors
    ///
    /// A `RuntimeError` if a document along the way is poisoned.
    pub fn translate(&self, id: NodeId) -> PyResult<(Self, NodeId)> {
        let mut here = self.clone();
        let mut id = id;
        loop {
            let next = match &*here.lock()? {
                Inner::Live(_) => return Ok((here.clone(), id)),
                Inner::Moved { into, translation } => {
                    if let Some(moved) = translation.get(&id) {
                        id = *moved;
                    }
                    into.clone()
                }
            };
            here = next;
        }
    }

    /// Runs `f` with this document borrowed for reading.
    ///
    /// # Errors
    ///
    /// Whatever `f` returns, or a `RuntimeError` if the document is poisoned.
    pub fn read<T>(&self, f: impl FnOnce(&Document) -> PyResult<T>) -> PyResult<T> {
        let here = self.resolve()?;
        let guard = here.lock()?;
        let Inner::Live(live) = &*guard else {
            unreachable!("just resolved to a live document");
        };
        f(&live.document)
    }

    /// Runs `f` with this document borrowed for writing.
    ///
    /// # Errors
    ///
    /// As [`Shared::read`].
    pub fn write<T>(&self, f: impl FnOnce(&mut Document) -> PyResult<T>) -> PyResult<T> {
        let here = self.resolve()?;
        let mut guard = here.lock()?;
        let Inner::Live(live) = &mut *guard else {
            unreachable!("just resolved to a live document");
        };
        f(&mut live.document)
    }

    /// Moves everything in `other` into this document.
    ///
    /// Afterwards `other` forwards here, so handles and wrappers taken from
    /// it go on working. Does nothing if the two are already the same
    /// document.
    ///
    /// # Errors
    ///
    /// A `RuntimeError` if either document is poisoned.
    pub fn absorb(&self, other: &Self) -> PyResult<()> {
        let here = self.resolve()?;
        let there = other.resolve()?;
        if Arc::ptr_eq(&here.0, &there.0) {
            return Ok(());
        }

        // Take the other document's contents out before touching this one, so
        // that only one lock is ever held at a time.
        let taken = {
            let mut guard = there.lock()?;
            let Inner::Live(live) = std::mem::take(&mut *guard) else {
                unreachable!("just resolved to a live document");
            };
            live
        };

        let translation = {
            let mut guard = here.lock()?;
            let Inner::Live(live) = &mut *guard else {
                unreachable!("just resolved to a live document");
            };
            let translation = live.document.absorb(taken.document);
            for (old, wrapper) in taken.wrappers {
                if let Some(new) = translation.get(&old) {
                    live.wrappers.insert(*new, wrapper);
                }
            }
            for old in taken.roots {
                if let Some(new) = translation.get(&old) {
                    live.roots.insert(*new);
                }
            }
            translation
        };

        *there.lock()? = Inner::Moved {
            into: here.clone(),
            translation: translation.clone(),
        };

        // The absorbed document's keeper hands over what it retains and then
        // forwards, so that the wrappers still holding it keep this
        // document's keeper alive.
        if let Some((_, reference)) = taken.keeper {
            Python::attach(|py| -> PyResult<()> {
                let Some(old) = reference.bind(py).upgrade() else {
                    return Ok(());
                };
                let old = old.cast_into::<Keeper>()?;
                let new = here.keeper(py)?;
                let mut old = old.try_borrow_mut()?;
                let moved: Vec<(NodeId, Py<PyAny>)> = old.retained.drain().collect();
                {
                    let mut new_keeper = new.bind(py).try_borrow_mut()?;
                    for (id, wrapper) in moved {
                        let id = translation.get(&id).copied().unwrap_or(id);
                        new_keeper.retained.insert(id, wrapper);
                    }
                }
                old.forward = Some(new);
                Ok(())
            })?;
        }
        Ok(())
    }

    /// Returns this document's keeper, making one if it has none.
    pub fn keeper(&self, py: Python<'_>) -> PyResult<Py<Keeper>> {
        let here = self.resolve()?;
        let existing = here.with_live(|live| {
            Ok(live
                .keeper
                .as_ref()
                .and_then(|(_, reference)| reference.bind(py).upgrade()))
        })?;
        if let Some(keeper) = existing {
            return Ok(keeper.cast_into::<Keeper>()?.unbind());
        }
        let token = next_token();
        let keeper = Bound::new(
            py,
            Keeper {
                token,
                home: here.clone(),
                retained: HashMap::new(),
                forward: None,
            },
        )?;
        let reference = PyWeakrefReference::new(&keeper)?.unbind();
        here.with_live(|live| {
            live.keeper = Some((token, reference));
            Ok(())
        })?;
        Ok(keeper.unbind())
    }

    /// Returns the Python wrapper for `id`, building one only if there is not
    /// one already.
    ///
    /// This is what makes `track[0] is track[0]` true. A wrapper built for an
    /// object something owns is retained; see "How long an object lives".
    ///
    /// # Errors
    ///
    /// Whatever `build` returns, or a `RuntimeError` if the document is
    /// poisoned.
    pub fn wrapper_for<'py>(
        &self,
        py: Python<'py>,
        id: NodeId,
        build: impl FnOnce() -> PyResult<Bound<'py, PyAny>>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let (here, id) = self.translate(id)?;
        // Written as `and_then` rather than a let-chain: let-chains are
        // stable from 1.88 and this workspace builds on 1.85.
        let cached = here.with_live(|live| {
            Ok(live
                .wrappers
                .get(&id)
                .and_then(|cached: &Cached| cached.reference.bind(py).upgrade()))
        })?;
        if let Some(wrapper) = cached {
            return Ok(wrapper);
        }

        let wrapper = build()?;
        here.register(py, id, &wrapper)?;
        Ok(wrapper)
    }

    /// Remembers `wrapper` as the Python object for `id`, a root.
    ///
    /// A wrapper built by a constructor never goes through
    /// [`Shared::wrapper_for`] — Python makes the object and hands it back —
    /// so it has to be put in the cache afterwards, or the next lookup would
    /// build a second wrapper for the same node and `item.markers[0] is
    /// marker` would be false. What a constructor builds is owned by nothing
    /// yet, so it is recorded as a root.
    ///
    /// # Errors
    ///
    /// A `RuntimeError` if the document is poisoned.
    pub fn remember(&self, id: NodeId, wrapper: &Bound<'_, PyAny>) -> PyResult<()> {
        let (here, id) = self.translate(id)?;
        here.with_live(|live| {
            live.roots.insert(id);
            Ok(())
        })?;
        here.register(wrapper.py(), id, wrapper)
    }

    /// Records `wrapper` in the cache, hands it this document's keeper, and
    /// retains it if something owns its node.
    fn register(&self, py: Python<'_>, id: NodeId, wrapper: &Bound<'_, PyAny>) -> PyResult<()> {
        let keeper = self.keeper(py)?;
        let token = crate::objects::registration_of(wrapper, |registration| {
            // A wrapper registered before keeps the keeper it was given.
            if let Ok(mut held) = registration.keeper.lock() {
                if held.is_none() {
                    *held = Some(keeper.clone_ref(py));
                }
            }
            registration.token
        })?;
        let reference = PyWeakrefReference::new(wrapper)?.unbind();
        let owned = self.with_live(|live| {
            live.wrappers.insert(id, Cached { reference, token });
            Ok(!live.roots.contains(&id))
        })?;
        if owned {
            retain(py, &keeper, id, wrapper);
        }
        Ok(())
    }

    /// Records that `id` is a root: a copy, a read, the answer of an
    /// algorithm, anything new that nothing owns yet.
    ///
    /// Call it before the object's wrapper is built, so that the wrapper is
    /// not retained.
    ///
    /// # Errors
    ///
    /// A `RuntimeError` if the document is poisoned.
    pub fn mark_root(&self, id: NodeId) -> PyResult<()> {
        let (here, id) = self.translate(id)?;
        here.with_live(|live| {
            live.roots.insert(id);
            Ok(())
        })
    }

    /// Records that something now owns `id`, and retains its wrapper.
    ///
    /// `wrapper` is the Python object that was handed in for it, if there was
    /// one; otherwise the cached wrapper, if still alive, is the one kept.
    ///
    /// # Errors
    ///
    /// A `RuntimeError` if the document is poisoned.
    pub fn mark_owned(
        &self,
        py: Python<'_>,
        id: NodeId,
        wrapper: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<()> {
        let (here, id) = self.translate(id)?;
        let cached = here.with_live(|live| {
            live.roots.remove(&id);
            Ok(match wrapper {
                Some(_) => None,
                None => live
                    .wrappers
                    .get(&id)
                    .and_then(|cached| cached.reference.bind(py).upgrade()),
            })
        })?;
        let Some(wrapper) = wrapper.cloned().or(cached) else {
            return Ok(());
        };
        let keeper = here.keeper(py)?;
        retain(py, &keeper, id, &wrapper);
        Ok(())
    }

    /// [`Shared::mark_owned`] for every object a stored value holds.
    ///
    /// # Errors
    ///
    /// A `RuntimeError` if the document is poisoned.
    pub fn mark_value_owned(&self, py: Python<'_>, value: &Any) -> PyResult<()> {
        let mut held = Vec::new();
        value.visit_objects(&mut |id| held.push(id));
        for id in held {
            self.mark_owned(py, id, None)?;
        }
        Ok(())
    }

    /// Records that nothing owns `id` any more.
    ///
    /// The wrapper, if there is one, is no longer retained, and lives only as
    /// long as Python holds it. If Python does not hold it, the object is
    /// freed now, along with whatever below it Python does not hold.
    ///
    /// The object is taken to have been owned only by what it was just taken
    /// out of; nothing searches the document for a second owner.
    ///
    /// # Errors
    ///
    /// A `RuntimeError` if the document is poisoned.
    pub fn released(&self, py: Python<'_>, id: NodeId) -> PyResult<()> {
        let (here, id) = self.translate(id)?;
        let keeper = here.keeper(py)?.into_bound(py);
        let retained = keeper
            .try_borrow_mut()
            .ok()
            .and_then(|mut keeper| keeper.retained.remove(&id));
        #[allow(deprecated)] // the replacement is an unsafe FFI call
        let held = match &retained {
            Some(wrapper) => wrapper.get_refcnt(py) > 1,
            None => here.with_live(|live| {
                Ok(live
                    .wrappers
                    .get(&id)
                    .is_some_and(|cached| cached.reference.bind(py).upgrade().is_some()))
            })?,
        };
        // Freed here rather than left to the wrapper's own going, which would
        // first search the whole document for another owner: taking every
        // child out of a long track one by one would cost the square of its
        // length. Dropping the cache entry first leaves the wrapper's going
        // nothing to do.
        let garbage = here.with_live(|live| {
            live.roots.insert(id);
            if held {
                return Ok(Vec::new());
            }
            live.wrappers.remove(&id);
            Ok(collect(py, live, Some(&keeper), id))
        })?;
        drop(retained);
        drop(garbage);
        Ok(())
    }

    /// Runs one of the edit operations on this document, as upstream's
    /// reference counting would have it run.
    ///
    /// An edit may drop objects from the document: a clip it overwrites, the
    /// first half of one it splits. Upstream's objects live on while Python
    /// holds them; so that these do too, every object with a wrapper is
    /// spared (see [`Document::spare`]), and each one the edit let go of is
    /// then [`Shared::released`] like a child taken out of a composition —
    /// kept while Python holds it, freed now if not.
    ///
    /// # Errors
    ///
    /// A `RuntimeError` if the document is poisoned. The edit's own result
    /// is handed back as it came.
    pub fn edit<T>(&self, py: Python<'_>, f: impl FnOnce(&mut Document) -> T) -> PyResult<T> {
        let here = self.resolve()?;
        let (result, spared) = here.with_live(|live| {
            let held: Vec<NodeId> = live.wrappers.keys().copied().collect();
            live.document.spare(held);
            let result = f(&mut live.document);
            Ok((result, live.document.take_spared()))
        })?;
        for id in spared {
            // An object shared with another owner — a time warp's item keeps
            // the clip's own effects — is still held, and stays put.
            if here.read(|document| Ok(document.owner_of(id).is_none()))? {
                here.released(py, id)?;
            }
        }
        Ok(result)
    }

    /// Called as the wrapper carrying `token` for `id` is freed.
    ///
    /// Forgets the wrapper and, if the object is a root that nothing turns
    /// out to own, frees it. Never fails and never waits: it runs while
    /// Python is freeing an object, where neither is allowed, so a document
    /// that is busy is left alone.
    pub fn wrapper_dropped(&self, py: Python<'_>, id: NodeId, token: u64) {
        let Ok((here, id)) = self.translate(id) else {
            return;
        };
        let keeper = here
            .try_with_live(|live| {
                Ok(live
                    .keeper
                    .as_ref()
                    .and_then(|(_, reference)| reference.bind(py).upgrade()))
            })
            .ok()
            .flatten()
            .and_then(|keeper| keeper.cast_into::<Keeper>().ok());
        let garbage = here.try_with_live(|live| {
            if live
                .wrappers
                .get(&id)
                .is_none_or(|cached| cached.token != token)
            {
                return Ok(Vec::new());
            }
            live.wrappers.remove(&id);
            if !live.roots.contains(&id) || !live.document.contains(id) {
                return Ok(Vec::new());
            }
            if live.document.owner_of(id).is_some() {
                live.roots.remove(&id);
                return Ok(Vec::new());
            }
            Ok(collect(py, live, keeper.as_ref(), id))
        });
        drop(keeper);
        drop(garbage);
    }

    /// Forgets the cached wrapper for `id`.
    ///
    /// Called when a node leaves the document, so that a node later given the
    /// same slot does not inherit the old object's wrapper. The arena bumps
    /// the slot's generation too, so a stale `NodeId` would not match anyway;
    /// this keeps the map from growing.
    ///
    /// # Errors
    ///
    /// A `RuntimeError` if the document is poisoned.
    pub fn forget(&self, id: NodeId) -> PyResult<()> {
        let (here, id) = self.translate(id)?;
        here.with_live(|live| {
            live.wrappers.remove(&id);
            live.roots.remove(&id);
            Ok(())
        })
    }

    /// Runs `f` on this document's contents, which must already be live.
    fn with_live<T>(&self, f: impl FnOnce(&mut Live) -> PyResult<T>) -> PyResult<T> {
        let here = self.resolve()?;
        let mut guard = here.lock()?;
        let Inner::Live(live) = &mut *guard else {
            unreachable!("just resolved to a live document");
        };
        f(live)
    }

    /// As [`Shared::with_live`], but gives up rather than wait if the
    /// document is in use.
    fn try_with_live<T>(&self, f: impl FnOnce(&mut Live) -> PyResult<T>) -> PyResult<T> {
        let mut here = self.clone();
        loop {
            let next = {
                let mut guard = match here.0.try_lock() {
                    Ok(guard) => guard,
                    Err(TryLockError::WouldBlock) => {
                        return Err(PyRuntimeError::new_err("the document is in use"));
                    }
                    Err(TryLockError::Poisoned(_)) => {
                        return Err(PyRuntimeError::new_err(
                            "the document was left inconsistent by an earlier error",
                        ));
                    }
                };
                match &mut *guard {
                    Inner::Live(live) => return f(live),
                    Inner::Moved { into, .. } => into.clone(),
                }
            };
            here = next;
        }
    }
}

/// Retains `wrapper` as the one for `id` in `keeper`.
///
/// A keeper busy being cleared by Python's collector is left alone: it is
/// letting go of everything anyway.
fn retain(py: Python<'_>, keeper: &Py<Keeper>, id: NodeId, wrapper: &Bound<'_, PyAny>) {
    if let Ok(mut keeper) = keeper.bind(py).try_borrow_mut() {
        keeper.retained.insert(id, wrapper.clone().unbind());
    }
}

/// Frees `root` and everything below it that Python does not hold.
///
/// Anything below that Python does hold is cut loose and becomes a root of
/// its own, as upstream's objects outlive a deleted parent they are still
/// referenced from. Returns the wrappers let go of, to be dropped once the
/// document is unlocked: dropping one may free it, and freeing a wrapper
/// takes the lock.
fn collect(
    py: Python<'_>,
    live: &mut Live,
    keeper: Option<&Bound<'_, Keeper>>,
    root: NodeId,
) -> Vec<Py<PyAny>> {
    // With no keeper, or one busy being cleared, nothing is retained and the
    // cache alone says what Python holds.
    let mut keeper = keeper.and_then(|keeper| keeper.try_borrow_mut().ok());
    let mut let_go = Vec::new();
    let mut garbage = Vec::new();
    let mut seen = HashSet::new();
    let mut pending = vec![root];

    while let Some(id) = pending.pop() {
        if !seen.insert(id) || !live.document.contains(id) {
            continue;
        }
        if id != root {
            let retained = keeper
                .as_mut()
                .and_then(|keeper| keeper.retained.remove(&id));
            // A retained wrapper with no reference but the keeper's dies
            // with its object; one Python holds elsewhere keeps it alive.
            #[allow(deprecated)] // the replacement is an unsafe FFI call
            let held = match &retained {
                Some(wrapper) => wrapper.get_refcnt(py) > 1,
                None => live
                    .wrappers
                    .get(&id)
                    .is_some_and(|cached| cached.reference.bind(py).upgrade().is_some()),
            };
            let_go.extend(retained);
            if held {
                if let Some(node) = live.document.get_mut(id) {
                    node.set_parent(None);
                }
                live.roots.insert(id);
                continue;
            }
        }
        garbage.push(id);
        if let Some(node) = live.document.get(id) {
            node.visit_owned(&mut |owned| pending.push(owned));
        }
    }

    for id in garbage {
        live.document.remove(id);
        live.wrappers.remove(&id);
        live.roots.remove(&id);
    }
    let_go
}
