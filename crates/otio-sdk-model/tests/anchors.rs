//! Which document a call is made in, for the calls where it is not obvious.
//!
//! A binding that hides the document reads this off the description rather
//! than guessing, and the guess that looks right — the first object the call
//! takes — is wrong for exactly the calls the whole exercise is about. So
//! they are named here: `otio_edit_insert` anchoring on its `item` would put
//! the call in the new clip's own document and then refuse the track.

use std::path::{Path, PathBuf};

use otio_sdk_model::model::{Param, Placement};

fn workspace() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("the crate sits two directories below the workspace root")
        .to_path_buf()
}

fn params(symbol: &str) -> Vec<Param> {
    let api = otio_sdk_model::describe(&workspace()).expect("the C ABI can be described");
    api.functions()
        .find(|function| function.symbol == symbol)
        .unwrap_or_else(|| panic!("the C ABI exports `{symbol}`"))
        .params
        .clone()
}

fn anchor(symbol: &str) -> String {
    let params = params(symbol);
    let anchored: Vec<&Param> = params.iter().filter(|param| param.anchor).collect();
    assert_eq!(
        anchored.len(),
        1,
        "`{symbol}` should have exactly one anchor, and has {}",
        anchored.len()
    );
    anchored[0].name.clone()
}

#[test]
fn a_call_with_a_receiver_is_made_where_the_receiver_is() {
    assert_eq!(anchor("otio_composition_append_child"), "parent");
    assert_eq!(anchor("otio_clip_set_media_reference"), "node_handle");
}

#[test]
fn an_edit_is_made_where_the_object_it_cannot_move_already_is() {
    // The item and the fill template are adopted; the composition is not, so
    // the call happens in the composition's document and the item comes to
    // it. Anchoring on the item instead is the defect this test exists for:
    // `edit.insert(newClip, track, …)` would refuse the track.
    assert_eq!(anchor("otio_edit_insert"), "composition");
    assert_eq!(anchor("otio_edit_overwrite"), "composition");
    assert_eq!(anchor("otio_edit_fill"), "track");
    assert_eq!(anchor("otio_edit_trim"), "item");
    assert_eq!(anchor("otio_edit_remove"), "composition");
}

#[test]
fn an_algorithm_is_made_where_its_subject_is() {
    assert_eq!(anchor("otio_algorithm_flatten_stack"), "stack");
    // A list of tracks is marked optional because the pointer may be null,
    // which is not a reason to look elsewhere for the document.
    assert_eq!(anchor("otio_algorithm_flatten_tracks"), "tracks");
    assert_eq!(anchor("otio_algorithm_track_trimmed_to_range"), "track");
}

#[test]
fn every_object_argument_of_an_anchored_call_is_placed() {
    let api = otio_sdk_model::describe(&workspace()).expect("the C ABI can be described");
    for function in api.functions() {
        for param in &function.params {
            if param.anchor {
                assert!(
                    param.placement.is_some(),
                    "`{}` anchors on `{}`, which is not an object",
                    function.symbol,
                    param.name
                );
            }
        }
    }
}

#[test]
fn the_anchor_is_never_an_object_the_call_would_move() {
    let api = otio_sdk_model::describe(&workspace()).expect("the C ABI can be described");
    for function in api.functions() {
        let Some(anchor) = function.params.iter().find(|param| param.anchor) else {
            continue;
        };
        let requires_something = function
            .params
            .iter()
            .any(|param| param.placement == Some(Placement::Require));
        assert!(
            !requires_something || anchor.placement == Some(Placement::Require),
            "`{}` anchors on `{}`, which it adopts, while it requires another object to be \
             where the call happens",
            function.symbol,
            anchor.name
        );
    }
}
