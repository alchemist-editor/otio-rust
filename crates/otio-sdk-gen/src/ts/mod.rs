//! The TypeScript SDK, for the browser and for Node.
//!
//! The package wraps `otio-capi` compiled to `wasm32-unknown-unknown`, which
//! means this backend marshals by hand in a way the others do not: there is
//! no C compiler on the far side, so every struct is written into the
//! module's linear memory field by field, at offsets this backend has to
//! know. Those offsets come from the description, which places every field
//! for both pointer widths — see [`layout`].
//!
//! What lives here and not in `otio-sdk-model` is the shape of the
//! TypeScript surface: which class a call hangs off, that
//! `track.childAt(0)` is a `Composable` rather than a bare node, and that the
//! edit operations gather under `edit` rather than becoming `editInsert` and
//! friends at the top level. Those are answers about TypeScript, and other
//! backends answer them differently — Go hands back a bare node and expects
//! you to ask it what it is. The description says what the library *does*;
//! this says what that looks like in TypeScript.

mod emit;
pub mod layout;
pub mod plan;

use std::path::PathBuf;

use otio_sdk_model::Api;

use crate::emit::File;

/// Where the TypeScript package lives, relative to the workspace root.
const DIR: &str = "crates/otio-wasm";

/// Writes the TypeScript SDK.
///
/// # Errors
///
/// Fails if anything in the interface fits none of the conventions this
/// backend knows how to spell.
pub fn generate(api: &Api) -> Result<Vec<File>, String> {
    let sdk = plan::plan(api)?;
    let artifacts = vec![
        emit::assertions(api)?,
        emit::exports(api),
        emit::types(api)?,
        emit::raw(api, &sdk)?,
        emit::values(api, &sdk)?,
        emit::api(api, &sdk)?,
    ];
    Ok(artifacts
        .into_iter()
        .map(|artifact| File {
            path: PathBuf::from(DIR).join(artifact.path),
            contents: artifact.text,
        })
        .collect())
}
