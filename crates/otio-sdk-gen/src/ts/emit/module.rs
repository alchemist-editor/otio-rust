//! An extension's module: work built in otio-rust that is not part of
//! OpenTimelineIO, imported on its own as `@alchemist-edit/otio/ext/<name>`.
//!
//! Two files come out of here for each extension:
//!
//! - `ts/src/generated/ext/<name>/raw.ts` — the extension's C entry points,
//!   marshalled the way `raw.ts` marshals the core's, with an interface of
//!   their own over the module's exports. The core's `WasmExports` does not
//!   name them, so nothing the core imports reaches them.
//! - `ts/src/ext/<name>.ts` — the entry point the package's `./ext/<name>`
//!   export names: the extension's functions, at the top level of the module.
//!
//! Neither loads anything. The extension's entry points are compiled into the
//! same `.wasm` as the core's, so the module calls the instance the core's
//! `init()` loaded, through the same runtime, and fails the way a core call
//! does: an `OtioError` for a failing status, and the core's own complaint if
//! nothing has been loaded yet.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;

use otio_sdk_model::{Api, EXTENSIONS};

use super::body::{classes_of, export_line, one_function, one_raw};
use super::{Artifact, WIDTH, extension_preamble, glossary, translate, wrap};
use crate::ts::plan::{Input, Member, Output, Sdk, camel, paragraphs, plan_module};

/// What the package is called on npm, which is what a user imports.
const PACKAGE: &str = "@alchemist-edit/otio";

/// Writes one extension's module.
///
/// `core` is the core SDK, planned: the extension's prose may name the core's
/// calls, and spells them the way the core does.
///
/// # Errors
///
/// Fails if the extension has a call its module cannot spell, or if the
/// model knows no source for it.
pub fn module(api: &Api, core: &Sdk, module: &str) -> Result<Vec<Artifact>, String> {
    let source = EXTENSIONS
        .iter()
        .find(|extension| extension.module == module)
        .ok_or_else(|| format!("the extension `{module}` has no source the model reads"))?
        .source;
    let members = plan_module(api, module)?;

    let mut names = glossary(core);
    for member in &members {
        names.insert(member.symbol.clone(), format!("{}()", member.name));
    }

    Ok(vec![
        raw(api, module, source, &members, &names)?,
        entry(api, module, source, &members, &names)?,
    ])
}

/// The TypeScript name of an extension, as the start of a type's name:
/// `sync` is `Sync`, `ori-annotations` is `OriAnnotations`.
fn pascal(module: &str) -> String {
    let spelled = camel(&module.replace('-', "_"));
    let mut characters = spelled.chars();
    characters.next().map_or_else(String::new, |first| {
        first.to_uppercase().chain(characters).collect()
    })
}

/// Writes `ts/src/generated/ext/<name>/raw.ts`.
fn raw(
    api: &Api,
    module: &str,
    source: &str,
    members: &[Member],
    names: &BTreeMap<String, String>,
) -> Result<Artifact, String> {
    let interface = format!("{}Exports", pascal(module));

    let mut bindings = String::new();
    for member in members {
        bindings.push_str(&one_raw(member, api, names)?);
    }

    let mut text = extension_preamble("//", source);
    let _ = write!(
        text,
        "/**\n\
         \x20* The `{module}` extension's C entry points, doing the marshalling and\n\
         \x20* nothing else, the way `../../raw.ts` does for OpenTimelineIO's own.\n\
         \x20*\n\
         \x20* They call the one module `{PACKAGE}` loaded, through the\n\
         \x20* runtime that loaded it: there is no second instance, and a failing\n\
         \x20* status becomes the same `OtioError` a call into the core throws.\n\
         \x20*/\n\n"
    );

    // Only what the bindings use: an unused import fails the type check.
    let runtime: Vec<&str> = ["check", "openStack", "readBuffer", "readCString", "release"]
        .into_iter()
        .filter(|name| bindings.contains(&format!("{name}(")))
        .collect();
    let mut imported: Vec<String> = vec!["exports as loaded".to_string()];
    imported.extend(runtime.iter().map(|name| (*name).to_string()));
    imported.sort_by_key(|name| name.to_lowercase());
    let _ = writeln!(
        text,
        "import {{ {} }} from \"../../../runtime.js\";",
        imported.join(", ")
    );
    for (layer, file) in [("types", "types"), ("values", "values")] {
        if bindings.contains(&format!("{layer}.")) {
            let _ = writeln!(text, "import * as {layer} from \"../../{file}.js\";");
        }
    }
    text.push('\n');
    if bindings.contains("NO_VALUE") {
        text.push_str(
            "/* The status that means \"the answer is nothing\", which is not a failure. */\nconst NO_VALUE = 3;\n\n",
        );
    }

    let _ = writeln!(
        text,
        "/** The entry points the `{module}` extension adds to the module's exports. */\n\
         interface {interface} {{"
    );
    let mut functions: Vec<_> = api
        .module_groups(module)
        .flat_map(|group| group.functions.iter())
        .collect();
    functions.sort_by(|a, b| a.symbol.cmp(&b.symbol));
    for function in functions {
        text.push_str(&export_line(function));
    }
    text.push_str("}\n\n");
    let _ = writeln!(
        text,
        "/**\n\
         \x20* The exports of the module the core loaded, as this extension's entry\n\
         \x20* points. They are compiled into the same `.wasm`; the core's\n\
         \x20* `WasmExports` simply does not name them.\n\
         \x20*/\n\
         function exports(): {interface} {{\n\
         \x20 return loaded() as unknown as {interface};\n\
         }}\n"
    );
    text.push_str(&bindings);

    Ok(Artifact {
        path: format!("ts/src/generated/ext/{module}/raw.ts"),
        text,
    })
}

/// Writes `ts/src/ext/<name>.ts`, the entry point a user imports.
fn entry(
    api: &Api,
    module: &str,
    source: &str,
    members: &[Member],
    names: &BTreeMap<String, String>,
) -> Result<Artifact, String> {
    let mut functions = String::new();
    for member in members {
        functions.push_str(&one_function(member, names, "")?);
    }

    let mut prose: Vec<String> = api
        .module_groups(module)
        .flat_map(|group| translate(&paragraphs(&group.docs), names))
        .collect();
    prose.push(format!(
        "This module is an otio-rust extension, **not part of OpenTimelineIO**. \
         Importing `{PACKAGE}` does not import it: it is imported on its own, from \
         `{PACKAGE}/ext/{module}`."
    ));
    prose.push(format!(
        "It loads nothing of its own. Its calls are compiled into the same `.wasm` as \
         the core's and run in the instance `{PACKAGE}` loaded, so await that \
         package's `init()` first, as for any call into the core. A call made before \
         then throws the same error a core call would, and one that fails throws an \
         `OtioError`."
    ));

    let mut text = extension_preamble("//", source);
    text.push_str("/**\n");
    for (position, paragraph) in prose.iter().enumerate() {
        if position > 0 {
            text.push_str(" *\n");
        }
        for line in wrap(paragraph, WIDTH - 3) {
            let _ = writeln!(text, " * {line}");
        }
    }
    let binding = camel(&module.replace('-', "_"));
    let _ = write!(
        text,
        " *\n\
         \x20* ```ts\n\
         \x20* import {{ init }} from \"{PACKAGE}\";\n\
         \x20* import * as {binding} from \"{PACKAGE}/ext/{module}\";\n\
         \x20*\n\
         \x20* await init();\n"
    );
    if members.iter().any(|member| member.name == "enable") {
        let _ = writeln!(text, " * {binding}.enable();");
    }
    text.push_str(" * ```\n */\n\n");

    // A call about an object finds the object's document the way the core's
    // own free functions do, through `objects.ts`, which is the package's and
    // not part of its surface. Importing it is what lets the module take a
    // `Timeline` rather than a document, without the core exporting the
    // plumbing to everyone else.
    let takes = |list: bool| {
        members.iter().any(|member| {
            member.inputs.iter().any(|input| match input {
                Input::Node { .. } => !list,
                Input::NodeList { .. } => list,
                _ => false,
            })
        })
    };
    let answers = members.iter().any(|member| {
        member
            .outputs
            .iter()
            .any(|output| matches!(output, Output::Node { .. }))
    });
    let plumbing: Vec<&str> = [
        ("adopt", answers),
        ("place", takes(false)),
        ("placeAll", takes(true)),
    ]
    .into_iter()
    .filter_map(|(name, used)| used.then_some(name))
    .collect();
    if !plumbing.is_empty() {
        let _ = writeln!(
            text,
            "import {{ {} }} from \"../objects.js\";",
            plumbing.join(", ")
        );
    }
    let classes: BTreeSet<&str> = members.iter().flat_map(classes_of).collect();
    if !classes.is_empty() {
        let _ = writeln!(
            text,
            "import type {{ {} }} from \"../generated/api.js\";",
            classes.into_iter().collect::<Vec<_>>().join(", ")
        );
    }
    let _ = writeln!(
        text,
        "import * as raw from \"../generated/ext/{module}/raw.js\";"
    );
    if functions.contains("types.") {
        text.push_str("import * as types from \"../generated/types.js\";\n");
    }
    text.push('\n');
    text.push_str(&functions);
    text.truncate(text.trim_end().len());
    text.push('\n');

    Ok(Artifact {
        path: format!("ts/src/ext/{module}.ts"),
        text,
    })
}
