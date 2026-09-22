//! Reading the raw items out of the C ABI crate's source.
//!
//! This is deliberately not a Rust parser. It reads the small, regular subset
//! of Rust that `otio-capi` is written in — rustfmt'd top-level items, one
//! attribute per line, doc comments directly above what they document — and
//! it fails loudly rather than guessing when it meets anything else. A parser
//! that shrugged at surprises would silently drop an entry point from every
//! SDK, which is the one failure this whole pipeline exists to prevent.

use std::fmt;
use std::path::Path;

/// Something the source said that this scanner cannot read.
#[derive(Debug)]
pub struct ScanError {
    /// Where the trouble is.
    pub location: String,
    /// What was wrong with it.
    pub message: String,
}

impl fmt::Display for ScanError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}: {}", self.location, self.message)
    }
}

impl std::error::Error for ScanError {}

/// What a scan produced, or the first thing it could not read.
pub type Scanned<T> = Result<T, ScanError>;

/// A `#[repr(C)]` enum, as written.
#[derive(Debug, Clone)]
pub struct RawEnum {
    /// The type's name, such as `OtioStatus`.
    pub name: String,
    /// The doc comment above it.
    pub docs: Vec<String>,
    /// Its variants, in declaration order.
    pub variants: Vec<RawVariant>,
}

/// One variant of a `#[repr(C)]` enum.
#[derive(Debug, Clone)]
pub struct RawVariant {
    /// The variant's Rust name, such as `NullPointer`.
    pub name: String,
    /// The discriminant it is assigned.
    pub value: i64,
    /// The doc comment above it.
    pub docs: Vec<String>,
}

/// A `#[repr(C)]` struct, as written.
#[derive(Debug, Clone)]
pub struct RawStruct {
    /// The type's name, such as `OtioRationalTime`.
    pub name: String,
    /// The doc comment above it.
    pub docs: Vec<String>,
    /// Its fields, in declaration order.
    pub fields: Vec<RawField>,
}

/// One field of a `#[repr(C)]` struct.
#[derive(Debug, Clone)]
pub struct RawField {
    /// The field's name.
    pub name: String,
    /// Its Rust type, as written.
    pub rust_type: String,
    /// The doc comment above it.
    pub docs: Vec<String>,
}

/// An exported entry point, as written.
#[derive(Debug, Clone)]
pub struct RawFunction {
    /// The exported symbol, such as `otio_clip_new`.
    pub name: String,
    /// The doc comment above it.
    pub docs: Vec<String>,
    /// Its parameters, in declaration order.
    pub params: Vec<RawParam>,
    /// Its return type as written, or `None` for a function returning unit.
    pub returns: Option<String>,
    /// The body, used to tell what the function actually does with its
    /// arguments rather than what its prose claims.
    pub body: String,
}

/// One parameter of an entry point.
#[derive(Debug, Clone)]
pub struct RawParam {
    /// The parameter's name.
    pub name: String,
    /// Its Rust type, as written.
    pub rust_type: String,
}

/// Everything one scan found.
#[derive(Debug, Default)]
pub struct Source {
    /// Every `#[repr(C)]` enum.
    pub enums: Vec<RawEnum>,
    /// Every `#[repr(C)]` struct.
    pub structs: Vec<RawStruct>,
    /// Every exported entry point.
    pub functions: Vec<RawFunction>,
}

/// Reads every `.rs` file in a directory, in name order.
///
/// # Errors
///
/// Fails if the directory cannot be read, or if any file in it says something
/// this scanner does not know how to read.
pub fn directory(path: &Path) -> Scanned<Source> {
    let mut paths: Vec<_> = std::fs::read_dir(path)
        .map_err(|error| ScanError {
            location: path.display().to_string(),
            message: error.to_string(),
        })?
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| path.extension().is_some_and(|extension| extension == "rs"))
        .collect();
    paths.sort();

    let mut source = Source::default();
    for path in paths {
        let text = std::fs::read_to_string(&path).map_err(|error| ScanError {
            location: path.display().to_string(),
            message: error.to_string(),
        })?;
        file(&path.display().to_string(), &text, &mut source)?;
    }
    Ok(source)
}

/// Reads one file into a scan.
fn file(location: &str, text: &str, into: &mut Source) -> Scanned<()> {
    let lines: Vec<&str> = text.lines().collect();
    let mut docs: Vec<String> = Vec::new();
    let mut attributes: Vec<String> = Vec::new();
    let mut index = 0;

    while index < lines.len() {
        let line = lines[index];
        let trimmed = line.trim_start();
        let indented = line.len() != trimmed.len();

        if let Some(doc) = trimmed.strip_prefix("///") {
            docs.push(doc.strip_prefix(' ').unwrap_or(doc).to_string());
            index += 1;
            continue;
        }
        if trimmed.starts_with("#[") {
            if !trimmed.ends_with(']') {
                return Err(ScanError {
                    location: format!("{location}:{}", index + 1),
                    message: "an attribute spanning several lines".to_string(),
                });
            }
            attributes.push(trimmed.to_string());
            index += 1;
            continue;
        }
        if trimmed.is_empty() {
            docs.clear();
            attributes.clear();
            index += 1;
            continue;
        }
        // A `//!` header, an ordinary comment or anything indented is not the
        // start of a top-level item.
        if trimmed.starts_with("//") || indented {
            index += 1;
            continue;
        }

        let here = format!("{location}:{}", index + 1);
        let repr_c = attributes.iter().any(|attribute| attribute == "#[repr(C)]");
        let exported = attributes
            .iter()
            .any(|attribute| attribute == "#[unsafe(no_mangle)]");

        if repr_c && trimmed.starts_with("pub enum ") {
            let (item, next) = block(&lines, index, &here)?;
            into.enums.push(parse_enum(&here, &item, docs.clone())?);
            index = next;
        } else if repr_c && trimmed.starts_with("pub struct ") {
            let (item, next) = block(&lines, index, &here)?;
            into.structs.push(parse_struct(&here, &item, docs.clone())?);
            index = next;
        } else if exported && trimmed.contains("extern \"C\" fn ") {
            let (item, next) = block(&lines, index, &here)?;
            into.functions
                .push(parse_function(&here, &item, docs.clone())?);
            index = next;
        } else {
            index += 1;
        }
        docs.clear();
        attributes.clear();
    }
    Ok(())
}

/// Takes a top-level item, from its first line to the `}` that closes it.
///
/// Every item this scanner cares about has a braced body, and rustfmt puts
/// its closing brace alone in the first column, so that is the terminator.
fn block(lines: &[&str], start: usize, location: &str) -> Scanned<(String, usize)> {
    let mut collected = String::new();
    for (offset, line) in lines[start..].iter().enumerate() {
        collected.push_str(line);
        collected.push('\n');
        if *line == "}" {
            return Ok((collected, start + offset + 1));
        }
    }
    Err(ScanError {
        location: location.to_string(),
        message: "an item that is never closed by a `}` in the first column".to_string(),
    })
}

/// Splits a braced body into the head before `{` and the lines inside it.
fn body_lines(item: &str) -> (&str, Vec<&str>) {
    let open = item.find('{').unwrap_or(item.len());
    let head = item[..open].trim();
    let rest = item.get(open + 1..).unwrap_or("");
    let inner = rest.strip_suffix("}\n").unwrap_or(rest);
    (head, inner.lines().collect())
}

/// Reads the doc comment lines that precede a field or variant.
fn take_docs(pending: &mut Vec<String>) -> Vec<String> {
    std::mem::take(pending)
}

/// Reads a `#[repr(C)] pub enum`.
fn parse_enum(location: &str, item: &str, docs: Vec<String>) -> Scanned<RawEnum> {
    let (head, lines) = body_lines(item);
    let name = head.trim_start_matches("pub enum ").trim().to_string();
    let mut variants = Vec::new();
    let mut pending: Vec<String> = Vec::new();

    for line in lines {
        let trimmed = line.trim();
        if let Some(doc) = trimmed.strip_prefix("///") {
            pending.push(doc.strip_prefix(' ').unwrap_or(doc).to_string());
            continue;
        }
        if trimmed.is_empty() || trimmed.starts_with("//") {
            continue;
        }
        let entry = trimmed.trim_end_matches(',');
        let (variant, value) = entry.split_once('=').ok_or_else(|| ScanError {
            location: location.to_string(),
            message: format!("the variant `{entry}` of `{name}` has no explicit discriminant"),
        })?;
        let value = value.trim().parse::<i64>().map_err(|_| ScanError {
            location: location.to_string(),
            message: format!(
                "the variant `{entry}` of `{name}` has a discriminant that is not a plain integer"
            ),
        })?;
        variants.push(RawVariant {
            name: variant.trim().to_string(),
            value,
            docs: take_docs(&mut pending),
        });
    }

    if variants.is_empty() {
        return Err(ScanError {
            location: location.to_string(),
            message: format!("`{name}` has no variants"),
        });
    }
    Ok(RawEnum {
        name,
        docs,
        variants,
    })
}

/// Reads a `#[repr(C)] pub struct`.
fn parse_struct(location: &str, item: &str, docs: Vec<String>) -> Scanned<RawStruct> {
    let (head, lines) = body_lines(item);
    let name = head.trim_start_matches("pub struct ").trim().to_string();
    let mut fields = Vec::new();
    let mut pending: Vec<String> = Vec::new();

    for line in lines {
        let trimmed = line.trim();
        if let Some(doc) = trimmed.strip_prefix("///") {
            pending.push(doc.strip_prefix(' ').unwrap_or(doc).to_string());
            continue;
        }
        if trimmed.is_empty() || trimmed.starts_with("//") {
            continue;
        }
        let entry = trimmed.trim_end_matches(',');
        let entry = entry.strip_prefix("pub ").ok_or_else(|| ScanError {
            location: location.to_string(),
            message: format!("the field `{entry}` of `{name}` is not public"),
        })?;
        let (field, rust_type) = entry.split_once(':').ok_or_else(|| ScanError {
            location: location.to_string(),
            message: format!("the field `{entry}` of `{name}` does not name its type"),
        })?;
        fields.push(RawField {
            name: field.trim().to_string(),
            rust_type: rust_type.trim().to_string(),
            docs: take_docs(&mut pending),
        });
    }

    Ok(RawStruct { name, docs, fields })
}

/// Reads an exported entry point.
fn parse_function(location: &str, item: &str, docs: Vec<String>) -> Scanned<RawFunction> {
    let marker = "extern \"C\" fn ";
    let at = item.find(marker).ok_or_else(|| ScanError {
        location: location.to_string(),
        message: "an exported item that is not an `extern \"C\" fn`".to_string(),
    })?;
    let after = &item[at + marker.len()..];
    let open = after.find('(').ok_or_else(|| ScanError {
        location: location.to_string(),
        message: "a function with no parameter list".to_string(),
    })?;
    let name = after[..open].trim().to_string();

    let mut depth = 0usize;
    let mut close = None;
    for (offset, character) in after[open..].char_indices() {
        match character {
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    close = Some(open + offset);
                    break;
                }
            }
            _ => {}
        }
    }
    let close = close.ok_or_else(|| ScanError {
        location: location.to_string(),
        message: format!("the parameter list of `{name}` is never closed"),
    })?;

    let mut params = Vec::new();
    for entry in split_top_level(&after[open + 1..close]) {
        let (param, rust_type) = entry.split_once(':').ok_or_else(|| ScanError {
            location: location.to_string(),
            message: format!("the parameter `{entry}` of `{name}` does not name its type"),
        })?;
        params.push(RawParam {
            name: param.trim().to_string(),
            rust_type: normalize(rust_type),
        });
    }

    let tail = &after[close + 1..];
    let brace = tail.find('{').ok_or_else(|| ScanError {
        location: location.to_string(),
        message: format!("`{name}` has no body"),
    })?;
    let returns = tail[..brace]
        .trim()
        .strip_prefix("->")
        .map(normalize)
        .filter(|returns| !returns.is_empty());

    Ok(RawFunction {
        name,
        docs,
        params,
        returns,
        body: tail[brace..].to_string(),
    })
}

/// Collapses the whitespace rustfmt may have wrapped a type across.
fn normalize(rust_type: &str) -> String {
    rust_type.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Splits a parameter list on the commas between parameters, ignoring any
/// inside brackets.
fn split_top_level(list: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut depth = 0usize;
    let mut current = String::new();
    for character in list.chars() {
        match character {
            '(' | '[' | '<' => {
                depth += 1;
                current.push(character);
            }
            ')' | ']' | '>' => {
                depth = depth.saturating_sub(1);
                current.push(character);
            }
            ',' if depth == 0 => {
                parts.push(std::mem::take(&mut current));
            }
            _ => current.push(character),
        }
    }
    parts.push(current);
    parts
        .into_iter()
        .map(|part| part.trim().to_string())
        .filter(|part| !part.is_empty())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{Source, file};

    #[test]
    fn it_reads_an_enum_a_struct_and_a_function() {
        let text = "\
/// A status.
#[repr(C)]
pub enum OtioStatus {
    /// Fine.
    Ok = 0,
    /// Not fine.
    Bad = 1,
}

/// A time.
#[repr(C)]
#[derive(Debug)]
pub struct OtioRationalTime {
    /// How many.
    pub value: f64,
    /// Per second.
    pub rate: f64,
}

/// Does a thing.
///
/// And says so.
#[unsafe(no_mangle)]
pub unsafe extern \"C\" fn otio_thing(
    document: *const OtioDocument,
    out_value: *mut f64,
) -> OtioStatus {
    guard(|| Ok(()))
}
";
        let mut source = Source::default();
        file("test.rs", text, &mut source).expect("the sample reads");

        assert_eq!(source.enums.len(), 1);
        assert_eq!(source.enums[0].name, "OtioStatus");
        assert_eq!(source.enums[0].variants.len(), 2);
        assert_eq!(source.enums[0].variants[1].value, 1);

        assert_eq!(source.structs.len(), 1);
        assert_eq!(source.structs[0].fields[0].rust_type, "f64");

        assert_eq!(source.functions.len(), 1);
        let function = &source.functions[0];
        assert_eq!(function.name, "otio_thing");
        assert_eq!(function.docs, ["Does a thing.", "", "And says so."]);
        assert_eq!(function.params.len(), 2);
        assert_eq!(function.params[0].rust_type, "*const OtioDocument");
        assert_eq!(function.returns.as_deref(), Some("OtioStatus"));
    }

    #[test]
    fn a_variant_without_a_discriminant_is_an_error() {
        let text = "\
/// A status.
#[repr(C)]
pub enum OtioStatus {
    /// Fine.
    Ok,
}
";
        let mut source = Source::default();
        let error = file("test.rs", text, &mut source).expect_err("it should refuse this");
        assert!(
            error.message.contains("no explicit discriminant"),
            "{error}"
        );
    }
}
