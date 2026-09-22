//! Where each field of a `#[repr(C)]` struct sits, on `wasm32`.
//!
//! The JavaScript side reads and writes these structs through a `DataView`
//! over the module's linear memory, so it needs byte offsets rather than field
//! names.
//!
//! The numbers themselves come from the description rather than being
//! computed here: `otio-sdk-model` already places every field for both pointer
//! widths, because a struct holding a pointer is a different size in a
//! `wasm32` module than on a 64-bit host — `OtioBuffer` is eight bytes there
//! and sixteen here. This module is only the `wasm32` view of that, plus the
//! sizes of the scalars, which no description needs to carry.
//!
//! Getting a number wrong would be a silent misread rather than a failure, so
//! the generator also emits [`super::emit`]'s Rust assertions, which make the
//! `wasm32` build fail if any offset here disagrees with the compiler.

use otio_sdk_model::{Api, Type};

/// How wide a pointer is in the module's memory.
///
/// The one place the target is named. Everything else asks the description
/// for the number at this width.
pub const POINTER: usize = 4;

/// The size and alignment of one type, in the module's memory.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Size {
    /// How many bytes the type occupies, including tail padding.
    pub size: usize,
    /// What address it has to start at.
    pub alignment: usize,
}

/// One field, placed.
#[derive(Debug, Clone)]
pub struct Placement {
    /// The field's name.
    pub name: String,
    /// Its byte offset from the start of the struct.
    pub offset: usize,
    /// Its type.
    pub kind: Type,
}

/// A struct's fields, placed, with the whole struct's size.
#[derive(Debug, Clone)]
pub struct Layout {
    /// The struct's name.
    pub name: String,
    /// Its fields, in declaration order, with offsets.
    pub fields: Vec<Placement>,
    /// The size and alignment of the struct itself.
    pub size: Size,
}

/// Returns the size and alignment of a type in the module's memory.
///
/// # Errors
///
/// Fails for a named type the description does not declare.
pub fn size_of(kind: &Type, api: &Api) -> Result<Size, String> {
    let scalar = |size: usize| Size {
        size,
        alignment: size,
    };
    Ok(match kind {
        Type::Bool => scalar(1),
        // A `#[repr(C)]` enum is a C `int`.
        Type::Int32 | Type::Uint32 | Type::Enum(_) => scalar(4),
        Type::Double | Type::Int64 | Type::Uint64 => scalar(8),
        // A `wasm32` pointer is a 32-bit offset into the linear memory, and
        // `size_t` is the same width.
        Type::Size | Type::Text | Type::Bytes | Type::Document | Type::List(_) => scalar(POINTER),
        Type::Node => named("OtioNode", api)?,
        Type::Struct(name) => named(name, api)?,
    })
}

/// The size of one of the description's structs.
fn named(name: &str, api: &Api) -> Result<Size, String> {
    let structure = api
        .structure(name)
        .ok_or_else(|| format!("`{name}` is not a struct of the interface"))?;
    if structure.fields.is_empty() {
        return Err(format!(
            "`{name}` is opaque, so it has no layout the SDK can read"
        ));
    }
    Ok(Size {
        size: structure.layout.size.at(POINTER),
        alignment: structure.layout.align.at(POINTER),
    })
}

/// Places every field of a struct, at the module's pointer width.
///
/// # Errors
///
/// Fails for a struct the description does not declare.
pub fn layout_of(name: &str, api: &Api) -> Result<Layout, String> {
    let structure = api
        .structure(name)
        .ok_or_else(|| format!("`{name}` is not a struct of the interface"))?;
    Ok(Layout {
        name: name.to_string(),
        fields: structure
            .fields
            .iter()
            .map(|field| Placement {
                name: field.name.clone(),
                offset: field.offset.at(POINTER),
                kind: field.ty.clone(),
            })
            .collect(),
        size: named(name, api)?,
    })
}
