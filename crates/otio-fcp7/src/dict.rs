//! Translating between an XML subtree and an OTIO metadata dictionary.
//!
//! FCP 7 XML carries far more per-element detail than OTIO has fields for —
//! colour settings, effect parameter curves, Premiere's own bookkeeping — and
//! an adapter that dropped it would lose something on every file it touched.
//! Upstream's answer is to stash the whole subtree under the `fcp_xml`
//! metadata key and write it back out on the way past. These two functions are
//! that translation.

use otio_core::{Any, AnyDictionary};
use otio_xml::Element;

/// The metadata key every FCP 7 XML subtree is stashed under.
pub const META_NAMESPACE: &str = "fcp_xml";

/// Turns the subtree under an element into a metadata dictionary.
///
/// Attributes become keys prefixed with `@`. A child with children of its own
/// becomes a nested dictionary; a child without becomes its text, or
/// [`Any::Null`] where it has none. A tag that appears more than once becomes
/// an array.
///
/// `ignore_tags` names children to leave out: usually the ones the adapter has
/// already turned into real OTIO fields, so that they are not written twice.
/// Under a `timecode` element, `frame` and `string` are dropped as well, since
/// both are recomputed on write and would otherwise go stale as soon as the
/// timeline moved.
///
/// A leaf element's attributes are lost, because a leaf becomes a bare value
/// rather than a dictionary. Upstream has the same hole and says so; no FCP 7
/// XML seen in the wild puts attributes on a leaf that matters.
#[must_use]
pub fn xml_tree_to_dict(node: &Element, ignore_tags: &[&str]) -> AnyDictionary {
    let timing_tags: &[&str] = if node.tag == "timecode" {
        &["frame", "string"]
    } else {
        &[]
    };

    let mut out = AnyDictionary::new();
    for (name, value) in node.attributes.iter() {
        out.insert(format!("@{name}"), Any::String(value.to_string()));
    }

    for child in &node.children {
        if ignore_tags.contains(&child.tag.as_str()) || timing_tags.contains(&child.tag.as_str()) {
            continue;
        }

        let value = if child.children.is_empty() {
            child
                .text
                .as_ref()
                .map_or(Any::Null, |text| Any::String(text.clone()))
        } else {
            Any::Dictionary(xml_tree_to_dict(child, &[]))
        };

        match out.get_mut(&child.tag) {
            None => {
                out.insert(child.tag.clone(), value);
            }
            Some(Any::Vector(existing)) => existing.push(value),
            Some(slot) => {
                let first = std::mem::replace(slot, Any::Null);
                *slot = Any::Vector(vec![first, value]);
            }
        }
    }

    out
}

/// Builds an XML subtree from a metadata dictionary, the inverse of
/// [`xml_tree_to_dict`].
///
/// Keys prefixed with `@` become attributes, except `@id`, which the writer
/// mints itself so that back-references stay consistent.
///
/// `timecode`, `rate` and `link` are dropped, because all three go stale the
/// moment anything in the timeline moves and the writer recomputes them. Under
/// a `samplecharacteristics` element only `timecode` is dropped: the rate
/// there describes the media, not the edit, so it is not the writer's to
/// recompute.
///
/// # Ordering
///
/// Children come out in the order the dictionary iterates, which for OTIO
/// metadata is sorted by key rather than the order the file had them in.
/// Upstream lands in the same place for the same reason: its metadata is
/// backed by a `std::map`, so a dictionary that has been through an OTIO
/// object comes back sorted.
#[must_use]
pub fn dict_to_xml_tree(data: &AnyDictionary, tag: &str) -> Element {
    let ignore_keys: &[&str] = if tag == "samplecharacteristics" {
        &["timecode"]
    } else {
        &["timecode", "rate", "link"]
    };

    let mut element = Element::new(tag);
    for (key, value) in data {
        if key == "@id" {
            continue;
        }
        if let Some(name) = key.strip_prefix('@') {
            element.attributes.set(name, value_to_text(value));
        }
    }

    for (key, value) in data {
        if key.starts_with('@') || ignore_keys.contains(&key.as_str()) {
            continue;
        }
        append_value(&mut element, key, value);
    }

    element
}

/// Appends whatever elements a metadata value calls for under `tag`.
fn append_value(parent: &mut Element, tag: &str, value: &Any) {
    match value {
        Any::Dictionary(nested) => parent.push(dict_to_xml_tree(nested, tag)),
        Any::Vector(items) => {
            for item in items {
                append_value(parent, tag, item);
            }
        }
        Any::Null => parent.push(Element::new(tag)),
        _ => parent.push(Element::with_text(tag, value_to_text(value))),
    }
}

/// Renders a scalar metadata value as element text.
///
/// Everything read out of a file is a string, so this only has work to do for
/// values a caller put in by hand. The formatting follows Python's `str`,
/// which is what upstream writes, so a `true` comes out as `True` and a whole
/// float keeps its `.0`.
fn value_to_text(value: &Any) -> String {
    match value {
        Any::String(text) => text.clone(),
        Any::Bool(true) => "True".to_string(),
        Any::Bool(false) => "False".to_string(),
        Any::Int(number) => number.to_string(),
        Any::UInt(number) => number.to_string(),
        Any::Double(number) => {
            if number.is_finite() && number.fract() == 0.0 {
                format!("{number:.1}")
            } else {
                number.to_string()
            }
        }
        _ => String::new(),
    }
}

/// Reads a string out of a metadata dictionary.
#[must_use]
pub fn dict_str<'a>(data: &'a AnyDictionary, key: &str) -> Option<&'a str> {
    data.get(key).and_then(Any::as_str)
}

/// Reads a nested dictionary out of a metadata dictionary.
#[must_use]
pub fn dict_sub<'a>(data: &'a AnyDictionary, key: &str) -> Option<&'a AnyDictionary> {
    data.get(key).and_then(Any::as_dictionary)
}

/// Reads this adapter's own namespace out of an object's metadata.
#[must_use]
pub fn fcp_metadata(metadata: &AnyDictionary) -> Option<&AnyDictionary> {
    dict_sub(metadata, META_NAMESPACE).filter(|dict| !dict.is_empty())
}

#[cfg(test)]
mod tests {
    use otio_core::{Any, AnyDictionary};
    use otio_xml::{Element, parse};

    use super::{dict_to_xml_tree, xml_tree_to_dict};

    #[test]
    fn repeated_tags_become_an_array() {
        let tree = parse("<p><a>1</a><a>2</a><a>3</a></p>").expect("well-formed");
        let dict = xml_tree_to_dict(&tree, &[]);
        assert_eq!(
            dict.get("a"),
            Some(&Any::Vector(vec![
                Any::String("1".into()),
                Any::String("2".into()),
                Any::String("3".into()),
            ]))
        );
    }

    #[test]
    fn an_empty_element_round_trips_as_null() {
        // Upstream's `test_xml_tree_to_dict` pins this: an empty element has
        // to survive as an empty element, not vanish and not become "".
        let tree = parse("<top><empty/></top>").expect("well-formed");
        let dict = xml_tree_to_dict(&tree, &[]);
        assert_eq!(dict.get("empty"), Some(&Any::Null));

        let rebuilt = dict_to_xml_tree(&dict, "top");
        assert!(
            rebuilt
                .find("empty")
                .expect("empty is present")
                .text
                .is_none()
        );
        assert_eq!(xml_tree_to_dict(&rebuilt, &[]), dict);
    }

    #[test]
    fn attributes_become_prefixed_keys() {
        let tree = parse(r#"<p n="1"><c x="2">t</c></p>"#).expect("well-formed");
        let dict = xml_tree_to_dict(&tree, &[]);
        assert_eq!(dict.get("@n"), Some(&Any::String("1".into())));
        // A leaf becomes its text, so its own attributes are dropped. Upstream
        // has the same hole.
        assert_eq!(dict.get("c"), Some(&Any::String("t".into())));
    }

    #[test]
    fn timing_that_would_go_stale_is_dropped() {
        let tree = parse(
            "<timecode><string>00:00:00:00</string><frame>0</frame>\
             <displayformat>NDF</displayformat></timecode>",
        )
        .expect("well-formed");
        let dict = xml_tree_to_dict(&tree, &[]);
        assert!(!dict.contains_key("string"));
        assert!(!dict.contains_key("frame"));
        assert_eq!(dict.get("displayformat"), Some(&Any::String("NDF".into())));
    }

    #[test]
    fn rate_and_link_are_dropped_on_the_way_out_but_not_for_media() {
        let mut dict = AnyDictionary::new();
        dict.insert("rate".into(), Any::Dictionary(AnyDictionary::new()));
        dict.insert("link".into(), Any::Null);
        dict.insert("width".into(), Any::String("1920".into()));

        assert!(dict_to_xml_tree(&dict, "clipitem").find("rate").is_none());
        assert!(
            dict_to_xml_tree(&dict, "samplecharacteristics")
                .find("rate")
                .is_some()
        );
    }

    #[test]
    fn the_minted_id_is_not_carried_over() {
        // The writer assigns ids itself, so a stale one from the input would
        // collide with the ones it mints.
        let mut dict = AnyDictionary::new();
        dict.insert("@id".into(), Any::String("clipitem-7".into()));
        dict.insert("@frameBlend".into(), Any::String("FALSE".into()));

        let element: Element = dict_to_xml_tree(&dict, "clipitem");
        assert!(!element.attributes.contains("id"));
        assert_eq!(element.attributes.get("frameBlend"), Some("FALSE"));
    }

    /// Upstream's `test_xml_tree_to_dict`, against the filter and the
    /// dictionary it has to read as.
    ///
    /// The pair is vendored from upstream, so this pins the translation
    /// against a file someone else wrote rather than against this port's own
    /// idea of it.
    #[test]
    fn a_premiere_filter_reads_as_upstreams_reference_dictionary() {
        let xml = sample("premiere_example_filter.xml");
        let json = sample("premiere_example_filter.json");

        let dict = xml_tree_to_dict(&parse(&xml).expect("well-formed"), &[]);
        let reference = otio_core::json::parse(&json).expect("well-formed JSON");
        assert_dict_matches(&dict, &reference, "$");

        // And back: the dictionary rebuilds a tree that reads as itself.
        let rebuilt = dict_to_xml_tree(&dict, "filter");
        assert_eq!(xml_tree_to_dict(&rebuilt, &[]), dict);

        // Upstream also compares the pretty-printed tree against the file
        // byte for byte. That cannot hold here, and does not hold upstream
        // either once the dictionary has been through an OTIO object: OTIO
        // metadata is an ordered map, so the children come back sorted by tag
        // rather than in the order the file had them. Re-reading the output
        // is the part that is actually load-bearing.
        let reparsed = parse(&otio_xml::to_pretty_string(&rebuilt)).expect("well-formed");
        assert_eq!(xml_tree_to_dict(&reparsed, &[]), dict);
    }

    fn sample(name: &str) -> String {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/data")
            .join(name);
        std::fs::read_to_string(&path).unwrap_or_else(|error| panic!("reading {name}: {error}"))
    }

    /// Asserts that a metadata dictionary matches a reference JSON object.
    fn assert_dict_matches(dict: &AnyDictionary, reference: &otio_core::json::Value, path: &str) {
        let entries = reference
            .as_object()
            .unwrap_or_else(|| panic!("{path} should be an object"));
        assert_eq!(
            dict.len(),
            entries.len(),
            "{path} has {} keys, the reference has {}",
            dict.len(),
            entries.len()
        );
        for (key, value) in entries {
            let found = dict
                .get(key)
                .unwrap_or_else(|| panic!("{path} is missing `{key}`"));
            assert_value_matches(found, value, &format!("{path}.{key}"));
        }
    }

    fn assert_value_matches(value: &Any, reference: &otio_core::json::Value, path: &str) {
        match reference {
            otio_core::json::Value::Null => assert_eq!(value, &Any::Null, "{path}"),
            otio_core::json::Value::String(text) => {
                assert_eq!(value, &Any::String(text.clone()), "{path}");
            }
            otio_core::json::Value::Object(_) => match value {
                Any::Dictionary(nested) => assert_dict_matches(nested, reference, path),
                other => panic!("{path} should be a dictionary, found {}", other.type_name()),
            },
            otio_core::json::Value::Array(items) => match value {
                Any::Vector(found) => {
                    assert_eq!(found.len(), items.len(), "{path} has the wrong length");
                    for (index, item) in items.iter().enumerate() {
                        assert_value_matches(&found[index], item, &format!("{path}[{index}]"));
                    }
                }
                other => panic!("{path} should be an array, found {}", other.type_name()),
            },
            other => panic!("{path}: the reference holds an unexpected {other:?}"),
        }
    }
}
