//! The generator that turns the C ABI into the TypeScript SDK.
//!
//! It runs in four steps, one module each:
//!
//! 1. [`abi`] reads `otio-capi`'s source into a model of the C ABI.
//! 2. [`layout`] works out where each field of each struct sits in the
//!    module's linear memory, on `wasm32`.
//! 3. [`plan`] decides what each entry point becomes in TypeScript: which
//!    class it hangs off, what it is called, which parameter is the result,
//!    and which ones may be absent. Anything it cannot place is an error.
//! 4. [`emit`] writes the TypeScript, and the Rust assertions that hold step 2
//!    honest.
//!
//! The output is checked in, and `otio-ts-gen --check` regenerates it and
//! compares, so a change to the C ABI that the SDK has not accounted for fails
//! CI rather than reaching a user.

pub mod abi;
pub mod emit;
pub mod layout;
pub mod plan;
