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
//! Identity is kept by [`Shared::wrapper_for`], which caches a weak reference
//! to each node's Python wrapper and hands the same one back next time. The
//! reference is weak on purpose: a strong one would make a cycle that runs
//! from the wrapper through the document's cache back to the wrapper, and
//! Python's collector cannot see through Rust to break it.
//!
//! Every borrow of the document is taken for one call and dropped before
//! returning to Python. Holding one across a call back into Python would
//! deadlock the moment that code touched the same document.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard};

use otio_core::{Document, NodeId};

use pyo3::exceptions::PyRuntimeError;
use pyo3::prelude::*;
use pyo3::types::{PyWeakrefMethods, PyWeakrefReference};
use pyo3::{Py, PyAny};

/// A document, plus the Python wrappers handed out for its nodes.
#[derive(Default)]
struct Inner {
    document: Document,
    /// Weak references to the wrapper for each node, keyed by node.
    wrappers: HashMap<NodeId, Py<PyWeakrefReference>>,
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
    #[must_use]
    pub fn is(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
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

    /// Runs `f` with the document borrowed for reading.
    ///
    /// # Errors
    ///
    /// Whatever `f` returns, or a `RuntimeError` if the document is poisoned.
    pub fn read<T>(&self, f: impl FnOnce(&Document) -> PyResult<T>) -> PyResult<T> {
        f(&self.lock()?.document)
    }

    /// Runs `f` with the document borrowed for writing.
    ///
    /// # Errors
    ///
    /// As [`Shared::read`].
    pub fn write<T>(&self, f: impl FnOnce(&mut Document) -> PyResult<T>) -> PyResult<T> {
        f(&mut self.lock()?.document)
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
        // Written as `and_then` rather than a let-chain: let-chains are
        // stable from 1.88 and this workspace builds on 1.85.
        let cached = self
            .lock()?
            .wrappers
            .get(&id)
            .and_then(|reference: &Py<PyWeakrefReference>| reference.bind(py).upgrade());
        if let Some(wrapper) = cached {
            return Ok(wrapper);
        }

        let wrapper = build()?;
        let reference = PyWeakrefReference::new(&wrapper)?;
        self.lock()?.wrappers.insert(id, reference.unbind());
        Ok(wrapper)
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
        self.lock()?.wrappers.remove(&id);
        Ok(())
    }
}
