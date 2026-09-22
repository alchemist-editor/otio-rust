//! Checks that `include/otio.h` declares exactly what the library exports.
//!
//! A C header is a second statement of the ABI, so it can disagree with the
//! first one. A missing declaration is merely inconvenient; a declaration
//! whose parameters have drifted is a crash in someone else's program that no
//! compiler will catch, because the C side believes the header and the Rust
//! side believes itself.
//!
//! So this reads both, reduces every function to its name and the C spelling
//! of its types, and insists the two agree. The C test program that links
//! against the library proves the header compiles and that the calls it makes
//! work; this proves the header covers all of them.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::Path;

/// One function, as name and the C spelling of its signature.
type Signature = BTreeMap<String, String>;

/// Turns a Rust type into the C type the header should spell it with.
fn c_type(rust: &str) -> String {
    let rust = rust.trim();
    if let Some(inner) = rust.strip_prefix("*mut *mut ") {
        return format!("{} **", c_type(inner).trim_end());
    }
    if let Some(inner) = rust.strip_prefix("*const ") {
        return format!("const {} *", c_type(inner).trim_end());
    }
    if let Some(inner) = rust.strip_prefix("*mut ") {
        return format!("{} *", c_type(inner).trim_end());
    }
    match rust {
        "usize" => "size_t".to_string(),
        "i64" => "int64_t".to_string(),
        "u64" => "uint64_t".to_string(),
        "u32" => "uint32_t".to_string(),
        "i32" => "int32_t".to_string(),
        "u8" => "uint8_t".to_string(),
        "f64" => "double".to_string(),
        "c_char" => "char".to_string(),
        "" => "void".to_string(),
        other => other.to_string(),
    }
}

/// Renders a return type and a list of parameter types as one line.
fn render(name: &str, params: &[String], ret: &str) -> String {
    let mut line = String::new();
    let ret = if ret.ends_with('*') {
        ret.to_string()
    } else {
        format!("{ret} ")
    };
    write!(line, "{ret}{name}(").expect("writing to a String cannot fail");
    if params.is_empty() {
        line.push_str("void");
    } else {
        line.push_str(&params.join(", "));
    }
    line.push(')');
    line
}

/// Splits a parameter list on the commas that separate its parameters.
fn split_params(params: &str) -> Vec<String> {
    params
        .split(',')
        .map(str::trim)
        .filter(|part| !part.is_empty() && *part != "void")
        .map(str::to_string)
        .collect()
}

/// Reads every `#[unsafe(no_mangle)]` entry point out of the crate's source.
fn exports() -> Signature {
    let source_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut found = Signature::new();

    let mut files: Vec<_> = std::fs::read_dir(&source_dir)
        .expect("the crate has a src directory")
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| path.extension().is_some_and(|extension| extension == "rs"))
        .collect();
    files.sort();

    for file in files {
        let source = std::fs::read_to_string(&file).expect("a source file can be read");
        let mut rest = source.as_str();
        while let Some(at) = rest.find("#[unsafe(no_mangle)]") {
            rest = &rest[at + "#[unsafe(no_mangle)]".len()..];
            let Some(at) = rest.find("extern \"C\" fn ") else {
                break;
            };
            let after = &rest[at + "extern \"C\" fn ".len()..];
            let open = after.find('(').expect("a function has a parameter list");
            let name = after[..open].trim().to_string();
            let close = after.find(')').expect("a parameter list is closed");
            let params: Vec<String> = split_params(&after[open + 1..close])
                .iter()
                .map(|part| {
                    let (_, rust) = part.split_once(':').expect("a parameter names its type");
                    c_type(rust)
                })
                .collect();

            let tail = &after[close + 1..];
            let brace = tail.find('{').expect("a function has a body");
            let ret = tail[..brace]
                .trim()
                .strip_prefix("->")
                .map_or_else(String::new, c_type);
            let ret = if ret.is_empty() {
                "void".to_string()
            } else {
                ret
            };

            found.insert(name.clone(), render(&name, &params, &ret));
            rest = tail;
        }
    }

    assert!(
        found.len() > 200,
        "expected the crate to export hundreds of functions, found {}",
        found.len()
    );
    found
}

/// Strips a parameter's name, leaving its type.
fn param_type(declaration: &str) -> String {
    let trimmed = declaration.trim();
    let name_start = trimmed
        .rfind(|character: char| !character.is_alphanumeric() && character != '_')
        .map_or(0, |index| index + 1);
    let mut kind = trimmed[..name_start].trim().to_string();
    if kind.is_empty() {
        // A parameter with no name at all, such as `void`.
        kind = trimmed.to_string();
    }
    if kind.ends_with('*') {
        kind
    } else {
        kind.trim().to_string()
    }
}

/// Reads every function declaration out of the header.
fn declarations() -> Signature {
    let header = Path::new(env!("CARGO_MANIFEST_DIR")).join("include/otio.h");
    let text = std::fs::read_to_string(&header).expect("include/otio.h can be read");
    let mut found = Signature::new();

    let mut current: Option<String> = None;
    for line in text.lines() {
        if let Some(open) = current.as_mut() {
            open.push(' ');
            open.push_str(line.trim());
        } else {
            let starts = line
                .chars()
                .next()
                .is_some_and(|character| character.is_alphabetic());
            if !starts || !line.contains("otio_") || !line.contains('(') {
                continue;
            }
            current = Some(line.trim().to_string());
        }

        let Some(declaration) = current.as_ref() else {
            continue;
        };
        if !declaration.ends_with(");") {
            continue;
        }

        let declaration = declaration.trim_end_matches(';');
        let open = declaration.find('(').expect("checked above");
        let close = declaration.rfind(')').expect("checked above");
        let head = declaration[..open].trim();
        let name_start = head
            .rfind(|character: char| !character.is_alphanumeric() && character != '_')
            .map_or(0, |index| index + 1);
        let name = head[name_start..].to_string();
        let ret = head[..name_start].trim().to_string();
        let params: Vec<String> = split_params(&declaration[open + 1..close])
            .iter()
            .map(|part| param_type(part))
            .collect();

        found.insert(name.clone(), render(&name, &params, &ret));
        current = None;
    }

    found
}

#[test]
fn the_header_declares_every_exported_function() {
    let exports = exports();
    let declarations = declarations();

    let missing: Vec<_> = exports
        .keys()
        .filter(|name| !declarations.contains_key(*name))
        .collect();
    assert!(
        missing.is_empty(),
        "include/otio.h does not declare these exported functions: {missing:#?}"
    );
}

#[test]
fn the_header_declares_nothing_the_library_does_not_export() {
    let exports = exports();
    let declarations = declarations();

    let extra: Vec<_> = declarations
        .keys()
        .filter(|name| !exports.contains_key(*name))
        .collect();
    assert!(
        extra.is_empty(),
        "include/otio.h declares functions the library does not export: {extra:#?}"
    );
}

#[test]
fn every_declaration_matches_its_rust_signature() {
    let exports = exports();
    let declarations = declarations();

    let mut wrong = Vec::new();
    for (name, expected) in &exports {
        let Some(declared) = declarations.get(name) else {
            continue;
        };
        if declared != expected {
            wrong.push(format!("  header: {declared}\n  rust:   {expected}"));
        }
    }
    assert!(
        wrong.is_empty(),
        "these declarations in include/otio.h have drifted from the library:\n{}",
        wrong.join("\n\n")
    );
}

#[test]
fn every_call_that_can_fail_hands_its_message_back() {
    // A message read by a second call is one another thread may have
    // overwritten in between, so every call that returns a status takes the
    // place to write its message as its last parameter.
    let wrong: Vec<_> = exports()
        .into_iter()
        .filter(|(_, signature)| signature.starts_with("OtioStatus "))
        .filter(|(_, signature)| {
            !signature.ends_with(", OtioBuffer *)") && !signature.ends_with("(OtioBuffer *)")
        })
        .map(|(name, _)| name)
        .collect();
    assert!(
        wrong.is_empty(),
        "these return a status but do not end in `OtioBuffer *out_error`: {wrong:#?}"
    );
}
