//! Reading the C ABI out of `otio-capi`'s source.
//!
//! The SDK is generated from the same text a reviewer reads, so there is no
//! second description of the API to keep in step with the first one. What this
//! module recovers is:
//!
//! - every `#[unsafe(no_mangle)] extern "C"` entry point, with its parameters,
//!   its return type and its doc comment;
//! - every `#[repr(C)]` struct and enum, with per-field and per-variant doc
//!   comments;
//! - three things a signature cannot say, which the bodies can:
//!   - a `*const c_char` read with `optional_text` accepts null, so it is an
//!     optional argument rather than a required one;
//!   - an `OtioNode` read with `optional_node` accepts the none handle;
//!   - a call that raises `Fault::no_value` can answer "there is nothing",
//!     which is a value in TypeScript and not an error.
//!
//! The parser is deliberately literal about the shapes `otio-capi` is written
//! in rather than being a Rust parser. Anything it cannot read is an error
//! that stops generation, so a surprise shows up as a failed build rather than
//! as a function quietly missing from the SDK.

use std::collections::BTreeSet;
use std::fmt;
use std::path::Path;

/// A type as it appears in the ABI, in Rust's spelling.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Type {
    /// No type at all: a function that returns nothing.
    Void,
    /// `bool`.
    Bool,
    /// `f64`.
    F64,
    /// `u8`.
    U8,
    /// `i32`.
    I32,
    /// `u32`.
    U32,
    /// `i64`.
    I64,
    /// `u64`.
    U64,
    /// `usize`, which is 32 bits wide on `wasm32`.
    Usize,
    /// `c_char`, which only ever appears behind a pointer.
    Char,
    /// One of the ABI's own structs or enums, such as `OtioRationalTime`.
    Named(String),
    /// A pointer, mutable or not.
    Pointer {
        /// Whether the pointee may be written through.
        mutable: bool,
        /// What it points at.
        inner: Box<Type>,
    },
}

impl Type {
    /// Reads a type from the Rust source's spelling of it.
    fn parse(text: &str) -> Result<Self, Error> {
        let text = text.trim();
        if let Some(rest) = text.strip_prefix("*const ") {
            return Ok(Self::Pointer {
                mutable: false,
                inner: Box::new(Self::parse(rest)?),
            });
        }
        if let Some(rest) = text.strip_prefix("*mut ") {
            return Ok(Self::Pointer {
                mutable: true,
                inner: Box::new(Self::parse(rest)?),
            });
        }
        Ok(match text {
            "" | "()" => Self::Void,
            "bool" => Self::Bool,
            "f64" => Self::F64,
            "u8" => Self::U8,
            "i32" => Self::I32,
            "u32" => Self::U32,
            "i64" => Self::I64,
            "u64" => Self::U64,
            "usize" => Self::Usize,
            "c_char" => Self::Char,
            named if named.starts_with("Otio") => Self::Named(named.to_string()),
            other => return Err(Error(format!("unrecognised type `{other}`"))),
        })
    }

    /// Returns what this points at, if it is a pointer.
    #[must_use]
    pub fn pointee(&self) -> Option<&Self> {
        match self {
            Self::Pointer { inner, .. } => Some(inner),
            _ => None,
        }
    }

    /// Returns the ABI type's name, if it is one of them.
    #[must_use]
    pub fn named(&self) -> Option<&str> {
        match self {
            Self::Named(name) => Some(name),
            _ => None,
        }
    }
}

impl fmt::Display for Type {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Void => formatter.write_str("()"),
            Self::Bool => formatter.write_str("bool"),
            Self::F64 => formatter.write_str("f64"),
            Self::U8 => formatter.write_str("u8"),
            Self::I32 => formatter.write_str("i32"),
            Self::U32 => formatter.write_str("u32"),
            Self::I64 => formatter.write_str("i64"),
            Self::U64 => formatter.write_str("u64"),
            Self::Usize => formatter.write_str("usize"),
            Self::Char => formatter.write_str("c_char"),
            Self::Named(name) => formatter.write_str(name),
            Self::Pointer { mutable, inner } => {
                write!(
                    formatter,
                    "*{} {inner}",
                    if *mutable { "mut" } else { "const" }
                )
            }
        }
    }
}

/// One parameter of an entry point.
#[derive(Debug, Clone)]
pub struct Parameter {
    /// The parameter's name, as the Rust source spells it.
    pub name: String,
    /// Its type.
    pub kind: Type,
}

/// One `#[unsafe(no_mangle)] extern "C"` entry point.
#[derive(Debug, Clone)]
pub struct Function {
    /// The exported symbol, such as `otio_item_source_range`.
    pub name: String,
    /// The doc comment above it, one entry per line, markers stripped.
    pub doc: Vec<String>,
    /// Its parameters, in order.
    pub parameters: Vec<Parameter>,
    /// What it returns.
    pub returns: Type,
    /// Parameters the body reads with `optional_text` or `optional_node`, so
    /// null and the none handle are accepted rather than rejected.
    pub optional: BTreeSet<String>,
    /// Whether the call can answer `OTIO_STATUS_NO_VALUE`.
    pub no_value: bool,
}

impl Function {
    /// Returns whether the call reports failure through an [`Type::Named`]
    /// `OtioStatus` rather than answering directly.
    #[must_use]
    pub fn fallible(&self) -> bool {
        self.returns.named() == Some("OtioStatus")
    }

    /// Returns the parameters whose names begin with `out_`, in order.
    #[must_use]
    pub fn outputs(&self) -> Vec<&Parameter> {
        self.parameters
            .iter()
            .filter(|parameter| parameter.name.starts_with("out_"))
            .collect()
    }
}

/// One variant of a `#[repr(C)]` enum.
#[derive(Debug, Clone)]
pub struct Variant {
    /// The Rust spelling, such as `NoValue`.
    pub name: String,
    /// Its doc comment.
    pub doc: Vec<String>,
    /// Its discriminant.
    pub value: i64,
}

/// A `#[repr(C)]` enum.
#[derive(Debug, Clone)]
pub struct Enumeration {
    /// The type's name, such as `OtioStatus`.
    pub name: String,
    /// Its doc comment.
    pub doc: Vec<String>,
    /// Its variants, in declaration order.
    pub variants: Vec<Variant>,
}

/// One field of a `#[repr(C)]` struct.
#[derive(Debug, Clone)]
pub struct Field {
    /// The field's name.
    pub name: String,
    /// Its doc comment.
    pub doc: Vec<String>,
    /// Its type.
    pub kind: Type,
}

/// A `#[repr(C)]` struct.
#[derive(Debug, Clone)]
pub struct Record {
    /// The type's name, such as `OtioRationalTime`.
    pub name: String,
    /// Its doc comment.
    pub doc: Vec<String>,
    /// Its fields, in declaration order.
    pub fields: Vec<Field>,
}

/// Everything the C ABI declares.
#[derive(Debug, Clone, Default)]
pub struct Abi {
    /// Every exported entry point, sorted by name.
    pub functions: Vec<Function>,
    /// Every `#[repr(C)]` enum, sorted by name.
    pub enumerations: Vec<Enumeration>,
    /// Every `#[repr(C)]` struct, sorted by name.
    pub records: Vec<Record>,
}

impl Abi {
    /// Finds an enum by name.
    #[must_use]
    pub fn enumeration(&self, name: &str) -> Option<&Enumeration> {
        self.enumerations
            .iter()
            .find(|enumeration| enumeration.name == name)
    }

    /// Finds a struct by name.
    #[must_use]
    pub fn record(&self, name: &str) -> Option<&Record> {
        self.records.iter().find(|record| record.name == name)
    }

    /// Reads the ABI out of a directory of Rust source files.
    ///
    /// # Errors
    ///
    /// Fails if the directory cannot be read, or if it holds a declaration
    /// this parser does not understand.
    pub fn read(source_dir: &Path) -> Result<Self, Error> {
        let mut files: Vec<_> = std::fs::read_dir(source_dir)
            .map_err(|error| Error(format!("{}: {error}", source_dir.display())))?
            .filter_map(|entry| entry.ok().map(|entry| entry.path()))
            .filter(|path| path.extension().is_some_and(|extension| extension == "rs"))
            .collect();
        files.sort();

        let mut abi = Self::default();
        for file in &files {
            let text = std::fs::read_to_string(file)
                .map_err(|error| Error(format!("{}: {error}", file.display())))?;
            parse_file(&text, &mut abi)
                .map_err(|error| Error(format!("{}: {error}", file.display())))?;
        }

        abi.functions.sort_by(|a, b| a.name.cmp(&b.name));
        abi.enumerations.sort_by(|a, b| a.name.cmp(&b.name));
        abi.records.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(abi)
    }
}

/// Something the parser could not read.
#[derive(Debug)]
pub struct Error(pub String);

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for Error {}

/// Reads one source file into the ABI being built.
fn parse_file(text: &str, abi: &mut Abi) -> Result<(), Error> {
    let lines: Vec<&str> = text.lines().collect();
    let mut doc: Vec<String> = Vec::new();
    let mut exported = false;
    let mut c_layout = false;
    let mut index = 0;

    while index < lines.len() {
        let line = lines[index];
        let trimmed = line.trim();

        if let Some(rest) = trimmed.strip_prefix("///") {
            doc.push(rest.strip_prefix(' ').unwrap_or(rest).to_string());
            index += 1;
            continue;
        }
        if trimmed == "#[unsafe(no_mangle)]" {
            exported = true;
            index += 1;
            continue;
        }
        if trimmed == "#[repr(C)]" {
            c_layout = true;
            index += 1;
            continue;
        }
        // Attributes and blank lines sit between a doc comment and its item,
        // so neither of them ends the comment.
        if trimmed.is_empty() || trimmed.starts_with("#[") || trimmed.starts_with("//") {
            index += 1;
            continue;
        }

        if exported && is_entry_point(trimmed) {
            let (function, next) = parse_function(&lines, index, std::mem::take(&mut doc))?;
            abi.functions.push(function);
            index = next;
            exported = false;
            c_layout = false;
            continue;
        }
        if c_layout && trimmed.starts_with("pub struct ") {
            let (record, next) = parse_record(&lines, index, std::mem::take(&mut doc))?;
            abi.records.push(record);
            index = next;
            c_layout = false;
            continue;
        }
        if c_layout && trimmed.starts_with("pub enum ") {
            let (enumeration, next) = parse_enumeration(&lines, index, std::mem::take(&mut doc))?;
            abi.enumerations.push(enumeration);
            index = next;
            c_layout = false;
            continue;
        }

        doc.clear();
        exported = false;
        c_layout = false;
        index += 1;
    }

    Ok(())
}

/// Returns whether a line opens an `extern "C"` function.
fn is_entry_point(line: &str) -> bool {
    line.starts_with("pub extern \"C\" fn ") || line.starts_with("pub unsafe extern \"C\" fn ")
}

/// Reads one entry point, returning it and the line after its body.
fn parse_function(
    lines: &[&str],
    start: usize,
    doc: Vec<String>,
) -> Result<(Function, usize), Error> {
    // The signature runs until the parenthesis that closes the parameter list,
    // which may be several lines down.
    let mut signature = String::new();
    let mut index = start;
    let mut depth = 0usize;
    let mut closed = false;
    while index < lines.len() {
        let line = lines[index];
        for character in line.chars() {
            match character {
                '(' => depth += 1,
                ')' => {
                    depth -= 1;
                    if depth == 0 {
                        closed = true;
                    }
                }
                _ => {}
            }
        }
        signature.push_str(line.trim());
        signature.push(' ');
        index += 1;
        if closed {
            break;
        }
    }
    if !closed {
        return Err(Error(format!(
            "unterminated parameter list at line {}",
            start + 1
        )));
    }

    // Whatever follows the parameter list, up to the opening brace, is the
    // return type. It is on the same line in this codebase, but reading on
    // until the brace costs nothing and does not care.
    let mut tail = signature
        .rsplit_once(')')
        .map(|(_, tail)| tail.trim().to_string())
        .unwrap_or_default();
    while !tail.contains('{') && index < lines.len() {
        tail.push(' ');
        tail.push_str(lines[index].trim());
        index += 1;
    }
    let returns = match tail.split_once('{') {
        Some((head, _)) => Type::parse(head.trim().strip_prefix("->").unwrap_or(""))?,
        None => {
            return Err(Error(format!(
                "no body for the function at line {}",
                start + 1
            )));
        }
    };

    let open = signature
        .find('(')
        .ok_or_else(|| Error(format!("no parameter list at line {}", start + 1)))?;
    let close = signature
        .rfind(')')
        .ok_or_else(|| Error(format!("no parameter list at line {}", start + 1)))?;
    let name = signature[..open]
        .rsplit(' ')
        .find(|word| !word.is_empty())
        .unwrap_or_default()
        .to_string();

    let mut parameters = Vec::new();
    for part in signature[open + 1..close].split(',') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        let (parameter_name, kind) = part.split_once(':').ok_or_else(|| {
            Error(format!(
                "parameter `{part}` of `{name}` does not name a type"
            ))
        })?;
        parameters.push(Parameter {
            name: parameter_name.trim().to_string(),
            kind: Type::parse(kind)?,
        });
    }

    // The body reaches the closing brace in column zero, which is how every
    // top-level item in this codebase ends.
    let body_start = index;
    while index < lines.len() && lines[index] != "}" {
        index += 1;
    }
    let body = lines[body_start..index.min(lines.len())].join("\n");
    index = (index + 1).min(lines.len());

    let mut optional = BTreeSet::new();
    for parameter in &parameters {
        let text = &parameter.name;
        if body.contains(&format!("optional_text({text},"))
            || body.contains(&format!("optional_node({text})"))
        {
            optional.insert(text.clone());
        }
    }
    // `OtioReadOptions` and `OtioWriteOptions` carry their own optional
    // strings, which the body reads out of the struct rather than out of a
    // parameter. Those are recorded against the field's own name.
    for field in ["name_column", "video_format"] {
        if body.contains(&format!("optional_text(options.{field},")) {
            optional.insert(field.to_string());
        }
    }

    let no_value =
        body.contains("no_value(") || doc.iter().any(|line| line.contains("OTIO_STATUS_NO_VALUE"));

    Ok((
        Function {
            name,
            doc,
            parameters,
            returns,
            optional,
            no_value,
        },
        index,
    ))
}

/// Reads one `#[repr(C)]` struct, returning it and the line after it.
fn parse_record(lines: &[&str], start: usize, doc: Vec<String>) -> Result<(Record, usize), Error> {
    let name = lines[start]
        .trim()
        .trim_start_matches("pub struct ")
        .trim_end_matches('{')
        .trim()
        .to_string();

    // A tuple struct, such as `pub struct OtioDocument(pub(crate) Document);`,
    // is an opaque handle rather than a layout the SDK reads.
    if !lines[start].trim_end().ends_with('{') {
        return Ok((
            Record {
                name: name.trim_end_matches(';').to_string(),
                doc,
                fields: Vec::new(),
            },
            start + 1,
        ));
    }

    let mut fields = Vec::new();
    let mut field_doc: Vec<String> = Vec::new();
    let mut index = start + 1;
    while index < lines.len() {
        let trimmed = lines[index].trim();
        index += 1;
        if trimmed == "}" {
            break;
        }
        if let Some(rest) = trimmed.strip_prefix("///") {
            field_doc.push(rest.strip_prefix(' ').unwrap_or(rest).to_string());
            continue;
        }
        if trimmed.is_empty() || trimmed.starts_with("//") || trimmed.starts_with("#[") {
            continue;
        }
        let declaration = trimmed
            .trim_start_matches("pub ")
            .trim_end_matches(',')
            .trim();
        let (field_name, kind) = declaration.split_once(':').ok_or_else(|| {
            Error(format!(
                "field `{declaration}` of `{name}` does not name a type"
            ))
        })?;
        fields.push(Field {
            name: field_name.trim().to_string(),
            doc: std::mem::take(&mut field_doc),
            kind: Type::parse(kind)?,
        });
    }

    Ok((Record { name, doc, fields }, index))
}

/// Reads one `#[repr(C)]` enum, returning it and the line after it.
fn parse_enumeration(
    lines: &[&str],
    start: usize,
    doc: Vec<String>,
) -> Result<(Enumeration, usize), Error> {
    let name = lines[start]
        .trim()
        .trim_start_matches("pub enum ")
        .trim_end_matches('{')
        .trim()
        .to_string();

    let mut variants: Vec<Variant> = Vec::new();
    let mut variant_doc: Vec<String> = Vec::new();
    let mut index = start + 1;
    while index < lines.len() {
        let trimmed = lines[index].trim();
        index += 1;
        if trimmed == "}" {
            break;
        }
        if let Some(rest) = trimmed.strip_prefix("///") {
            variant_doc.push(rest.strip_prefix(' ').unwrap_or(rest).to_string());
            continue;
        }
        if trimmed.is_empty() || trimmed.starts_with("//") || trimmed.starts_with("#[") {
            continue;
        }
        let declaration = trimmed.trim_end_matches(',').trim();
        let (variant_name, value) = match declaration.split_once('=') {
            Some((variant_name, value)) => {
                let value = value.trim().parse::<i64>().map_err(|_| {
                    Error(format!(
                        "variant `{declaration}` of `{name}` has a discriminant this parser cannot read"
                    ))
                })?;
                (variant_name.trim().to_string(), value)
            }
            // An implicit discriminant continues from the one before it, which
            // is C's rule and Rust's.
            None => (
                declaration.to_string(),
                variants.last().map_or(0, |last| last.value + 1),
            ),
        };
        variants.push(Variant {
            name: variant_name,
            doc: std::mem::take(&mut variant_doc),
            value,
        });
    }

    Ok((
        Enumeration {
            name,
            doc,
            variants,
        },
        index,
    ))
}
