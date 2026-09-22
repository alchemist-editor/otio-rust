//! Spelling one name several ways.
//!
//! The C ABI names everything in `snake_case` with an `otio_` on the front.
//! Go wants `PascalCase` with its initialisms in capitals, Swift wants
//! `camelCase`, Zig wants `snake_case` back again. Rather than each backend
//! growing its own half-right word splitter, they share this one.

/// The words a `snake_case` name is made of.
#[must_use]
pub fn words(name: &str) -> Vec<&str> {
    name.split('_').filter(|word| !word.is_empty()).collect()
}

/// The words a `PascalCase` name is made of.
///
/// A capital starts a word, and so does the first digit after a letter, so
/// `Cmx3600` is `Cmx` and `3600` and the `Cmx` can be recognised for the
/// initialism it is.
#[must_use]
pub fn split_pascal(name: &str) -> Vec<String> {
    let mut parts: Vec<String> = Vec::new();
    let mut current = String::new();
    let mut previous: Option<char> = None;
    for character in name.chars() {
        let starts = character.is_uppercase()
            || (character.is_ascii_digit() && previous.is_some_and(char::is_alphabetic));
        if starts && !current.is_empty() {
            parts.push(std::mem::take(&mut current));
        }
        current.push(character);
        previous = Some(character);
    }
    if !current.is_empty() {
        parts.push(current);
    }
    parts
}

/// Spells a `PascalCase` name again, capitalising initialisms in full.
///
/// `Cmx3600` becomes `CMX3600` and `InvalidUtf8` becomes `InvalidUTF8`,
/// which is how a Go programmer would have written them.
#[must_use]
pub fn respell(name: &str, initialisms: &[&str]) -> String {
    split_pascal(name)
        .into_iter()
        .map(|word| {
            let lower = word.to_lowercase();
            if initialisms.contains(&lower.as_str()) {
                lower.to_uppercase()
            } else {
                word
            }
        })
        .collect()
}

/// The words a language spells in capitals however they fall.
///
/// Go's own convention is that an initialism keeps its case: `URL`, not
/// `Url`. Swift and Zig differ, so a backend that wants another set passes
/// its own to [`pascal_with`].
pub const INITIALISMS: &[&str] = &[
    "abi", "ale", "api", "cmx", "edl", "fps", "id", "json", "ok", "otio", "smpte", "url", "utf",
    "utf8", "uuid", "xml",
];

/// Spells a `snake_case` name in `PascalCase`, with the usual initialisms.
#[must_use]
pub fn pascal(name: &str) -> String {
    pascal_with(name, INITIALISMS)
}

/// Spells a `snake_case` name in `PascalCase`, capitalising the given words
/// in full.
#[must_use]
pub fn pascal_with(name: &str, initialisms: &[&str]) -> String {
    words(name)
        .into_iter()
        .map(|word| capitalize(word, initialisms))
        .collect()
}

/// Spells a `snake_case` name in `camelCase`, with the usual initialisms.
///
/// The first word stays lowercase even when it is an initialism, which is
/// what Swift and TypeScript both do: `urlOfClip`, not `URLOfClip`.
#[must_use]
pub fn camel(name: &str) -> String {
    camel_with(name, INITIALISMS)
}

/// Spells a `snake_case` name in `camelCase` with a given set of initialisms.
#[must_use]
pub fn camel_with(name: &str, initialisms: &[&str]) -> String {
    let mut spelled = String::new();
    for (index, word) in words(name).into_iter().enumerate() {
        if index == 0 {
            spelled.push_str(&word.to_lowercase());
        } else {
            spelled.push_str(&capitalize(word, initialisms));
        }
    }
    spelled
}

/// Lowers the first letter of a name, leaving the rest alone.
///
/// `TimeRange` becomes `timeRange`, which is how most languages spell a name
/// they mean to keep to themselves.
#[must_use]
pub fn uncapitalize(name: &str) -> String {
    let mut characters = name.chars();
    match characters.next() {
        Some(first) => first.to_lowercase().collect::<String>() + characters.as_str(),
        None => String::new(),
    }
}

/// Capitalises one word, in full if it is an initialism.
fn capitalize(word: &str, initialisms: &[&str]) -> String {
    let lower = word.to_lowercase();
    if initialisms.contains(&lower.as_str()) {
        return lower.to_uppercase();
    }
    let mut characters = word.chars();
    match characters.next() {
        Some(first) => first.to_uppercase().collect::<String>() + characters.as_str(),
        None => String::new(),
    }
}

/// Turns a C enum constant into the part that names the variant.
///
/// `OTIO_FORMAT_CMX_3600` with the prefix `OTIO_FORMAT_` becomes `CMX3600`:
/// the underscores go and the capitals stay, which is how a language with
/// Go's initialism rule would have spelled it by hand.
#[must_use]
pub fn constant_suffix(constant: &str, prefix: &str) -> String {
    constant
        .strip_prefix(prefix)
        .unwrap_or(constant)
        .replace('_', "")
}

/// The prefix every constant of one enum shares, such as `OTIO_STATUS_`.
///
/// Taken from the constants themselves rather than from the type's name,
/// because the C ABI does not always spell one from the other:
/// `OtioValueKind`'s constants are `OTIO_VALUE_...`, not
/// `OTIO_VALUE_KIND_...`.
#[must_use]
pub fn shared_prefix(constants: &[String]) -> String {
    let Some(first) = constants.first() else {
        return String::new();
    };
    let mut shared = first.as_str();
    for constant in &constants[1..] {
        let common = first
            .char_indices()
            .take_while(|(index, character)| constant[*index..].starts_with(*character))
            .count();
        shared = &shared[..shared.len().min(common)];
    }
    // Stop at the last underscore, so a shared word is never cut in half.
    match shared.rfind('_') {
        Some(at) => shared[..=at].to_string(),
        None => String::new(),
    }
}

/// Every interface symbol a piece of documentation names.
///
/// A doc comment carried into Go that still says `otio_node_name` sends the
/// reader to a function their language does not have, so a backend rewrites
/// these. Collecting them here means a backend rewrites the ones that exist
/// and leaves alone anything that merely looks like one.
#[must_use]
pub fn references(summary: &str, body: &[String]) -> Vec<String> {
    let mut found: Vec<String> = Vec::new();
    let mut consider = |text: &str| {
        let bytes: Vec<char> = text.chars().collect();
        let mut index = 0;
        while index < bytes.len() {
            let rest: String = bytes[index..].iter().collect();
            if rest.starts_with("otio_") || rest.starts_with("OTIO_") {
                let symbol: String = rest
                    .chars()
                    .take_while(|character| character.is_alphanumeric() || *character == '_')
                    .collect();
                if symbol.len() > 5 && !found.contains(&symbol) {
                    found.push(symbol.clone());
                }
                index += symbol.chars().count();
            } else {
                index += 1;
            }
        }
    };
    consider(summary);
    for paragraph in body {
        consider(paragraph);
    }
    found
}

#[cfg(test)]
mod tests {
    use super::{camel, constant_suffix, pascal, references, shared_prefix};

    #[test]
    fn it_spells_names_the_way_each_language_would() {
        assert_eq!(pascal("media_reference"), "MediaReference");
        assert_eq!(pascal("to_json"), "ToJSON");
        assert_eq!(pascal("target_url"), "TargetURL");
        assert_eq!(camel("target_url"), "targetURL");
        assert_eq!(camel("url_of_clip"), "urlOfClip");
    }

    #[test]
    fn it_respells_initialisms_in_a_pascal_name() {
        use super::{INITIALISMS, respell, split_pascal};
        assert_eq!(split_pascal("Fcp7Xml"), ["Fcp", "7", "Xml"]);
        assert_eq!(split_pascal("Cmx3600"), ["Cmx", "3600"]);
        assert_eq!(respell("Cmx3600", INITIALISMS), "CMX3600");
        assert_eq!(respell("InvalidUtf8", INITIALISMS), "InvalidUTF8");
        assert_eq!(respell("OtioJson", INITIALISMS), "OTIOJSON");
    }

    #[test]
    fn it_finds_the_prefix_a_set_of_constants_shares() {
        let constants = [
            "OTIO_VALUE_BOOL".to_string(),
            "OTIO_VALUE_BOX2D".to_string(),
            "OTIO_VALUE_COLOR".to_string(),
        ];
        assert_eq!(shared_prefix(&constants), "OTIO_VALUE_");
        assert_eq!(
            constant_suffix("OTIO_FORMAT_CMX_3600", "OTIO_FORMAT_"),
            "CMX3600"
        );
    }

    #[test]
    fn it_collects_the_symbols_documentation_names() {
        let found = references(
            "Compare against `otio_node_is_none`.",
            &["Reports `OTIO_STATUS_NO_VALUE` when unset.".to_string()],
        );
        assert_eq!(found, ["otio_node_is_none", "OTIO_STATUS_NO_VALUE"]);
    }
}
