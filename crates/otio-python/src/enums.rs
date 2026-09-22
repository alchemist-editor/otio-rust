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
//! [`pybind11_enum!`] gives an enum those methods, in the one `#[pymethods]`
//! block PyO3 allows it, along with whatever else that enum defines.
//!
//! Two places differ from pybind11, both where pybind11 is unsound:
//!
//! - A number that names no value is refused with `ValueError`. pybind11
//!   keeps it, printing as `<MissingFramePolicy.???: 7>`, and hands the C++
//!   library an enum holding a value it does not have; a Rust enum cannot
//!   hold one at all.
//! - `cls()` with no number gives the first value, as `cls.__new__(cls)`
//!   must for unpickling. pybind11 separates `__new__`, which makes an
//!   uninitialised value, from `__init__`, which demands the number; PyO3
//!   has only the one constructor, so `cls()` is accepted where pybind11
//!   raises `TypeError`.
//!
//! Pickle protocols 0 and 1 are refused with `TypeError` ("cannot pickle"),
//! where pybind11 aborts the whole interpreter trying to allocate the base
//! object; protocols 2 and up give the bytes pybind11 gives, so a pickle
//! written by either loads in the other.

/// Adds pybind11's constructor, pickling and hash to a fieldless enum, plus
/// the items given in braces, as that enum's `#[pymethods]` block.
///
/// ```ignore
/// pybind11_enum!(PyReferencePoint "ReferencePoint" [Source, Sequence, Fit] {});
/// ```
macro_rules! pybind11_enum {
    ($ty:ident $name:literal [$($variant:ident),+ $(,)?] { $($extra:tt)* }) => {
        #[::pyo3::pymethods]
        impl $ty {
            /// Makes the value numbered `value`, as a pybind11 enum does.
            /// With no number it is the first value, which is what
            /// unpickling starts from before `__setstate__`.
            #[new]
            #[pyo3(signature = (value = None))]
            fn __new__(value: Option<i64>) -> ::pyo3::PyResult<Self> {
                const VALUES: &[$ty] = &[$($ty::$variant),+];
                match value {
                    None => Ok(VALUES[0]),
                    Some(value) => Self::from_number(value),
                }
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
        }
    };
}

pub(crate) use pybind11_enum;
