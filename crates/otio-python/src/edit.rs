//! The edit operations: upstream's C++ `otio::algo` editing algorithms.
//!
//! Upstream's C++ library has ten of these (`editAlgorithm.h`) and its
//! Python package binds none of them. They are bound here, on
//! `otio-core::edit`, and `opentimelineio.algorithms` exports them under
//! upstream's C++ names, with its parameter names in snake case and its
//! defaults, as a binding of upstream's would have them:
//!
//! ```text
//! overwrite(item, composition, range, remove_transitions=True, fill_template=None)
//! insert(item, composition, time, remove_transitions=True, fill_template=None)
//! trim(item, delta_in, delta_out, fill_template=None)
//! slice(composition, time, remove_transitions=True)
//! slip(item, delta)
//! slide(item, delta)
//! ripple(item, delta_in, delta_out)
//! roll(item, delta_in, delta_out)
//! fill(item, track, track_time, reference_point=ReferencePoint.Source)
//! remove(composition, time, fill=True, fill_template=None)
//! ```
//!
//! A failure raises what upstream's `ErrorStatusHandler` raises for the
//! outcome upstream's algorithm reports: `NotAChildError` for "not a child
//! of", and `ValueError` with upstream's wording for the rest.
//!
//! Every object an edit touches is first brought into one document, as
//! appending does, and objects an edit takes out stay alive while Python
//! holds them, as upstream's do; see [`crate::arena::Shared::edit`].

use opentime::{RationalTime, TimeRange};
use otio_core::edit::{self, ReferencePoint};
use otio_core::{Document, Error, Node, NodeId};

use pyo3::exceptions::{PyTypeError, PyValueError};
use pyo3::prelude::*;

use crate::arena::Shared;
use crate::errors::NotAChildError;
use crate::objects::{core_error, handle_of};
use crate::opentime::{PyRationalTime, PyTimeRange};

/// Which clock a three- or four-point edit lines its media up against, as
/// upstream's `otio::algo::ReferencePoint`.
#[pyclass(
    name = "ReferencePoint",
    module = "opentimelineio.algorithms",
    eq,
    eq_int,
    from_py_object
)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum PyReferencePoint {
    /// Use the media's own timing, and take as much of it as fits.
    Source,
    /// Line the media up against the track, trimming it to the gap.
    Sequence,
    /// Stretch or squeeze the media to fill the gap exactly.
    Fit,
}

crate::enums::pybind11_enum!(PyReferencePoint "ReferencePoint" [Source, Sequence, Fit] {});

impl From<PyReferencePoint> for ReferencePoint {
    fn from(point: PyReferencePoint) -> Self {
        match point {
            PyReferencePoint::Source => Self::Source,
            PyReferencePoint::Sequence => Self::Sequence,
            PyReferencePoint::Fit => Self::Fit,
        }
    }
}

/// The exception upstream's `ErrorStatusHandler` raises for what upstream's
/// algorithm reports in place of `error`.
///
/// Upstream sets these outcomes with no details, so the message is the
/// outcome's own description, word for word.
fn edit_error(error: Error) -> PyErr {
    match error {
        Error::NotAnItem => PyValueError::new_err("object is not descendent of Item type"),
        Error::NotAGap => PyValueError::new_err("object is not descendent of Gap type"),
        Error::CannotTrimTransition => PyValueError::new_err("cannot trim transition"),
        Error::NotAChild { .. } | Error::NotAChildOf { .. } => {
            NotAChildError::new_err("item is not a child of specified object")
        }
        other => match core_error::<()>(Err(other)) {
            Err(error) => error,
            Ok(()) => unreachable!("an error converts to an error"),
        },
    }
}

/// What an argument must be, for [`object`] to check.
#[derive(Clone, Copy)]
enum Kind {
    Item,
    Composition,
}

/// Reads an object argument, refusing one of the wrong kind with the
/// `TypeError` pybind11 raises for an argument it cannot convert.
fn object(
    function: &str,
    parameter: &str,
    kind: Kind,
    value: &Bound<'_, PyAny>,
) -> PyResult<crate::objects::Handle> {
    let wrong = || {
        let expected = match kind {
            Kind::Item => "Item",
            Kind::Composition => "Composition",
        };
        PyTypeError::new_err(format!(
            "{function}(): incompatible function arguments: {parameter} must be \
             an opentimelineio._otio.{expected}"
        ))
    };
    let handle = handle_of(value).map_err(|_| wrong())?;
    let fits = handle.with(|node| {
        Ok(match kind {
            Kind::Item => node.item().is_some(),
            Kind::Composition => {
                matches!(node, Node::Track(_) | Node::Stack(_) | Node::Composition(_))
            }
        })
    })?;
    if fits { Ok(handle) } else { Err(wrong()) }
}

/// The objects one edit works on, all brought into one document.
struct Gathered<'py> {
    home: Shared,
    /// Each object's id in `home`, in the order given.
    ids: Vec<NodeId>,
    /// The Python objects, in the same order, for recording ownership.
    wrappers: Vec<Bound<'py, PyAny>>,
}

/// Brings `objects` into the first one's document, as appending one object
/// to another does.
fn gather<'py>(
    objects: Vec<(crate::objects::Handle, Bound<'py, PyAny>)>,
) -> PyResult<Gathered<'py>> {
    let home = objects
        .first()
        .map(|(handle, _)| handle.live())
        .transpose()?
        .map(|(shared, _)| shared)
        .unwrap_or_default();
    let mut ids = Vec::with_capacity(objects.len());
    let mut wrappers = Vec::with_capacity(objects.len());
    for (handle, wrapper) in objects {
        home.absorb(&handle.shared)?;
        ids.push(handle.live()?.1);
        wrappers.push(wrapper);
    }
    Ok(Gathered {
        home,
        ids,
        wrappers,
    })
}

impl Gathered<'_> {
    /// Runs the edit, then records as owned every object given that the
    /// edit put somewhere: the incoming item, the fill template.
    fn run(
        &self,
        f: impl FnOnce(&mut Document, &[NodeId]) -> otio_core::Result<()>,
    ) -> PyResult<()> {
        let py = match self.wrappers.first() {
            Some(wrapper) => wrapper.py(),
            None => return Ok(()),
        };
        let ids = &self.ids;
        self.home
            .edit(py, |document| f(document, ids))?
            .map_err(edit_error)?;
        for (id, wrapper) in self.ids.iter().zip(&self.wrappers) {
            let owned = self
                .home
                .read(|document| Ok(document.owner_of(*id).is_some()))?;
            if owned {
                self.home.mark_owned(py, *id, Some(wrapper))?;
            }
        }
        Ok(())
    }
}

/// Reads an optional fill template argument.
fn template<'py>(
    function: &str,
    value: Option<&Bound<'py, PyAny>>,
) -> PyResult<Option<(crate::objects::Handle, Bound<'py, PyAny>)>> {
    value
        .filter(|value| !value.is_none())
        .map(|value| {
            Ok((
                object(function, "fill_template", Kind::Item, value)?,
                value.clone(),
            ))
        })
        .transpose()
}

/// Overwrites whatever is under `range` in `composition` with `item`.
#[pyfunction]
#[pyo3(signature = (item, composition, range, remove_transitions = true, fill_template = None))]
fn overwrite(
    item: &Bound<'_, PyAny>,
    composition: &Bound<'_, PyAny>,
    range: PyTimeRange,
    remove_transitions: bool,
    fill_template: Option<&Bound<'_, PyAny>>,
) -> PyResult<()> {
    let mut objects = vec![
        (
            object("overwrite", "composition", Kind::Composition, composition)?,
            composition.clone(),
        ),
        (object("overwrite", "item", Kind::Item, item)?, item.clone()),
    ];
    objects.extend(template("overwrite", fill_template)?);
    let range: TimeRange = range.0;
    gather(objects)?.run(|document, ids| {
        edit::overwrite(
            document,
            ids[1],
            ids[0],
            range,
            remove_transitions,
            ids.get(2).copied(),
        )
    })
}

/// Inserts `item` into `composition` at `time`, pushing what follows along.
#[pyfunction]
#[pyo3(signature = (item, composition, time, remove_transitions = true, fill_template = None))]
fn insert(
    item: &Bound<'_, PyAny>,
    composition: &Bound<'_, PyAny>,
    time: PyRationalTime,
    remove_transitions: bool,
    fill_template: Option<&Bound<'_, PyAny>>,
) -> PyResult<()> {
    let mut objects = vec![
        (
            object("insert", "composition", Kind::Composition, composition)?,
            composition.clone(),
        ),
        (object("insert", "item", Kind::Item, item)?, item.clone()),
    ];
    objects.extend(template("insert", fill_template)?);
    let time: RationalTime = time.0;
    gather(objects)?.run(|document, ids| {
        edit::insert(
            document,
            ids[1],
            ids[0],
            time,
            remove_transitions,
            ids.get(2).copied(),
        )
    })
}

/// Moves `item`'s start by `delta_in` and its end by `delta_out`, without
/// moving anything else.
#[pyfunction]
#[pyo3(signature = (item, delta_in, delta_out, fill_template = None))]
fn trim(
    item: &Bound<'_, PyAny>,
    delta_in: PyRationalTime,
    delta_out: PyRationalTime,
    fill_template: Option<&Bound<'_, PyAny>>,
) -> PyResult<()> {
    let mut objects = vec![(object("trim", "item", Kind::Item, item)?, item.clone())];
    objects.extend(template("trim", fill_template)?);
    gather(objects)?.run(|document, ids| {
        edit::trim(
            document,
            ids[0],
            delta_in.0,
            delta_out.0,
            ids.get(1).copied(),
        )
    })
}

/// Cuts whatever is at `time` in `composition` in two.
#[pyfunction]
#[pyo3(signature = (composition, time, remove_transitions = true))]
fn slice(
    composition: &Bound<'_, PyAny>,
    time: PyRationalTime,
    remove_transitions: bool,
) -> PyResult<()> {
    let objects = vec![(
        object("slice", "composition", Kind::Composition, composition)?,
        composition.clone(),
    )];
    gather(objects)?.run(|document, ids| edit::slice(document, ids[0], time.0, remove_transitions))
}

/// Moves which part of its media `item` shows by `delta`.
#[pyfunction]
fn slip(item: &Bound<'_, PyAny>, delta: PyRationalTime) -> PyResult<()> {
    let objects = vec![(object("slip", "item", Kind::Item, item)?, item.clone())];
    gather(objects)?.run(|document, ids| edit::slip(document, ids[0], delta.0))
}

/// Moves `item` along its track by `delta`, stretching the item before it.
#[pyfunction]
fn slide(item: &Bound<'_, PyAny>, delta: PyRationalTime) -> PyResult<()> {
    let objects = vec![(object("slide", "item", Kind::Item, item)?, item.clone())];
    gather(objects)?.run(|document, ids| edit::slide(document, ids[0], delta.0))
}

/// Moves `item`'s start and end, moving everything after it to suit.
#[pyfunction]
fn ripple(
    item: &Bound<'_, PyAny>,
    delta_in: PyRationalTime,
    delta_out: PyRationalTime,
) -> PyResult<()> {
    let objects = vec![(object("ripple", "item", Kind::Item, item)?, item.clone())];
    gather(objects)?.run(|document, ids| edit::ripple(document, ids[0], delta_in.0, delta_out.0))
}

/// Moves the cuts either side of `item`, trading time with its neighbours.
#[pyfunction]
fn roll(
    item: &Bound<'_, PyAny>,
    delta_in: PyRationalTime,
    delta_out: PyRationalTime,
) -> PyResult<()> {
    let objects = vec![(object("roll", "item", Kind::Item, item)?, item.clone())];
    gather(objects)?.run(|document, ids| edit::roll(document, ids[0], delta_in.0, delta_out.0))
}

/// Drops `item` into the gap at `track_time` on `track`.
#[pyfunction]
#[pyo3(
    signature = (item, track, track_time, reference_point = PyReferencePoint::Source),
    text_signature = "(item, track, track_time, reference_point=ReferencePoint.Source)"
)]
fn fill(
    item: &Bound<'_, PyAny>,
    track: &Bound<'_, PyAny>,
    track_time: PyRationalTime,
    reference_point: PyReferencePoint,
) -> PyResult<()> {
    let objects = vec![
        (
            object("fill", "track", Kind::Composition, track)?,
            track.clone(),
        ),
        (object("fill", "item", Kind::Item, item)?, item.clone()),
    ];
    gather(objects)?.run(|document, ids| {
        edit::fill(
            document,
            ids[1],
            ids[0],
            track_time.0,
            reference_point.into(),
        )
    })
}

/// Takes out whatever is at `time` in `composition`, leaving a gap, or
/// `fill_template`, in its place when `fill` is set.
#[pyfunction]
#[pyo3(signature = (composition, time, fill = true, fill_template = None))]
fn remove(
    composition: &Bound<'_, PyAny>,
    time: PyRationalTime,
    fill: bool,
    fill_template: Option<&Bound<'_, PyAny>>,
) -> PyResult<()> {
    let mut objects = vec![(
        object("remove", "composition", Kind::Composition, composition)?,
        composition.clone(),
    )];
    objects.extend(template("remove", fill_template)?);
    gather(objects)?
        .run(|document, ids| edit::remove(document, ids[0], time.0, fill, ids.get(1).copied()))
}

/// Registers the edit operations on an `algo` submodule of `module`, named
/// after upstream's C++ namespace; `opentimelineio.algorithms` exports them.
pub fn register(module: &Bound<'_, PyModule>) -> PyResult<()> {
    let algo = PyModule::new(module.py(), "algo")?;
    algo.add_class::<PyReferencePoint>()?;
    algo.add_function(wrap_pyfunction!(overwrite, &algo)?)?;
    algo.add_function(wrap_pyfunction!(insert, &algo)?)?;
    algo.add_function(wrap_pyfunction!(trim, &algo)?)?;
    algo.add_function(wrap_pyfunction!(slice, &algo)?)?;
    algo.add_function(wrap_pyfunction!(slip, &algo)?)?;
    algo.add_function(wrap_pyfunction!(slide, &algo)?)?;
    algo.add_function(wrap_pyfunction!(ripple, &algo)?)?;
    algo.add_function(wrap_pyfunction!(roll, &algo)?)?;
    algo.add_function(wrap_pyfunction!(fill, &algo)?)?;
    algo.add_function(wrap_pyfunction!(remove, &algo)?)?;
    module.add_submodule(&algo)?;
    Ok(())
}
