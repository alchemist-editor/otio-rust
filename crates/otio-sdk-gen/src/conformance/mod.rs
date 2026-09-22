//! Rendering the conformance scenarios into each SDK's own tests.
//!
//! The scenarios are data in `otio_sdk_model::conformance`. Each backend has
//! a module here that turns them into a test file in its language, written
//! into the directory that language's CI job already tests, so a rendered
//! scenario is one that runs. A scenario a backend cannot render stops
//! generation rather than being skipped.

pub mod cpp;
pub mod csharp;
pub mod go;
pub mod objc;
pub mod swift;
pub mod ts;
pub mod zig;

use otio_sdk_model::conformance::Scenario;

/// Escapes text for a double-quoted string literal in the C family: Go,
/// Swift, C#, C++, Objective-C, Zig and TypeScript all read these escapes the
/// same way.
#[must_use]
pub fn quoted(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('"');
    for character in text.chars() {
        match character {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            '\r' => out.push_str("\\r"),
            other => out.push(other),
        }
    }
    out.push('"');
    out
}

/// Formats a number the way every target reads a double literal: always with
/// a decimal point, so `24` is not read as an integer.
#[must_use]
pub fn number(value: f64) -> String {
    let text = format!("{value}");
    if text.contains('.') || text.contains('e') {
        text
    } else {
        format!("{text}.0")
    }
}

/// A scenario's documentation, wrapped to fit a comment of `width` columns
/// after `prefix`.
#[must_use]
pub fn comment(scenario: &Scenario, prefix: &str, width: usize) -> Vec<String> {
    let mut lines = Vec::new();
    let mut line = String::new();
    for word in scenario.docs.split_whitespace() {
        if !line.is_empty() && prefix.len() + line.len() + 1 + word.len() > width {
            lines.push(format!("{prefix}{line}"));
            line.clear();
        }
        if !line.is_empty() {
            line.push(' ');
        }
        line.push_str(word);
    }
    if !line.is_empty() {
        lines.push(format!("{prefix}{line}"));
    }
    lines
}
