//! Building the shared adapter error with messages that name the element at
//! fault.
//!
//! FCP 7 XML files run to tens of thousands of lines and are usually machine
//! written, so "no `start` element" is not much help on its own. Every message
//! here carries the tag and `id` of the element being read when it went wrong,
//! which is what a person actually greps the file for.

use otio_adapter::Error;
use otio_xml::Element;

use crate::util::identify;

/// An element this adapter needs was not there.
pub fn missing(tag: &str, parent: &Element) -> Error {
    Error::parse(format!("no `{tag}` element in {}", identify(parent)))
}

/// An element's text was not a number.
pub fn not_a_number(tag: &str, text: &str) -> Error {
    Error::parse(format!(
        "`{tag}` element holds `{text}`, which is not a number"
    ))
}

/// No rate applies to an element, in it or in anything above it.
///
/// Every timed element in FCP XML either carries a `rate` or inherits one, so
/// this means the file is incomplete rather than merely unusual.
pub fn no_rate(element: &Element) -> Error {
    Error::parse(format!("no rate applies to {}", identify(element)))
}

/// A clip item names neither a file, a nested sequence nor a generator, so
/// there is nothing to point it at.
pub fn unsupported_clip_item(element: &Element) -> Error {
    Error::parse(format!(
        "{} has no file, sequence or generator to refer to",
        identify(element)
    ))
}

/// An element was pushed onto the inheritance stack twice, which would mean
/// the document nests inside itself.
pub fn circular_inheritance(element: &Element) -> Error {
    Error::parse(format!("`{}` element contains itself", element.tag))
}

/// The file holds no top-level sequence, so there is no timeline in it.
pub fn no_sequences() -> Error {
    Error::parse("no top-level sequences found")
}
