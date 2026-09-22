//! `AnyDictionary` and `AnyVector`: upstream's containers for metadata.
//!
//! Upstream stores metadata as C++ `AnyDictionary` and `AnyVector` values,
//! and hands Python a proxy object for each that reads and writes the C++
//! container in place. Reading `clip.metadata["tags"]` gives a live
//! `AnyVector`, so `clip.metadata["tags"].append("x")` changes the clip, and
//! upstream's own plugin manifest relies on that (`manifest.adapters.extend`).
//! Either class can also be built on its own, `AnyDictionary()`, as a
//! container that belongs to nothing.
//!
//! The same two classes are used for both here. A view names its container
//! by the way to it — an object, which of the object's dictionaries
//! ([`Bag`]), and the keys and indices leading down from there ([`Step`]) —
//! rather than by holding it, because nothing may hold a borrow of a document
//! past one call (see [`crate::arena`]). A container that belongs to nothing
//! lives in the metadata of a hidden object in a document of its own.
//!
//! Upstream raises "has been destroyed" when the C++ container a proxy points
//! at is gone. The same error is raised here when the way to the container
//! no longer leads to one: the object was freed, or the entry was removed or
//! replaced by a value of another kind. One difference follows from naming
//! by path: where upstream's proxy of a replaced entry reports it destroyed,
//! a view here sees the entry that replaced it.
//!
//! Upstream builds the rest of the `MutableMapping` and `MutableSequence`
//! interfaces in Python on top of the methods written here, and so does
//! `core/_core_utils.py`.

use std::sync::atomic::{AtomicBool, Ordering};

use otio_core::schema::Base;
use otio_core::{Any, AnyDictionary, Node};

use pyo3::exceptions::{PyIndexError, PyKeyError, PyStopIteration, PyValueError};
use pyo3::prelude::*;
use pyo3::types::PyDict;
use pyo3::{IntoPyObjectExt, Py, PyAny, PyTraverseError, PyVisit};

use crate::arena::Shared;
use crate::objects::{Handle, dynamic_fields_mut};
use crate::values::{any_to_python, python_to_any};

/// Which dictionary on an object a container view starts from.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Bag {
    /// The object's `metadata`, which every named object has.
    Metadata,
    /// A generator reference's `parameters`, which only it has.
    Parameters,
    /// The dynamic fields of an object upstream's two root classes, or a
    /// schema registered from Python, describe.
    Dynamic,
}

/// One step down from a container to a container inside it.
#[derive(Clone)]
enum Step {
    /// The entry under a key of a dictionary.
    Key(String),
    /// The item at an index of a vector.
    Index(usize),
}

/// The way to one container: an object, one of its dictionaries, and the
/// steps down from there.
#[derive(Clone)]
struct Place {
    handle: Handle,
    bag: Bag,
    path: Vec<Step>,
}

/// A container found by following a [`Place`].
enum Found<'a> {
    Dictionary(&'a AnyDictionary),
    Vector(&'a [Any]),
}

/// A container found by following a [`Place`], for writing.
enum FoundMut<'a> {
    Dictionary(&'a mut AnyDictionary),
    Vector(&'a mut Vec<Any>),
}

/// The key under which a free-standing `AnyVector` keeps its items in its
/// hidden object's metadata.
const VECTOR_KEY: &str = "";

impl Place {
    /// The place one step further down.
    fn down(&self, step: Step) -> Self {
        let mut path = self.path.clone();
        path.push(step);
        Self {
            handle: self.handle.clone(),
            bag: self.bag,
            path,
        }
    }

    /// The document this container lives in now.
    fn home(&self) -> PyResult<Shared> {
        Ok(self.handle.live()?.0)
    }

    /// Runs `f` on the container, or on `None` if there is none there.
    fn read<T>(&self, f: impl FnOnce(Option<Found<'_>>) -> PyResult<T>) -> PyResult<T> {
        let (shared, id) = self.handle.live()?;
        let empty = AnyDictionary::new();
        shared.read(|document| {
            let Some(node) = document.get(id) else {
                return f(None);
            };
            let root = match self.bag {
                // An object with no metadata reads as an empty mapping
                // rather than an error, which is what upstream's base class
                // does; so does one with no dynamic fields.
                Bag::Metadata => node.base().map_or(&empty, |base| &base.metadata),
                Bag::Parameters => match node {
                    Node::GeneratorReference(reference) => &reference.parameters,
                    _ => return f(None),
                },
                Bag::Dynamic => match node {
                    Node::Dynamic(dynamic) => &dynamic.fields,
                    _ => &empty,
                },
            };
            let mut found = Found::Dictionary(root);
            for step in &self.path {
                let value = match (found, step) {
                    (Found::Dictionary(entries), Step::Key(key)) => entries.get(key),
                    (Found::Vector(items), Step::Index(index)) => items.get(*index),
                    _ => None,
                };
                found = match value {
                    Some(Any::Dictionary(entries)) => Found::Dictionary(entries),
                    Some(Any::Vector(items)) => Found::Vector(items),
                    _ => return f(None),
                };
            }
            f(Some(found))
        })
    }

    /// Runs `f` on the container for writing, or on `None` if there is none
    /// there.
    fn write<T>(&self, f: impl FnOnce(Option<FoundMut<'_>>) -> PyResult<T>) -> PyResult<T> {
        let (shared, id) = self.handle.live()?;
        shared.write(|document| {
            let Some(node) = document.get_mut(id) else {
                return f(None);
            };
            let schema = node.schema_name().to_string();
            let root = match self.bag {
                Bag::Metadata => {
                    &mut node
                        .base_mut()
                        .ok_or_else(|| {
                            PyValueError::new_err(format!("a {schema} has no metadata"))
                        })?
                        .metadata
                }
                Bag::Parameters => match node {
                    Node::GeneratorReference(reference) => &mut reference.parameters,
                    _ => return f(None),
                },
                Bag::Dynamic => dynamic_fields_mut(node)?,
            };
            let mut found = FoundMut::Dictionary(root);
            for step in &self.path {
                let value = match (found, step) {
                    (FoundMut::Dictionary(entries), Step::Key(key)) => entries.get_mut(key),
                    (FoundMut::Vector(items), Step::Index(index)) => items.get_mut(*index),
                    _ => None,
                };
                found = match value {
                    Some(Any::Dictionary(entries)) => FoundMut::Dictionary(entries),
                    Some(Any::Vector(items)) => FoundMut::Vector(items),
                    _ => return f(None),
                };
            }
            f(Some(found))
        })
    }

    /// The Python object for a value read out of this container at `step`:
    /// a view for a container, a Python value for anything else.
    fn value_to_python(
        &self,
        py: Python<'_>,
        anchor: Option<&Py<PyAny>>,
        step: Step,
        value: &Any,
    ) -> PyResult<Py<PyAny>> {
        let anchor = anchor.map(|anchor| anchor.clone_ref(py));
        match value {
            Any::Dictionary(_) => PyAnyDictionary::at(self.down(step), anchor).into_py_any(py),
            Any::Vector(_) => PyAnyVector::at(self.down(step), anchor).into_py_any(py),
            // The value is converted against the document as it is now: an
            // object id read out of it is an id there, not in whatever
            // document the view was first made in.
            value => any_to_python(py, &self.home()?, value),
        }
    }
}

/// A free-standing container: a hidden object in a document of its own.
///
/// The document's keeper is returned with it, as the container's anchor, so
/// that the objects put in it keep their Python wrappers while it lives.
fn free_standing(py: Python<'_>, metadata: AnyDictionary) -> PyResult<(Handle, Py<PyAny>)> {
    let handle = Handle::alone(Node::SerializableObjectWithMetadata(Base {
        metadata,
        ..Base::default()
    }));
    let keeper = handle.shared.keeper(py)?.into_any();
    Ok((handle, keeper))
}

/// A dictionary of metadata, as upstream's `AnyDictionary`.
///
/// Every object's `metadata` is one of these, as are the dictionaries inside
/// it and a generator reference's `parameters`; `AnyDictionary()` builds one
/// that belongs to nothing.
#[pyclass(name = "AnyDictionary", module = "opentimelineio._otio")]
pub struct PyAnyDictionary {
    place: Place,
    /// Set by `_testing.test_AnyDictionary_destroy`, which upstream uses to
    /// delete the C++ dictionary from under its proxy.
    destroyed: AtomicBool,
    /// What keeps the container there while this view lives: the Python
    /// object whose dictionary it is, or for a free-standing container its
    /// document's keeper (see [`free_standing`]).
    ///
    /// Upstream's proxy holds nothing, so a proxy that outlives its object
    /// reports its dictionary destroyed; `read_from_string(s).metadata`
    /// would be unusable. Holding the object instead is a deliberate
    /// difference, and never turns an upstream success into a failure.
    anchor: Option<Py<PyAny>>,
}

/// The error upstream raises for a proxy whose dictionary has gone.
fn dictionary_destroyed() -> PyErr {
    PyValueError::new_err("Underlying C++ AnyDictionary has been destroyed")
}

impl PyAnyDictionary {
    /// A view of the dictionary at `place`.
    fn at(place: Place, anchor: Option<Py<PyAny>>) -> Self {
        Self {
            place,
            destroyed: AtomicBool::new(false),
            anchor,
        }
    }

    /// A view of one of an object's dictionaries, holding `owner`, the
    /// object's Python wrapper, if given.
    pub fn of(owner: Option<&Bound<'_, PyAny>>, handle: Handle, bag: Bag) -> Self {
        Self::at(
            Place {
                handle,
                bag,
                path: Vec::new(),
            },
            owner.map(|owner| owner.clone().unbind()),
        )
    }

    /// Runs `f` on the dictionary.
    fn entries<T>(&self, f: impl FnOnce(&AnyDictionary) -> PyResult<T>) -> PyResult<T> {
        if self.destroyed.load(Ordering::Relaxed) {
            return Err(dictionary_destroyed());
        }
        self.place.read(|found| match found {
            Some(Found::Dictionary(entries)) => f(entries),
            _ => Err(dictionary_destroyed()),
        })
    }

    /// Runs `f` on the dictionary, for writing.
    fn entries_mut<T>(&self, f: impl FnOnce(&mut AnyDictionary) -> PyResult<T>) -> PyResult<T> {
        if self.destroyed.load(Ordering::Relaxed) {
            return Err(dictionary_destroyed());
        }
        self.place.write(|found| match found {
            Some(FoundMut::Dictionary(entries)) => f(entries),
            _ => Err(dictionary_destroyed()),
        })
    }

    /// Returns the dictionary copied into an ordinary Python `dict`, for
    /// printing.
    fn to_plain(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        let entries = self.entries(|entries| Ok(entries.clone()))?;
        let home = self.place.home()?;
        let dict = PyDict::new(py);
        for (key, value) in &entries {
            dict.set_item(key, any_to_python(py, &home, value)?)?;
        }
        dict.into_py_any(py)
    }
}

#[pymethods]
impl PyAnyDictionary {
    /// Builds an empty dictionary that belongs to nothing.
    #[new]
    fn new(py: Python<'_>) -> PyResult<Self> {
        let (handle, keeper) = free_standing(py, AnyDictionary::new())?;
        let place = Place {
            handle,
            bag: Bag::Metadata,
            path: Vec::new(),
        };
        Ok(Self::at(place, Some(keeper)))
    }

    fn __getitem__(&self, py: Python<'_>, key: &str) -> PyResult<Py<PyAny>> {
        // Copied out first: turning an object id into its wrapper reads the
        // document again, and a borrow lasts one call; see [`crate::arena`].
        let value = self.entries(|entries| {
            entries
                .get(key)
                .cloned()
                .ok_or_else(|| PyKeyError::new_err(key.to_string()))
        })?;
        self.place
            .value_to_python(py, self.anchor.as_ref(), Step::Key(key.to_string()), &value)
    }

    fn __setitem__(&self, key: &str, item: &Bound<'_, PyAny>) -> PyResult<()> {
        if self.destroyed.load(Ordering::Relaxed) {
            return Err(dictionary_destroyed());
        }
        let home = self.place.home()?;
        let value = python_to_any(&home, item)?;
        let held = value.clone();
        self.entries_mut(|entries| {
            entries.insert(key.to_string(), value);
            Ok(())
        })?;
        home.mark_value_owned(item.py(), &held)
    }

    /// Upstream's name for the write its Python `__setitem__` delegates to.
    fn __internal_setitem__(&self, key: &str, item: &Bound<'_, PyAny>) -> PyResult<()> {
        self.__setitem__(key, item)
    }

    fn __delitem__(&self, key: &str) -> PyResult<()> {
        self.entries_mut(|entries| {
            entries
                .remove(key)
                .map(|_| ())
                .ok_or_else(|| PyKeyError::new_err(key.to_string()))
        })
    }

    fn __len__(&self) -> PyResult<usize> {
        self.entries(|entries| Ok(entries.len()))
    }

    /// Iterates over the keys, refusing to go on if a key is added or
    /// removed meanwhile, as upstream's iterator does.
    fn __iter__(slf: &Bound<'_, Self>) -> PyResult<PyAnyDictionaryIterator> {
        let keys = slf
            .borrow()
            .entries(|entries| Ok(entries.keys().cloned().collect()))?;
        Ok(PyAnyDictionaryIterator {
            container: slf.clone().unbind(),
            keys,
            at: 0,
        })
    }

    fn __contains__(&self, key: &str) -> PyResult<bool> {
        self.entries(|entries| Ok(entries.contains_key(key)))
    }

    fn __repr__(&self, py: Python<'_>) -> PyResult<String> {
        Ok(self.to_plain(py)?.bind(py).repr()?.to_string())
    }

    fn __str__(&self, py: Python<'_>) -> PyResult<String> {
        self.__repr__(py)
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

/// Iterates over an `AnyDictionary`'s keys.
#[pyclass(name = "AnyDictionaryIterator", module = "opentimelineio._otio")]
pub struct PyAnyDictionaryIterator {
    container: Py<PyAnyDictionary>,
    /// The keys when iteration began. Upstream compares a mutation stamp;
    /// comparing the keys catches the same changes, since replacing a value
    /// in place does not move upstream's stamp either.
    keys: Vec<String>,
    at: usize,
}

#[pymethods]
impl PyAnyDictionaryIterator {
    fn __iter__(slf: PyRef<'_, Self>) -> PyRef<'_, Self> {
        slf
    }

    fn __next__(&mut self, py: Python<'_>) -> PyResult<String> {
        let unchanged = self.container.borrow(py).entries(|entries| {
            Ok(entries.len() == self.keys.len()
                && entries
                    .keys()
                    .zip(&self.keys)
                    .all(|(now, then)| now == then))
        })?;
        if !unchanged {
            return Err(PyValueError::new_err("container mutated during iteration"));
        }
        let key = self
            .keys
            .get(self.at)
            .cloned()
            .ok_or_else(|| PyStopIteration::new_err(()))?;
        self.at += 1;
        Ok(key)
    }
}

/// A list of metadata values, as upstream's `AnyVector`.
///
/// A list read out of metadata is one of these, and writes through to it;
/// `AnyVector()` builds one that belongs to nothing. As upstream's, it has
/// no `__eq__`: it compares equal only to itself.
#[pyclass(name = "AnyVector", module = "opentimelineio._otio")]
pub struct PyAnyVector {
    place: Place,
    /// Set by `_testing.test_AnyVector_destroy`.
    destroyed: AtomicBool,
    /// As [`PyAnyDictionary::anchor`].
    anchor: Option<Py<PyAny>>,
}

/// The error upstream raises for a proxy whose vector has gone.
fn vector_destroyed() -> PyErr {
    PyValueError::new_err("Underlying C++ AnyVector object has been destroyed")
}

/// Upstream's `adjusted_vector_index`: a negative index counts from the end.
/// The result may still be out of range either way.
fn adjusted(index: i64, len: usize) -> i64 {
    if index < 0 {
        index.saturating_add(i64::try_from(len).unwrap_or(i64::MAX))
    } else {
        index
    }
}

/// An adjusted index, if it names an item of a vector of `len`.
fn in_range(index: i64, len: usize) -> Option<usize> {
    usize::try_from(index).ok().filter(|index| *index < len)
}

impl PyAnyVector {
    /// A view of the vector at `place`.
    fn at(place: Place, anchor: Option<Py<PyAny>>) -> Self {
        Self {
            place,
            destroyed: AtomicBool::new(false),
            anchor,
        }
    }

    /// Runs `f` on the vector.
    fn items<T>(&self, f: impl FnOnce(&[Any]) -> PyResult<T>) -> PyResult<T> {
        if self.destroyed.load(Ordering::Relaxed) {
            return Err(vector_destroyed());
        }
        self.place.read(|found| match found {
            Some(Found::Vector(items)) => f(items),
            _ => Err(vector_destroyed()),
        })
    }

    /// Runs `f` on the vector, for writing.
    fn items_mut<T>(&self, f: impl FnOnce(&mut Vec<Any>) -> PyResult<T>) -> PyResult<T> {
        if self.destroyed.load(Ordering::Relaxed) {
            return Err(vector_destroyed());
        }
        self.place.write(|found| match found {
            Some(FoundMut::Vector(items)) => f(items),
            _ => Err(vector_destroyed()),
        })
    }

    /// Converts a value for storing here, refusing first if the vector has
    /// gone.
    fn incoming(&self, item: &Bound<'_, PyAny>) -> PyResult<(Shared, Any)> {
        if self.destroyed.load(Ordering::Relaxed) {
            return Err(vector_destroyed());
        }
        let home = self.place.home()?;
        let value = python_to_any(&home, item)?;
        Ok((home, value))
    }

    /// Reads the item at an index already known to be in range.
    fn item(&self, py: Python<'_>, index: usize) -> PyResult<Option<Py<PyAny>>> {
        let Some(value) = self.items(|items| Ok(items.get(index).cloned()))? else {
            return Ok(None);
        };
        self.place
            .value_to_python(py, self.anchor.as_ref(), Step::Index(index), &value)
            .map(Some)
    }
}

#[pymethods]
impl PyAnyVector {
    /// Builds an empty vector that belongs to nothing.
    #[new]
    fn new(py: Python<'_>) -> PyResult<Self> {
        let mut metadata = AnyDictionary::new();
        metadata.insert(VECTOR_KEY.to_string(), Any::Vector(Vec::new()));
        let (handle, keeper) = free_standing(py, metadata)?;
        let place = Place {
            handle,
            bag: Bag::Metadata,
            path: vec![Step::Key(VECTOR_KEY.to_string())],
        };
        Ok(Self::at(place, Some(keeper)))
    }

    /// Reads one item. Slicing and the rest of the list interface are built
    /// on this in `_core_utils.py`, as upstream's are.
    fn __internal_getitem__(&self, py: Python<'_>, index: i64) -> PyResult<Py<PyAny>> {
        let len = self.items(|items| Ok(items.len()))?;
        in_range(adjusted(index, len), len)
            .map_or(Ok(None), |index| self.item(py, index))?
            .ok_or_else(|| PyIndexError::new_err("list index out of range"))
    }

    fn __internal_setitem__(&self, index: i64, item: &Bound<'_, PyAny>) -> PyResult<()> {
        let (home, value) = self.incoming(item)?;
        let held = value.clone();
        self.items_mut(|items| {
            let at = in_range(adjusted(index, items.len()), items.len())
                .ok_or_else(|| PyIndexError::new_err("list assignment index out of range"))?;
            items[at] = value;
            Ok(())
        })?;
        home.mark_value_owned(item.py(), &held)
    }

    /// Deletes one item, as upstream does: an index past either end deletes
    /// the last item rather than raising, because upstream compares the
    /// adjusted index as an unsigned number. Only an empty vector raises.
    fn __internal_delitem__(&self, index: i64) -> PyResult<()> {
        self.items_mut(|items| {
            if items.is_empty() {
                return Err(PyIndexError::new_err("list index out of range"));
            }
            match in_range(adjusted(index, items.len()), items.len()) {
                Some(at) => {
                    items.remove(at);
                }
                None => {
                    items.pop();
                }
            }
            Ok(())
        })
    }

    /// Inserts an item before `index`, as upstream does: an index past
    /// either end appends, for the reason [`Self::__internal_delitem__`]
    /// gives.
    #[pyo3(name = "__internal_insert")]
    fn internal_insert(&self, index: i64, item: &Bound<'_, PyAny>) -> PyResult<()> {
        let (home, value) = self.incoming(item)?;
        let held = value.clone();
        self.items_mut(|items| {
            match in_range(adjusted(index, items.len()), items.len()) {
                Some(at) => items.insert(at, value),
                None => items.push(value),
            }
            Ok(())
        })?;
        home.mark_value_owned(item.py(), &held)
    }

    fn __len__(&self) -> PyResult<usize> {
        self.items(|items| Ok(items.len()))
    }

    fn __iter__(slf: &Bound<'_, Self>) -> PyResult<PyAnyVectorIterator> {
        slf.borrow().items(|_| Ok(()))?;
        Ok(PyAnyVectorIterator {
            container: slf.clone().unbind(),
            at: 0,
        })
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

/// Iterates over an `AnyVector`'s items.
///
/// Upstream's does not check for changes made meanwhile, and neither does
/// this: it reads whatever is at the next index, and stops at the end.
#[pyclass(name = "AnyVectorIterator", module = "opentimelineio._otio")]
pub struct PyAnyVectorIterator {
    container: Py<PyAnyVector>,
    at: usize,
}

#[pymethods]
impl PyAnyVectorIterator {
    fn __iter__(slf: PyRef<'_, Self>) -> PyRef<'_, Self> {
        slf
    }

    fn __next__(&mut self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        let item = self.container.borrow(py).item(py, self.at)?;
        self.at += 1;
        item.ok_or_else(|| PyStopIteration::new_err(()))
    }
}

/// Renders one of an object's dictionaries the way Python's `repr()` of it
/// would.
pub fn bag_repr(py: Python<'_>, handle: &Handle, bag: Bag) -> PyResult<String> {
    PyAnyDictionary::of(None, handle.clone(), bag).__repr__(py)
}

/// Returns whether `value` is one of these containers, and the document it
/// lives in if so.
pub fn home_of(value: &Bound<'_, PyAny>) -> Option<Shared> {
    if let Ok(dictionary) = value.cast::<PyAnyDictionary>() {
        return dictionary.borrow().place.home().ok();
    }
    if let Ok(vector) = value.cast::<PyAnyVector>() {
        return vector.borrow().place.home().ok();
    }
    None
}

/// Upstream's `_testing.test_AnyDictionary_destroy`: deletes the dictionary
/// from under its proxy.
///
/// There is no C++ dictionary to delete here; the view is marked as if its
/// dictionary had gone, which is all the test can see.
#[pyfunction]
#[pyo3(name = "test_AnyDictionary_destroy")]
fn test_any_dictionary_destroy(d: PyRef<'_, PyAnyDictionary>) {
    d.destroyed.store(true, Ordering::Relaxed);
}

/// Upstream's `_testing.test_AnyVector_destroy`; see
/// [`test_any_dictionary_destroy`].
#[pyfunction]
#[pyo3(name = "test_AnyVector_destroy")]
fn test_any_vector_destroy(v: PyRef<'_, PyAnyVector>) {
    v.destroyed.store(true, Ordering::Relaxed);
}

/// Registers the containers on a module, and their test hooks on its
/// `_testing` submodule.
pub fn register(module: &Bound<'_, PyModule>) -> PyResult<()> {
    module.add_class::<PyAnyDictionary>()?;
    module.add_class::<PyAnyDictionaryIterator>()?;
    module.add_class::<PyAnyVector>()?;
    module.add_class::<PyAnyVectorIterator>()?;
    // The name these bindings used before the class became upstream's
    // `AnyDictionary`, kept for code written against them.
    module.add("AnyDictionaryProxy", module.getattr("AnyDictionary")?)?;

    let testing = crate::testing::submodule(module)?;
    testing.add_function(wrap_pyfunction!(test_any_dictionary_destroy, &testing)?)?;
    testing.add_function(wrap_pyfunction!(test_any_vector_destroy, &testing)?)?;
    Ok(())
}
