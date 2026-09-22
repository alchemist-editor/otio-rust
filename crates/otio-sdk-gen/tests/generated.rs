//! The drift check.
//!
//! Everything under `sdk/` is generated from `crates/otio-capi`, and every
//! one of those files is committed, so that a change to the API surface shows
//! up in review. This asks whether they are still what the C ABI says they
//! should be.
//!
//! It fails for the reason the whole pipeline exists: someone changed the C
//! ABI — added a function, moved an argument, reworded a doc comment — and
//! the SDKs built on it have not been told. Running `cargo run -p
//! otio-sdk-gen` and committing what it writes is the fix.

#[test]
fn the_generated_sdks_are_what_the_c_abi_says_they_should_be() {
    let workspace = otio_sdk_gen::workspace_root();
    if let Err(message) = otio_sdk_gen::run(&workspace, true, &[]) {
        panic!("{message}");
    }
}

#[test]
fn every_target_writes_something() {
    let api = otio_sdk_model::describe(&otio_sdk_gen::workspace_root())
        .expect("the C ABI can be described");
    for (name, generate) in otio_sdk_gen::TARGETS {
        let files = generate(&api).unwrap_or_else(|error| panic!("the {name} SDK: {error}"));
        assert!(!files.is_empty(), "the {name} SDK wrote no files");
        for file in files {
            assert!(
                !file.contents.trim().is_empty(),
                "the {name} SDK wrote {} empty",
                file.path.display()
            );
        }
    }
}
