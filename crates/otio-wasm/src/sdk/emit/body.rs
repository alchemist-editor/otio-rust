//! The two generated TypeScript layers: the calls, and the classes over them.

use std::fmt::Write as _;

use crate::sdk::abi::{Abi, Type};
use crate::sdk::layout::size_of;
use crate::sdk::plan::{
    HIERARCHY, Input, Member, Output, Receiver, Sdk, VALUES, camel, node_class, ts_name,
};

use std::collections::BTreeMap;

use super::{Artifact, glossary, input_type, preamble, result_type, translate, tsdoc};

/// The TypeScript name of a C entry point's low-level binding.
pub fn raw_name(symbol: &str) -> String {
    crate::sdk::plan::camel(symbol.trim_start_matches("otio_"))
}

/// What a value looks like at the WebAssembly boundary.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Word {
    /// A 32-bit integer: a number in JavaScript.
    I32,
    /// A 64-bit integer, which JavaScript spells as a `bigint`.
    I64,
    /// A double.
    F64,
}

impl Word {
    const fn ts(self) -> &'static str {
        match self {
            Self::I32 | Self::F64 => "number",
            Self::I64 => "bigint",
        }
    }
}

/// How a Rust type crosses into WebAssembly.
///
/// The rules are not guesses: they were read off the compiled module. Every
/// struct, however small, is passed as a pointer to a copy, and a struct
/// returned by value becomes a pointer parameter in front of the others with
/// the function returning nothing. `i64` and `u64` stay 64 bits wide, which is
/// why the bindings pass `BigInt` for those and nothing else.
fn word(kind: &Type, abi: &Abi) -> Word {
    match kind {
        Type::F64 => Word::F64,
        Type::I64 | Type::U64 => Word::I64,
        Type::Named(named) if abi.enumeration(named).is_some() => Word::I32,
        _ => Word::I32,
    }
}

/// The WebAssembly-level signature of one entry point.
fn signature(function: &crate::sdk::abi::Function, abi: &Abi) -> (Vec<Word>, Option<Word>) {
    let mut words = Vec::new();
    let returns_struct =
        matches!(&function.returns, Type::Named(named) if abi.record(named).is_some());
    if returns_struct {
        words.push(Word::I32);
    }
    for parameter in &function.parameters {
        words.push(word(&parameter.kind, abi));
    }
    let result = if returns_struct || function.returns == Type::Void {
        None
    } else {
        Some(word(&function.returns, abi))
    };
    (words, result)
}

/// Writes `ts/src/generated/exports.ts`: the module's own interface.
pub fn exports(abi: &Abi) -> Artifact {
    let mut text = preamble("//");
    text.push_str(
        "/**\n\
         \x20* What the WebAssembly module exports.\n\
         \x20*\n\
         \x20* These signatures are the C ABI as WebAssembly spells it, which is not\n\
         \x20* quite how Rust spells it: every struct crosses as a pointer to a copy,\n\
         \x20* a struct returned by value becomes a pointer parameter in front of the\n\
         \x20* others, and `i64` stays sixty-four bits wide, so JavaScript passes a\n\
         \x20* `bigint` for it and a `number` for everything else.\n\
         \x20*\n\
         \x20* `tests/exports.test.ts` reads the compiled module and checks every one\n\
         \x20* of these against the signature the module actually declares.\n\
         \x20*/\n\
         export interface WasmExports {\n\
         \x20 /** The module's linear memory. */\n\
         \x20 readonly memory: WebAssembly.Memory;\n\
         \x20 /** Reserves bytes in that memory for the caller to write into. */\n\
         \x20 readonly otio_wasm_alloc: (size: number) => number;\n\
         \x20 /** Releases a block from `otio_wasm_alloc`. */\n\
         \x20 readonly otio_wasm_free: (pointer: number, size: number) => void;\n\
         \x20 /** The alignment `otio_wasm_alloc` guarantees. */\n\
         \x20 readonly otio_wasm_alignment: () => number;\n",
    );
    for function in &abi.functions {
        let (words, result) = signature(function, abi);
        let parameters: Vec<String> = words
            .iter()
            .enumerate()
            .map(|(index, word)| format!("a{index}: {}", word.ts()))
            .collect();
        let _ = writeln!(
            text,
            "  readonly {}: ({}) => {};",
            function.name,
            parameters.join(", "),
            result.map_or("void", Word::ts)
        );
    }
    text.push_str("}\n");
    Artifact {
        path: "ts/src/generated/exports.ts".to_string(),
        text,
    }
}

/// Writes `ts/src/generated/raw.ts`: one function per C entry point.
///
/// # Errors
///
/// Fails on a shape the emitter has no rule for.
pub fn raw(abi: &Abi, sdk: &Sdk) -> Result<Artifact, String> {
    let mut text = preamble("//");
    text.push_str(
        "/**\n\
         \x20* One function per C entry point, doing the marshalling and nothing else.\n\
         \x20*\n\
         \x20* These are the whole C ABI, faithfully: nothing is left out and nothing\n\
         \x20* is renamed beyond dropping the `otio_` prefix. What they do not do is\n\
         \x20* read like TypeScript — a document is a number here, and an object is a\n\
         \x20* handle. The classes in `api.ts` are the surface; this is what they call.\n\
         \x20*\n\
         \x20* Three things do happen here, because they are mechanical and nobody\n\
         \x20* should have to remember them: a failing status becomes a thrown\n\
         \x20* `OtioError`, `OTIO_STATUS_NO_VALUE` becomes `undefined`, and every\n\
         \x20* buffer the library hands back is read and freed before returning.\n\
         \x20*/\n\n",
    );
    text.push_str(
        "import { check, exports, openStack, readBuffer, readCString } from \"../runtime.js\";\n",
    );
    text.push_str("import * as types from \"./types.js\";\n");
    text.push_str("import * as values from \"./values.js\";\n\n");
    text.push_str(
        "/* The status that means \"the answer is nothing\", which is not a failure. */\nconst NO_VALUE = 3;\n\n",
    );

    let mut members: Vec<&Member> = sdk.free.iter().chain(sdk.internal.iter()).collect();
    for class in sdk.classes.values() {
        members.extend(class.properties.iter().flat_map(|property| {
            std::iter::once(&property.getter)
                .chain(property.setter.iter())
                .chain(property.clear.iter())
        }));
        members.extend(class.methods.iter());
        members.extend(class.statics.iter());
    }
    members.sort_by(|left, right| left.symbol.cmp(&right.symbol));

    let names = glossary(sdk);
    for member in members {
        text.push_str(&one_raw(member, abi, &names)?);
    }

    Ok(Artifact {
        path: "ts/src/generated/raw.ts".to_string(),
        text,
    })
}

/// The parameters a low-level binding takes, before its own arguments.
fn receiver_parameters(member: &Member) -> Vec<String> {
    match &member.receiver {
        Receiver::None => Vec::new(),
        Receiver::Document | Receiver::Borrowed(_) => vec!["document: number".to_string()],
        Receiver::Node => vec![
            "document: number".to_string(),
            "node: types.NodeHandle".to_string(),
        ],
        Receiver::Value(value) => vec![format!("self: values.{value}Like")],
    }
}

/// Emits one low-level binding.
fn one_raw(member: &Member, abi: &Abi, names: &BTreeMap<String, String>) -> Result<String, String> {
    let name = raw_name(&member.symbol);
    let mut parameters = receiver_parameters(member);
    for input in &member.inputs {
        parameters.push(format!(
            "{}: {}{}",
            input.name(),
            input_type(input),
            if input.optional() { " | undefined" } else { "" }
        ));
    }

    let mut text = tsdoc(&translate(&member.doc, names), "");
    let _ = writeln!(
        text,
        "export function {name}({}): {} {{",
        parameters.join(", "),
        result_type(member, abi)?
    );
    text.push_str("  const $stack = openStack();\n  try {\n");

    // The call's arguments, in the order the C entry point takes them.
    let mut arguments: Vec<String> = Vec::new();
    let mut preludes = String::new();
    let mut outputs: Vec<(String, &Output)> = Vec::new();
    let mut count_slot = String::new();

    let returns_struct =
        matches!(&member.returns, Type::Named(named) if abi.record(named).is_some());
    let mut sret = String::new();
    if returns_struct {
        let named = member.returns.named().unwrap_or_default();
        let (size, alignment) = (
            size_of(&member.returns, abi)?.size,
            size_of(&member.returns, abi)?.alignment,
        );
        sret = "$sret".to_string();
        let _ = writeln!(
            preludes,
            "    const $sret = $stack.alloc({size}, {alignment}); /* {named} */"
        );
        arguments.push(sret.clone());
    }

    // The receiver, then the arguments, then the results, in the order the C
    // signature lists them. Walking the original parameter list keeps that
    // order right without the emitter having to remember it.
    let function = abi
        .functions
        .iter()
        .find(|function| function.name == member.symbol)
        .ok_or_else(|| format!("`{}` vanished from the ABI", member.symbol))?;

    let mut used_inputs = 0usize;
    let mut index = 0usize;
    while index < function.parameters.len() {
        let parameter = &function.parameters[index];
        index += 1;

        if parameter.kind.pointee().and_then(Type::named) == Some("OtioDocument") {
            arguments.push("document".to_string());
            continue;
        }
        if parameter.name == "capacity" {
            arguments.push("$capacity".to_string());
            continue;
        }
        if parameter.name == "out_count" && member.list {
            count_slot = "$count".to_string();
            let _ = writeln!(preludes, "    const $count = $stack.alloc(4, 4);");
            arguments.push("$count".to_string());
            continue;
        }
        if let Some(bare) = parameter.name.strip_prefix("out_") {
            let output = member
                .outputs
                .iter()
                .find(|output| output.name() == bare)
                .ok_or_else(|| format!("`{}` lost its result `{bare}`", member.symbol))?;
            let slot = format!("$out{}", outputs.len());
            arguments.push(slot.clone());
            outputs.push((slot, output));
            continue;
        }
        // The receiver's own handle, or its value. The plan recorded which
        // parameter it came from, because it is the first handle the call
        // takes rather than always the one after the document.
        if member.receiver_at == Some(index - 1) {
            match &member.receiver {
                Receiver::Node => {
                    let _ = writeln!(preludes, "    const $receiver = $stack.node(node);");
                }
                Receiver::Value(value) => {
                    let _ = writeln!(
                        preludes,
                        "    const $receiver = $stack.record(types.write{value}, types.sizeOf{value}, types.alignOf{value}, self);"
                    );
                }
                other => {
                    return Err(format!(
                        "`{}` has a receiver at a parameter but is {other:?}",
                        member.symbol
                    ));
                }
            }
            arguments.push("$receiver".to_string());
            continue;
        }

        // Anything left is one of the member's own arguments. Bytes and node
        // lists each swallow the length that follows them.
        let input = member
            .inputs
            .get(used_inputs)
            .ok_or_else(|| format!("`{}` has a parameter no argument matches", member.symbol))?;
        used_inputs += 1;
        let slot = format!("$arg{}", used_inputs - 1);
        preludes.push_str(&marshal(input, &slot, abi)?);
        match input {
            Input::Bytes { .. } | Input::NodeList { .. } => {
                arguments.push(format!("{slot}.pointer"));
                arguments.push(format!("{slot}.length"));
                index += 1;
            }
            _ => arguments.push(slot),
        }
    }

    for (slot, output) in &outputs {
        let (size, alignment) = output_reserve(output, abi)?;
        if member.list {
            let _ = writeln!(preludes, "    let {slot} = 0;");
        } else {
            let _ = writeln!(
                preludes,
                "    const {slot} = $stack.alloc({size}, {alignment});"
            );
        }
    }

    text.push_str(&preludes);

    let call = format!("exports().{}({})", member.symbol, arguments.join(", "));

    if member.list {
        let _ = writeln!(text, "    let $capacity = 0;");
        let _ = writeln!(text, "    check({call});");
        let _ = writeln!(
            text,
            "    $capacity = $stack.view.getUint32({count_slot}, true);"
        );
        for (slot, output) in &outputs {
            let (size, alignment) = output_reserve(output, abi)?;
            let _ = writeln!(
                text,
                "    {slot} = $stack.alloc($capacity * {size}, {alignment});"
            );
        }
        let _ = writeln!(text, "    check({call});");
        let _ = writeln!(
            text,
            "    const $found = $stack.view.getUint32({count_slot}, true);"
        );
        let mut reads = Vec::new();
        for (slot, output) in &outputs {
            let (size, _) = output_reserve(output, abi)?;
            let read = read_output(output, &format!("{slot} + $index * {size}"))?;
            reads.push((
                crate::sdk::plan::camel(output.name()),
                format!("Array.from({{ length: $found }}, (_unused, $index) => {read})"),
            ));
        }
        if reads.len() == 1 {
            let _ = writeln!(text, "    return {};", reads[0].1);
        } else {
            let _ = writeln!(text, "    return {{");
            for (name, expression) in &reads {
                let _ = writeln!(text, "      {name}: {expression},");
            }
            let _ = writeln!(text, "    }};");
        }
    } else if member.fallible {
        if member.no_value {
            let _ = writeln!(text, "    const $status = {call};");
            let _ = writeln!(text, "    if ($status === NO_VALUE) {{");
            let _ = writeln!(text, "      return undefined;");
            let _ = writeln!(text, "    }}");
            let _ = writeln!(text, "    check($status);");
        } else {
            let _ = writeln!(text, "    check({call});");
        }
        match outputs.as_slice() {
            [] => {}
            [(slot, output)] => {
                let _ = writeln!(text, "    return {};", read_output(output, slot)?);
            }
            several => {
                let _ = writeln!(text, "    return {{");
                for (slot, output) in several {
                    let _ = writeln!(
                        text,
                        "      {}: {},",
                        crate::sdk::plan::camel(output.name()),
                        read_output(output, slot)?
                    );
                }
                let _ = writeln!(text, "    }};");
            }
        }
    } else if returns_struct {
        let named = ts_name(member.returns.named().unwrap_or_default());
        let _ = writeln!(text, "    {call};");
        let _ = writeln!(text, "    return types.read{named}($stack.view, {sret});");
    } else {
        let expression = read_direct(&member.returns, &call, abi)?;
        if member.returns == Type::Void {
            let _ = writeln!(text, "    {call};");
        } else {
            let _ = writeln!(text, "    return {expression};");
        }
    }

    text.push_str("  } finally {\n    $stack.close();\n  }\n}\n\n");
    Ok(text)
}

/// Writes the lines that put one argument into the module's memory.
fn marshal(input: &Input, slot: &str, abi: &Abi) -> Result<String, String> {
    let name = input.name();
    Ok(match input {
        Input::Number { kind, .. } => match kind {
            Type::I64 | Type::U64 => format!("    const {slot} = BigInt({name});\n"),
            _ => format!("    const {slot} = {name};\n"),
        },
        Input::Boolean { .. } => format!("    const {slot} = {name} ? 1 : 0;\n"),
        Input::Enumeration { ts, .. } => format!("    const {slot} = types.encode{ts}({name});\n"),
        Input::Text { optional: true, .. } => {
            format!("    const {slot} = {name} === undefined ? 0 : $stack.text({name});\n")
        }
        Input::Text { .. } => format!("    const {slot} = $stack.text({name});\n"),
        Input::Node { optional: true, .. } => format!(
            "    const {slot} = {name} === undefined ? $stack.noneNode() : $stack.node({name});\n"
        ),
        Input::Node { .. } => format!("    const {slot} = $stack.node({name});\n"),
        Input::Bytes { .. } => format!("    const {slot} = $stack.bytes({name});\n"),
        Input::NodeList { .. } => format!("    const {slot} = $stack.nodes({name});\n"),
        Input::Record { ts, optional, .. } => {
            let _ = abi;
            if *optional {
                format!(
                    "    const {slot} = {name} === undefined ? 0 : $stack.record(types.write{ts}, types.sizeOf{ts}, types.alignOf{ts}, {name});\n"
                )
            } else {
                format!(
                    "    const {slot} = $stack.record(types.write{ts}, types.sizeOf{ts}, types.alignOf{ts}, {name});\n"
                )
            }
        }
    })
}

/// How much room one result needs.
fn output_reserve(output: &Output, abi: &Abi) -> Result<(usize, usize), String> {
    Ok(match output {
        Output::Boolean { .. } => (1, 1),
        Output::Number { kind, .. } => {
            let size = size_of(kind, abi)?;
            (size.size, size.alignment)
        }
        Output::Enumeration { .. } | Output::Document { .. } => (4, 4),
        Output::Text { .. } | Output::Bytes { .. } => (8, 4),
        Output::Node { .. } => (8, 4),
        Output::Record { ts, .. } => {
            let size = size_of(&Type::Named(format!("Otio{ts}")), abi)?;
            (size.size, size.alignment)
        }
    })
}

/// The expression that reads one result back out of the module's memory.
fn read_output(output: &Output, at: &str) -> Result<String, String> {
    Ok(match output {
        Output::Boolean { .. } => format!("$stack.view.getUint8({at}) !== 0"),
        Output::Number { kind, .. } => match kind {
            Type::F64 => format!("$stack.view.getFloat64({at}, true)"),
            Type::I32 => format!("$stack.view.getInt32({at}, true)"),
            Type::U32 | Type::Usize => format!("$stack.view.getUint32({at}, true)"),
            Type::I64 => format!("Number($stack.view.getBigInt64({at}, true))"),
            Type::U64 => format!("Number($stack.view.getBigUint64({at}, true))"),
            other => return Err(format!("no rule reads a `{other}` result")),
        },
        Output::Enumeration { ts, .. } => {
            format!("types.decode{ts}($stack.view.getInt32({at}, true))")
        }
        Output::Text { .. } => format!("readBuffer({at}, \"text\")"),
        Output::Bytes { .. } => format!("readBuffer({at}, \"bytes\")"),
        Output::Node { .. } => format!("types.readNodeHandle($stack.view, {at})"),
        Output::Document { .. } => format!("$stack.view.getUint32({at}, true)"),
        Output::Record { ts, .. } => format!("types.read{ts}($stack.view, {at})"),
    })
}

/// The expression that turns a directly returned value into TypeScript.
fn read_direct(kind: &Type, call: &str, abi: &Abi) -> Result<String, String> {
    Ok(match kind {
        Type::Void => call.to_string(),
        Type::Bool => format!("{call} !== 0"),
        Type::F64 | Type::I32 | Type::U32 | Type::Usize => call.to_string(),
        Type::I64 | Type::U64 => format!("Number({call})"),
        Type::Pointer { inner, .. } if inner.named() == Some("OtioDocument") => call.to_string(),
        Type::Named(named) if abi.enumeration(named).is_some() => {
            format!("types.decode{}({call})", ts_name(named))
        }
        Type::Pointer { inner, .. } if **inner == Type::Char => format!("readCString({call})"),
        other => return Err(format!("no rule returns a `{other}`")),
    })
}

/// Writes `ts/src/generated/api.ts`: the classes a user of the SDK touches.
///
/// The shape is upstream OpenTimelineIO's, because that is the API the people
/// who will use this already know: the same class hierarchy, the same names
/// for the same ideas, `sourceRange` where Python says `source_range`. A
/// `Clip` is built on its own and put inside a `Track` afterwards, which is
/// what `otio_document_absorb` is for.
///
/// # Errors
///
/// Fails on a member the emitter has no rule for.
pub fn api(abi: &Abi, sdk: &Sdk) -> Result<Artifact, String> {
    let mut text = preamble("//");
    text.push_str(
        "/**\n\
         \x20* The object model, as classes.\n\
         \x20*\n\
         \x20* The hierarchy and the names are upstream OpenTimelineIO's, so someone\n\
         \x20* who knows its Python or C++ API knows this one. What is not upstream's\n\
         \x20* is the arena underneath: every object lives in a document, and an\n\
         \x20* object built on its own gets a document of its own until it is put\n\
         \x20* inside something, at which point it moves into that document. None of\n\
         \x20* that is visible here, which is the point of it.\n\
         \x20*/\n\n",
    );
    text.push_str(
        "import { adopt, bind, builders, deferred, place, register, Doc } from \"../objects.js\";\n",
    );
    text.push_str("import { metadataOf, type Metadata } from \"../metadata.js\";\n");
    text.push_str("import * as raw from \"./raw.js\";\n");
    text.push_str("import * as types from \"./types.js\";\n");
    text.push_str("import * as values from \"./values.js\";\n\n");

    let names = glossary(sdk);
    let mut building: Vec<&str> = Vec::new();
    for (name, base) in HIERARCHY {
        let class = sdk.classes.get(*name).cloned().unwrap_or_default();
        if class.statics.iter().any(|member| member.constructs) {
            building.push(name);
        }
        text.push_str(&one_class(name, *base, &class, abi, &names)?);
    }

    // The functions that belong to no class: the ten edit operations, the
    // algorithms, and the handful of things that are about the library rather
    // than about an object.
    let mut grouped: BTreeMap<Option<String>, Vec<&Member>> = BTreeMap::new();
    for member in &sdk.free {
        grouped
            .entry(member.namespace.clone())
            .or_default()
            .push(member);
    }
    for (namespace, members) in &grouped {
        match namespace {
            None => {
                for member in members {
                    text.push_str(&one_function(member, abi, &names, "")?);
                }
            }
            Some(namespace) => {
                let _ = writeln!(text, "{}", namespace_doc(namespace));
                let _ = writeln!(text, "export const {namespace} = {{");
                for member in members {
                    text.push_str(&one_function(member, abi, &names, "  ")?);
                }
                text.push_str("};\n\n");
            }
        }
    }

    // The registry the wrapper factory looks names up in. It is filled after
    // the classes exist, because a class cannot be referred to before it is
    // declared.
    text.push_str(
        "/*\n\
         \x20* Which class wraps each kind of object. `otio_node_kind` answers with\n\
         \x20* the kind and this turns it into the class, so a handle read out of a\n\
         \x20* document arrives as the right sort of thing without anyone asking.\n\
         \x20*/\n\
         register({\n",
    );
    let kinds = abi
        .enumeration("OtioNodeKind")
        .ok_or("the ABI has no OtioNodeKind")?;
    for variant in &kinds.variants {
        let class = HIERARCHY
            .iter()
            .map(|(name, _)| *name)
            .find(|name| *name == variant.name);
        let _ = writeln!(
            text,
            "  {}: {},",
            crate::sdk::emit::variant_name_of(&variant.name),
            class.unwrap_or("Node")
        );
    }
    text.push_str("});\n\n");

    // `new Clip()` runs `Item`'s constructor before `Clip`'s. Only the most
    // derived one should make an object, so each constructor asks whether a
    // class below it is going to, and this is the list it asks against.
    text.push_str(
        "/*\n\
         \x20* The classes whose constructor builds an object of its own.\n\
         \x20*/\n\
         builders([\n",
    );
    for name in &building {
        let _ = writeln!(text, "  {name},");
    }
    text.push_str("]);\n");

    Ok(Artifact {
        path: "ts/src/generated/api.ts".to_string(),
        text,
    })
}

/// Emits one class.
fn one_class(
    name: &str,
    base: Option<&str>,
    class: &crate::sdk::plan::Class,
    abi: &Abi,
    names: &BTreeMap<String, String>,
) -> Result<String, String> {
    let mut text = String::new();

    // A constructor's arguments become an options object, which is as close as
    // TypeScript gets to the keyword arguments upstream's Python uses.
    let constructor = class.statics.iter().find(|member| member.constructs);
    if let Some(constructor) = constructor {
        let _ = writeln!(text, "/** What a new {name} can be given. */");
        let _ = writeln!(text, "export interface {name}Options {{");
        for input in &constructor.inputs {
            if !input.optional() {
                let default = option_default(&constructor.symbol, input)?;
                let _ = writeln!(
                    text,
                    "  /** Defaults to `{}`. */",
                    default.replace("values.", "")
                );
            }
            let _ = writeln!(text, "  {}?: {};", input.name(), input_type(input));
        }
        text.push_str("}\n\n");
    }

    let doc = class_doc(name, constructor, abi);
    text.push_str(&tsdoc(&translate(&doc, names), ""));
    match base {
        Some(base) => {
            let _ = writeln!(text, "export class {name} extends {base} {{");
        }
        None => {
            let _ = writeln!(text, "export class {name} {{");
        }
    }

    if let Some(constructor) = constructor {
        let mut arguments: Vec<String> = Vec::new();
        for input in &constructor.inputs {
            let field = format!("options.{}", input.name());
            arguments.push(if input.optional() {
                field
            } else {
                format!("{field} ?? {}", option_default(&constructor.symbol, input)?)
            });
        }
        text.push_str(&tsdoc(&translate(&constructor.doc, names), "  "));
        let _ = writeln!(text, "  constructor(options: {name}Options = {{}}) {{");
        if base.is_some() {
            text.push_str("    super();\n");
        }
        let _ = writeln!(
            text,
            "    if (deferred(new.target, {name})) {{\n      /* A class below this one is building the object. */\n      return;\n    }}"
        );
        text.push_str("    const document = Doc.create();\n");
        let _ = writeln!(
            text,
            "    bind(this, document, raw.{}(document.pointer{}{}));",
            raw_name(&constructor.symbol),
            if arguments.is_empty() { "" } else { ", " },
            arguments.join(", ")
        );
        for (class, comment, statement) in AFTER_CONSTRUCTION {
            if *class != name {
                continue;
            }
            for line in comment.lines() {
                let _ = writeln!(text, "    // {}", line.trim());
            }
            let _ = writeln!(text, "    {statement}");
        }
        text.push_str("  }\n\n");
    } else if base.is_none() {
        text.push_str(
            "  /**\n\
             \x20  * Objects are not built this way: each concrete class has its own\n\
             \x20  * constructor, and a handle read out of a document arrives through\n\
             \x20  * the factory rather than through `new`.\n\
             \x20  *\n\
             \x20  * Where an object lives is recorded beside it, by `bind`, and not on\n\
             \x20  * it: a `Clip` has the API a clip has and nothing else.\n\
             \x20  *\n\
             \x20  * @internal\n\
             \x20  */\n\
             \x20 constructor() {\n\
             \x20   /* Nothing: `bind` records where this object is. */\n\
             \x20 }\n\n\
             \x20 /**\n\
             \x20  * The free-form metadata this object carries.\n\
             \x20  *\n\
             \x20  * A tree of dictionaries, arrays and values, addressed by path:\n\
             \x20  * `clip.metadata.get(\"cmx_3600.reel\")`. Adapters keep whatever the\n\
             \x20  * format they read said and OTIO has no field for in here, and it\n\
             \x20  * round-trips whether or not anything understands it.\n\
             \x20  */\n\
             \x20 get metadata(): Metadata {\n\
             \x20   return metadataOf(this);\n\
             \x20 }\n\n\
             \x20 /**\n\
             \x20  * Whether this and another wrapper name the same object.\n\
             \x20  *\n\
             \x20  * Reading the same object twice gives the same wrapper, so `===`\n\
             \x20  * agrees with this. It is here for the cases where a wrapper has\n\
             \x20  * been round-tripped through something that copied it.\n\
             \x20  */\n\
             \x20 equals(other: Node): boolean {\n\
             \x20   const mine = place(this);\n\
             \x20   const theirs = place(other);\n\
             \x20   return (\n\
             \x20     mine.doc.same(theirs.doc) &&\n\
             \x20     mine.handle.index === theirs.handle.index &&\n\
             \x20     mine.handle.generation === theirs.handle.generation\n\
             \x20   );\n\
             \x20 }\n\n\
             \x20 /**\n\
             \x20  * Releases the timeline this object belongs to, and everything in it.\n\
             \x20  *\n\
             \x20  * Not required: a timeline nothing refers to any more is released by\n\
             \x20  * the garbage collector, which is correct but late. This is for code\n\
             \x20  * that would rather say when — a viewer that opens files one after\n\
             \x20  * another, say. Anything that lived here throws afterwards.\n\
             \x20  *\n\
             \x20  * `using timeline = readFromBytes(...)` calls this at the end of the\n\
             \x20  * block, where the runtime supports it.\n\
             \x20  */\n\
             \x20 dispose(): void {\n\
             \x20   place(this).doc.dispose();\n\
             \x20 }\n\n\
             \x20 /** The same as `dispose`, for `using`. */\n\
             \x20 [Symbol.dispose](): void {\n\
             \x20   this.dispose();\n\
             \x20 }\n\n",
        );
    } else {
        // An abstract class in the middle of the hierarchy has no constructor
        // of its own, and TypeScript needs one that matches its base's.
        text.push_str("  /** @internal */\n  constructor() {\n    super();\n  }\n\n");
    }

    for property in &class.properties {
        text.push_str(&one_property(property, abi, names)?);
    }
    for member in &class.methods {
        text.push_str(&one_method(member, abi, false, names)?);
    }
    for member in &class.statics {
        if member.constructs {
            continue;
        }
        text.push_str(&one_method(member, abi, true, names)?);
    }

    text.push_str("}\n\n");
    Ok(text)
}

/// The comment introducing a group of free functions.
fn namespace_doc(namespace: &str) -> &'static str {
    match namespace {
        "edit" => {
            "/**\n             \x20* The ten edit operations.\n             \x20*\n             \x20* These are the moves an editor makes: insert, overwrite, trim, ripple,\n             \x20* roll, slip, slide, fill, remove and slice. They are functions rather\n             \x20* than methods because each one is about two objects and belongs to\n             \x20* neither, which is how upstream arranges them too.\n             \x20*/"
        }
        _ => {
            "/**\n             \x20* The algorithms that build one composition out of another.\n             \x20*\n             \x20* Each answers with something new rather than changing what it was\n             \x20* given.\n             \x20*/"
        }
    }
}

/// Emits a function belonging to no class.
fn one_function(
    member: &Member,
    abi: &Abi,
    names: &BTreeMap<String, String>,
    indent: &str,
) -> Result<String, String> {
    let parameters = parameter_list(member);

    let mut text = tsdoc(&translate(&member.doc, names), indent);
    let inner = format!("{indent}  ");
    if indent.is_empty() {
        let _ = writeln!(
            text,
            "export function {}({}): {} {{",
            member.name,
            parameters.join(", "),
            wrapped_type(member, abi)?
        );
    } else {
        let _ = writeln!(
            text,
            "{indent}{}({}): {} {{",
            member.name,
            parameters.join(", "),
            wrapped_type(member, abi)?
        );
    }
    text.push_str(&indented(&call(member)?, &inner));
    let _ = writeln!(
        text,
        "{indent}}}{}\n",
        if indent.is_empty() { "" } else { "," }
    );
    Ok(text)
}

/// The doc comment on a class, which the C ABI does not have one of.
fn class_doc(name: &str, constructor: Option<&Member>, abi: &Abi) -> Vec<String> {
    if let Some(constructor) = constructor {
        // The constructor's own comment describes the thing being made, which
        // is what a reader of the class wants.
        let mut doc = constructor.doc.clone();
        doc.push(String::new());
        doc.push(
            "An object built this way lives on its own until it is put inside \
             something else, at which point it moves into that object's timeline."
                .to_string(),
        );
        return doc;
    }
    // Otherwise the kind's own description, which `OtioNodeKind` carries.
    abi.enumeration("OtioNodeKind")
        .and_then(|kinds| kinds.variants.iter().find(|variant| variant.name == name))
        .map(|variant| variant.doc.clone())
        .unwrap_or_else(|| vec![format!("A {name}.")])
}

/// Emits a property: a reader, a writer, and the `undefined` that unsets it.
fn one_property(
    property: &crate::sdk::plan::Property,
    abi: &Abi,
    names: &BTreeMap<String, String>,
) -> Result<String, String> {
    let mut text = tsdoc(&translate(&property.getter.doc, names), "  ");
    let optional = property.clear.is_some();
    let read = wrapped_type(&property.getter, abi)?;
    let _ = writeln!(text, "  get {}(): {read} {{", property.name);
    text.push_str(&indented(&call(&property.getter)?, "    "));
    text.push_str("  }\n\n");

    let Some(setter) = &property.setter else {
        return Ok(text);
    };
    let written = api_input_type(&setter.inputs[0], &setter.symbol);
    text.push_str(&tsdoc(&translate(&setter.doc, names), "  "));
    let _ = writeln!(
        text,
        "  set {}(value: {written}{}) {{",
        property.name,
        if optional || setter.inputs[0].optional() {
            " | undefined"
        } else {
            ""
        }
    );
    if let Some(clear) = &property.clear {
        text.push_str("    if (value === undefined) {\n");
        text.push_str(&indented(&call(clear)?, "      "));
        text.push_str("      return;\n    }\n");
    }
    text.push_str(&indented(
        &call_with(setter, &["value".to_string()])?,
        "    ",
    ));
    text.push_str("  }\n\n");
    Ok(text)
}

/// Emits a method.
fn one_method(
    member: &Member,
    abi: &Abi,
    statik: bool,
    names: &BTreeMap<String, String>,
) -> Result<String, String> {
    let parameters = parameter_list(member);

    let mut text = tsdoc(&translate(&member.doc, names), "  ");
    let _ = writeln!(
        text,
        "  {}{}({}): {} {{",
        if statik { "static " } else { "" },
        member.name,
        parameters.join(", "),
        wrapped_type(member, abi)?
    );
    text.push_str(&indented(&call(member)?, "    "));
    text.push_str("  }\n\n");
    Ok(text)
}

/// The parameter list of a class member, as TypeScript declares it.
///
/// An optional argument is written `name?` only where nothing required comes
/// after it, because TypeScript will not take a required parameter behind an
/// optional one. Where one does, the argument keeps its place and says
/// `| undefined` instead, which is the same thing to a caller who passes
/// `undefined` and keeps the C ABI's order intact.
fn parameter_list(member: &Member) -> Vec<String> {
    let last_required = member.inputs.iter().rposition(|input| !input.optional());
    member
        .inputs
        .iter()
        .enumerate()
        .map(|(index, input)| {
            let name = input.name();
            let kind = api_input_type(input, &member.symbol);
            if !input.optional() {
                return format!("{name}: {kind}");
            }
            match last_required {
                Some(required) if index < required => format!("{name}: {kind} | undefined"),
                _ => format!("{name}?: {kind}"),
            }
        })
        .collect()
}

/// The type a class member takes, which is an object where the low-level
/// layer takes a handle.
fn api_input_type(input: &Input, symbol: &str) -> String {
    let class = node_class(symbol);
    match input {
        Input::Node { .. } => class.to_string(),
        Input::NodeList { .. } => format!("readonly {class}[]"),
        other => input_type(other),
    }
}

/// The type a class member answers with, which wraps handles as objects.
fn wrapped_type(member: &Member, abi: &Abi) -> Result<String, String> {
    let raw = result_type(member, abi)?;
    Ok(raw.replace("types.NodeHandle", node_class(&member.symbol)))
}

/// Emits the call into the low-level layer, wrapping handles on the way back.
///
/// Returns the body's lines, unindented; the caller indents them.
fn call(member: &Member) -> Result<Vec<String>, String> {
    let arguments: Vec<String> = member
        .inputs
        .iter()
        .map(|input| input.name().to_string())
        .collect();
    call_with(member, &arguments)
}

/// The same, with the arguments named explicitly.
fn call_with(member: &Member, arguments: &[String]) -> Result<Vec<String>, String> {
    let mut lines = Vec::new();
    let mut leading = Vec::new();
    match &member.receiver {
        Receiver::None => {}
        Receiver::Value(_) => leading.push("this".to_string()),
        // A free function that edits a timeline finds it through its subject,
        // which is what keeps the document out of its argument list.
        Receiver::Borrowed(subject) => {
            lines.push(format!("const at = place({subject});"));
            leading.push("at.document".to_string());
        }
        Receiver::Document => {
            lines.push("const at = place(this);".to_string());
            leading.push("at.document".to_string());
        }
        Receiver::Node => {
            lines.push("const at = place(this);".to_string());
            leading.push("at.document".to_string());
            leading.push("at.handle".to_string());
        }
    }

    let mut passed = Vec::new();
    for (input, argument) in member.inputs.iter().zip(arguments.iter()) {
        // An object handed to a call that edits has to be in the same document,
        // so it moves there first. One handed to a question does not move:
        // absorbing a whole timeline because somebody asked whether it held
        // something would be a surprise.
        let bring = if member.mutates { "adopt" } else { "handleOf" };
        passed.push(match input {
            Input::Node { optional: true, .. } => {
                format!("{argument} === undefined ? undefined : at.doc.{bring}({argument})")
            }
            Input::Node { .. } => format!("at.doc.{bring}({argument})"),
            Input::NodeList { .. } => {
                format!("{argument}.map((each) => at.doc.{bring}(each))")
            }
            _ => argument.clone(),
        });
    }

    let call = format!(
        "raw.{}({})",
        raw_name(&member.symbol),
        leading
            .into_iter()
            .chain(passed)
            .collect::<Vec<_>>()
            .join(", ")
    );

    // A handle is two integers; the class it names is what a caller wants.
    let class = node_class(&member.symbol);
    let nodes: Vec<&Output> = member
        .outputs
        .iter()
        .filter(|output| matches!(output, Output::Node { .. }))
        .collect();
    if nodes.is_empty() {
        let answers = !member.outputs.is_empty()
            || (!member.fallible && member.returns != crate::sdk::abi::Type::Void);
        if answers {
            lines.push(format!("return {call};"));
        } else {
            lines.push(format!("{call};"));
        }
        return Ok(lines);
    }

    match member.outputs.as_slice() {
        [Output::Node { .. }] if member.list => {
            lines.push(format!(
                "return {call}.map((handle) => adopt<{class}>(at.doc, handle));"
            ));
        }
        [Output::Node { .. }] if member.no_value => {
            lines.push(format!("const handle = {call};"));
            lines.push(format!(
                "return handle === undefined ? undefined : adopt<{class}>(at.doc, handle);"
            ));
        }
        [Output::Node { .. }] => {
            lines.push(format!("return adopt<{class}>(at.doc, {call});"));
        }
        several => {
            lines.push(format!("const found = {call};"));
            lines.push("return {".to_string());
            for output in several {
                let field = crate::sdk::plan::camel(output.name());
                let wrapped = match (output, member.list, member.no_value) {
                    (Output::Node { .. }, true, _) => {
                        format!("found.{field}.map((handle) => adopt<{class}>(at.doc, handle))")
                    }
                    (Output::Node { .. }, false, _) => {
                        format!("adopt<{class}>(at.doc, found.{field})")
                    }
                    _ => format!("found.{field}"),
                };
                lines.push(format!("  {field}: {wrapped},"));
            }
            lines.push("};".to_string());
        }
    }
    Ok(lines)
}

/// Indents a body's lines and joins them.
fn indented(lines: &[String], indent: &str) -> String {
    let mut text = String::new();
    for line in lines {
        let _ = writeln!(text, "{indent}{line}");
    }
    text
}

/// What a constructor does after the C call, where upstream's own constructor
/// does more than `otio_*_new` does.
///
/// The C ABI builds exactly the object it is asked for and nothing else, which
/// is right for C and one step short of what upstream's Python and C++
/// constructors do. Rather than change the ABI under the other bindings, the
/// difference is made up here, once per class, in the open.
const AFTER_CONSTRUCTION: &[(&str, &str, &str)] = &[(
    "Timeline",
    "Upstream's `Timeline()` comes with an empty stack called `tracks`, and\n     enough code assumes one that a timeline without it is a trap.",
    "this.tracks = new Stack({ name: \"tracks\" });",
)];

/// What a constructor passes for an option nobody gave it.
///
/// Upstream's Python gives every constructor argument a default, and this is
/// the same list for the handful of C parameters that are not optional. A
/// record defaults to its own empty value by convention; anything else needs
/// an entry here, and a constructor whose argument has neither stops the
/// generator rather than guessing.
const CONSTRUCTOR_DEFAULTS: &[(&str, &str, &str)] = &[
    // Upstream's `LinearTimeWarp(time_scalar=1.0)`: a warp nobody has set is
    // one that changes nothing.
    ("otio_linear_time_warp_new", "timeScalar", "1"),
];

/// The expression a constructor uses for an option it was not given.
fn option_default(symbol: &str, input: &Input) -> Result<String, String> {
    if let Some((.., expression)) = CONSTRUCTOR_DEFAULTS
        .iter()
        .find(|(function, field, _)| *function == symbol && *field == input.name())
    {
        return Ok((*expression).to_string());
    }
    match input {
        Input::Record { ts, .. } => Ok(format!("new values.{ts}()")),
        Input::Boolean { .. } => Ok("false".to_string()),
        other => Err(format!(
            "`{symbol}` has to be given `{}`, and no rule says what it should be \
             when nobody does; add it to CONSTRUCTOR_DEFAULTS",
            other.name()
        )),
    }
}

/// The default each value class's constructor gives a field it is not given.
///
/// These are the core's own defaults, from `impl Default`, so a
/// `new RationalTime()` here is the same thing as a `RationalTime()` in
/// upstream's Python.
const DEFAULTS: &[(&str, &[&str])] = &[
    ("RationalTime", &["0", "1"]),
    ("TimeRange", &["new RationalTime()", "new RationalTime()"]),
    ("TimeTransform", &["new RationalTime()", "1", "-1"]),
];

/// Writes `ts/src/generated/values.ts`: the time types, as classes.
///
/// They are values, not objects in a document: a `RationalTime` is two
/// numbers, so it has no handle, no arena and nothing to release. Every method
/// on them is a pure function of its arguments, which is why they are the one
/// part of the SDK that works the same whether or not anything has been loaded
/// from a file.
///
/// # Errors
///
/// Fails on a member the emitter has no rule for.
pub fn values(abi: &Abi, sdk: &Sdk) -> Result<Artifact, String> {
    let mut text = preamble("//");
    text.push_str(
        "/**\n\
         \x20* Rational time, time spans, and the transform between them.\n\
         \x20*\n\
         \x20* This is OpenTimelineIO's `opentime`, with upstream's names: a\n\
         \x20* `RationalTime` is a value over a rate, a `TimeRange` is a start and a\n\
         \x20* duration rather than a start and an end, and the arithmetic rounds the\n\
         \x20* way upstream's does, because the same file has to open the same way in\n\
         \x20* every tool that reads it.\n\
         \x20*\n\
         \x20* Each class has a `…Like` interface beside it holding just the fields, so\n\
         \x20* anywhere the SDK takes a time you can pass a plain object:\n\
         \x20* `clip.sourceRange = { startTime, duration }` works.\n\
         \x20*/\n\n",
    );
    text.push_str("import * as raw from \"./raw.js\";\n");
    text.push_str("import * as types from \"./types.js\";\n\n");

    let names = glossary(sdk);
    for name in VALUES {
        let otio = format!("Otio{name}");
        let record = abi
            .record(&otio)
            .ok_or_else(|| format!("`{otio}` is not a struct of the ABI"))?;
        let class = sdk.classes.get(*name).cloned().unwrap_or_default();
        let defaults = DEFAULTS
            .iter()
            .find(|(class, _)| class == name)
            .map(|(_, defaults)| *defaults)
            .ok_or_else(|| format!("`{name}` has no declared defaults"))?;
        if defaults.len() != record.fields.len() {
            return Err(format!(
                "`{name}` has {} fields but {} defaults",
                record.fields.len(),
                defaults.len()
            ));
        }

        let field_type = |kind: &Type| match kind {
            Type::F64 => "number".to_string(),
            other => super::qualified(other.named().unwrap_or_default()).replace("values.", ""),
        };

        let _ = writeln!(
            text,
            "/** The fields of a {name}, for anywhere one can be passed as a plain object. */"
        );
        let _ = writeln!(text, "export interface {name}Like {{");
        for field in &record.fields {
            text.push_str(&tsdoc(&translate(&field.doc, &names), "  "));
            let mut kind = field_type(&field.kind);
            if kind.ends_with("Like") {
                // A nested time may itself be given as a plain object.
            } else if VALUES.contains(&kind.as_str()) {
                kind = format!("{kind}Like");
            }
            let _ = writeln!(text, "  readonly {}: {kind};", camel(&field.name));
        }
        text.push_str("}\n\n");

        text.push_str(&tsdoc(&translate(&record.doc, &names), ""));
        let _ = writeln!(text, "export class {name} implements {name}Like {{");
        for field in &record.fields {
            text.push_str(&tsdoc(&translate(&field.doc, &names), "  "));
            let _ = writeln!(
                text,
                "  readonly {}: {};",
                camel(&field.name),
                field_type(&field.kind)
            );
        }
        text.push('\n');

        let parameters: Vec<String> = record
            .fields
            .iter()
            .zip(defaults)
            .map(|(field, default)| {
                let mut kind = field_type(&field.kind);
                if VALUES.contains(&kind.as_str()) {
                    kind = format!("{kind}Like");
                }
                format!("{}: {kind} = {default}", camel(&field.name))
            })
            .collect();
        let _ = writeln!(text, "  constructor({}) {{", parameters.join(", "));
        for field in &record.fields {
            let name = camel(&field.name);
            let kind = field_type(&field.kind);
            if VALUES.contains(&kind.as_str()) {
                // A plain object is accepted and kept as the class, so that
                // what comes back out of a `RationalTime` is always one.
                let _ = writeln!(
                    text,
                    "    this.{name} = {name} instanceof {kind} ? {name} : new {kind}({});",
                    DEFAULTS
                        .iter()
                        .find(|(class, _)| *class == kind)
                        .map(|(_, _)| ())
                        .map_or_else(String::new, |()| fields_of(abi, &format!("Otio{kind}"))
                            .into_iter()
                            .map(|field| format!("{name}.{field}"))
                            .collect::<Vec<_>>()
                            .join(", "))
                );
            } else {
                let _ = writeln!(text, "    this.{name} = {name};");
            }
        }
        text.push_str("  }\n\n");

        // Inside this module the classes are in scope by their own names, so
        // the qualification the other generated files need is noise here.
        let mut members = String::new();
        for property in &class.properties {
            members.push_str(&one_property(property, abi, &names)?);
        }
        for member in &class.methods {
            members.push_str(&one_method(member, abi, false, &names)?);
        }
        for member in &class.statics {
            members.push_str(&one_method(member, abi, true, &names)?);
        }
        text.push_str(&members.replace("values.", ""));

        let _ = writeln!(
            text,
            "  /** A {name} reads as its own constructor call. */\n\
             \x20 toString(): string {{\n\
             \x20   return `{name}({})`;\n\
             \x20 }}\n",
            record
                .fields
                .iter()
                .map(|field| format!("${{this.{}}}", camel(&field.name)))
                .collect::<Vec<_>>()
                .join(", ")
        );
        text.push_str("}\n\n");
    }

    Ok(Artifact {
        path: "ts/src/generated/values.ts".to_string(),
        text,
    })
}

/// The field names of a struct, in order.
fn fields_of(abi: &Abi, otio: &str) -> Vec<String> {
    abi.record(otio).map_or_else(Vec::new, |record| {
        record
            .fields
            .iter()
            .map(|field| camel(&field.name))
            .collect()
    })
}
