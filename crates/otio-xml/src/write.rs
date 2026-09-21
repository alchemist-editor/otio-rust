//! Writing an [`Element`] tree back out as XML.

use crate::Element;

/// The indentation Python's `toprettyxml` is called with by the adapters.
const INDENT: &str = "    ";

/// Writes a tree as indented XML.
///
/// The output matches what Python's `xml.dom.minidom.toprettyxml(indent='
/// ')` produces, because that is what the upstream adapters emit and their
/// tests compare against byte for byte. Three of its habits are worth naming,
/// since none of them is what a fresh implementation would choose:
///
/// - The declaration is `<?xml version="1.0" ?>`, with a space before `?>`,
///   because `minidom` joins an empty list of extra declarations into it.
/// - An element whose only content is text keeps that text on its own line,
///   while an element with children puts each child on a line of its own.
/// - `"` is escaped to `&quot;` in character data as well as in attribute
///   values, which is unnecessary but harmless, and which a byte-for-byte
///   comparison against upstream's output notices.
#[must_use]
pub fn to_pretty_string(root: &Element) -> String {
    let mut out = String::from("<?xml version=\"1.0\" ?>\n");
    write_element(&mut out, root, 0);
    out
}

fn write_element(out: &mut String, element: &Element, depth: usize) {
    for _ in 0..depth {
        out.push_str(INDENT);
    }
    out.push('<');
    out.push_str(&element.tag);
    for (name, value) in element.attributes.iter() {
        out.push(' ');
        out.push_str(name);
        out.push_str("=\"");
        escape_into(out, value);
        out.push('"');
    }

    let text = element.text.as_deref().unwrap_or("");
    if element.children.is_empty() {
        if text.is_empty() {
            out.push_str("/>\n");
        } else {
            out.push('>');
            escape_into(out, text);
            out.push_str("</");
            out.push_str(&element.tag);
            out.push_str(">\n");
        }
        return;
    }

    out.push_str(">\n");
    if !text.is_empty() {
        // An element with both text and children is not something the
        // adapters build, but `minidom` would put the text on its own
        // indented line, so do the same rather than drop it.
        for _ in 0..=depth {
            out.push_str(INDENT);
        }
        escape_into(out, text);
        out.push('\n');
    }
    for child in &element.children {
        write_element(out, child, depth + 1);
    }
    for _ in 0..depth {
        out.push_str(INDENT);
    }
    out.push_str("</");
    out.push_str(&element.tag);
    out.push_str(">\n");
}

/// Escapes text the way `minidom`'s `_write_data` does, quotes included.
fn escape_into(out: &mut String, value: &str) {
    for character in value.chars() {
        match character {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            _ => out.push(character),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::to_pretty_string;
    use crate::{Element, parse};

    #[test]
    fn nests_children_and_keeps_text_on_one_line() {
        let mut root = Element::new("filter");
        let mut effect = Element::new("effect");
        effect.push_text("name", "Time Remap");
        effect.push(Element::new("parameter"));
        root.push(effect);

        assert_eq!(
            to_pretty_string(&root),
            "<?xml version=\"1.0\" ?>\n\
             <filter>\n\
             \x20   <effect>\n\
             \x20       <name>Time Remap</name>\n\
             \x20       <parameter/>\n\
             \x20   </effect>\n\
             </filter>\n"
        );
    }

    #[test]
    fn escapes_markup_and_quotes() {
        let mut root = Element::new("a");
        root.attributes.set("note", "a \"quoted\" & <marked> value");
        root.text = Some("3 < 4 & \"so\"".to_string());

        assert_eq!(
            to_pretty_string(&root),
            "<?xml version=\"1.0\" ?>\n\
             <a note=\"a &quot;quoted&quot; &amp; &lt;marked&gt; value\">\
             3 &lt; 4 &amp; &quot;so&quot;</a>\n"
        );
    }

    #[test]
    fn round_trips_through_the_parser() {
        let source = "<?xml version=\"1.0\" ?>\n\
                      <xmeml version=\"4\">\n\
                      \x20   <project>\n\
                      \x20       <name>cut</name>\n\
                      \x20       <children/>\n\
                      \x20   </project>\n\
                      </xmeml>\n";
        let tree = parse(source).expect("well-formed document");
        assert_eq!(to_pretty_string(&tree), source);
    }
}
