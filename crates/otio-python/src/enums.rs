//! What every enum here shares with a pybind11 `py::enum_`.
//!
//! Upstream binds `MissingFramePolicy`, `NeighborGapPolicy` and
//! `MediaReferencePolicy` with `py::enum_`, and `ReferencePoint`, which only
//! these bindings expose to Python, follows them. A pybind11 enum value can be
//! built from its number (`MediaReferencePolicy(2)`), hashes as that number,
//! and pickles as `__getstate__` giving the number and `__setstate__` taking
//! it back on an instance made by `cls.__new__(cls)`. That last is what
//! `copy.copy` and `copy.deepcopy` go through too, so without it a value
//! cannot be copied, and neither can anything holding one: the adapter layer
//! deep-copies its keyword arguments, so `write_to_file(..., media_policy=...)`
//! failed before it began.
//!
//! A pybind11 enum value also prints as `<NeighborGapPolicy.never: 0>`
//! (`repr`) and `NeighborGapPolicy.never` (`str`), has `name` and `value`,
//! converts with `int()` and, through `__index__`, anywhere Python wants an
//! integer (`hex()`, `float()`, list indices). Its class has `__members__`, a
//! new `dict` of every value by name, in the order upstream binds them, each
//! time it is read, from the class or from a value. And since upstream's
//! enums are C++ enums that convert to their number, pybind11 compares them
//! as `int(self) == other`: equal to their number whatever its type (`1.0`,
//! `True`), and to a value of another enum with the same number, and never
//! to `None`. They have no ordering.
//!
//! [`pybind11_enum!`] gives an enum all of that, in the one `#[pymethods]`
//! block PyO3 allows it, along with whatever else that enum defines. The
//! enum's `#[pyclass]` must not ask for PyO3's own `eq` or `eq_int`, which
//! compare only with the same enum or an `int`.
//!
//! Where it differs from pybind11, it is where pybind11 is unsound, or where
//! PyO3 cannot follow:
//!
//! - A number that names no value is refused with `ValueError`. pybind11
//!   keeps it, printing as `<MissingFramePolicy.???: 7>`, and hands the C++
//!   library an enum holding a value it does not have; a Rust enum cannot
//!   hold one at all.
//! - `cls()` with no number gives the value numbered 0, as `cls.__new__(cls)`
//!   must give some value for unpickling. pybind11 separates `__new__`,
//!   which makes an uninitialised value, from `__init__`, which demands the
//!   number; PyO3 has only the one constructor, so `cls()` is accepted where
//!   pybind11 raises `TypeError`.
//! - pybind11's class also has `__entries`, the dict `__members__` is made
//!   from with each value's docstring beside it, and a docstring that lists
//!   the members. They are its own bookkeeping, and are not reproduced.
//! - pybind11's metaclass refuses to let `__members__` be replaced on the
//!   class. A PyO3 class has the plain `type` metaclass, which lets any class
//!   attribute be replaced, so `cls.__members__ = {}` goes through here.
//!
//! Pickle protocols 0 and 1 are refused with `TypeError` ("cannot pickle"),
//! where pybind11 aborts the whole interpreter trying to allocate the base
//! object; protocols 2 and up give the bytes pybind11 gives, so a pickle
//! written by either loads in the other.

use pyo3::prelude::*;
use pyo3::types::PyDict;

/// `__members__` on an enum's class: a descriptor, so that reading it from
/// the class or from a value makes a new `dict`, as pybind11's static
/// property does, rather than handing out one that a caller could change.
#[pyclass(name = "EnumMembers", module = "opentimelineio._otio", frozen)]
pub(crate) struct Members {
    /// Every value's name, in the order upstream binds them.
    pub(crate) names: &'static [&'static str],
}

#[pymethods]
impl Members {
    /// A new `dict` of the values of `owner`, by name.
    fn __get__<'py>(
        &self,
        instance: &Bound<'py, PyAny>,
        owner: Option<&Bound<'py, PyAny>>,
    ) -> PyResult<Bound<'py, PyDict>> {
        let owner = match owner {
            Some(owner) if !owner.is_none() => owner.clone(),
            _ => instance.get_type().into_any(),
        };
        let members = PyDict::new(instance.py());
        for name in self.names {
            members.set_item(name, owner.getattr(name)?)?;
        }
        Ok(members)
    }
}

/// Adds what a pybind11 `py::enum_` has to a fieldless enum, plus the items
/// given in braces, as that enum's `#[pymethods]` block.
///
/// The values are listed with their Python names, in the order upstream
/// binds them, which is the order of `__members__`.
///
/// ```ignore
/// pybind11_enum!(PyReferencePoint "ReferencePoint" [
///     Source = "Source",
///     Sequence = "Sequence",
///     Fit = "Fit",
/// ] {});
/// ```
macro_rules! pybind11_enum {
    (
        $ty:ident $name:literal [$($variant:ident = $python:literal),+ $(,)?]
        { $($extra:tt)* }
    ) => {
        #[::pyo3::pymethods]
        impl $ty {
            /// Makes the value numbered `value`, as a pybind11 enum does.
            /// With no number it is the value numbered 0, which is what
            /// unpickling starts from before `__setstate__`.
            #[new]
            #[pyo3(signature = (value = None))]
            fn __new__(value: Option<i64>) -> ::pyo3::PyResult<Self> {
                Self::from_number(value.unwrap_or(0))
            }

            /// Every value by name, as a new `dict` each time.
            #[classattr]
            fn __members__() -> $crate::enums::Members {
                $crate::enums::Members {
                    names: &[$($python),+],
                }
            }

            /// The value's name, as a pybind11 enum spells it.
            #[getter]
            fn name(&self) -> &'static str {
                self.python_name()
            }

            /// The value's number, as a pybind11 enum has it.
            #[getter]
            fn value(&self) -> i64 {
                *self as i64
            }

            /// `<Name.value: number>`, as a pybind11 enum prints.
            fn __repr__(&self) -> ::std::string::String {
                format!("<{}.{}: {}>", $name, self.python_name(), *self as i64)
            }

            /// `Name.value`, as a pybind11 enum prints.
            fn __str__(&self) -> ::std::string::String {
                format!("{}.{}", $name, self.python_name())
            }

            /// The value's number.
            fn __int__(&self) -> i64 {
                *self as i64
            }

            /// The value's number, wherever Python wants an integer.
            fn __index__(&self) -> i64 {
                *self as i64
            }

            /// `int(self) == other`, as pybind11 compares an enum that
            /// converts to its number, and never equal to `None`.
            fn __eq__(
                &self,
                other: &::pyo3::Bound<'_, ::pyo3::PyAny>,
            ) -> ::pyo3::PyResult<bool> {
                use ::pyo3::types::PyAnyMethods as _;
                use ::pyo3::IntoPyObject as _;
                if other.is_none() {
                    return Ok(false);
                }
                let number = (*self as i64).into_pyobject(other.py())?;
                ::pyo3::types::PyAnyMethods::eq(number.as_any(), other)
            }

            /// `int(self) != other`, and always unequal to `None`.
            fn __ne__(
                &self,
                other: &::pyo3::Bound<'_, ::pyo3::PyAny>,
            ) -> ::pyo3::PyResult<bool> {
                Ok(!self.__eq__(other)?)
            }

            /// The value's number, which is all a pickle holds.
            fn __getstate__(&self) -> i64 {
                *self as i64
            }

            /// Becomes the value numbered `state`.
            fn __setstate__(&mut self, state: i64) -> ::pyo3::PyResult<()> {
                *self = Self::from_number(state)?;
                Ok(())
            }

            /// Hashes as the value's number, as a pybind11 enum does.
            fn __hash__(&self) -> isize {
                *self as isize
            }

            $($extra)*
        }

        impl $ty {
            /// The value numbered `number`, or `ValueError` if none is.
            fn from_number(number: i64) -> ::pyo3::PyResult<Self> {
                [$($ty::$variant),+]
                    .into_iter()
                    .find(|value| *value as i64 == number)
                    .ok_or_else(|| {
                        ::pyo3::exceptions::PyValueError::new_err(format!(
                            "{number} is not a valid {}",
                            $name
                        ))
                    })
            }

            /// The value's name in Python.
            const fn python_name(self) -> &'static str {
                match self {
                    $($ty::$variant => $python),+
                }
            }
        }
    };
}

pub(crate) use pybind11_enum;
