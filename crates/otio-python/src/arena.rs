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

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard};

use otio_core::{Document, NodeId};

use pyo3::exceptions::PyRuntimeError;
use pyo3::prelude::*;
use pyo3::types::{PyWeakrefMethods, PyWeakrefReference};
use pyo3::{Py, PyAny};

/// A document, plus the Python wrappers handed out for its nodes.
#[derive(Default)]
struct Live {
    document: Document,
    /// Weak references to the wrapper for each node, keyed by node.
    wrappers: HashMap<NodeId, Py<PyWeakrefReference>>,
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

thread_local! {
    /// The documents this thread's [`Shared::read`] and [`Shared::write`]
    /// calls have borrowed, innermost last.
    static BORROWED: RefCell<Vec<Shared>> = const { RefCell::new(Vec::new()) };
}

/// Takes a document off [`BORROWED`] when its borrow ends, however it ends.
struct Borrow;

impl Borrow {
    fn begin(shared: &Shared) -> Self {
        BORROWED.with(|borrowed| borrowed.borrow_mut().push(shared.clone()));
        Self
    }
}

impl Drop for Borrow {
    fn drop(&mut self) {
        BORROWED.with(|borrowed| {
            borrowed.borrow_mut().pop();
        });
    }
}

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
        let _borrow = Borrow::begin(&here);
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
        let _borrow = Borrow::begin(&here);
        f(&mut live.document)
    }

    /// The document the innermost [`Shared::read`] or [`Shared::write`]
    /// running on this thread has borrowed, if one is running.
    ///
    /// An error raised inside one names its objects by handle alone; this
    /// says which document the handle belongs to.
    #[must_use]
    pub fn borrowed() -> Option<Self> {
        BORROWED.with(|borrowed| borrowed.borrow().last().cloned())
    }

    /// Whether this thread is inside a borrow of this document, so that
    /// taking it again would deadlock.
    #[must_use]
    pub fn is_borrowed(&self) -> bool {
        BORROWED.with(|borrowed| {
            borrowed
                .borrow()
                .iter()
                .any(|each| Arc::ptr_eq(&each.0, &self.0))
        })
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
            translation
        };

        *there.lock()? = Inner::Moved {
            into: here,
            translation,
        };
        Ok(())
    }

    /// Returns the Python wrapper for `id`, building one only if there is not
    /// one already.
    ///
    /// This is what makes `track[0] is track[0]` true.
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
                .and_then(|reference: &Py<PyWeakrefReference>| reference.bind(py).upgrade()))
        })?;
        if let Some(wrapper) = cached {
            return Ok(wrapper);
        }

        let wrapper = build()?;
        let reference = PyWeakrefReference::new(&wrapper)?;
        here.with_live(|live| {
            live.wrappers.insert(id, reference.unbind());
            Ok(())
        })?;
        Ok(wrapper)
    }

    /// Remembers `wrapper` as the Python object for `id`.
    ///
    /// A wrapper built by a constructor never goes through
    /// [`Shared::wrapper_for`] — Python makes the object and hands it back —
    /// so it has to be put in the cache afterwards, or the next lookup would
    /// build a second wrapper for the same node and `item.markers[0] is
    /// marker` would be false.
    ///
    /// # Errors
    ///
    /// A `RuntimeError` if the document is poisoned.
    pub fn remember(&self, id: NodeId, wrapper: &Bound<'_, PyAny>) -> PyResult<()> {
        let (here, id) = self.translate(id)?;
        let reference = PyWeakrefReference::new(wrapper)?;
        here.with_live(|live| {
            live.wrappers.insert(id, reference.unbind());
            Ok(())
        })
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
}
