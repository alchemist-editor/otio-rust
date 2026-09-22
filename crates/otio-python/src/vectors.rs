//! `MarkerVector` and `EffectVector`: an item's markers and effects.
//!
//! Upstream binds each of an item's two lists as a class of its own, from
//! one template (`MutableSequencePyAPI` in its `otio_utils.h`), and hands
//! back a live view of the item's list: `item.markers.append(m)` changes the
//! item. Each class takes only its own kind of object, and each can also be
//! built on its own, `MarkerVector()`, as a list that belongs to no item;
//! `copy.copy(item.markers)` builds one that way. The rest of the list
//! interface is written in Python on top of these methods, by the same
//! `_add_mutable_sequence_methods` as `AnyVector`'s.
//!
//! Here the list is the item's, reached through its handle, and putting an
//! object in moves it into the item's document; see [`crate::arena`] for why
//! that is necessary and what it costs. A list that belongs to no item is
//! the list of a hidden item, as a free-standing `AnyVector` is the metadata
//! of a hidden object; see [`crate::containers`].

use otio_core::NodeId;
use otio_core::schema::{ItemData, Node};

use pyo3::exceptions::{PyIndexError, PyStopIteration, PyTypeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::PyList;
use pyo3::{IntoPyObjectExt, Py, PyAny, PyTraverseError, PyVisit};

use crate::arena::Shared;
use crate::containers::{adjusted, in_range};
use crate::objects::{Handle, PyEffect, PyMarker, handle_of, wrap, wrap_root};

/// Which list of an item a [`NodeList`] stands for.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Which {
    /// The item's effects.
    Effects,
    /// The item's markers.
    Markers,
}

impl Which {
    /// The class of object the list holds, as upstream names it, with its
    /// article.
    const fn kind(self) -> &'static str {
        match self {
            Self::Effects => "an Effect",
            Self::Markers => "a Marker",
        }
    }

    /// Borrows the list this stands for.
    const fn of(self, item: &ItemData) -> &Vec<NodeId> {
        match self {
            Self::Effects => &item.effects,
            Self::Markers => &item.markers,
        }
    }

    /// Borrows the list this stands for, mutably.
    const fn of_mut(self, item: &mut ItemData) -> &mut Vec<NodeId> {
        match self {
            Self::Effects => &mut item.effects,
            Self::Markers => &mut item.markers,
        }
    }

    /// Whether `value` is the kind of object this list holds.
    ///
    /// Upstream's list takes a `Marker*` or an `Effect*`, so pybind11 turns
    /// anything else away, subclasses defined in Python included; `None` too,
    /// because the argument is declared `none(false)`.
    fn admits(self, value: &Bound<'_, PyAny>) -> bool {
        match self {
            Self::Effects => value.cast::<PyEffect>().is_ok(),
            Self::Markers => value.cast::<PyMarker>().is_ok(),
        }
    }
}

/// An item's effects or markers: the item, and which of its lists.
pub struct NodeList {
    pub handle: Handle,
    pub which: Which,
}

impl NodeList {
    /// Returns the document these objects live in.
    pub fn home(&self) -> Shared {
        self.handle.shared.clone()
    }

    /// Reads the list of handles.
    fn ids(&self) -> PyResult<Vec<NodeId>> {
        Ok(self
            .handle
            .with(|node| Ok(node.item().map(|item| self.which.of(item).clone())))?
            .unwrap_or_default())
    }

    /// Refuses anything but the kind of object this list holds.
    fn check(&self, value: &Bound<'_, PyAny>) -> PyResult<()> {
        if self.which.admits(value) {
            return Ok(());
        }
        Err(PyTypeError::new_err(format!(
            "incompatible function arguments: expected {}, got {}",
            self.which.kind(),
            value.get_type().name()?
        )))
    }

    /// Moves `value` into this item's document and returns its handle there,
    /// refusing anything but the kind of object this list holds.
    pub fn adopt(&self, value: &Bound<'_, PyAny>) -> PyResult<NodeId> {
        self.check(value)?;
        let incoming = handle_of(value)?;
        self.handle.shared.absorb(&incoming.shared)?;
        let (shared, id) = incoming.live()?;
        shared.mark_owned(value.py(), id, Some(value))?;
        Ok(id)
    }

    /// Returns a wrapper for one of these objects.
    fn wrapper<'py>(&self, py: Python<'py>, id: NodeId) -> PyResult<Bound<'py, PyAny>> {
        wrap(py, &self.handle.sibling(id)?)
    }

    /// Runs `f` on the list, for writing.
    pub fn with_list<T>(&self, f: impl FnOnce(&mut Vec<NodeId>) -> PyResult<T>) -> PyResult<T> {
        let which = self.which;
        self.handle.with_mut(|node| {
            let item = node
                .item_mut()
                .ok_or_else(|| PyValueError::new_err("this object has no effects or markers"))?;
            f(which.of_mut(item))
        })
    }

    /// Returns these objects copied into an ordinary list, for printing.
    pub fn to_list(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        let list = PyList::empty(py);
        for id in self.ids()? {
            list.append(self.wrapper(py, id)?)?;
        }
        list.into_py_any(py)
    }

    /// Reads the object at an index already adjusted, if there is one.
    fn item(&self, py: Python<'_>, index: usize) -> PyResult<Option<Py<PyAny>>> {
        match self.ids()?.get(index) {
            Some(id) => Ok(Some(self.wrapper(py, *id)?.unbind())),
            None => Ok(None),
        }
    }

    /// Upstream's `get_item`: a negative index counts from the end.
    fn get_item(&self, py: Python<'_>, index: i64) -> PyResult<Py<PyAny>> {
        let len = self.ids()?.len();
        in_range(adjusted(index, len), len)
            .map_or(Ok(None), |index| self.item(py, index))?
            .ok_or_else(|| PyIndexError::new_err("list index out of range"))
    }

    /// Upstream's `set_item`. The object replaced is let go of, and freed
    /// unless something else holds it.
    fn set_item(&self, index: i64, value: &Bound<'_, PyAny>) -> PyResult<()> {
        self.check(value)?;
        let len = self.ids()?.len();
        let at = in_range(adjusted(index, len), len)
            .ok_or_else(|| PyIndexError::new_err("list assignment index out of range"))?;
        let id = self.adopt(value)?;
        let old = self.with_list(|list| {
            let slot = list
                .get_mut(at)
                .ok_or_else(|| PyIndexError::new_err("list assignment index out of range"))?;
            Ok(std::mem::replace(slot, id))
        })?;
        if old != id {
            self.home().released(value.py(), old)?;
        }
        Ok(())
    }

    /// Upstream's `del_item`: an index past either end deletes the last
    /// object rather than raising, because upstream compares the adjusted
    /// index as an unsigned number. Only an empty list raises.
    fn del_item(&self, py: Python<'_>, index: i64) -> PyResult<()> {
        let old = self.with_list(|list| {
            let at = in_range(adjusted(index, list.len()), list.len())
                .or_else(|| list.len().checked_sub(1))
                .ok_or_else(|| PyIndexError::new_err("list index out of range"))?;
            Ok(list.remove(at))
        })?;
        self.home().released(py, old)
    }

    /// Upstream's `insert`: an index past either end appends, for the
    /// reason [`NodeList::del_item`] gives.
    fn insert(&self, index: i64, value: &Bound<'_, PyAny>) -> PyResult<()> {
        let id = self.adopt(value)?;
        self.with_list(|list| {
            match in_range(adjusted(index, list.len()), list.len()) {
                Some(at) => list.insert(at, id),
                None => list.push(id),
            }
            Ok(())
        })
    }

    /// A list that belongs to no item: the list of a hidden item, a root
    /// whose wrapper is returned as the list's anchor, so that the item and
    /// what only it holds are freed when the last view of the list goes.
    fn free_standing(py: Python<'_>, which: Which) -> PyResult<(Self, Py<PyAny>)> {
        let handle = Handle::alone(Node::Item(ItemData::new()));
        let anchor = wrap_root(py, &handle)?.unbind();
        Ok((Self { handle, which }, anchor))
    }
}

/// Writes a list class and its iterator; the two lists differ only in what
/// they hold.
macro_rules! node_vector {
    (
        $(#[$doc:meta])*
        $class:ident, $name:literal, $iterator:ident, $iterator_name:literal, $which:expr
    ) => {
        $(#[$doc])*
        #[pyclass(name = $name, module = "opentimelineio._otio")]
        pub struct $class {
            list: NodeList,
            /// What keeps the list there while this view lives: the item's
            /// Python wrapper, as upstream's `reference_internal` keeps the
            /// item alive, or for a list that belongs to no item the hidden
            /// item's.
            anchor: Option<Py<PyAny>>,
        }

        impl $class {
            /// A view of `item`'s list, holding `owner`, the item's wrapper.
            pub fn of(owner: &Bound<'_, PyAny>, handle: Handle) -> Self {
                Self {
                    list: NodeList {
                        handle,
                        which: $which,
                    },
                    anchor: Some(owner.clone().unbind()),
                }
            }
        }

        #[pymethods]
        impl $class {
            /// Builds an empty list that belongs to no item.
            #[new]
            fn new(py: Python<'_>) -> PyResult<Self> {
                let (list, anchor) = NodeList::free_standing(py, $which)?;
                Ok(Self {
                    list,
                    anchor: Some(anchor),
                })
            }

            fn __len__(&self) -> PyResult<usize> {
                Ok(self.list.ids()?.len())
            }

            /// The `__internal_` names are upstream's. Slicing, `append`,
            /// `extend`, `remove`, `pop`, `index` and `count` are all written
            /// once in Python in terms of these four and `__len__`; see
            /// `_core_utils.py`.
            #[pyo3(signature = (index))]
            fn __internal_getitem__(&self, py: Python<'_>, index: i64) -> PyResult<Py<PyAny>> {
                self.list.get_item(py, index)
            }

            #[pyo3(signature = (index, item))]
            fn __internal_setitem__(&self, index: i64, item: &Bound<'_, PyAny>) -> PyResult<()> {
                self.list.set_item(index, item)
            }

            #[pyo3(signature = (index))]
            fn __internal_delitem__(&self, py: Python<'_>, index: i64) -> PyResult<()> {
                self.list.del_item(py, index)
            }

            #[pyo3(name = "__internal_insert", signature = (index, item))]
            fn internal_insert(&self, index: i64, item: &Bound<'_, PyAny>) -> PyResult<()> {
                self.list.insert(index, item)
            }

            fn __iter__(slf: &Bound<'_, Self>) -> $iterator {
                $iterator {
                    container: slf.clone().unbind(),
                    at: 0,
                }
            }

            fn __traverse__(&self, visit: PyVisit<'_>) -> Result<(), PyTraverseError> {
                if let Some(anchor) = &self.anchor {
                    visit.call(anchor)?;
                }
                Ok(())
            }

            fn __clear__(&mut self) {
                self.anchor = None;
            }
        }

        /// Iterates over the list, reading whatever is at the next index as
        /// upstream's iterator does, and stopping at the end.
        #[pyclass(name = $iterator_name, module = "opentimelineio._otio")]
        pub struct $iterator {
            container: Py<$class>,
            at: usize,
        }

        #[pymethods]
        impl $iterator {
            fn __iter__(slf: PyRef<'_, Self>) -> PyRef<'_, Self> {
                slf
            }

            fn __next__(&mut self, py: Python<'_>) -> PyResult<Py<PyAny>> {
                let item = self.container.borrow(py).list.item(py, self.at)?;
                self.at += 1;
                item.ok_or_else(|| PyStopIteration::new_err(()))
            }

            fn __traverse__(&self, visit: PyVisit<'_>) -> Result<(), PyTraverseError> {
                visit.call(&self.container)
            }
        }
    };
}

node_vector!(
    /// An item's markers, as upstream's `MarkerVector`: a list that writes
    /// through to the item and holds only markers.
    PyMarkerVector,
    "MarkerVector",
    PyMarkerVectorIterator,
    "MarkerVectorIterator",
    Which::Markers
);

node_vector!(
    /// An item's effects, as upstream's `EffectVector`: a list that writes
    /// through to the item and holds only effects.
    PyEffectVector,
    "EffectVector",
    PyEffectVectorIterator,
    "EffectVectorIterator",
    Which::Effects
);

/// Returns whether `value` is an item's markers or effects, and the
/// document its objects live in if so.
pub fn home_of(value: &Bound<'_, PyAny>) -> Option<Shared> {
    if let Ok(markers) = value.cast::<PyMarkerVector>() {
        return Some(markers.borrow().list.home());
    }
    if let Ok(effects) = value.cast::<PyEffectVector>() {
        return Some(effects.borrow().list.home());
    }
    None
}

/// Registers the two lists and their iterators on a module.
pub fn register(module: &Bound<'_, PyModule>) -> PyResult<()> {
    module.add_class::<PyMarkerVector>()?;
    module.add_class::<PyMarkerVectorIterator>()?;
    module.add_class::<PyEffectVector>()?;
    module.add_class::<PyEffectVectorIterator>()?;
    Ok(())
}
