//! Where each field of a `#[repr(C)]` struct sits, on `wasm32`.
//!
//! The JavaScript side reads and writes these structs through a `DataView`
//! over the module's linear memory, so it needs byte offsets rather than field
//! names. Computing them here rather than hard-coding them in TypeScript means
//! a field added to the C ABI moves the ones after it automatically.
//!
//! The numbers are `wasm32`'s, which differ from a 64-bit host's wherever a
//! pointer or a `usize` is involved: `OtioBuffer` is sixteen bytes on the host
//! and eight in the module. Getting that wrong would be a silent misread, so
//! the generator also emits [`super::emit`]'s Rust assertions, which make the
//! `wasm32` build fail if any number here is wrong.

use super::abi::{Abi, Type};

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
/// Fails for a named type the ABI does not declare.
pub fn size_of(kind: &Type, abi: &Abi) -> Result<Size, String> {
    let scalar = |size: usize| Size {
        size,
        alignment: size,
    };
    Ok(match kind {
        Type::Void => Size {
            size: 0,
            alignment: 1,
        },
        Type::Bool | Type::U8 | Type::Char => scalar(1),
        Type::I32 | Type::U32 => scalar(4),
        Type::F64 | Type::I64 | Type::U64 => scalar(8),
        // A `wasm32` pointer is a 32-bit offset into the linear memory, and
        // `usize` is the same width.
        Type::Usize | Type::Pointer { .. } => scalar(4),
        Type::Named(name) => {
            if abi.enumeration(name).is_some() {
                // A `#[repr(C)]` enum is a C `int`.
                return Ok(scalar(4));
            }
            let record = abi
                .record(name)
                .ok_or_else(|| format!("`{name}` is neither a struct nor an enum of the ABI"))?;
            if record.fields.is_empty() {
                return Err(format!(
                    "`{name}` is opaque, so it has no layout the SDK can read"
                ));
            }
            layout_of(name, abi)?.size
        }
    })
}

/// Places every field of a struct.
///
/// # Errors
///
/// Fails for a struct the ABI does not declare, or one holding a type whose
/// size cannot be computed.
pub fn layout_of(name: &str, abi: &Abi) -> Result<Layout, String> {
    let record = abi
        .record(name)
        .ok_or_else(|| format!("`{name}` is not a struct of the ABI"))?;

    let mut offset = 0usize;
    let mut alignment = 1usize;
    let mut fields = Vec::with_capacity(record.fields.len());
    for field in &record.fields {
        let size = size_of(&field.kind, abi)?;
        offset = offset.next_multiple_of(size.alignment);
        alignment = alignment.max(size.alignment);
        fields.push(Placement {
            name: field.name.clone(),
            offset,
            kind: field.kind.clone(),
        });
        offset += size.size;
    }

    Ok(Layout {
        name: name.to_string(),
        fields,
        // C pads a struct out to its own alignment, so an array of them keeps
        // every element aligned.
        size: Size {
            size: offset.next_multiple_of(alignment),
            alignment,
        },
    })
}
