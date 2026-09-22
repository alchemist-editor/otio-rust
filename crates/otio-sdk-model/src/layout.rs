//! Where every field of a value struct sits, and how big the struct is.
//!
//! A backend with a C compiler behind it never needs this: it includes the
//! header and lets the compiler place the fields. A backend without one does.
//! The WebAssembly target reaches the library through a linear memory and has
//! to write a `RationalTime` into it byte by byte, which means knowing that
//! `rate` begins at offset 8 and that the whole struct is 16 bytes.
//!
//! The rules are C's, which `#[repr(C)]` is defined to follow: a field starts
//! at the next offset that its own alignment divides, the struct's alignment
//! is the largest of its fields', and its size is rounded up to that. They
//! are computed twice, because a pointer is four bytes on `wasm32` and eight
//! elsewhere, and three of these structs hold one.
//!
//! Computing a layout is guessing until something checks it. `otio-capi`
//! asserts the size of every struct that crosses by value in a `const` block,
//! which the compiler evaluates, so those assertions are the compiler's own
//! answer; [`check`] compares them against what was computed here and fails
//! the build on any disagreement.

use std::collections::BTreeMap;

use crate::model::{ByWidth, Field, Layout, Struct, Type};

/// The pointer widths a generated SDK is built for.
const WIDTHS: [usize; 2] = [4, 8];

/// Works out where each struct's fields sit, and how big it is.
///
/// # Errors
///
/// Fails if a field has a type whose size this cannot work out, which means
/// the C ABI has grown a kind of field no SDK knows how to marshal.
pub fn apply(structs: &mut [Struct]) -> Result<(), String> {
    // A struct may hold another, so the ones a struct is built from have to
    // be measured first. The graph has no cycles, because a struct cannot
    // hold itself by value, so repeating until nothing is left to place
    // terminates.
    let mut known: BTreeMap<String, Layout> = BTreeMap::new();
    while known.len() < structs.len() {
        let before = known.len();
        for item in structs.iter() {
            if known.contains_key(&item.name) {
                continue;
            }
            if let Some(layout) = place(&item.fields, &known) {
                known.insert(item.name.clone(), layout.0);
            }
        }
        if known.len() == before {
            let missing: Vec<&str> = structs
                .iter()
                .filter(|item| !known.contains_key(&item.name))
                .map(|item| item.name.as_str())
                .collect();
            return Err(format!(
                "cannot work out the layout of {}: a field has a type with no \
                 size this knows about",
                missing.join(", ")
            ));
        }
    }

    for item in structs.iter_mut() {
        let (layout, offsets) =
            place(&item.fields, &known).expect("every struct was measured above");
        item.layout = layout;
        for (field, offset) in item.fields.iter_mut().zip(offsets) {
            field.offset = offset;
        }
    }
    Ok(())
}

/// Checks the computed sizes against the ones the C ABI asserts.
///
/// # Errors
///
/// Fails if they disagree, or if the C ABI asserts a size for something this
/// does not know about.
pub fn check(structs: &[Struct], asserted: &BTreeMap<String, usize>) -> Result<(), String> {
    for (name, size) in asserted {
        let Some(item) = structs.iter().find(|item| &item.name == name) else {
            return Err(format!(
                "otio-capi asserts a size for {name}, which is not a struct this description carries"
            ));
        };
        // The assertions are evaluated where the crate is compiled, which is
        // a 64-bit host everywhere this project builds.
        if item.layout.size.pointer64 != *size {
            return Err(format!(
                "{name} is {} bytes by this description's reckoning, but \
                 otio-capi asserts {size}; the layout rules here are wrong",
                item.layout.size.pointer64
            ));
        }
    }
    Ok(())
}

/// Lays out a run of fields, answering the struct's layout and each offset.
fn place(fields: &[Field], known: &BTreeMap<String, Layout>) -> Option<(Layout, Vec<ByWidth>)> {
    let mut offsets = vec![
        ByWidth {
            pointer32: 0,
            pointer64: 0
        };
        fields.len()
    ];
    let mut size = ByWidth {
        pointer32: 0,
        pointer64: 0,
    };
    let mut align = ByWidth {
        pointer32: 1,
        pointer64: 1,
    };

    for width in WIDTHS {
        let mut at = 0;
        let mut widest = 1;
        for (field, offset) in fields.iter().zip(offsets.iter_mut()) {
            let (field_size, field_align) = measure(&field.ty, width, known)?;
            at = round_up(at, field_align);
            match width {
                4 => offset.pointer32 = at,
                _ => offset.pointer64 = at,
            }
            at += field_size;
            widest = widest.max(field_align);
        }
        let total = round_up(at, widest);
        match width {
            4 => {
                size.pointer32 = total;
                align.pointer32 = widest;
            }
            _ => {
                size.pointer64 = total;
                align.pointer64 = widest;
            }
        }
    }

    Some((Layout { size, align }, offsets))
}

/// How big one field is, and what it is aligned to, for a pointer `width`.
fn measure(ty: &Type, width: usize, known: &BTreeMap<String, Layout>) -> Option<(usize, usize)> {
    let pair = match ty {
        Type::Bool => (1, 1),
        Type::Int32 | Type::Uint32 => (4, 4),
        // A `#[repr(C)]` enum with no integer of its own is a C enum, which
        // every target this builds for makes four bytes.
        Type::Enum(_) => (4, 4),
        Type::Double | Type::Int64 | Type::Uint64 => (8, 8),
        // A pointer, and a `usize`, are as wide as the target says.
        Type::Size | Type::Text | Type::Bytes | Type::Document => (width, width),
        Type::Struct(name) => {
            let layout = known.get(name)?;
            (layout.size.at(width), layout.align.at(width))
        }
        // A node is `OtioNode`, a struct in its own right, so it is
        // measured as one rather than assumed to be two `u32`s here.
        Type::Node => {
            let layout = known.get("OtioNode")?;
            (layout.size.at(width), layout.align.at(width))
        }
        // A list never crosses by value; it is a pointer and a count, which
        // are separate parameters rather than one field.
        Type::List(_) => return None,
    };
    Some(pair)
}

/// Rounds `value` up to the next multiple of `align`.
const fn round_up(value: usize, align: usize) -> usize {
    value.div_ceil(align) * align
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Docs;

    fn field(name: &str, ty: Type) -> Field {
        Field {
            name: name.to_string(),
            ty,
            offset: ByWidth {
                pointer32: 0,
                pointer64: 0,
            },
            docs: Docs::default(),
        }
    }

    #[test]
    fn a_pair_of_doubles_is_sixteen_bytes() {
        let fields = vec![field("value", Type::Double), field("rate", Type::Double)];
        let (layout, offsets) = place(&fields, &BTreeMap::new()).expect("it can be laid out");
        assert_eq!(layout.size.pointer64, 16);
        assert_eq!(layout.align.pointer64, 8);
        assert_eq!(offsets[1].pointer64, 8);
        // No pointer, so a 32-bit target sees the same thing.
        assert_eq!(layout.size.pointer32, 16);
        assert_eq!(offsets[1].pointer32, 8);
    }

    #[test]
    fn a_bool_before_a_double_is_padded_out() {
        let fields = vec![field("has", Type::Bool), field("time", Type::Double)];
        let (layout, offsets) = place(&fields, &BTreeMap::new()).expect("it can be laid out");
        assert_eq!(offsets[0].pointer64, 0);
        assert_eq!(
            offsets[1].pointer64, 8,
            "the double waits for its alignment"
        );
        assert_eq!(layout.size.pointer64, 16);
    }

    #[test]
    fn a_pointer_moves_what_follows_it_on_a_narrow_target() {
        let fields = vec![field("rate", Type::Double), field("name", Type::Text)];
        let (layout, offsets) = place(&fields, &BTreeMap::new()).expect("it can be laid out");
        assert_eq!(offsets[1].pointer32, 8);
        assert_eq!(offsets[1].pointer64, 8);
        assert_eq!(layout.size.pointer32, 16, "padded out to the double's 8");
        assert_eq!(layout.size.pointer64, 16);
    }

    #[test]
    fn a_struct_inside_a_struct_is_measured_by_its_own_layout() {
        let mut known = BTreeMap::new();
        let inner = place(
            &[field("x", Type::Double), field("y", Type::Double)],
            &known,
        )
        .expect("it can be laid out");
        known.insert("OtioV2d".to_string(), inner.0);

        let fields = vec![
            field("min", Type::Struct("OtioV2d".to_string())),
            field("max", Type::Struct("OtioV2d".to_string())),
        ];
        let (layout, offsets) = place(&fields, &known).expect("it can be laid out");
        assert_eq!(layout.size.pointer64, 32);
        assert_eq!(offsets[1].pointer64, 16);
    }
}
