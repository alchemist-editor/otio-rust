//! Reading an XML document into an [`Element`] tree.

use std::fmt;

use crate::{Attributes, Element};

/// Why a document could not be read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseError {
    /// What went wrong.
    pub message: String,
    /// The byte offset in the input where it went wrong.
    pub offset: usize,
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} at byte {}", self.message, self.offset)
    }
}

impl std::error::Error for ParseError {}

/// Reads an XML document and returns its root element.
///
/// Character data is decoded: the five predefined entities, numeric character
/// references and `CDATA` sections all become ordinary text. Comments,
/// processing instructions and the document type declaration are skipped.
///
/// # Errors
///
/// Returns a [`ParseError`] if the document is not well formed, or if it has
/// no root element.
pub fn parse(input: &str) -> Result<Element, ParseError> {
    let mut parser = Parser {
        input: input.as_bytes(),
        position: 0,
    };
    parser.parse_document()
}

struct Parser<'a> {
    input: &'a [u8],
    position: usize,
}

impl Parser<'_> {
    fn error<T>(&self, message: impl Into<String>) -> Result<T, ParseError> {
        Err(ParseError {
            message: message.into(),
            offset: self.position,
        })
    }

    fn peek(&self) -> Option<u8> {
        self.input.get(self.position).copied()
    }

    fn starts_with(&self, prefix: &str) -> bool {
        self.input[self.position..].starts_with(prefix.as_bytes())
    }

    fn skip_whitespace(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\t' | b'\r' | b'\n')) {
            self.position += 1;
        }
    }

    /// Skips past the next occurrence of `terminator`, failing if there is
    /// none.
    fn skip_until_after(&mut self, terminator: &str) -> Result<(), ParseError> {
        match find(&self.input[self.position..], terminator.as_bytes()) {
            Some(offset) => {
                self.position += offset + terminator.len();
                Ok(())
            }
            None => self.error(format!("unterminated `{terminator}`")),
        }
    }

    /// Skips comments, processing instructions and doctype declarations, plus
    /// the whitespace around them. Returns whether anything was skipped.
    fn skip_misc(&mut self) -> Result<bool, ParseError> {
        let start = self.position;
        loop {
            self.skip_whitespace();
            if self.starts_with("<!--") {
                self.position += 4;
                self.skip_until_after("-->")?;
            } else if self.starts_with("<?") {
                self.position += 2;
                self.skip_until_after("?>")?;
            } else if self.starts_with("<!DOCTYPE") {
                self.skip_doctype()?;
            } else {
                break;
            }
        }
        Ok(self.position != start)
    }

    /// Skips a doctype declaration, including an internal subset in `[]`.
    fn skip_doctype(&mut self) -> Result<(), ParseError> {
        self.position += "<!DOCTYPE".len();
        let mut depth = 0usize;
        while let Some(byte) = self.peek() {
            self.position += 1;
            match byte {
                b'[' => depth += 1,
                b']' => depth = depth.saturating_sub(1),
                b'>' if depth == 0 => return Ok(()),
                _ => {}
            }
        }
        self.error("unterminated `<!DOCTYPE`")
    }

    fn parse_document(&mut self) -> Result<Element, ParseError> {
        self.skip_misc()?;
        if self.peek() != Some(b'<') {
            return self.error("expected a root element");
        }
        let root = self.parse_element()?;
        self.skip_misc()?;
        if self.position != self.input.len() {
            return self.error("trailing content after the root element");
        }
        Ok(root)
    }

    /// Parses one element, with the cursor on its opening `<`.
    fn parse_element(&mut self) -> Result<Element, ParseError> {
        self.position += 1;
        let tag = self.parse_name()?;
        let attributes = self.parse_attributes()?;

        if self.starts_with("/>") {
            self.position += 2;
            return Ok(Element {
                tag,
                attributes,
                text: None,
                children: Vec::new(),
            });
        }
        if self.peek() != Some(b'>') {
            return self.error(format!("malformed opening tag for `{tag}`"));
        }
        self.position += 1;

        let (text, children) = self.parse_content(&tag)?;
        Ok(Element {
            tag,
            attributes,
            text,
            children,
        })
    }

    /// Parses an element's content up to and including its closing tag.
    ///
    /// The text returned is the character data before the first child, which
    /// is what `ElementTree` reports as an element's `text`. Character data
    /// after a child — its `tail` — is discarded, since no format this crate
    /// serves puts meaning there.
    fn parse_content(&mut self, tag: &str) -> Result<(Option<String>, Vec<Element>), ParseError> {
        let mut text = String::new();
        let mut children: Vec<Element> = Vec::new();

        loop {
            match self.peek() {
                None => return self.error(format!("unclosed element `{tag}`")),
                Some(b'<') => {
                    if self.starts_with("</") {
                        self.position += 2;
                        let closing = self.parse_name()?;
                        if closing != tag {
                            return self.error(format!("`{tag}` closed by `{closing}`"));
                        }
                        self.skip_whitespace();
                        if self.peek() != Some(b'>') {
                            return self.error(format!("malformed closing tag for `{tag}`"));
                        }
                        self.position += 1;
                        let text = if children.is_empty() && !text.is_empty() {
                            Some(text)
                        } else {
                            None
                        };
                        return Ok((text, children));
                    }
                    if self.starts_with("<!--") {
                        self.position += 4;
                        self.skip_until_after("-->")?;
                    } else if self.starts_with("<![CDATA[") {
                        self.position += "<![CDATA[".len();
                        let start = self.position;
                        self.skip_until_after("]]>")?;
                        let raw = &self.input[start..self.position - "]]>".len()];
                        if children.is_empty() {
                            text.push_str(&decode_utf8(raw, self.position)?);
                        }
                    } else if self.starts_with("<?") {
                        self.position += 2;
                        self.skip_until_after("?>")?;
                    } else {
                        children.push(self.parse_element()?);
                    }
                }
                Some(_) => {
                    let start = self.position;
                    while !matches!(self.peek(), None | Some(b'<')) {
                        self.position += 1;
                    }
                    let raw = &self.input[start..self.position];
                    if children.is_empty() {
                        let chunk = decode_utf8(raw, start)?;
                        text.push_str(&unescape(&chunk));
                    }
                }
            }
        }
    }

    fn parse_name(&mut self) -> Result<String, ParseError> {
        let start = self.position;
        while let Some(byte) = self.peek() {
            if byte.is_ascii_whitespace() || matches!(byte, b'/' | b'>' | b'=') {
                break;
            }
            self.position += 1;
        }
        if start == self.position {
            return self.error("expected a tag name");
        }
        decode_utf8(&self.input[start..self.position], start)
    }

    fn parse_attributes(&mut self) -> Result<Attributes, ParseError> {
        let mut attributes = Attributes::new();
        loop {
            self.skip_whitespace();
            match self.peek() {
                None => return self.error("unterminated opening tag"),
                Some(b'>' | b'/') => return Ok(attributes),
                Some(_) => {}
            }

            let name = self.parse_name()?;
            self.skip_whitespace();
            if self.peek() != Some(b'=') {
                return self.error(format!("attribute `{name}` has no value"));
            }
            self.position += 1;
            self.skip_whitespace();

            let quote = match self.peek() {
                Some(quote @ (b'"' | b'\'')) => quote,
                _ => return self.error(format!("attribute `{name}` is not quoted")),
            };
            self.position += 1;
            let start = self.position;
            while self.peek() != Some(quote) {
                if self.peek().is_none() {
                    return self.error(format!("unterminated value for attribute `{name}`"));
                }
                self.position += 1;
            }
            let raw = decode_utf8(&self.input[start..self.position], start)?;
            self.position += 1;

            attributes.set(name, unescape(&raw));
        }
    }
}

/// Decodes a slice of the input as UTF-8, reporting the offset on failure.
fn decode_utf8(raw: &[u8], offset: usize) -> Result<String, ParseError> {
    std::str::from_utf8(raw)
        .map(str::to_owned)
        .map_err(|_| ParseError {
            message: "input is not valid UTF-8".to_string(),
            offset,
        })
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || haystack.len() < needle.len() {
        return None;
    }
    (0..=haystack.len() - needle.len())
        .find(|&start| &haystack[start..start + needle.len()] == needle)
}

/// Replaces entity and character references with the text they stand for.
///
/// An unrecognized reference is left alone rather than rejected, which is what
/// the editorial applications writing these files expect: a stray `&` in a
/// clip name should not make the whole timeline unreadable.
fn unescape(value: &str) -> String {
    if !value.contains('&') {
        return value.to_string();
    }

    let mut out = String::with_capacity(value.len());
    let mut rest = value;
    while let Some(start) = rest.find('&') {
        out.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        let Some(end) = after.find(';') else {
            out.push('&');
            rest = after;
            continue;
        };
        let reference = &after[..end];
        match resolve_reference(reference) {
            Some(resolved) => out.push_str(&resolved),
            None => {
                out.push('&');
                out.push_str(reference);
                out.push(';');
            }
        }
        rest = &after[end + 1..];
    }
    out.push_str(rest);
    out
}

fn resolve_reference(reference: &str) -> Option<String> {
    match reference {
        "amp" => return Some("&".to_string()),
        "lt" => return Some("<".to_string()),
        "gt" => return Some(">".to_string()),
        "quot" => return Some("\"".to_string()),
        "apos" => return Some("'".to_string()),
        _ => {}
    }

    let digits = reference.strip_prefix('#')?;
    let code = if let Some(hex) = digits.strip_prefix(['x', 'X']) {
        u32::from_str_radix(hex, 16).ok()?
    } else {
        digits.parse::<u32>().ok()?
    };
    char::from_u32(code).map(|c| c.to_string())
}

#[cfg(test)]
mod tests {
    use super::parse;

    #[test]
    fn reads_tags_attributes_and_text() {
        let root = parse(
            r#"<?xml version="1.0"?>
            <clipitem id="clipitem-1" frameBlend="FALSE">
                <name>shot_010</name>
                <rate><timebase>30</timebase><ntsc>TRUE</ntsc></rate>
            </clipitem>"#,
        )
        .expect("well-formed document");

        assert_eq!(root.tag, "clipitem");
        assert_eq!(root.attributes.get("id"), Some("clipitem-1"));
        assert_eq!(
            root.attributes.iter().map(|(k, _)| k).collect::<Vec<_>>(),
            vec!["id", "frameBlend"]
        );
        assert_eq!(
            root.find("name").map(|e| e.text_or_empty()),
            Some("shot_010")
        );
        assert_eq!(
            root.find_path(&["rate", "ntsc"]).map(|e| e.text_or_empty()),
            Some("TRUE")
        );
    }

    #[test]
    fn an_element_with_no_characters_has_no_text() {
        // `ElementTree` reports both of these as `None`, and the adapters
        // depend on that to round-trip an empty metadata value.
        let root = parse("<top><empty/><also></also></top>").expect("well-formed document");
        assert!(root.find("empty").expect("empty is present").text.is_none());
        assert!(root.find("also").expect("also is present").text.is_none());
    }

    #[test]
    fn decodes_references_and_cdata() {
        let root =
            parse(r#"<a><b>Tom &amp; Jerry &#38; co &#x3c;3</b><c><![CDATA[a < b]]></c></a>"#)
                .expect("well-formed document");
        assert_eq!(
            root.find("b").map(|e| e.text_or_empty()),
            Some("Tom & Jerry & co <3")
        );
        assert_eq!(root.find("c").map(|e| e.text_or_empty()), Some("a < b"));
    }

    #[test]
    fn leaves_an_unrecognized_reference_alone() {
        // Editorial applications do emit bare ampersands in clip names, and
        // refusing the file over one would lose the whole timeline.
        let root = parse("<a>R&amp;D &widget; 100&amp;</a>").expect("well-formed document");
        assert_eq!(root.text_or_empty(), "R&D &widget; 100&");
    }

    #[test]
    fn skips_comments_declarations_and_doctypes() {
        let root = parse(
            r#"<?xml version="1.0" encoding="UTF-8"?>
            <!DOCTYPE xmeml>
            <!-- a comment -->
            <xmeml version="4"><project/></xmeml>
            <!-- trailing -->"#,
        )
        .expect("well-formed document");
        assert_eq!(root.tag, "xmeml");
        assert_eq!(root.children.len(), 1);
    }

    #[test]
    fn rejects_a_mismatched_closing_tag() {
        let error = parse("<a><b></a></b>").expect_err("tags are mismatched");
        assert!(error.message.contains("closed by"), "{}", error.message);
    }

    #[test]
    fn rejects_an_unclosed_element() {
        assert!(parse("<a><b></b>").is_err());
    }
}
