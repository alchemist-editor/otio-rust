//! `opentimelineio._otio.TestObject`: the schema upstream's bindings add for
//! their own regression tests.
//!
//! Upstream's extension registers a `Test` schema (version 1) deriving from
//! `SerializableObjectWithMetadata`, bound as `_otio.TestObject`
//! (`otio_tests.cpp`). It is part of the type registry every upstream build
//! has, so it appears in `type_version_map()` and in the checked-in
//! `CORE_VERSION_MAP.cpp` that `autogen_version_map` regenerates, and the
//! serialized-schema documentation generator skips it by name. This is the
//! same class and schema, held as a dynamic object like any other schema
//! registered from Python.
//!
//! Two deliberate differences: the name has a default, because the registry
//! here builds an instance with no arguments when it reads one from a file
//! (upstream's C++ factory does not go through the Python constructor); and
//! it does not print on creation and destruction as upstream's does.

use otio_core::AnyDictionary;
use otio_core::schema::{Base, DynamicObject, Node};

use pyo3::prelude::*;
use pyo3::{Py, PyAny};

use crate::objects::{Handle, PySerializableObject, PySerializableObjectWithMetadata};

/// The schema name upstream gives its test object.
const SCHEMA_NAME: &str = "Test";

/// The schema version upstream gives its test object.
const SCHEMA_VERSION: u32 = 1;

/// Upstream's regression-test object: a named object with metadata.
#[pyclass(
    name = "TestObject",
    module = "opentimelineio._otio",
    extends = PySerializableObjectWithMetadata,
    subclass
)]
pub struct PyTestObject;

#[pymethods]
impl PyTestObject {
    #[new]
    #[pyo3(signature = (name = String::new()))]
    fn new(name: String) -> PyClassInitializer<Self> {
        let handle = Handle::alone(Node::Dynamic(DynamicObject {
            schema_name: SCHEMA_NAME.to_string(),
            schema_version: SCHEMA_VERSION,
            base: Some(Base {
                name,
                metadata: AnyDictionary::new(),
                extension: None,
            }),
            fields: AnyDictionary::new(),
        }));
        PyClassInitializer::from(PySerializableObject::from(handle))
            .add_subclass(PySerializableObjectWithMetadata)
            .add_subclass(Self)
    }

    /// The object stored in metadata under `key`, or `None` if what is
    /// stored there is not an object.
    fn lookup(slf: &Bound<'_, Self>, key: &str) -> PyResult<Option<Py<PyAny>>> {
        let value = slf.getattr("metadata")?.call_method1("get", (key,))?;
        Ok(value
            .is_instance_of::<PySerializableObject>()
            .then(|| value.unbind()))
    }

    fn __repr__(slf: &Bound<'_, Self>) -> PyResult<String> {
        let name: String = slf.getattr("name")?.extract()?;
        Ok(format!(
            "<TestObject named '{name}' at id {:#x}>",
            slf.as_ptr() as usize
        ))
    }
}

/// Adds `TestObject` to the extension module and registers its schema.
///
/// This runs after the registry's functions are on the module, and goes
/// through `register_serializable_object_type` so the class is remembered
/// for reading, as a class registered from Python is.
pub fn register(module: &Bound<'_, PyModule>) -> PyResult<()> {
    module.add_class::<PyTestObject>()?;
    let class = module.getattr("TestObject")?;
    module
        .getattr("register_serializable_object_type")?
        .call1((class, SCHEMA_NAME, SCHEMA_VERSION))?;
    Ok(())
}
