//! What a backend hands back.

use std::path::PathBuf;

/// One generated file.
pub struct File {
    /// Where it goes, relative to the workspace root.
    pub path: PathBuf,
    /// What it says.
    pub contents: String,
}
