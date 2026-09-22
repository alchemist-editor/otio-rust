//! The Swift SDK.
//!
//! Swift is the second target, and the first one with a type system rich
//! enough to say most of what the C ABI means. Where Go has to hand back a
//! sentinel error for "there is nothing here", Swift has an optional; where
//! Go has one `Node` type and an `AsClip` method, Swift has a class per
//! schema and `as? Clip`; where Go returns `(T, error)`, Swift throws.
//!
//! The shape follows OpenTimelineIO's own Swift bindings, which are the
//! reference for what a Swift programmer expects an OTIO API to look like:
//!
//! - **A class per schema, deriving as the schemas derive.** `Clip` is an
//!   `Item` is a `Composable` is a `SerializableObjectWithMetadata` is a
//!   `SerializableObject`, so a `Clip` has every method of each, and
//!   `as? Clip` asks what an object really is. Every handle that comes back
//!   from the library is built as the class its schema names, so the
//!   downcast tells the truth.
//! - **Values are structs**: `RationalTime`, `TimeRange`, `TimeTransform`,
//!   `Equatable` and `Hashable` as upstream's are.
//! - **Failure is `throws`**, with one `OTIOError` carrying a `Status`, as
//!   upstream throws one `OTIOError` carrying its own status enum.
//! - **`OTIO_STATUS_NO_VALUE` is an optional**, not an error. An item with no
//!   source range answers `nil`, which is what upstream's
//!   `sourceRange: TimeRange?` says too.
//! - **Real enums**, with the C interface's own values.
//! - **Compositions are not collections**, as upstream deliberately leaves
//!   them: `children()` plus throwing `append`/`insert`/`remove`, because
//!   re-parenting can fail and has side effects.
//!
//! Where it departs from upstream it is because the arena underneath says
//! something upstream's reference counting does not, and every departure is
//! written down in `docs/adr/0003-sdk-generation.md`.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::PathBuf;

use otio_sdk_model::model::{
    Api, CResult, Docs, Enum, Function, Group, Param, ParamRole, Receiver, Role, Struct, Type,
};
use otio_sdk_model::names;

use crate::emit::File;

/// Where the Swift package lives, relative to the workspace root.
const DIR: &str = "sdk/swift";

/// Where the generated Swift sources go inside it.
const SOURCES: &str = "Sources/OpenTimelineIO";

/// The root of the OTIO schema ladder, which Swift spells as upstream does.
const ROOT: &str = "SerializableObject";

/// Four spaces, which is what Swift indents with.
const TAB: &str = "    ";

/// Generates every file of the Swift package.
///
/// # Errors
///
/// Fails if two calls would end up with the same selector on one Swift type,
/// or if a call cannot be written mechanically and is not hand-written here.
pub fn generate(api: &Api) -> Result<Vec<File>, String> {
    let backend = Backend::new(api);
    backend.check_names()?;
    Ok(vec![
        plain("Package.swift", PACKAGE),
        plain("Sources/COtio/module.modulemap", MODULE_MAP),
        plain("lib/.gitignore", LIB_GITIGNORE),
        plain("README.md", README),
        backend.assemble("Runtime.swift", backend.runtime()?),
        backend.assemble("Enums.swift", backend.enums()),
        backend.assemble("Values.swift", backend.values()?),
        backend.assemble("Schema.swift", backend.schema()),
        backend.assemble("Objects.swift", backend.objects()?),
        backend.assemble("Documents.swift", backend.documents()?),
        backend.assemble("Metadata.swift", backend.metadata()?),
    ])
}

/// A file whose contents do not depend on the description.
fn plain(name: &str, contents: &str) -> File {
    File {
        path: PathBuf::from(DIR).join(name),
        contents: contents.to_string(),
    }
}

/// The state a backend carries while it writes.
struct Backend<'a> {
    api: &'a Api,
    /// How each interface symbol is spelled in Swift, for the documentation.
    spellings: BTreeMap<String, String>,
}

impl<'a> Backend<'a> {
    fn new(api: &'a Api) -> Self {
        let mut spellings = BTreeMap::new();
        for group in &api.groups {
            for function in &group.functions {
                spellings.insert(function.symbol.clone(), member_name(group, function));
            }
        }
        for item in &api.enums {
            let swift = enum_name(&item.name);
            for variant in &item.variants {
                spellings.insert(
                    variant.c_name.clone(),
                    format!("{swift}.{}", variant_name(&variant.name)),
                );
            }
        }
        Self { api, spellings }
    }

    /// Fails if two calls would land on one Swift type with the same
    /// selector.
    ///
    /// Swift tells two members apart by their argument labels as well as
    /// their name, so `rescaled(to:)` and `rescaled(by:)` live together
    /// happily where Go would have had to rename one. Only a full selector
    /// clashes — and it clashes on a subclass too, since a method declared on
    /// `Item` is reachable on a `Clip`.
    fn check_names(&self) -> Result<(), String> {
        let mut placed: Vec<(String, String, String)> = Vec::new();
        for (owner, selector) in RESERVED {
            placed.push(((*owner).to_string(), (*selector).to_string(), String::new()));
        }
        for item in &self.api.structs {
            if item.plumbing || item.name == "OtioNode" {
                continue;
            }
            // A value struct's own fields are members of it too, and a
            // getter named after one would be a redeclaration.
            for field in &item.fields {
                placed.push((
                    value_name(&item.name),
                    names::camel(&field.name),
                    String::new(),
                ));
            }
        }
        let mut clashes = Vec::new();
        for group in &self.api.groups {
            for function in &group.functions {
                if self.skipped(function) {
                    continue;
                }
                let owner = self.owner_of(group, function);
                let selector = self.selector_of(group, function);
                for (other_owner, other_selector, other_symbol) in &placed {
                    if *other_selector != selector || !self.meet(&owner, other_owner) {
                        continue;
                    }
                    let first = if other_symbol.is_empty() {
                        format!("the hand-written {other_owner}.{other_selector}")
                    } else {
                        format!("`{other_symbol}`")
                    };
                    clashes.push(format!(
                        "{first} and `{}` are both {selector} on {owner}",
                        function.symbol
                    ));
                }
                placed.push((owner, selector, function.symbol.clone()));
            }
        }
        if clashes.is_empty() {
            return Ok(());
        }
        Err(format!(
            "the Swift names collide:\n  {}\n\nGive one of each pair another name in \
             `otio-sdk-model/src/overrides.rs`.",
            clashes.join("\n  ")
        ))
    }

    /// Whether this backend leaves a call out of the generated surface.
    fn skipped(&self, function: &Function) -> bool {
        let _ = self;
        matches!(function.role, Role::Plumbing | Role::Destructor)
            || BY_HAND.contains(&function.symbol.as_str())
    }

    /// Whether two owners are places a caller could reach the same selector
    /// from, which for a class is anywhere on its line of descent.
    fn meet(&self, left: &str, right: &str) -> bool {
        if left == right {
            return true;
        }
        let (Some(one), Some(other)) =
            (left.strip_prefix("object:"), right.strip_prefix("object:"))
        else {
            return false;
        };
        self.api
            .ancestry(one)
            .iter()
            .any(|schema| schema.name == other)
            || self
                .api
                .ancestry(other)
                .iter()
                .any(|schema| schema.name == one)
    }

    /// The Swift type a call hangs off.
    fn owner_of(&self, group: &Group, function: &Function) -> String {
        let _ = self;
        match (&group.receiver, function.role) {
            (Receiver::None, _) => "OTIO".to_string(),
            (Receiver::Document, _) => "Document".to_string(),
            (Receiver::Node(_), Role::Constructor) => {
                if takes_a_document(function) {
                    "Document".to_string()
                } else {
                    format!("object:{ROOT}")
                }
            }
            (Receiver::Node(schema), _) => {
                if group.view {
                    group.name.clone()
                } else {
                    format!("object:{schema}")
                }
            }
            (Receiver::Value(what), _) => value_name(what),
        }
    }

    /// The full Swift selector a call is reached by, labels included.
    fn selector_of(&self, group: &Group, function: &Function) -> String {
        let name = member_name(group, function);
        if is_property(group, function) {
            return name;
        }
        let labels: String = labels_of(function)
            .into_iter()
            .map(|label| format!("{label}:"))
            .collect();
        format!("{name}({labels})")
    }
}

/// Whether a call is given a document to work in.
fn takes_a_document(function: &Function) -> bool {
    function
        .params
        .iter()
        .any(|param| matches!(param.role, ParamRole::DocumentIn | ParamRole::DocumentMut))
}

/// Whether a call is spelled as a property rather than as a method.
///
/// The rule upstream's bindings follow is that a stored field is a property
/// and anything computed or fallible is a method. Under an arena almost every
/// read of an object is fallible — the handle may name something that has
/// been removed — and Swift has no throwing property, so the rule lands
/// differently here than it does upstream: a getter is a property only where
/// the C ABI says the call cannot fail, which is to say on the value structs
/// and the enums.
fn is_property(group: &Group, function: &Function) -> bool {
    function.role == Role::Getter
        && !function.fallible()
        && function.inputs().count() == 0
        && !matches!(group.receiver, Receiver::None)
}

/// The labels a call's arguments carry.
///
/// Swift's convention is that the first argument is usually unlabelled,
/// because the method's own name already says what it takes, and the rest are
/// labelled. That gives `range.overlaps(other, epsilon: 0.5)` and
/// `clip.setMediaReference(reference)`, which is how upstream spells the same
/// calls.
fn labels_of(function: &Function) -> Vec<String> {
    let mut labels = Vec::new();
    for (index, param) in function.inputs().enumerate() {
        if index == 0 {
            labels.push("_".to_string());
        } else {
            labels.push(parameter_name(&param.name));
        }
    }
    labels
}

/// What a call is called in Swift.
fn member_name(group: &Group, function: &Function) -> String {
    let spelled = names::camel(&function.name);
    match (&group.receiver, function.role) {
        (Receiver::Node(schema), Role::Constructor) if takes_a_document(function) => {
            let class = schema_name(schema);
            if function.name == "new" {
                format!("new{class}")
            } else {
                format!("new{class}{}", names::pascal(&function.name))
            }
        }
        _ => spelled,
    }
}

/// The Swift name of a schema, which is upstream's own.
fn schema_name(schema: &str) -> String {
    schema.to_string()
}

/// The Swift name of an enum: `OtioNodeKind` becomes `NodeKind`.
fn enum_name(c_name: &str) -> String {
    names::respell(
        c_name.strip_prefix("Otio").unwrap_or(c_name),
        names::INITIALISMS,
    )
}

/// The Swift name of a value struct: `OtioRationalTime` becomes
/// `RationalTime`.
fn value_name(c_name: &str) -> String {
    enum_name(c_name)
}

/// The Swift name of an enum's case.
///
/// The C ABI's `InvalidUtf8` becomes `invalidUTF8`: Swift lowercases a
/// leading initialism in full and capitalises one anywhere else, which is
/// what `names::camel` does for a `snake_case` name and what this does for a
/// `PascalCase` one.
fn variant_name(pascal: &str) -> String {
    let words = names::split_pascal(pascal);
    let mut out = String::new();
    for (index, word) in words.iter().enumerate() {
        let lower = word.to_lowercase();
        if index == 0 {
            out.push_str(&lower);
        } else if names::INITIALISMS.contains(&lower.as_str()) {
            out.push_str(&lower.to_uppercase());
        } else {
            let mut characters = word.chars();
            if let Some(first) = characters.next() {
                out.push_str(&first.to_uppercase().collect::<String>());
                out.push_str(characters.as_str());
            }
        }
    }
    out
}

/// The Swift type a value has.
fn swift_type(ty: &Type) -> String {
    match ty {
        Type::Bool => "Bool".to_string(),
        Type::Double => "Double".to_string(),
        Type::Int64 => "Int64".to_string(),
        Type::Uint64 => "UInt64".to_string(),
        Type::Int32 => "Int32".to_string(),
        Type::Uint32 => "UInt32".to_string(),
        // Swift counts and indexes with `Int`, whatever C does.
        Type::Size => "Int".to_string(),
        Type::Text => "String".to_string(),
        Type::Bytes => "[UInt8]".to_string(),
        Type::Node => ROOT.to_string(),
        Type::Document => "Document".to_string(),
        Type::Struct(name) => value_name(name),
        Type::Enum(name) => enum_name(name),
        Type::List(inner) => format!("[{}]", swift_type(inner)),
    }
}

/// The C type a value crosses the boundary as, spelled the way Swift imports
/// it.
fn c_type(ty: &Type) -> String {
    match ty {
        Type::Bool => "Bool".to_string(),
        Type::Double => "Double".to_string(),
        Type::Int64 => "Int64".to_string(),
        Type::Uint64 => "UInt64".to_string(),
        Type::Int32 => "Int32".to_string(),
        Type::Uint32 => "UInt32".to_string(),
        Type::Size => "Int".to_string(),
        Type::Text | Type::Bytes => "OtioBuffer".to_string(),
        Type::Node => "OtioNode".to_string(),
        Type::Document => "OpaquePointer?".to_string(),
        Type::Struct(name) | Type::Enum(name) => name.clone(),
        Type::List(inner) => c_type(inner),
    }
}

/// An empty value of a C type, for an out-parameter waiting to be filled.
fn c_empty(api: &Api, ty: &Type) -> Result<String, String> {
    Ok(match ty {
        Type::Bool => "false".to_string(),
        Type::Double => "0".to_string(),
        Type::Int64 | Type::Uint64 | Type::Int32 | Type::Uint32 | Type::Size => "0".to_string(),
        Type::Text | Type::Bytes => "OtioBuffer()".to_string(),
        Type::Node => "OtioNode()".to_string(),
        Type::Document => "nil".to_string(),
        Type::Struct(name) => format!("{name}()"),
        Type::Enum(name) => {
            // A C enum imports as a struct on one Swift version and as an
            // enum on another, and neither has a zero-argument initializer,
            // so an out-parameter starts as the first value the enum declares.
            let item = api
                .enumeration(name)
                .ok_or_else(|| format!("`{name}` is not an enum this library has"))?;
            let first = item
                .variants
                .first()
                .ok_or_else(|| format!("`{name}` has no variants"))?;
            format!("cEnum({}, {name}.self)", first.value)
        }
        Type::List(_) => return Err("a list has no empty value of its own".to_string()),
    })
}

/// The value a call answers with when it is asked about an object from
/// another document and cannot report the mistake.
fn swift_zero(ty: &Type) -> Result<String, String> {
    Ok(match ty {
        Type::Bool => "false".to_string(),
        Type::Double | Type::Int64 | Type::Uint64 | Type::Int32 | Type::Uint32 | Type::Size => {
            "0".to_string()
        }
        Type::Text => "\"\"".to_string(),
        Type::Bytes | Type::List(_) => "[]".to_string(),
        other => {
            return Err(format!(
                "a call that cannot fail answers with a `{}`, which has no value to stand for \
                 an object from another document",
                other.c_name()
            ));
        }
    })
}

/// Turns a Swift value into the C one a call wants.
fn to_c(ty: &Type, value: &str) -> String {
    match ty {
        Type::Node => format!("{value}.handle"),
        Type::Enum(name) => format!("cEnum({value}.rawValue, {name}.self)"),
        _ => value.to_string(),
    }
}

/// Turns the C value a call gave back into a Swift one.
///
/// `owner` is the document a handle belongs to, since an object in Swift
/// carries the document it can be resolved against rather than making its
/// caller remember.
fn from_c(ty: &Type, value: &str, owner: &str) -> String {
    match ty {
        Type::Node => format!("makeObject({owner}, {value})"),
        Type::Document => format!("Document(owning: {value})"),
        Type::Text => format!("swiftText({value})"),
        Type::Bytes => format!("swiftBytes({value})"),
        Type::Enum(name) => format!("enumValue({value}, {}.self)", enum_name(name)),
        Type::Struct(name) => format!("{}({value})", value_name(name)),
        _ => value.to_string(),
    }
}

/// A run of generated lines, kept at the right indentation as closures open
/// and close around the call.
struct Lines {
    out: Vec<String>,
    depth: usize,
}

impl Lines {
    fn new() -> Self {
        Self {
            out: Vec::new(),
            depth: 0,
        }
    }

    fn push(&mut self, line: &str) {
        if line.is_empty() {
            self.out.push(String::new());
        } else {
            self.out.push(format!("{}{line}", TAB.repeat(self.depth)));
        }
    }

    fn open(&mut self, line: &str) {
        self.push(line);
        self.depth += 1;
    }

    fn close(&mut self) {
        self.depth -= 1;
        self.push("}");
    }
}

/// A closure the call has to happen inside: a borrowed C string, a struct
/// lent as a pointer, a buffer a list is read into.
struct Scope {
    /// Lines that have to come before the closure opens.
    before: Vec<String>,
    /// The expression the closure is passed to.
    call: String,
    /// The closure's parameter, with its type, or empty for none.
    binding: String,
}

/// One call, being written into one place.
struct Site<'a> {
    api: &'a Api,
    function: &'a Function,
    /// The Swift expression for the document pointer the call works in.
    document: String,
    /// The Swift expression for the handle or value the call is about.
    receiver: String,
    /// The Swift expression for the document anything handed back belongs to.
    owner: String,
    /// The class a constructor's handle should be handed back as, so that
    /// `document.newClip` answers with a `Clip` rather than a bare object.
    wrap: Option<String>,
}

/// A call, written out.
struct Rendered {
    /// The Swift parameters, with their labels and defaults.
    params: Vec<String>,
    /// What the call answers with, or the empty string for nothing.
    result: String,
    /// Whether it throws.
    throwing: bool,
    /// The lines of the body, at no indentation.
    body: Vec<String>,
}

impl Site<'_> {
    /// Writes the call out.
    #[allow(clippy::too_many_lines)]
    fn render(&self) -> Result<Rendered, String> {
        let function = self.function;
        let throwing = function.fallible();
        let mut params: Vec<String> = Vec::new();
        let mut args: Vec<String> = Vec::new();
        let mut scopes: Vec<Scope> = Vec::new();
        let mut pre: Vec<String> = Vec::new();
        let mut frees: Vec<String> = Vec::new();
        let mut results: Vec<(String, String, String)> = Vec::new();
        let mut lists: Vec<(String, String, Type)> = Vec::new();
        let mut length: Option<String> = None;
        // Each is the throwing check and the plain question it asks, since
        // a call that cannot fail has no way to report the mistake.
        let mut guarded: Vec<(String, String)> = Vec::new();
        let mut labelled = 0usize;

        if takes_a_document(function) && self.owner != "nil" {
            // ARC may release the last reference to a document at its last
            // use, which is the line that reads its pointer. The call has to
            // happen while it is still alive.
            scopes.push(Scope {
                before: Vec::new(),
                call: format!("withExtendedLifetime({})", self.owner),
                binding: String::new(),
            });
        }

        for param in &function.params {
            let local = format!("c{}", names::pascal(&param.name));
            match param.role {
                ParamRole::DocumentIn | ParamRole::DocumentMut => args.push(self.document.clone()),
                ParamRole::DocumentTaken => {
                    return Err(format!(
                        "`{}` consumes a document, so it cannot be emitted mechanically; write \
                         it by hand and add it to `BY_HAND`",
                        function.symbol
                    ));
                }
                ParamRole::Receiver => args.push(self.receiver.clone()),
                ParamRole::Length => {
                    let taken = length.take().ok_or_else(|| {
                        format!(
                            "`{}` states a length with nothing before it",
                            function.symbol
                        )
                    })?;
                    args.push(taken);
                }
                ParamRole::ListCapacity => args.push("{capacity}".to_string()),
                ParamRole::OutputCount => args.push("&count".to_string()),
                ParamRole::OutputList => {
                    let Type::List(element) = &param.ty else {
                        return Err(format!("`{}` has a list that is not one", function.symbol));
                    };
                    let bare = param.name.strip_prefix("out_").unwrap_or(&param.name);
                    args.push(format!("{{list{}}}", lists.len()));
                    lists.push((
                        format!("list{}", lists.len()),
                        names::camel(bare),
                        (**element).clone(),
                    ));
                }
                ParamRole::Output => {
                    let bare = param.name.strip_prefix("out_").unwrap_or(&param.name);
                    let out = format!("out{}", names::pascal(bare));
                    // `var x = 0` is an `Int` whatever the call wants, so
                    // anything else numeric says what it is.
                    match &param.ty {
                        Type::Document => pre.push(format!("var {out}: OpaquePointer?")),
                        ty @ (Type::Double
                        | Type::Int64
                        | Type::Uint64
                        | Type::Int32
                        | Type::Uint32) => {
                            pre.push(format!("var {out}: {} = 0", c_type(ty)));
                        }
                        ty => pre.push(format!("var {out} = {}", c_empty(self.api, ty)?)),
                    }
                    args.push(format!("&{out}"));
                    let handed_back = from_c(&param.ty, &out, &self.owner);
                    match (&param.ty, self.wrap.as_deref()) {
                        (Type::Node, Some(class)) => results.push((
                            names::camel(bare),
                            class.to_string(),
                            format!("{class}(document: {}, handle: {out})", self.owner),
                        )),
                        _ => results.push((names::camel(bare), swift_type(&param.ty), handed_back)),
                    }
                    if matches!(param.ty, Type::Text | Type::Bytes) {
                        frees.push(format!("defer {{ otio_buffer_free({out}) }}"));
                    }
                }
                ParamRole::Bytes => {
                    let swift = parameter_name(&param.name);
                    params.push(declare(&swift, "[UInt8]", labelled, false));
                    labelled += 1;
                    scopes.push(Scope {
                        before: Vec::new(),
                        call: format!("{swift}.withUnsafeBufferPointer"),
                        binding: format!("{local}: UnsafeBufferPointer<UInt8>"),
                    });
                    args.push(format!("{local}.baseAddress"));
                    length = Some(format!("{local}.count"));
                }
                ParamRole::Input => {
                    let swift = parameter_name(&param.name);
                    self.input(
                        param,
                        &swift,
                        &local,
                        labelled,
                        &mut params,
                        &mut scopes,
                        &mut pre,
                        &mut args,
                        &mut length,
                        &mut guarded,
                    )?;
                    labelled += 1;
                }
            }
        }

        match &function.result {
            CResult::Value(ty) => results.push((
                "value".to_string(),
                swift_type(ty),
                from_c(ty, "value", &self.owner),
            )),
            CResult::StaticText => results.push((
                "value".to_string(),
                "String".to_string(),
                "staticText(value)".to_string(),
            )),
            CResult::Void | CResult::Status => {}
        }

        for (buffer, label, element) in &lists {
            results.push((
                label.clone(),
                format!("[{}]", swift_type(element)),
                format!(
                    "(0..<taken).map {{ {} }}",
                    from_c(element, &format!("{buffer}[$0]"), &self.owner)
                ),
            ));
        }

        let mut result = match results.len() {
            0 => String::new(),
            1 => results[0].1.clone(),
            _ => format!(
                "({})",
                results
                    .iter()
                    .map(|(name, ty, _)| format!("{name}: {ty}"))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        };
        if function.optional && !result.is_empty() {
            result.push('?');
        }
        let returned = if result.is_empty() {
            "Void".to_string()
        } else {
            result.clone()
        };

        let mut lines = Lines::new();
        for (checked, asked) in &guarded {
            if throwing {
                lines.push(&format!("try {checked}"));
                continue;
            }
            // A call that answers with a plain value has no error to hand
            // back, so an object from another document gets the answer it
            // deserves: no document contains one, and nothing equals one.
            let zero = match &function.result {
                CResult::Value(ty) => swift_zero(ty)?,
                other => {
                    return Err(format!(
                        "`{}` takes an object and returns `{other:?}`, so it has no way to say \
                         the object came from another document",
                        function.symbol
                    ));
                }
            };
            lines.push(&format!("guard {asked} else {{ return {zero} }}"));
        }
        for scope in &scopes {
            for line in &scope.before {
                lines.push(line);
            }
            let binding = if scope.binding.is_empty() {
                "()".to_string()
            } else {
                format!("({})", scope.binding)
            };
            let prefix = if throwing { "return try " } else { "return " };
            lines.open(&format!(
                "{prefix}{} {{ {binding} -> {returned} in",
                scope.call
            ));
        }
        for line in &pre {
            lines.push(line);
        }
        let extra = self.invoke(&mut lines, &args, &lists, &frees, &returned)?;
        match results.len() {
            0 => {}
            1 => lines.push(&format!("return {}", results[0].2)),
            _ => lines.push(&format!(
                "return ({})",
                results
                    .iter()
                    .map(|(name, _, expression)| format!("{name}: {expression}"))
                    .collect::<Vec<_>>()
                    .join(", ")
            )),
        }
        for _ in 0..extra {
            lines.close();
        }
        for _ in &scopes {
            lines.close();
        }

        Ok(Rendered {
            params,
            result,
            throwing,
            body: lines.out,
        })
    }
}

impl Site<'_> {
    /// Writes an argument the caller supplies.
    #[allow(clippy::too_many_arguments)]
    fn input(
        &self,
        param: &Param,
        swift: &str,
        local: &str,
        index: usize,
        params: &mut Vec<String>,
        scopes: &mut Vec<Scope>,
        pre: &mut Vec<String>,
        args: &mut Vec<String>,
        length: &mut Option<String>,
        guarded: &mut Vec<(String, String)>,
    ) -> Result<(), String> {
        // A handle is an index into one document's arena, and two documents
        // issue the same indices, so an object from elsewhere would resolve
        // to an unrelated object here rather than failing. Only the Swift
        // value knows where it came from, so every object a caller supplies
        // is checked.
        if self.owner != "nil" {
            match &param.ty {
                Type::Node => guarded.push((
                    format!("requireSameDocument({}, {swift})", self.owner),
                    format!("sameDocument({}, {swift})", self.owner),
                )),
                Type::List(inner) if **inner == Type::Node => guarded.push((
                    format!("requireSameDocumentAll({}, {swift})", self.owner),
                    format!("sameDocumentAll({}, {swift})", self.owner),
                )),
                _ => {}
            }
        }
        match (&param.ty, param.optional) {
            (Type::Text, true) => {
                params.push(declare(swift, "String", index, true));
                scopes.push(Scope {
                    before: Vec::new(),
                    call: format!("withOptionalCString({swift})"),
                    binding: format!("{local}: UnsafePointer<CChar>?"),
                });
                args.push(local.to_string());
            }
            (Type::Text, false) => {
                params.push(declare(swift, "String", index, false));
                scopes.push(Scope {
                    before: Vec::new(),
                    call: format!("{swift}.withCString"),
                    binding: format!("{local}: UnsafePointer<CChar>"),
                });
                args.push(local.to_string());
            }
            (Type::Node, true) => {
                params.push(declare(swift, ROOT, index, true));
                pre.push(format!("let {local} = {swift}?.handle ?? otio_node_none()"));
                args.push(local.to_string());
            }
            (Type::Node, false) => {
                params.push(declare(swift, ROOT, index, false));
                args.push(format!("{swift}.handle"));
            }
            (Type::Struct(name), true) => {
                params.push(declare(swift, &value_name(name), index, true));
                scopes.push(Scope {
                    before: Vec::new(),
                    call: format!("withOptionalC({swift})"),
                    binding: format!("{local}: UnsafePointer<{name}>?"),
                });
                args.push(local.to_string());
            }
            (Type::Struct(name), false) => {
                params.push(declare(swift, &value_name(name), index, false));
                scopes.push(Scope {
                    before: Vec::new(),
                    call: format!("{swift}.withC"),
                    binding: format!("{local}: {name}"),
                });
                args.push(local.to_string());
            }
            (Type::List(element), _) => {
                params.push(declare(
                    swift,
                    &format!("[{}]", swift_type(element)),
                    index,
                    false,
                ));
                let values = format!("{local}Values");
                scopes.push(Scope {
                    before: vec![format!(
                        "let {values} = {swift}.map {{ {} }}",
                        to_c(element, "$0")
                    )],
                    call: format!("{values}.withUnsafeBufferPointer"),
                    binding: format!("{local}: UnsafeBufferPointer<{}>", c_type(element)),
                });
                args.push(format!("{local}.baseAddress"));
                *length = Some(format!("{local}.count"));
            }
            (Type::Bytes | Type::Document, _) => {
                return Err(format!(
                    "`{}` takes a `{}` as an argument, which this does not write",
                    self.function.symbol,
                    param.ty.c_name()
                ));
            }
            (ty, _) => {
                params.push(declare(swift, &swift_type(ty), index, false));
                args.push(to_c(ty, swift));
            }
        }
        Ok(())
    }

    /// Writes the call itself, and the two-pass dance where it answers with a
    /// list. Answers with how many closures it left open.
    fn invoke(
        &self,
        lines: &mut Lines,
        args: &[String],
        lists: &[(String, String, Type)],
        frees: &[String],
        returned: &str,
    ) -> Result<usize, String> {
        let symbol = &self.function.symbol;

        if lists.is_empty() {
            let call = format!("{symbol}({})", args.join(", "));
            match &self.function.result {
                CResult::Status if self.function.optional => {
                    lines.push(&format!("let status = {call}"));
                    lines.push("if isNoValue(status) { return nil }");
                    lines.push("try check(status)");
                }
                CResult::Status => lines.push(&format!("try check({call})")),
                CResult::Void => lines.push(&call),
                CResult::Value(_) | CResult::StaticText => {
                    lines.push(&format!("let value = {call}"));
                }
            }
            for line in frees {
                lines.push(line);
            }
            return Ok(0);
        }

        lines.push("var count = 0");
        let room = if let Some(sizer) = self.function.sized_by.as_deref() {
            // This call empties what it reports, so it cannot be asked
            // twice. Another call says how long the answer will be, and this
            // one is made once into a buffer that size.
            lines.push(&format!(
                "// {symbol} answers and empties in one go, so the buffer is sized first."
            ));
            lines.push("var room = 0");
            lines.push(&format!("try check({})", self.sizing_call(sizer)?));
            "room".to_string()
        } else {
            let sized: Vec<String> = args
                .iter()
                .map(|argument| {
                    if argument.starts_with("{list") {
                        "nil".to_string()
                    } else if argument == "{capacity}" {
                        "0".to_string()
                    } else {
                        argument.clone()
                    }
                })
                .collect();
            lines.push(&format!("try check({symbol}({}))", sized.join(", ")));
            "count".to_string()
        };

        for (buffer, _, element) in lists {
            let c = c_type(element);
            lines.push(&format!(
                "var {buffer}Storage = [{c}](repeating: {}, count: {room})",
                c_empty(self.api, element)?
            ));
            lines.open(&format!(
                "return try {buffer}Storage.withUnsafeMutableBufferPointer {{ ({buffer}: inout \
                 UnsafeMutableBufferPointer<{c}>) -> {returned} in"
            ));
        }

        let filled: Vec<String> = args
            .iter()
            .map(|argument| {
                if let Some(index) = argument
                    .strip_prefix("{list")
                    .and_then(|rest| rest.strip_suffix('}'))
                    .and_then(|digits| digits.parse::<usize>().ok())
                {
                    return format!("{}.baseAddress", lists[index].0);
                }
                if argument == "{capacity}" {
                    return format!("{}.count", lists[0].0);
                }
                argument.clone()
            })
            .collect();
        lines.push(&format!("try check({symbol}({}))", filled.join(", ")));
        // A document does not change between the two calls, so this cannot
        // trip; it is here so that a mistaken count is a short array rather
        // than a crash in someone else's program.
        lines.push(&format!("let taken = min(count, {}.count)", lists[0].0));
        for line in frees {
            lines.push(line);
        }
        Ok(lists.len())
    }

    /// Writes the call that says how long a consuming list call's answer will
    /// be.
    fn sizing_call(&self, sizer: &str) -> Result<String, String> {
        let function = self
            .api
            .functions()
            .find(|candidate| candidate.symbol == sizer)
            .ok_or_else(|| format!("`{sizer}` is not a call this library has"))?;
        let mut args = Vec::new();
        for param in &function.params {
            match param.role {
                ParamRole::DocumentIn | ParamRole::DocumentMut => args.push(self.document.clone()),
                ParamRole::Receiver => args.push(self.receiver.clone()),
                ParamRole::Output => args.push("&room".to_string()),
                _ => {
                    return Err(format!(
                        "`{sizer}` takes a `{}`, so it cannot size another call's answer",
                        param.name
                    ));
                }
            }
        }
        Ok(format!("{sizer}({})", args.join(", ")))
    }
}

/// Spells one Swift parameter, with its label and any default.
fn declare(name: &str, ty: &str, index: usize, optional: bool) -> String {
    // Swift's convention is that the first argument carries no label,
    // because the method's own name already says what it takes.
    let label = if index == 0 {
        format!("_ {name}")
    } else {
        name.to_string()
    };
    if optional {
        format!("{label}: {ty}? = nil")
    } else {
        format!("{label}: {ty}")
    }
}

/// Words Swift will not let a parameter be called, and what to call them
/// instead.
///
/// They are words, not decorations, because the name is what someone reading
/// the documentation and writing the argument label sees.
const RENAMED: &[(&str, &str)] = &[
    ("in", "within"),
    ("where", "condition"),
    ("default", "fallback"),
    ("repeat", "repeated"),
    ("operator", "operation"),
    ("protocol", "shape"),
    ("extension", "suffix"),
    ("self", "object"),
    ("super", "parent"),
    ("class", "kind"),
    ("struct", "record"),
    ("enum", "choice"),
    ("func", "function"),
    ("import", "imported"),
    ("return", "result"),
    ("throw", "raised"),
    ("try", "attempt"),
    ("var", "variable"),
    ("let", "constant"),
    ("guard", "check"),
    ("defer", "deferred"),
    ("switch", "choice"),
    ("case", "branch"),
    ("subscript", "at"),
    ("inout", "borrowed"),
    ("internal", "hidden"),
    ("static", "shared"),
    ("nil", "nothing"),
    ("true", "yes"),
    ("false", "no"),
    ("is", "matches"),
    ("as", "asType"),
    ("do", "perform"),
    ("else", "otherwise"),
    ("if", "when"),
    ("while", "until"),
    ("init", "start"),
    ("deinit", "finish"),
    ("any", "anything"),
    ("some", "something"),
];

/// What a parameter is called in Swift.
fn parameter_name(name: &str) -> String {
    let spelled = names::camel(name);
    for (taken, instead) in RENAMED {
        if spelled == *taken {
            return (*instead).to_string();
        }
    }
    spelled
}

/// The calls this backend writes itself rather than emitting mechanically.
///
/// `otio_document_absorb` answers with a translation table, as two parallel
/// lists of handles, and every handle in the first of them names a document
/// the same call has just freed. Emitted mechanically that is a pair of
/// arrays half of which name nothing; written by hand it is a dictionary
/// from the objects the caller already holds to their new ones.
///
/// A symbol here is still in the description and still checked for a name
/// collision, so the hand-written version cannot quietly diverge from the
/// call it stands for.
const BY_HAND: &[&str] = &["otio_document_absorb"];

/// Selectors this SDK writes by hand, which a generated one may not take.
const RESERVED: &[(&str, &str)] = &[
    ("Document", "absorb(_:)"),
    ("Document", "close()"),
    ("Document", "open(_:)"),
    ("Document", "save(_:)"),
    ("Document", "pointer"),
    ("object:SerializableObject", "document"),
    ("object:SerializableObject", "documentPointer"),
    ("object:SerializableObject", "handle"),
    ("object:SerializableObject", "isA(_:)"),
    ("object:SerializableObjectWithMetadata", "metadata"),
];

impl Backend<'_> {
    /// Puts a header and the import around generated Swift, and names the
    /// file it goes in.
    fn assemble(&self, name: &str, body: String) -> File {
        let _ = self;
        let mut out = String::new();
        out.push_str("// Code generated by otio-sdk-gen from crates/otio-capi. DO NOT EDIT.\n\n");
        out.push_str("import COtio\n\n");
        out.push_str(body.trim_end());
        out.push('\n');
        File {
            path: PathBuf::from(DIR).join(SOURCES).join(name),
            contents: out,
        }
    }

    /// Rewrites a doc comment into Swift's shape: the interface's own symbols
    /// spelled the way this SDK spells them, `null` said as `nil`.
    fn doc(
        &self,
        docs: &Docs,
        notes: &[String],
        symbol: Option<&str>,
        absent: &str,
    ) -> Vec<String> {
        let mut paragraphs: Vec<String> = Vec::new();
        let summary = self.rewrite(&docs.summary);
        if summary.is_empty() {
            if symbol.is_some() {
                paragraphs.push("Wraps the interface's call of the same name.".to_string());
            }
        } else {
            paragraphs.push(summary);
        }
        for paragraph in &docs.body {
            paragraphs.push(self.rewrite(paragraph));
        }
        paragraphs.extend(notes.iter().cloned());
        for paragraph in &mut paragraphs {
            *paragraph = nil_for_null(paragraph, absent);
        }
        if let Some(symbol) = symbol {
            paragraphs.push(format!("C: `{symbol}`"));
        }

        let mut lines = Vec::new();
        for (index, paragraph) in paragraphs.iter().enumerate() {
            if index > 0 {
                lines.push("///".to_string());
            }
            for line in wrap(paragraph, 72) {
                if line.is_empty() {
                    lines.push("///".to_string());
                } else {
                    lines.push(format!("/// {line}"));
                }
            }
        }
        lines
    }

    /// Replaces the interface's own names with this SDK's.
    fn rewrite(&self, text: &str) -> String {
        let mut out = text.to_string();
        // Longest first, so `otio_node_kind` is not half-replaced by a
        // shorter symbol that happens to be a prefix of it.
        let mut symbols: Vec<&String> = self.spellings.keys().collect();
        symbols.sort_by_key(|symbol| std::cmp::Reverse(symbol.len()));
        for symbol in symbols {
            if !out.contains(symbol.as_str()) {
                continue;
            }
            let swift = &self.spellings[symbol];
            out = out.replace(&format!("`{symbol}`"), &format!("`{swift}`"));
            out = out.replace(symbol.as_str(), swift);
        }
        // Rust spells a link to another item `[`name`]`, which Swift renders
        // as a bracketed code span rather than as a link.
        out.replace("[`", "`").replace("`]", "`")
    }
}

/// Says what a Swift programmer says where the C interface's prose says
/// `null`.
fn nil_for_null(text: &str, instead: &str) -> String {
    let vowel = instead.starts_with(['a', 'e', 'i', 'o', 'u']);
    let mut words: Vec<String> = Vec::new();
    for word in text.split(' ') {
        let trimmed = word.trim_end_matches(|character: char| !character.is_alphanumeric());
        if !trimmed.eq_ignore_ascii_case("null") {
            words.push(word.to_string());
            continue;
        }
        // "a null key" becomes "an empty key", not "a empty key".
        if let Some(article) = words.last_mut() {
            if article == "a" && vowel {
                *article = "an".to_string();
            } else if article == "A" && vowel {
                *article = "An".to_string();
            } else if (article == "an" || article == "An") && !vowel {
                article.truncate(1);
            }
        }
        words.push(format!("{instead}{}", &word[trimmed.len()..]));
    }
    words.join(" ")
}

/// Wraps a paragraph at a width, on spaces.
fn wrap(text: &str, width: usize) -> Vec<String> {
    let mut lines = Vec::new();
    let mut current = String::new();
    for word in text.split_whitespace() {
        if !current.is_empty() && current.len() + 1 + word.len() > width {
            lines.push(std::mem::take(&mut current));
        }
        if !current.is_empty() {
            current.push(' ');
        }
        current.push_str(word);
    }
    if !current.is_empty() {
        lines.push(current);
    }
    if lines.is_empty() {
        lines.push(String::new());
    }
    lines
}

/// Writes a member of a type: its documentation, its signature and its body,
/// indented to sit inside an extension.
fn write_member(out: &mut String, doc: &[String], signature: &str, body: &[String]) {
    for line in doc {
        let _ = writeln!(out, "{TAB}{line}");
    }
    let _ = writeln!(out, "{TAB}{signature} {{");
    for line in body {
        if line.is_empty() {
            out.push('\n');
        } else {
            let _ = writeln!(out, "{TAB}{TAB}{line}");
        }
    }
    let _ = writeln!(out, "{TAB}}}\n");
}

impl Backend<'_> {
    /// Writes every call of one group into an extension of one type.
    fn emit_group(&self, out: &mut String, group: &Group, into: &str) -> Result<(), String> {
        self.emit_some(out, group, into, |_| true)
    }

    /// Writes the calls of one group that a filter accepts.
    fn emit_some(
        &self,
        out: &mut String,
        group: &Group,
        into: &str,
        wanted: impl Fn(&Function) -> bool,
    ) -> Result<(), String> {
        let mut members = String::new();
        for function in &group.functions {
            if self.skipped(function) || !wanted(function) {
                continue;
            }
            self.emit_function(&mut members, group, function)?;
        }
        if members.trim().is_empty() {
            return Ok(());
        }
        let _ = writeln!(out, "extension {into} {{");
        out.push_str(members.trim_end());
        let _ = writeln!(out, "\n}}\n");
        Ok(())
    }

    /// Writes one call.
    fn emit_function(
        &self,
        out: &mut String,
        group: &Group,
        function: &Function,
    ) -> Result<(), String> {
        let name = member_name(group, function);
        let mut lead: Vec<Scope> = Vec::new();
        let mut is_static = false;
        let mut wrap_as: Option<String> = None;

        let (document, receiver, owner) = match (&group.receiver, function.role) {
            (Receiver::None, _) => {
                is_static = true;
                (String::new(), String::new(), "nil".to_string())
            }
            (Receiver::Document, Role::Constructor | Role::Free) => {
                is_static = true;
                (String::new(), String::new(), "nil".to_string())
            }
            (Receiver::Document, _) => (
                "self.pointer".to_string(),
                String::new(),
                "self".to_string(),
            ),
            (Receiver::Node(schema), Role::Constructor) if takes_a_document(function) => {
                wrap_as = Some(schema_name(schema));
                (
                    "self.pointer".to_string(),
                    String::new(),
                    "self".to_string(),
                )
            }
            (Receiver::Node(_), Role::Constructor) => {
                is_static = true;
                (String::new(), String::new(), "nil".to_string())
            }
            (Receiver::Node(_), _) if group.view => (
                "self.object.documentPointer".to_string(),
                "self.object.handle".to_string(),
                "self.object.document".to_string(),
            ),
            (Receiver::Node(_), _) => (
                "self.documentPointer".to_string(),
                "self.handle".to_string(),
                "self.document".to_string(),
            ),
            (Receiver::Value(_), Role::Constructor | Role::Free) => {
                is_static = true;
                (String::new(), String::new(), "nil".to_string())
            }
            (Receiver::Value(what), _) => {
                let expression = if self.api.enumeration(what).is_some() {
                    format!("cEnum(self.rawValue, {what}.self)")
                } else {
                    lead.push(Scope {
                        before: Vec::new(),
                        call: "self.withC".to_string(),
                        binding: format!("cSelf: {what}"),
                    });
                    "cSelf".to_string()
                };
                (String::new(), expression, "nil".to_string())
            }
        };

        let site = Site {
            api: self.api,
            function,
            document,
            receiver,
            owner,
            wrap: wrap_as,
        };
        let mut rendered = site.render()?;
        if !lead.is_empty() {
            rendered = site.wrapped(rendered, &lead);
        }

        // Where the interface's prose already says an argument may be
        // absent, it is left to say it rather than said twice.
        let said = function.docs.summary.to_lowercase().contains("null")
            || function
                .docs
                .body
                .iter()
                .any(|paragraph| paragraph.to_lowercase().contains("null"));
        let mut notes = Vec::new();
        if !said {
            for param in function.inputs() {
                if !param.optional {
                    continue;
                }
                notes.push(format!(
                    "A nil `{}` means none.",
                    parameter_name(&param.name)
                ));
            }
        }
        let says_no_value = std::iter::once(&function.docs.summary)
            .chain(function.docs.body.iter())
            .any(|paragraph| paragraph.contains("NO_VALUE"));
        if function.optional && !says_no_value {
            notes.push(
                "Where there is nothing to report this answers nil, which is an answer rather \
                 than a failure."
                    .to_string(),
            );
        }
        let doc = self.doc(&function.docs, &notes, Some(&function.symbol), "nil");

        let signature = if is_property(group, function) {
            format!(
                "public var {name}: {}",
                if rendered.result.is_empty() {
                    "Void".to_string()
                } else {
                    rendered.result.clone()
                }
            )
        } else {
            signature(
                is_static,
                &name,
                &rendered.params,
                rendered.throwing,
                &rendered.result,
            )
        };
        write_member(out, &doc, &signature, &rendered.body);
        Ok(())
    }
}

impl Site<'_> {
    /// Puts a rendered body inside further closures, for a value receiver
    /// that has to be lent to the call as a C struct.
    fn wrapped(&self, rendered: Rendered, lead: &[Scope]) -> Rendered {
        let returned = if rendered.result.is_empty() {
            "Void".to_string()
        } else {
            rendered.result.clone()
        };
        let mut lines = Lines::new();
        for scope in lead {
            for line in &scope.before {
                lines.push(line);
            }
            let prefix = if rendered.throwing {
                "return try "
            } else {
                "return "
            };
            lines.open(&format!(
                "{prefix}{} {{ ({}) -> {returned} in",
                scope.call, scope.binding
            ));
        }
        let inner = lines.depth;
        for line in &rendered.body {
            lines.push(line);
        }
        for _ in 0..inner {
            lines.close();
        }
        Rendered {
            body: lines.out,
            ..rendered
        }
    }
}

/// Spells a Swift signature from its parts.
fn signature(
    is_static: bool,
    name: &str,
    params: &[String],
    throwing: bool,
    result: &str,
) -> String {
    let lead = if is_static {
        "public static func"
    } else {
        "public func"
    };
    let throws_clause = if throwing { " throws" } else { "" };
    let returns = if result.is_empty() {
        String::new()
    } else {
        format!(" -> {result}")
    };
    format!(
        "{lead} {name}({}){throws_clause}{returns}",
        params.join(", ")
    )
}

impl Backend<'_> {
    /// The document, the object handle, the error type and the plumbing the
    /// rest of the SDK calls.
    fn runtime(&self) -> Result<String, String> {
        let mut out = String::from(RUNTIME);
        for group in &self.api.groups {
            if group.receiver == Receiver::None {
                self.emit_group(&mut out, group, "OTIO")?;
            }
        }
        Ok(out)
    }

    /// The enums, as real Swift enums with the C interface's own values.
    fn enums(&self) -> String {
        let mut out = String::new();
        for item in &self.api.enums {
            self.emit_enum(&mut out, item);
        }
        out
    }

    fn emit_enum(&self, out: &mut String, item: &Enum) {
        let swift = enum_name(&item.name);
        for line in self.doc(&item.docs, &[], None, "nil") {
            let _ = writeln!(out, "{line}");
        }
        let _ = writeln!(out, "public enum {swift}: Int32, CaseIterable, Sendable {{");
        for (index, variant) in item.variants.iter().enumerate() {
            if index > 0 {
                out.push('\n');
            }
            for line in self.doc(&variant.docs, &[], None, "nil") {
                let _ = writeln!(out, "{TAB}{line}");
            }
            let _ = writeln!(
                out,
                "{TAB}case {} = {}",
                variant_name(&variant.name),
                variant.value
            );
        }
        let _ = writeln!(out, "}}\n");

        let _ = writeln!(out, "extension {swift}: CustomStringConvertible {{");
        let _ = writeln!(
            out,
            "{TAB}/// The name the C interface spells this value by."
        );
        let _ = writeln!(out, "{TAB}public var description: String {{");
        let _ = writeln!(out, "{TAB}{TAB}switch self {{");
        for variant in &item.variants {
            let _ = writeln!(
                out,
                "{TAB}{TAB}case .{}: return \"{}\"",
                variant_name(&variant.name),
                variant.c_name
            );
        }
        let _ = writeln!(out, "{TAB}{TAB}}}");
        let _ = writeln!(out, "{TAB}}}");
        let _ = writeln!(out, "}}\n");
    }

    /// The value structs, their conversions, and the calls on them.
    fn values(&self) -> Result<String, String> {
        let mut out = String::new();
        for item in &self.api.structs {
            // `OtioNode` is the handle, which the runtime spells itself, and
            // `OtioBuffer` is how text crosses the boundary, which nobody
            // using this should ever see.
            if item.plumbing || item.name == "OtioNode" {
                continue;
            }
            self.emit_value(&mut out, item);
        }
        for group in &self.api.groups {
            if let Receiver::Value(what) = &group.receiver {
                let into = value_name(what);
                self.emit_group(&mut out, group, &into)?;
            }
        }
        Ok(out)
    }

    fn emit_value(&self, out: &mut String, item: &Struct) {
        let swift = value_name(&item.name);
        for line in self.doc(&item.docs, &[], None, "nil") {
            let _ = writeln!(out, "{line}");
        }
        let _ = writeln!(
            out,
            "public struct {swift}: Equatable, Hashable, Sendable {{"
        );
        for (index, field) in item.fields.iter().enumerate() {
            if index > 0 {
                out.push('\n');
            }
            for line in self.doc(&field.docs, &[], None, "nil") {
                let _ = writeln!(out, "{TAB}{line}");
            }
            let _ = writeln!(
                out,
                "{TAB}public var {}: {}",
                names::camel(&field.name),
                swift_type(&field.ty)
            );
        }
        out.push('\n');
        let arguments: Vec<String> = item
            .fields
            .iter()
            .map(|field| format!("{}: {}", names::camel(&field.name), swift_type(&field.ty)))
            .collect();
        let _ = writeln!(out, "{TAB}/// Makes one from its parts.");
        let _ = writeln!(out, "{TAB}public init({}) {{", arguments.join(", "));
        for field in &item.fields {
            let name = names::camel(&field.name);
            let _ = writeln!(out, "{TAB}{TAB}self.{name} = {name}");
        }
        let _ = writeln!(out, "{TAB}}}");
        let _ = writeln!(out, "}}\n");

        let _ = writeln!(out, "extension {swift}: CValue {{");
        let _ = writeln!(out, "{TAB}internal typealias CType = {}\n", item.name);

        // Out of C.
        let _ = writeln!(out, "{TAB}/// Reads the value back out of the C interface.");
        let _ = writeln!(out, "{TAB}internal init(_ value: {}) {{", item.name);
        let read: Vec<String> = item
            .fields
            .iter()
            .map(|field| {
                let source = format!("value.{}", field.name);
                let converted = match &field.ty {
                    Type::Text => format!("staticText({source})"),
                    ty => from_c(ty, &source, "nil"),
                };
                format!("{}: {converted}", names::camel(&field.name))
            })
            .collect();
        let _ = writeln!(out, "{TAB}{TAB}self.init({})", read.join(", "));
        let _ = writeln!(out, "{TAB}}}\n");

        // Into C. Anything of variable length is borrowed for the length of
        // the call and released as the closure unwinds, so a caller cannot
        // leak one and the library cannot keep one.
        let _ = writeln!(
            out,
            "{TAB}/// Lends the value to a call, spelled the way the C interface wants it."
        );
        let _ = writeln!(
            out,
            "{TAB}internal func withC<R>(_ body: ({}) throws -> R) rethrows -> R {{",
            item.name
        );
        let mut lines = Lines::new();
        let mut bindings: Vec<(String, String)> = Vec::new();
        for field in &item.fields {
            let name = names::camel(&field.name);
            let local = format!("c{}", names::pascal(&field.name));
            match &field.ty {
                Type::Text => {
                    lines.open(&format!(
                        "return try withOptionalCString({name}.isEmpty ? nil : {name}) {{ \
                         ({local}: UnsafePointer<CChar>?) -> R in"
                    ));
                    bindings.push((field.name.clone(), local));
                }
                Type::Struct(inner) => {
                    lines.open(&format!(
                        "return try {name}.withC {{ ({local}: {inner}) -> R in"
                    ));
                    bindings.push((field.name.clone(), local));
                }
                ty => bindings.push((field.name.clone(), to_c(ty, &name))),
            }
        }
        let opened = lines.depth;
        lines.push(&format!("var out = {}()", item.name));
        for (field, value) in &bindings {
            lines.push(&format!("out.{field} = {value}"));
        }
        lines.push("return try body(out)");
        for _ in 0..opened {
            lines.close();
        }
        for line in &lines.out {
            let _ = writeln!(out, "{TAB}{TAB}{line}");
        }
        let _ = writeln!(out, "{TAB}}}");
        let _ = writeln!(out, "}}\n");
    }

    /// The schema ladder, as classes deriving as the schemas derive.
    fn schema(&self) -> String {
        let mut out = String::new();
        for schema in &self.api.schema {
            let Some(parent) = schema.parent.as_deref() else {
                continue;
            };
            for line in self.doc(&schema.docs, &[], None, "nil") {
                let _ = writeln!(out, "{line}");
            }
            let _ = writeln!(
                out,
                "public class {}: {} {{}}\n",
                schema_name(&schema.name),
                schema_name(parent)
            );
        }

        let _ = writeln!(
            out,
            "/// Which schema each one derives from, so that asking whether an object is an\n\
             /// `Item` can say yes for a clip."
        );
        let _ = writeln!(out, "internal let schemaParents: [NodeKind: NodeKind] = [");
        for schema in &self.api.schema {
            let Some(parent) = schema.parent.as_deref() else {
                continue;
            };
            let Some(above) = self.api.schema.iter().find(|item| item.name == parent) else {
                continue;
            };
            let _ = writeln!(
                out,
                "{TAB}.{}: .{},",
                variant_name(&schema.kind),
                variant_name(&above.kind)
            );
        }
        let _ = writeln!(out, "]\n");

        let _ = writeln!(
            out,
            "extension SerializableObject {{\n\
             {TAB}/// Whether the object is of a schema, or of one deriving from it.\n\
             {TAB}///\n\
             {TAB}/// An object whose document has gone, or whose handle no longer resolves,\n\
             {TAB}/// is of no schema at all, so this answers false rather than guessing.\n\
             {TAB}public func isA(_ schema: NodeKind) -> Bool {{\n\
             {TAB}{TAB}guard var kind = try? schemaKind() else {{ return false }}\n\
             {TAB}{TAB}while true {{\n\
             {TAB}{TAB}{TAB}if kind == schema {{ return true }}\n\
             {TAB}{TAB}{TAB}guard let parent = schemaParents[kind] else {{ return false }}\n\
             {TAB}{TAB}{TAB}kind = parent\n\
             {TAB}{TAB}}}\n\
             {TAB}}}\n\
             }}\n"
        );

        let _ = writeln!(
            out,
            "/// Builds the class an object's schema names.\n\
             ///\n\
             /// Every handle that comes back from the library goes through this, so a\n\
             /// downcast tells the truth: `node as? Clip` succeeds exactly when the object\n\
             /// really is a clip. An object whose kind cannot be read — a handle that no\n\
             /// longer resolves, or one belonging to no document — comes back as a plain\n\
             /// `SerializableObject` rather than as a guess.\n\
             internal func makeObject(_ document: Document?, _ handle: OtioNode) \
             -> SerializableObject {{\n\
             {TAB}guard let document, document.pointer != nil else {{\n\
             {TAB}{TAB}return SerializableObject(document: document, handle: handle)\n\
             {TAB}}}\n\
             {TAB}var outKind = cEnum(0, OtioNodeKind.self)\n\
             {TAB}guard isOK(otio_node_kind(document.pointer, handle, &outKind)) else {{\n\
             {TAB}{TAB}return SerializableObject(document: document, handle: handle)\n\
             {TAB}}}\n\
             {TAB}switch enumValue(outKind, NodeKind.self) {{"
        );
        for schema in &self.api.schema {
            let _ = writeln!(
                out,
                "{TAB}case .{}: return {}(document: document, handle: handle)",
                variant_name(&schema.kind),
                schema_name(&schema.name)
            );
        }
        let _ = writeln!(out, "{TAB}}}\n}}\n");
        out
    }

    /// The calls that are methods on the objects in a document.
    fn objects(&self) -> Result<String, String> {
        let mut out = String::new();
        for group in &self.api.groups {
            let Receiver::Node(schema) = &group.receiver else {
                continue;
            };
            if group.view {
                continue;
            }
            let into = schema_name(schema);
            // A constructor that needs a document to build in is written
            // with the documents; one that does not belongs to the type.
            self.emit_some(&mut out, group, &into, |function| {
                function.role != Role::Constructor
            })?;
            self.emit_some(&mut out, group, &into, |function| {
                function.role == Role::Constructor && !takes_a_document(function)
            })?;
        }
        Ok(out)
    }

    /// The calls that are methods on a document, and the ones that make one.
    fn documents(&self) -> Result<String, String> {
        let mut out = String::new();
        for group in &self.api.groups {
            if group.receiver == Receiver::Document {
                self.emit_group(&mut out, group, "Document")?;
            }
        }
        for group in &self.api.groups {
            if matches!(group.receiver, Receiver::Node(_)) && !group.view {
                self.emit_some(&mut out, group, "Document", |function| {
                    function.role == Role::Constructor && takes_a_document(function)
                })?;
            }
        }
        Ok(out)
    }

    /// The metadata dictionary every named object carries.
    fn metadata(&self) -> Result<String, String> {
        let mut out = String::new();
        for group in &self.api.groups {
            if !group.view {
                continue;
            }
            let Receiver::Node(schema) = &group.receiver else {
                continue;
            };
            let swift = &group.name;
            for line in self.doc(&group.docs, &[], None, "nil") {
                let _ = writeln!(out, "{line}");
            }
            let _ = writeln!(
                out,
                "public struct {swift} {{\n\
                 {TAB}internal let object: SerializableObject\n\
                 }}\n"
            );
            let _ = writeln!(
                out,
                "extension {} {{\n\
                 {TAB}/// The object's metadata, which is a dictionary of its own.\n\
                 {TAB}///\n\
                 {TAB}/// A path names a value inside it, a step at a time, separated by\n\
                 {TAB}/// dots: `cmx_3600.reel` reaches the reel of the dictionary the EDL\n\
                 {TAB}/// adapter left behind, and `takes[0]` the first entry of a list.\n\
                 {TAB}///\n\
                 {TAB}/// A path is followed, not created. Writing one step deep always\n\
                 {TAB}/// works, but a deeper one needs its dictionary to exist first:\n\
                 {TAB}///\n\
                 {TAB}/// ```swift\n\
                 {TAB}/// try clip.metadata.setDictionary(\"cmx_3600\")\n\
                 {TAB}/// try clip.metadata.setString(\"cmx_3600.reel\", value: \"ZZ100\")\n\
                 {TAB}/// ```\n\
                 {TAB}public var {}: {swift} {{\n\
                 {TAB}{TAB}{swift}(object: self)\n\
                 {TAB}}}\n\
                 }}\n",
                schema_name(schema),
                names::camel(swift)
            );
            self.emit_group(&mut out, group, swift)?;
        }
        Ok(out)
    }
}

/// The handle types, the error type, and the plumbing the rest calls.
const RUNTIME: &str = r#"/// Everything the library offers that belongs to no object.
public enum OTIO {}

/// A failure the library reported.
///
/// Where "there is nothing here" is one of the answers — an item with no
/// source range, a clip with no active media reference — the call answers
/// `nil` instead of throwing, because that is an answer rather than a
/// failure.
public struct OTIOError: Error, Equatable, CustomStringConvertible {
    /// What kind of failure it was.
    public let status: Status
    /// The sentence the library left about this one.
    public let message: String

    public init(status: Status, message: String) {
        self.status = status
        self.message = message
    }

    public var description: String {
        message.isEmpty ? String(describing: status) : message
    }
}

/// A document owns every object in a timeline.
///
/// It is the arena the core keeps its objects in, so an object is an index
/// into it rather than a pointer, and releasing the document releases the
/// whole graph at once. Handles into a released document go stale rather
/// than dangling.
///
/// A document is released when the last reference to it goes, so `close` is
/// not required; it is worth calling anyway, because it frees a whole
/// timeline at once and at a moment you chose. A document is not safe to use
/// from two threads while one of them is changing it.
public final class Document {
    internal var pointer: OpaquePointer?

    internal init(owning pointer: OpaquePointer?) {
        self.pointer = pointer
    }

    deinit {
        if let pointer {
            otio_document_free(pointer)
        }
    }

    /// Releases the document and every object in it.
    ///
    /// Calling it twice is harmless. Using an object of a closed document is
    /// not: its handle no longer resolves, and calls made with it throw.
    public func close() {
        if let pointer {
            otio_document_free(pointer)
        }
        pointer = nil
    }

    /// Reads a document from a file, working out its format from the name.
    ///
    /// It is the short way to say `readFromFile` when the suffix already
    /// says what the file holds, which is how upstream's `read_from_file`
    /// behaves when no adapter is named.
    public static func open(_ path: String) throws -> Document {
        try Document.readFromFile(formatOf(path), path: path)
    }

    /// Writes the document to a file, working out its format from the name.
    ///
    /// It is the short way to say `writeToFile`, as `open` is for
    /// `readFromFile`.
    public func save(_ path: String) throws {
        try writeToFile(formatOf(path), path: path)
    }

    /// Moves every object in another document into this one.
    ///
    /// It is how an object built on its own joins a timeline: build a `Clip`
    /// in a document of its own, absorb that document into the one holding
    /// the timeline, and append the clip where it belongs. A handle means
    /// nothing outside the document it was issued for, so the objects are
    /// moved rather than pointed at, and every one of them arrives under a
    /// new handle.
    ///
    /// `source` is consumed. On success it is emptied and closed, and the
    /// dictionary returned gives the new object for each object that came
    /// from it, so a handle held from before is translated by looking it up.
    /// On failure nothing moves and `source` is left alone. The source's root
    /// is not adopted, because this document has its own.
    ///
    /// C: `otio_document_absorb`
    @discardableResult
    public func absorb(_ source: Document) throws -> [SerializableObject: SerializableObject] {
        guard let target = pointer, source.pointer != nil else {
            throw OTIOError(status: .nullPointer, message: "otio: the document is closed")
        }
        // The call cannot be asked twice to size its answer, because the
        // first ask would already have consumed the source. The source's own
        // count is exactly how many objects will move.
        let moving = otio_document_node_count(source.pointer)
        var from = [OtioNode](repeating: OtioNode(), count: moving)
        var to = [OtioNode](repeating: OtioNode(), count: moving)
        var count = 0
        let status = from.withUnsafeMutableBufferPointer {
            (fromBuffer: inout UnsafeMutableBufferPointer<OtioNode>) -> OtioStatus in
            to.withUnsafeMutableBufferPointer {
                (toBuffer: inout UnsafeMutableBufferPointer<OtioNode>) -> OtioStatus in
                otio_document_absorb(
                    target, &source.pointer, fromBuffer.baseAddress, toBuffer.baseAddress,
                    moving, &count)
            }
        }
        try check(status)
        let taken = min(count, moving)
        var translated = [SerializableObject: SerializableObject](minimumCapacity: taken)
        for index in 0..<taken {
            let was = SerializableObject(document: source, handle: from[index])
            translated[was] = makeObject(self, to[index])
        }
        return translated
    }
}

/// An object in a document: which object, and which document.
///
/// It is the root of the OTIO schema ladder, and every schema below it is a
/// class deriving from it, so a `Clip` has every member of an `Item`, a
/// `Composable` and a `SerializableObjectWithMetadata`. Every handle the
/// library hands back arrives as the class its schema names, so `as? Clip`
/// asks what an object really is and gets a true answer.
///
/// Two objects are equal when they are the same object of the same document.
/// Upstream's Swift bindings keep one wrapper per object and compare with
/// `===`; here a handle is a value, so there may be several wrappers for one
/// object and `==` is the question worth asking.
public class SerializableObject: Hashable {
    /// The document the object lives in, or nil for one that names none.
    public let document: Document?

    internal let handle: OtioNode

    internal init(document: Document?, handle: OtioNode) {
        self.document = document
        self.handle = handle
    }

    /// Answers nil for an object that belongs to no document, so that a call
    /// made on one fails with a message rather than reaching into nothing.
    internal var documentPointer: OpaquePointer? {
        guard let document else { return nil }
        return document.pointer
    }

    public static func == (lhs: SerializableObject, rhs: SerializableObject) -> Bool {
        lhs.document === rhs.document
            && lhs.handle.index == rhs.handle.index
            && lhs.handle.generation == rhs.handle.generation
    }

    public func hash(into hasher: inout Hasher) {
        hasher.combine(document.map(ObjectIdentifier.init))
        hasher.combine(handle.index)
        hasher.combine(handle.generation)
    }
}

/// Reads a C enum out of a raw value.
///
/// A C enum without an extensibility attribute is imported as a struct on one
/// Swift release and as an enum on another, and this is written to work
/// either way.
@inline(__always)
internal func cEnum<T: RawRepresentable>(_ raw: Int32, _: T.Type = T.self) -> T
where T.RawValue: FixedWidthInteger {
    guard let value = T(rawValue: T.RawValue(truncatingIfNeeded: raw)) else {
        preconditionFailure("otio: \(raw) is not a value of \(T.self)")
    }
    return value
}

/// Reads one of this SDK's enums out of the C one it mirrors.
@inline(__always)
internal func enumValue<T: RawRepresentable, C: RawRepresentable>(_ raw: C, _: T.Type = T.self) -> T
where T.RawValue == Int32, C.RawValue: BinaryInteger {
    guard let value = T(rawValue: Int32(truncatingIfNeeded: raw.rawValue)) else {
        preconditionFailure(
            "otio: the library answered with a value of \(C.self) this SDK does not know. "
                + "The SDK is generated from the C interface and a test fails the build if the "
                + "two disagree, so a library and an SDK from the same tree cannot do this.")
    }
    return value
}

/// Whether a call succeeded.
@inline(__always)
internal func isOK(_ status: OtioStatus) -> Bool {
    let code: Status = enumValue(status)
    return code == .ok
}

/// Whether a call answered that there is nothing to report.
@inline(__always)
internal func isNoValue(_ status: OtioStatus) -> Bool {
    let code: Status = enumValue(status)
    return code == .noValue
}

/// Throws what the library said, if it said anything went wrong.
@inline(__always)
internal func check(_ status: OtioStatus) throws {
    let code: Status = enumValue(status)
    if code != .ok {
        throw OTIOError(status: code, message: staticText(otio_error_message()))
    }
}

/// Copies a string the library owns forever and the caller never frees.
internal func staticText(_ pointer: UnsafePointer<CChar>?) -> String {
    guard let pointer else { return "" }
    return String(cString: pointer)
}

/// Copies a buffer of text out of the library.
internal func swiftText(_ buffer: OtioBuffer) -> String {
    guard let data = buffer.data, buffer.len > 0 else { return "" }
    return String(decoding: UnsafeRawBufferPointer(start: data, count: buffer.len), as: UTF8.self)
}

/// Copies a buffer of bytes out of the library.
internal func swiftBytes(_ buffer: OtioBuffer) -> [UInt8] {
    guard let data = buffer.data, buffer.len > 0 else { return [] }
    return Array(UnsafeRawBufferPointer(start: data, count: buffer.len))
}

/// Lends a string that may not be there at all.
internal func withOptionalCString<R>(
    _ text: String?, _ body: (UnsafePointer<CChar>?) throws -> R
) rethrows -> R {
    guard let text else { return try body(nil) }
    return try text.withCString { (pointer: UnsafePointer<CChar>) -> R in try body(pointer) }
}

/// A value this SDK can lend to a call as the C struct it mirrors.
internal protocol CValue {
    associatedtype CType
    func withC<R>(_ body: (CType) throws -> R) rethrows -> R
}

/// Lends a value that may not be there at all, as a pointer C reads or not.
internal func withOptionalC<V: CValue, R>(
    _ value: V?, _ body: (UnsafePointer<V.CType>?) throws -> R
) rethrows -> R {
    guard let value else { return try body(nil) }
    return try value.withC { (lent: V.CType) -> R in
        try withUnsafePointer(to: lent) { (pointer: UnsafePointer<V.CType>) -> R in
            try body(pointer)
        }
    }
}

/// Whether every object named belongs to a document.
///
/// A handle is an index into one document's arena, and two documents issue
/// the same indices, so an object from one would resolve to an unrelated
/// object in another rather than failing. Nothing in the handle says where it
/// came from: the Swift object carries that, and this is where it is used. An
/// object that is none belongs to no document and means "no object", so it is
/// allowed everywhere.
internal func sameDocument(_ owner: Document?, _ objects: SerializableObject?...) -> Bool {
    objects.allSatisfy { object in
        guard let object else { return true }
        return object.document === owner || object.isNone
    }
}

/// `sameDocument`, as something to throw rather than something to ask.
internal func requireSameDocument(
    _ owner: Document?, _ objects: SerializableObject?...
) throws {
    for object in objects {
        guard let object else { continue }
        if object.document === owner || object.isNone { continue }
        throw OTIOError(
            status: .invalidArgument, message: "otio: the object belongs to another document")
    }
}

/// `sameDocument`, for a whole list of objects.
internal func sameDocumentAll(_ owner: Document?, _ objects: [SerializableObject]) -> Bool {
    objects.allSatisfy { $0.document === owner || $0.isNone }
}

/// `requireSameDocument`, for a whole list of objects.
internal func requireSameDocumentAll(
    _ owner: Document?, _ objects: [SerializableObject]
) throws {
    for object in objects where !(object.document === owner || object.isNone) {
        throw OTIOError(
            status: .invalidArgument, message: "otio: the object belongs to another document")
    }
}

/// The part of a path after its last dot, which is what names a format.
internal func suffixOf(_ path: String) -> String {
    let name = path.split(separator: "/").last.map(String.init) ?? path
    guard let dot = name.lastIndex(of: "."), dot != name.startIndex else { return "" }
    return String(name[name.index(after: dot)...])
}

/// The format a path's suffix names, or a failure saying none does.
internal func formatOf(_ path: String) throws -> Format {
    let suffix = suffixOf(path)
    guard let format = try Format.fromSuffix(suffix) else {
        throw OTIOError(
            status: .noValue, message: "otio: no format is written with the suffix .\(suffix)")
    }
    return format
}

"#;

/// The package manifest.
const PACKAGE: &str = r#"// swift-tools-version:5.9
// Code generated by otio-sdk-gen from crates/otio-capi. DO NOT EDIT.

import PackageDescription

let package = Package(
    name: "OpenTimelineIO",
    products: [
        .library(name: "OpenTimelineIO", targets: ["OpenTimelineIO"])
    ],
    targets: [
        // The C interface, as its own module. The header is the one in
        // `crates/otio-capi/include`, read where it lives rather than copied,
        // so the package cannot describe an older interface than the library.
        .systemLibrary(name: "COtio", path: "Sources/COtio"),
        .target(
            name: "OpenTimelineIO",
            dependencies: ["COtio"],
            linkerSettings: [
                .linkedLibrary("m", .when(platforms: [.linux])),
                .linkedLibrary("dl", .when(platforms: [.linux])),
                .linkedLibrary("pthread", .when(platforms: [.linux])),
                .linkedLibrary("iconv", .when(platforms: [.macOS])),
                .linkedFramework("CoreFoundation", .when(platforms: [.macOS])),
                .linkedFramework("Security", .when(platforms: [.macOS])),
            ]
        ),
        .testTarget(name: "OpenTimelineIOTests", dependencies: ["OpenTimelineIO"]),
    ]
)
"#;

/// The module map that makes the C interface a Swift module.
const MODULE_MAP: &str = r#"// Code generated by otio-sdk-gen from crates/otio-capi. DO NOT EDIT.
//
// The header is read where it lives, four directories up, so that the Swift
// package and the library cannot describe two different interfaces. The
// static library itself is built by cargo and looked for in `lib/`; see this
// package's README.
module COtio {
    header "../../../../crates/otio-capi/include/otio.h"
    link "otio"
    export *
}
"#;

/// What keeps the built library out of the repository.
const LIB_GITIGNORE: &str =
    "# The static library the package links against, which CI builds.\n*\n!.gitignore\n";

/// The package's own documentation.
const README: &str = r#"# OpenTimelineIO for Swift

Read, write and edit OpenTimelineIO timelines from Swift.

```swift
import OpenTimelineIO
```

This package is generated from the C interface of the otio-rust core, so it
carries the whole data model: the schemas, the composition algorithms, the ten
edit operations and the file-format adapters. Do not edit the `.swift` files
under `Sources/OpenTimelineIO` by hand — see [`../README.md`](../README.md)
for how they are made and regenerated.

## Building

The package links against a static `libotio` it expects to find in `lib/`,
and the linker is told where that is on the command line:

```sh
cargo build -p otio-capi --release
cp target/release/libotio.a sdk/swift/lib/

cd sdk/swift
swift test -Xlinker -L"$PWD/lib"
```

The library itself is not checked in; `lib/.gitignore` keeps it out.

## Using it

Everything lives in a `Document`, which owns the objects in it:

```swift
let document = try Document.open("cut.edl")
defer { document.close() }

let timeline = try document.root()
for clip in try timeline.findClips() {
    print(try clip.name(), try clip.duration())
}
```

An object is a class of its schema, so `as?` asks what one really is:

```swift
for child in try track.children() {
    if let clip = child as? Clip, let reference = try clip.mediaReference() as? ExternalReference {
        print(try reference.targetURL())
    }
}
```

A call that can fail throws an `OTIOError` carrying a `Status`. Where "there
is nothing here" is one of the answers — an item with no source range, a clip
with no active media reference — the call answers `nil` instead, because that
is an answer rather than a failure:

```swift
if let span = try clip.sourceRange() {
    print(span)
}
```

## What this follows, and where it differs

The shape is OpenTimelineIO's own Swift bindings: a class per schema deriving
as the schemas derive, values as structs, real enums, `throws` for failure,
and compositions that are deliberately not Swift collections. Every
deliberate departure is written down in
[ADR 0003](../../docs/adr/0003-sdk-generation.md).
"#;
