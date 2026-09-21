//! A small XML tree, parser and pretty printer.
//!
//! This exists to serve the OpenTimelineIO XML adapters — Final Cut Pro 7 and
//! Final Cut Pro X — and is deliberately not a general-purpose XML library.
//! The workspace carries no third-party dependencies, so it is written here
//! rather than pulled in.
//!
//! The tree is Python's `xml.etree.ElementTree` model, because the adapters
//! being ported are written against it: an element has a tag, ordered
//! attributes, an optional block of character data, and child elements. The
//! character data upstream calls `tail` — text after a child's closing tag —
//! is not kept, because nothing in the formats being read puts meaning there
//! and the pretty printer never writes any.
//!
//! [`to_pretty_string`] reproduces the output of Python's
//! `xml.dom.minidom.toprettyxml(indent='    ')`, which is what the upstream
//! adapters emit, down to the `<?xml version="1.0" ?>` declaration and the
//! way a lone text child stays on its parent's line.
//!
//! ```
//! let root = otio_xml::parse("<filter><effect><name>Time Remap</name></effect></filter>")?;
//! assert_eq!(root.tag, "filter");
//!
//! let name = root.find("effect").and_then(|e| e.find("name")).unwrap();
//! assert_eq!(name.text_or_empty(), "Time Remap");
//! # Ok::<(), otio_xml::ParseError>(())
//! ```

mod parse;
mod write;

pub use parse::{ParseError, parse};
pub use write::to_pretty_string;

/// An element's attributes, in the order they were written.
///
/// Order is part of the round trip: a file read and written back out should
/// not shuffle its attributes, and the adapters append attributes such as
/// `id` after the ones they read.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Attributes {
    entries: Vec<(String, String)>,
}

impl Attributes {
    /// Creates an empty attribute list.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            entries: Vec::new(),
        }
    }

    /// Returns the value for a name, if it is set.
    #[must_use]
    pub fn get(&self, name: &str) -> Option<&str> {
        self.entries
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.as_str())
    }

    /// Returns whether a name is set.
    #[must_use]
    pub fn contains(&self, name: &str) -> bool {
        self.get(name).is_some()
    }

    /// Sets a name, replacing it in place if it is already present and
    /// appending it otherwise.
    pub fn set(&mut self, name: impl Into<String>, value: impl Into<String>) {
        let name = name.into();
        let value = value.into();
        if let Some(entry) = self.entries.iter_mut().find(|(key, _)| *key == name) {
            entry.1 = value;
        } else {
            self.entries.push((name, value));
        }
    }

    /// Iterates over the attributes in order.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &str)> {
        self.entries
            .iter()
            .map(|(key, value)| (key.as_str(), value.as_str()))
    }

    /// Returns how many attributes are set.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Returns whether no attributes are set.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

impl<'a> IntoIterator for &'a Attributes {
    type Item = (&'a str, &'a str);
    type IntoIter = Box<dyn Iterator<Item = (&'a str, &'a str)> + 'a>;

    fn into_iter(self) -> Self::IntoIter {
        Box::new(self.iter())
    }
}

/// An XML element.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Element {
    /// The tag name.
    pub tag: String,
    /// The attributes, in document order.
    pub attributes: Attributes,
    /// The character data directly inside this element, before any child.
    ///
    /// `None` where there is none, which is how `ElementTree` reports both
    /// `<tag/>` and `<tag></tag>`. The adapters lean on that: a metadata value
    /// of `None` is what makes an empty element survive a round trip.
    pub text: Option<String>,
    /// The child elements, in document order.
    pub children: Vec<Element>,
}

impl Element {
    /// Creates an element with a tag and nothing else.
    #[must_use]
    pub fn new(tag: impl Into<String>) -> Self {
        Self {
            tag: tag.into(),
            ..Self::default()
        }
    }

    /// Creates an element holding a single block of text.
    #[must_use]
    pub fn with_text(tag: impl Into<String>, text: impl Into<String>) -> Self {
        Self {
            tag: tag.into(),
            text: Some(text.into()),
            ..Self::default()
        }
    }

    /// Returns this element's text, or the empty string if it has none.
    #[must_use]
    pub fn text_or_empty(&self) -> &str {
        self.text.as_deref().unwrap_or("")
    }

    /// Returns the first direct child with the given tag.
    #[must_use]
    pub fn find(&self, tag: &str) -> Option<&Element> {
        self.children.iter().find(|child| child.tag == tag)
    }

    /// Returns the first direct child with the given tag, mutably.
    pub fn find_mut(&mut self, tag: &str) -> Option<&mut Element> {
        self.children.iter_mut().find(|child| child.tag == tag)
    }

    /// Iterates over the direct children with the given tag.
    pub fn find_all<'a>(&'a self, tag: &'a str) -> impl Iterator<Item = &'a Element> + 'a {
        self.children.iter().filter(move |child| child.tag == tag)
    }

    /// Follows a chain of tags down from this element, one direct child at a
    /// time.
    ///
    /// This is the subset of `ElementTree`'s path syntax the adapters use:
    /// `element.find_path(&["rate", "timebase"])` is upstream's
    /// `element.find("./rate/timebase")`.
    #[must_use]
    pub fn find_path(&self, tags: &[&str]) -> Option<&Element> {
        let mut current = self;
        for tag in tags {
            current = current.find(tag)?;
        }
        Some(current)
    }

    /// Returns the first direct child with the given tag whose own child
    /// `child_tag` has the given text.
    ///
    /// Upstream writes this as `./effect[effecttype='generator']`.
    #[must_use]
    pub fn find_with_child_text(
        &self,
        tag: &str,
        child_tag: &str,
        child_text: &str,
    ) -> Option<&Element> {
        self.children.iter().find(|child| {
            child.tag == tag
                && child
                    .find(child_tag)
                    .is_some_and(|found| found.text_or_empty() == child_text)
        })
    }

    /// Iterates over every descendant, in document order, not including this
    /// element.
    pub fn descendants(&self) -> impl Iterator<Item = &Element> {
        Descendants {
            stack: self.children.iter().rev().collect(),
        }
    }

    /// Gets the first direct child with the given tag, adding an empty one if
    /// there is none.
    pub fn get_or_create_child(&mut self, tag: &str) -> &mut Element {
        if let Some(index) = self.children.iter().position(|child| child.tag == tag) {
            return &mut self.children[index];
        }
        self.children.push(Element::new(tag));
        self.children.last_mut().expect("a child was just pushed")
    }

    /// Appends a child element.
    pub fn push(&mut self, child: Element) {
        self.children.push(child);
    }

    /// Appends a child element carrying a block of text.
    pub fn push_text(&mut self, tag: impl Into<String>, text: impl Into<String>) {
        self.children.push(Element::with_text(tag, text));
    }

    /// Returns whether this element has no children and no text.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.children.is_empty() && self.text.is_none()
    }
}

/// A depth-first walk over an element's descendants.
struct Descendants<'a> {
    stack: Vec<&'a Element>,
}

impl<'a> Iterator for Descendants<'a> {
    type Item = &'a Element;

    fn next(&mut self) -> Option<Self::Item> {
        let element = self.stack.pop()?;
        self.stack.extend(element.children.iter().rev());
        Some(element)
    }
}

#[cfg(test)]
mod tests {
    use super::{Attributes, Element};

    #[test]
    fn attributes_keep_insertion_order_and_replace_in_place() {
        let mut attributes = Attributes::new();
        attributes.set("b", "1");
        attributes.set("a", "2");
        attributes.set("b", "3");

        assert_eq!(
            attributes.iter().collect::<Vec<_>>(),
            vec![("b", "3"), ("a", "2")]
        );
    }

    #[test]
    fn descendants_walk_in_document_order() {
        let mut root = Element::new("root");
        let mut first = Element::new("first");
        first.push(Element::new("nested"));
        root.push(first);
        root.push(Element::new("second"));

        let tags: Vec<_> = root.descendants().map(|e| e.tag.as_str()).collect();
        assert_eq!(tags, vec!["first", "nested", "second"]);
    }

    #[test]
    fn find_path_walks_direct_children_only() {
        let mut root = Element::new("root");
        let mut rate = Element::new("rate");
        rate.push_text("timebase", "30");
        root.push(rate);

        assert_eq!(
            root.find_path(&["rate", "timebase"])
                .map(Element::text_or_empty),
            Some("30")
        );
        assert!(root.find_path(&["timebase"]).is_none());
    }
}
