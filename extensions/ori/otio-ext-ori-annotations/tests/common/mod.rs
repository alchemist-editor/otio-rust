//! What the tests share.

#![allow(dead_code)]

use otio_core::Document;

/// Installs and enables the extension.
pub fn enable() {
    otio_ext_ori_annotations::install();
    otio_ext::enable(otio_ext_ori_annotations::ID).expect("enables");
}

/// A vendored fixture's text.
pub fn fixture(name: &str) -> String {
    std::fs::read_to_string(format!(
        "{}/tests/fixtures/{name}",
        env!("CARGO_MANIFEST_DIR")
    ))
    .expect("fixture is vendored")
}

/// A document as upstream's Python writes it, with four-space indents and
/// no newline at the end.
pub fn write(document: &Document) -> String {
    otio_core::to_string_pretty(document, 4)
        .expect("writes")
        .trim_end()
        .to_string()
}
