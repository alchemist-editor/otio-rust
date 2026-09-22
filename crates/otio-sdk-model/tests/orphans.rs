//! Which adopted objects a binding checks for a parent before moving them.
//!
//! A binding that hides the document moves an adopted object's whole
//! document into the receiver's before the call, and a move cannot be taken
//! back. So where the core refuses an object that already has a parent, the
//! binding asks first and refuses with the core's own words (#75). These
//! tests hold that list and those words to the core.

use std::path::{Path, PathBuf};

use otio_sdk_model::ALREADY_PARENTED;
use otio_sdk_model::model::Placement;

fn workspace() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("the crate sits two directories below the workspace root")
        .to_path_buf()
}

#[test]
fn a_binding_refuses_a_parented_object_in_the_core_s_words() {
    assert_eq!(
        ALREADY_PARENTED,
        otio_core::Error::ChildAlreadyParented.to_string()
    );
}

#[test]
fn only_the_calls_that_make_a_child_check_for_a_parent() {
    let api = otio_sdk_model::describe(&workspace()).expect("the C ABI can be described");
    let mut orphans: Vec<(String, String)> = api
        .functions()
        .flat_map(|function| {
            function
                .params
                .iter()
                .filter(|param| param.placement == Some(Placement::AdoptOrphan))
                .map(|param| (function.symbol.clone(), param.name.clone()))
        })
        .collect();
    orphans.sort();
    let expected = [
        ("otio_composition_append_child", "child"),
        ("otio_composition_insert_child", "child"),
        ("otio_edit_insert", "item"),
        ("otio_edit_overwrite", "item"),
    ];
    assert_eq!(
        orphans,
        expected
            .iter()
            .map(|(symbol, name)| ((*symbol).to_string(), (*name).to_string()))
            .collect::<Vec<_>>(),
        "the list in placement.rs's module documentation says why each of these, and no \
         other adopted object, is checked for a parent"
    );
}

#[test]
fn a_parent_is_asked_about_before_anything_else_moves() {
    // Each binding brings a call's objects over in the order the call takes
    // them (C++, whose argument order is the compiler's, by asking about the
    // checked one in a statement of its own), so an adopted object ahead of
    // the one checked for a parent would already have moved its timeline in
    // by the time the check refused.
    let api = otio_sdk_model::describe(&workspace()).expect("the C ABI can be described");
    for function in api.functions() {
        let placements: Vec<Placement> = function
            .params
            .iter()
            .filter_map(|param| param.placement)
            .collect();
        let Some(checked) = placements
            .iter()
            .rposition(|placement| *placement == Placement::AdoptOrphan)
        else {
            continue;
        };
        assert!(
            !placements[..checked].contains(&Placement::Adopt),
            "`{}` adopts an object before the one it checks for a parent",
            function.symbol
        );
    }
}
