//! The C# SDK.
//!
//! C# is the fifth target and the second with no upstream OpenTimelineIO
//! binding to copy. What things are *called* still follows upstream — the
//! schema names, the member names, the bare-noun getter and the `Set` prefix
//! — spelled the way .NET spells names. What the binding *is* had to be
//! decided here, and the nearest precedent upstream has is its Java
//! bindings: a managed language with classes, garbage collection and
//! exceptions, wrapping the same C++ library.
//!
//! What that gives:
//!
//! - **A class per schema, deriving as the schemas derive**, so `Clip` has
//!   every member of `Item`, `Composable` and `SerializableObject`, and `is`
//!   and `as` ask what an object really is. Every handle the library hands
//!   back is built as the class its schema names, so the cast tells the
//!   truth.
//! - **Values are structs** with their fields as get-only properties.
//! - **Failure is an exception**: `OtioException`, carrying the `Status`.
//! - **`OTIO_STATUS_NO_VALUE` is `null`**, through a nullable return, which
//!   is how C# spells "there is nothing here" rather than how it spells
//!   "something went wrong".
//! - **`Document` is `IDisposable`**, so `using` frees a whole timeline at a
//!   moment the caller chose.
//! - **Nothing is unsafe.** Every call crosses through `DllImport` with
//!   blittable arguments, so the SDK compiles without `AllowUnsafeBlocks`
//!   and a caller never sees a pointer.
//!
//! Every deliberate departure is written down in
//! `docs/adr/0003-sdk-generation.md`.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::path::PathBuf;

use otio_sdk_model::model::{
    Api, CResult, Docs, Enum, Function, Group, Param, ParamRole, Receiver, Role, Struct, Type,
};
use otio_sdk_model::names;

use crate::emit::File;

/// Where the C# project lives, relative to the workspace root.
const DIR: &str = "sdk/csharp";

/// Where the generated sources go inside it.
const SOURCES: &str = "OpenTimelineIO";

/// The root of the OTIO schema ladder, which C# spells as upstream does.
const ROOT: &str = "SerializableObject";

/// Four spaces, which is what C# indents with.
const TAB: &str = "    ";

/// The words .NET spells in capitals however they fall.
///
/// .NET's own rule is that a two-letter acronym keeps both letters and
/// anything longer is capitalised like an ordinary word, so it is `Url`,
/// `Json` and `Utf8` where Go writes `URL`, `JSON` and `UTF8`.
const INITIALISMS: &[&str] = &["id"];

/// Generates every file of the C# project.
///
/// # Errors
///
/// Fails if two calls would end up with the same name on one C# type, or if
/// a call cannot be written mechanically and is not hand-written here.
pub fn generate(api: &Api) -> Result<Vec<File>, String> {
    let backend = Backend::new(api);
    backend.check_names()?;
    Ok(vec![
        plain("OpenTimelineIO/OpenTimelineIO.csproj", PROJECT),
        plain(".gitignore", BUILD_GITIGNORE),
        plain("lib/.gitignore", LIB_GITIGNORE),
        plain("README.md", README),
        backend.assemble("Interop.cs", backend.interop()?),
        backend.assemble("Runtime.cs", backend.runtime()?),
        backend.assemble("Enums.cs", backend.enums()?),
        backend.assemble("Values.cs", backend.values()?),
        backend.assemble("Schema.cs", backend.schema()),
        backend.assemble("Objects.cs", backend.objects()?),
        backend.assemble("Documents.cs", backend.documents()?),
        backend.assemble("Metadata.cs", backend.metadata()?),
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
    /// How each interface symbol is spelled in C#, for the documentation.
    spellings: BTreeMap<String, String>,
    /// Type names a generated member also answers to.
    ///
    /// C# looks a simple name up among a class's members before it looks
    /// among the types, so inside `Item` the name `Color` is the method
    /// `Color()` and not the struct. Where that happens the type is spelled
    /// in full, which is the only thing that reaches past the member.
    shadowed: BTreeSet<String>,
}

impl<'a> Backend<'a> {
    fn new(api: &'a Api) -> Self {
        let mut spellings = BTreeMap::new();
        for group in &api.groups {
            for function in &group.functions {
                spellings.insert(function.symbol.clone(), member_name(api, group, function));
            }
        }
        for item in &api.enums {
            let sharp = enum_name(&item.name);
            for variant in &item.variants {
                spellings.insert(
                    variant.c_name.clone(),
                    format!("{sharp}.{}", variant_name(&variant.name)),
                );
            }
        }
        let mut types: BTreeSet<String> = BTreeSet::new();
        for item in &api.structs {
            types.insert(value_name(&item.name));
        }
        for item in &api.enums {
            types.insert(enum_name(&item.name));
        }
        let mut shadowed = BTreeSet::new();
        for group in &api.groups {
            for function in &group.functions {
                let name = member_name(api, group, function);
                if types.contains(&name) {
                    shadowed.insert(name);
                }
            }
        }
        Self {
            api,
            spellings,
            shadowed,
        }
    }

    /// Fails if two calls would land on one C# type with the same name.
    ///
    /// C# inherits members down a class hierarchy, so a method declared on
    /// `Item` is reachable on a `Clip`; two calls named the same on the two
    /// would hide one another rather than overload. Two named the same on
    /// `Item` and on `Effect` are fine, because neither derives from the
    /// other.
    fn check_names(&self) -> Result<(), String> {
        let mut placed: Vec<(String, String, String)> = Vec::new();
        for (owner, name) in RESERVED {
            placed.push(((*owner).to_string(), (*name).to_string(), String::new()));
        }
        for item in &self.api.structs {
            if item.plumbing || item.name == "OtioNode" {
                continue;
            }
            // A value struct's own fields are properties of it, and a getter
            // named after one would be a redeclaration.
            placed.push((
                value_name(&item.name),
                "ToString".to_string(),
                String::new(),
            ));
            for field in &item.fields {
                placed.push((
                    value_name(&item.name),
                    member_case(&field.name),
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
                let name = member_name(self.api, group, function);
                for (other_owner, other_name, other_symbol) in &placed {
                    if *other_name != name || !self.meet(&owner, other_owner) {
                        continue;
                    }
                    let first = if other_symbol.is_empty() {
                        format!("the hand-written {other_owner}.{other_name}")
                    } else {
                        format!("`{other_symbol}`")
                    };
                    clashes.push(format!(
                        "{first} and `{}` are both {name} on {owner}",
                        function.symbol
                    ));
                }
                placed.push((owner, name, function.symbol.clone()));
            }
        }
        if clashes.is_empty() {
            return Ok(());
        }
        Err(format!(
            "the C# names collide:\n  {}\n\nGive one of each pair another name in \
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

    /// Whether two owners are places a caller could reach the same name from,
    /// which for a class is anywhere on its line of descent.
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

    /// The C# type a call hangs off.
    fn owner_of(&self, group: &Group, function: &Function) -> String {
        match (&group.receiver, function.role) {
            (Receiver::None, _) => "Otio".to_string(),
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
            // An enum cannot carry members of its own, so its calls are
            // extension methods in a static class, and its constructors land
            // on `Otio` beside the other free functions.
            (Receiver::Value(what), role) => {
                if self.api.enumeration(what).is_some() {
                    if matches!(role, Role::Constructor | Role::Free) {
                        "Otio".to_string()
                    } else {
                        format!("{}Extensions", enum_name(what))
                    }
                } else {
                    value_name(what)
                }
            }
        }
    }
}

/// Whether a call is given a document to work in.
fn takes_a_document(function: &Function) -> bool {
    function
        .params
        .iter()
        .any(|param| matches!(param.role, ParamRole::DocumentIn | ParamRole::DocumentMut))
}

/// Spells a name the way .NET spells a public member.
fn member_case(name: &str) -> String {
    names::pascal_with(name, INITIALISMS)
}

/// What a call is called in C#.
///
/// `enum_receiver` says whether the call hangs off one of the C interface's
/// enums, which in C# can carry no members of its own: those land on `Otio`
/// beside the free functions, and take their enum's name so that
/// `Otio.FromSuffix` does not have to be guessed at.
fn member_name_in(group: &Group, function: &Function, enum_receiver: bool) -> String {
    let spelled = member_case(&function.name);
    match (&group.receiver, function.role) {
        (Receiver::Node(schema), Role::Constructor) if takes_a_document(function) => {
            let class = schema_name(schema);
            if function.name == "new" {
                format!("New{class}")
            } else {
                format!("New{class}{spelled}")
            }
        }
        // A free function or constructor lifted off an enum keeps the enum's
        // name, since `Otio.FromSuffix` would say nothing about what it makes.
        (Receiver::Value(what), Role::Constructor | Role::Free) if enum_receiver => {
            format!("{}{spelled}", enum_name(what))
        }
        (Receiver::Document, Role::Constructor) if function.name == "new" => "New".to_string(),
        _ => spelled,
    }
}

/// What a call is called in C#, with the interface to ask what its receiver
/// is.
fn member_name(api: &Api, group: &Group, function: &Function) -> String {
    let enum_receiver = match &group.receiver {
        Receiver::Value(what) => api.enumeration(what).is_some(),
        _ => false,
    };
    member_name_in(group, function, enum_receiver)
}

/// The C# name of a schema, which is upstream's own.
fn schema_name(schema: &str) -> String {
    schema.to_string()
}

/// The C# name of an enum: `OtioNodeKind` becomes `NodeKind`.
fn enum_name(c_name: &str) -> String {
    names::respell(c_name.strip_prefix("Otio").unwrap_or(c_name), INITIALISMS)
}

/// The C# name of a value struct: `OtioRationalTime` becomes `RationalTime`.
fn value_name(c_name: &str) -> String {
    enum_name(c_name)
}

/// The C# name of an enum's member, which is its Rust name respelled the way
/// .NET capitalises: `InvalidUtf8` stays `InvalidUtf8` where Go writes
/// `InvalidUTF8`.
fn variant_name(pascal: &str) -> String {
    names::respell(pascal, INITIALISMS)
}

/// Spells a type name in full where a member of the same name would
/// otherwise answer to it.
fn qualified(name: &str, shadowed: &BTreeSet<String>) -> String {
    if shadowed.contains(name) {
        format!("global::OpenTimelineIO.{name}")
    } else {
        name.to_string()
    }
}

/// The C# type a value has in the SDK's own surface.
fn sharp_type(ty: &Type, shadowed: &BTreeSet<String>) -> String {
    match ty {
        Type::Bool => "bool".to_string(),
        Type::Double => "double".to_string(),
        Type::Int64 => "long".to_string(),
        Type::Uint64 => "ulong".to_string(),
        Type::Int32 => "int".to_string(),
        Type::Uint32 => "uint".to_string(),
        // C# counts and indexes with `int`, whatever C does.
        Type::Size => "int".to_string(),
        Type::Text => "string".to_string(),
        Type::Bytes => "byte[]".to_string(),
        Type::Node => ROOT.to_string(),
        Type::Document => "Document".to_string(),
        Type::Struct(name) => qualified(&value_name(name), shadowed),
        Type::Enum(name) => qualified(&enum_name(name), shadowed),
        Type::List(inner) => format!("{}[]", sharp_type(inner, shadowed)),
    }
}

/// The type a value crosses the boundary as, spelled the way `DllImport`
/// wants it.
///
/// Everything here is blittable, so the marshaller pins rather than copies
/// and the SDK needs no unsafe code of its own.
fn c_type(ty: &Type) -> String {
    match ty {
        // C's `bool` is one byte; C#'s marshals as a four-byte Win32 `BOOL`
        // unless told otherwise, so it crosses as the byte it really is.
        Type::Bool => "byte".to_string(),
        Type::Double => "double".to_string(),
        Type::Int64 => "long".to_string(),
        Type::Uint64 => "ulong".to_string(),
        Type::Int32 => "int".to_string(),
        Type::Uint32 => "uint".to_string(),
        Type::Size => "nuint".to_string(),
        Type::Text | Type::Document => "IntPtr".to_string(),
        Type::Bytes => "byte[]?".to_string(),
        Type::Node => "Native.OtioNode".to_string(),
        Type::Struct(name) => format!("Native.{name}"),
        Type::Enum(name) => enum_name(name),
        Type::List(inner) => format!("{}[]?", c_type(inner).trim_end_matches('?')),
    }
}

/// The type an out-parameter of that kind is declared with.
fn out_type(ty: &Type) -> String {
    match ty {
        Type::Text | Type::Bytes => "Native.OtioBuffer".to_string(),
        other => c_type(other),
    }
}

/// Turns a C# value into the C one a call wants.
fn to_c(ty: &Type, value: &str) -> String {
    match ty {
        Type::Bool => format!("({value} ? (byte)1 : (byte)0)"),
        Type::Size => format!("(nuint){value}"),
        Type::Node => format!("{value}.Handle"),
        Type::Struct(_) => format!("{value}.ToNative()"),
        _ => value.to_string(),
    }
}

/// Turns the C value a call gave back into a C# one.
///
/// `owner` is the document a handle belongs to, since an object in C# carries
/// the document it can be resolved against rather than making its caller
/// remember.
fn from_c(ty: &Type, value: &str, owner: &str, shadowed: &BTreeSet<String>) -> String {
    match ty {
        Type::Bool => format!("{value} != 0"),
        Type::Size => format!("(int){value}"),
        Type::Node => format!("Interop.MakeObject({owner}, {value})"),
        Type::Document => format!("new Document({value})"),
        Type::Text => format!("Interop.Text({value})"),
        Type::Bytes => format!("Interop.Bytes({value})"),
        Type::Struct(name) => format!(
            "{}.FromNative({value})",
            qualified(&value_name(name), shadowed)
        ),
        _ => value.to_string(),
    }
}

/// The value a call answers with when it is asked about an object from
/// another document and has no way to report the mistake.
fn sharp_zero(ty: &Type) -> Result<String, String> {
    Ok(match ty {
        Type::Bool => "false".to_string(),
        Type::Double | Type::Int64 | Type::Uint64 | Type::Int32 | Type::Uint32 | Type::Size => {
            "0".to_string()
        }
        Type::Text => "string.Empty".to_string(),
        other => {
            return Err(format!(
                "a call that cannot fail answers with a `{}`, which has no value to stand for \
                 an object from another document",
                other.c_name()
            ));
        }
    })
}

/// A run of generated lines, kept at the right indentation as blocks open and
/// close around the call.
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

    /// Opens a block: the line, a brace, and one more level of indent.
    fn open(&mut self, line: &str) {
        self.push(line);
        self.push("{");
        self.depth += 1;
    }

    fn close(&mut self) {
        self.depth -= 1;
        self.push("}");
    }
}

/// One call, being written into one place.
struct Site<'a> {
    api: &'a Api,
    /// Type names a generated member also answers to.
    shadowed: &'a BTreeSet<String>,
    function: &'a Function,
    /// The C# expression for the document pointer the call works in.
    document: String,
    /// The C# expression for the handle or value the call is about.
    receiver: String,
    /// The C# expression for the document anything handed back belongs to,
    /// or `null` where the call belongs to no document.
    owner: String,
    /// The class a constructor's handle should be handed back as, so that
    /// `document.NewClip` answers with a `Clip` rather than a bare object.
    wrap: Option<String>,
}

/// A call, written out.
struct Rendered {
    /// The C# parameters, with their types.
    params: Vec<String>,
    /// What the call answers with, or `void`.
    result: String,
    /// The lines of the body, at no indentation.
    body: Vec<String>,
}

impl Site<'_> {
    /// Writes the call out.
    #[allow(clippy::too_many_lines)]
    fn render(&self) -> Result<Rendered, String> {
        let function = self.function;
        let mut params: Vec<String> = Vec::new();
        let mut args: Vec<String> = Vec::new();
        let mut pre: Vec<String> = Vec::new();
        // Each is the name, the C# type and the expression that reads it.
        let mut results: Vec<(String, String, String)> = Vec::new();
        let mut lists: Vec<(String, String, Type)> = Vec::new();
        let mut length: Option<String> = None;
        // Each is the check that throws and the plain question it asks, since
        // a call that cannot fail has no way to report the mistake.
        let mut guarded: Vec<(String, String)> = Vec::new();
        let mut scratch = false;

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
                ParamRole::OutputCount => args.push("out count".to_string()),
                ParamRole::OutputList => {
                    let Type::List(element) = &param.ty else {
                        return Err(format!("`{}` has a list that is not one", function.symbol));
                    };
                    let bare = param.name.strip_prefix("out_").unwrap_or(&param.name);
                    args.push(format!("{{list{}}}", lists.len()));
                    lists.push((
                        format!("buffer{}", lists.len()),
                        member_case(bare),
                        (**element).clone(),
                    ));
                }
                ParamRole::Output => {
                    let bare = param.name.strip_prefix("out_").unwrap_or(&param.name);
                    let out = format!("out{}", names::pascal(bare));
                    args.push(format!("out var {out}"));
                    let handed_back = from_c(&param.ty, &out, &self.owner, self.shadowed);
                    match (&param.ty, self.wrap.as_deref()) {
                        (Type::Node, Some(class)) => results.push((
                            member_case(bare),
                            class.to_string(),
                            format!("new {class}({}, {out})", self.owner),
                        )),
                        _ => {
                            results.push((
                                member_case(bare),
                                sharp_type(&param.ty, self.shadowed),
                                handed_back,
                            ));
                        }
                    }
                }
                ParamRole::Bytes => {
                    let sharp = parameter_name(&param.name);
                    params.push(format!("byte[] {sharp}"));
                    args.push(sharp.clone());
                    length = Some(format!("(nuint){sharp}.Length"));
                }
                ParamRole::Input => {
                    let sharp = parameter_name(&param.name);
                    self.input(
                        param,
                        &sharp,
                        &local,
                        &mut params,
                        &mut pre,
                        &mut args,
                        &mut length,
                        &mut guarded,
                        &mut scratch,
                    )?;
                }
            }
        }

        match &function.result {
            CResult::Value(ty) => results.push((
                "Value".to_string(),
                sharp_type(ty, self.shadowed),
                from_c(ty, "answer", &self.owner, self.shadowed),
            )),
            CResult::StaticText => results.push((
                "Value".to_string(),
                "string".to_string(),
                "Interop.StaticText(answer)".to_string(),
            )),
            CResult::Void | CResult::Status => {}
        }

        for (buffer, label, element) in &lists {
            results.push((
                label.clone(),
                format!("{}[]", sharp_type(element, self.shadowed)),
                format!("{buffer}Taken"),
            ));
        }

        let single = results.len() == 1;
        let mut result = match results.len() {
            0 => "void".to_string(),
            1 => results[0].1.clone(),
            _ => format!(
                "({})",
                results
                    .iter()
                    .map(|(name, ty, _)| format!("{ty} {}", names::uncapitalize(name)))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        };
        // "There is nothing here" is an answer, so it comes back as null.
        // A reference type is already nullable with a `?`; a value type needs
        // `Nullable<T>`, which the same `?` spells.
        if function.optional && result != "void" {
            result.push('?');
        }

        let mut lines = Lines::new();
        for (checked, asked) in &guarded {
            if function.fallible() {
                lines.push(&format!("{checked};"));
                continue;
            }
            let zero = match &function.result {
                CResult::Value(ty) => sharp_zero(ty)?,
                other => {
                    return Err(format!(
                        "`{}` takes an object and returns `{other:?}`, so it has no way to say \
                         the object came from another document",
                        function.symbol
                    ));
                }
            };
            lines.open(&format!("if (!{asked})"));
            lines.push(&format!("return {zero};"));
            lines.close();
        }

        if scratch {
            lines.push("var scratch = new Interop.Scratch();");
            lines.open("try");
        }
        for line in &pre {
            lines.push(line);
        }
        self.invoke(&mut lines, &args, &lists, &result)?;
        for (buffer, _, element) in &lists {
            let sharp = sharp_type(element, self.shadowed);
            lines.push(&format!("var {buffer}Taken = new {sharp}[(int)count];"));
            lines.open("for (int slot = 0; slot < (int)count; slot++)");
            lines.push(&format!(
                "{buffer}Taken[slot] = {};",
                from_c(
                    element,
                    &format!("{buffer}[slot]"),
                    &self.owner,
                    self.shadowed
                )
            ));
            lines.close();
        }
        match results.len() {
            0 => {}
            1 if single => lines.push(&format!("return {};", results[0].2)),
            _ => lines.push(&format!(
                "return ({});",
                results
                    .iter()
                    .map(|(_, _, expression)| expression.clone())
                    .collect::<Vec<_>>()
                    .join(", ")
            )),
        }
        if scratch {
            lines.close();
            lines.open("finally");
            lines.push("scratch.Dispose();");
            lines.close();
        }

        Ok(Rendered {
            params,
            result,
            body: lines.out,
        })
    }

    /// Writes an argument the caller supplies.
    #[allow(clippy::too_many_arguments)]
    fn input(
        &self,
        param: &Param,
        sharp: &str,
        local: &str,
        params: &mut Vec<String>,
        pre: &mut Vec<String>,
        args: &mut Vec<String>,
        length: &mut Option<String>,
        guarded: &mut Vec<(String, String)>,
        scratch: &mut bool,
    ) -> Result<(), String> {
        // A handle is an index into one document's arena, and two documents
        // issue the same indices, so an object from elsewhere would resolve
        // to an unrelated object here rather than failing. Only the C# value
        // knows where it came from, so every object a caller supplies is
        // checked.
        if self.owner != "null" {
            match &param.ty {
                Type::Node => guarded.push((
                    format!("Interop.RequireSameDocument({}, {sharp})", self.owner),
                    format!("Interop.SameDocument({}, {sharp})", self.owner),
                )),
                Type::List(inner) if **inner == Type::Node => guarded.push((
                    format!("Interop.RequireSameDocumentAll({}, {sharp})", self.owner),
                    format!("Interop.SameDocumentAll({}, {sharp})", self.owner),
                )),
                _ => {}
            }
        }
        match (&param.ty, param.optional) {
            (Type::Text, optional) => {
                *scratch = true;
                params.push(format!("string{} {sharp}", if optional { "?" } else { "" }));
                pre.push(format!("var {local} = scratch.Utf8({sharp});"));
                args.push(local.to_string());
            }
            (Type::Node, true) => {
                params.push(format!("{ROOT}? {sharp}"));
                pre.push(format!(
                    "var {local} = {sharp}?.Handle ?? Native.otio_node_none();"
                ));
                args.push(local.to_string());
            }
            (Type::Node, false) => {
                params.push(format!("{ROOT} {sharp}"));
                args.push(format!("{sharp}.Handle"));
            }
            (Type::Struct(name), true) => {
                *scratch = true;
                params.push(format!(
                    "{}? {sharp}",
                    qualified(&value_name(name), self.shadowed)
                ));
                pre.push(format!(
                    "var {local} = {sharp} is null ? IntPtr.Zero : scratch.Struct({sharp}.Value{});",
                    if self.needs_scratch(name) {
                        ".ToNative(scratch)"
                    } else {
                        ".ToNative()"
                    }
                ));
                args.push(local.to_string());
            }
            (Type::Struct(name), false) => {
                if self.needs_scratch(name) {
                    *scratch = true;
                    params.push(format!(
                        "{} {sharp}",
                        qualified(&value_name(name), self.shadowed)
                    ));
                    pre.push(format!("var {local} = {sharp}.ToNative(scratch);"));
                    args.push(local.to_string());
                } else {
                    params.push(format!(
                        "{} {sharp}",
                        qualified(&value_name(name), self.shadowed)
                    ));
                    args.push(format!("{sharp}.ToNative()"));
                }
            }
            (Type::List(element), _) => {
                params.push(format!("{}[] {sharp}", sharp_type(element, self.shadowed)));
                pre.push(format!(
                    "var {local} = new {}[{sharp}.Length];",
                    c_type(element).trim_end_matches('?')
                ));
                pre.push(format!("for (int slot = 0; slot < {sharp}.Length; slot++)"));
                pre.push("{".to_string());
                pre.push(format!(
                    "{TAB}{local}[slot] = {};",
                    to_c(element, &format!("{sharp}[slot]"))
                ));
                pre.push("}".to_string());
                args.push(local.to_string());
                *length = Some(format!("(nuint){sharp}.Length"));
            }
            (Type::Bytes | Type::Document, _) => {
                return Err(format!(
                    "`{}` takes a `{}` as an argument, which this does not write",
                    self.function.symbol,
                    param.ty.c_name()
                ));
            }
            (ty, _) => {
                params.push(format!("{} {sharp}", sharp_type(ty, self.shadowed)));
                args.push(to_c(ty, sharp));
            }
        }
        Ok(())
    }

    /// Whether a value struct needs the scratch space to reach C, which is
    /// true exactly where it carries text.
    fn needs_scratch(&self, c_name: &str) -> bool {
        self.api
            .structure(c_name)
            .is_some_and(|item| item.fields.iter().any(|field| field.ty == Type::Text))
    }

    /// Writes the call itself, and the two-pass dance where it answers with a
    /// list.
    fn invoke(
        &self,
        lines: &mut Lines,
        args: &[String],
        lists: &[(String, String, Type)],
        result: &str,
    ) -> Result<(), String> {
        let symbol = &self.function.symbol;
        let keep = if self.owner == "null" {
            None
        } else {
            Some(format!("GC.KeepAlive({});", self.owner))
        };

        if lists.is_empty() {
            let call = format!("Native.{symbol}({})", args.join(", "));
            match &self.function.result {
                CResult::Status => {
                    lines.push(&format!("var status = {call};"));
                    if let Some(line) = &keep {
                        lines.push(line);
                    }
                    if self.function.optional && result != "void" {
                        lines.open("if (status == Status.NoValue)");
                        lines.push("return null;");
                        lines.close();
                    }
                    lines.push("Interop.Check(status);");
                }
                CResult::Void => {
                    lines.push(&format!("{call};"));
                    if let Some(line) = &keep {
                        lines.push(line);
                    }
                }
                CResult::Value(_) | CResult::StaticText => {
                    lines.push(&format!("var answer = {call};"));
                    if let Some(line) = &keep {
                        lines.push(line);
                    }
                }
            }
            return Ok(());
        }

        let check = |lines: &mut Lines, call: String| {
            lines.push(&format!("Interop.Check({call});"));
        };

        lines.push("nuint count;");
        let room = if let Some(sizer) = self.function.sized_by.as_deref() {
            // This call empties what it reports, so it cannot be asked twice.
            // Another call says how long the answer will be, and this one is
            // made once into a buffer that size.
            lines.push(&format!(
                "// {symbol} answers and empties in one go, so the buffer is sized first."
            ));
            lines.push("nuint room;");
            check(lines, self.sizing_call(sizer)?);
            "(int)room".to_string()
        } else {
            let sized: Vec<String> = args
                .iter()
                .map(|argument| {
                    if argument.starts_with("{list") {
                        "null".to_string()
                    } else if argument == "{capacity}" {
                        "0".to_string()
                    } else {
                        argument.clone()
                    }
                })
                .collect();
            check(lines, format!("Native.{symbol}({})", sized.join(", ")));
            "(int)count".to_string()
        };

        for (buffer, _, element) in lists {
            lines.push(&format!(
                "var {buffer} = new {}[{room}];",
                c_type(element).trim_end_matches('?')
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
                    return lists[index].0.clone();
                }
                if argument == "{capacity}" {
                    return format!("(nuint){}.Length", lists[0].0);
                }
                argument.clone()
            })
            .collect();
        check(lines, format!("Native.{symbol}({})", filled.join(", ")));
        if let Some(line) = &keep {
            lines.push(line);
        }
        // A document does not change between the two calls, so this cannot
        // trip; it is here so that a mistaken count is a short array rather
        // than a crash in someone else's program.
        lines.push(&format!(
            "if ((int)count > {}.Length) {{ count = (nuint){}.Length; }}",
            lists[0].0, lists[0].0
        ));
        Ok(())
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
                ParamRole::Output => args.push("out room".to_string()),
                _ => {
                    return Err(format!(
                        "`{sizer}` takes a `{}`, so it cannot size another call's answer",
                        param.name
                    ));
                }
            }
        }
        Ok(format!("Native.{sizer}({})", args.join(", ")))
    }
}

/// Words C# will not let a parameter be called, and what to call them
/// instead.
///
/// They are words, not decorations, because the name is what someone reading
/// the documentation and writing a named argument sees.
const RENAMED: &[(&str, &str)] = &[
    ("in", "within"),
    ("out", "result"),
    ("ref", "reference"),
    ("params", "arguments"),
    ("default", "fallback"),
    ("base", "parent"),
    ("this", "self"),
    ("object", "item"),
    ("string", "text"),
    ("int", "number"),
    ("bool", "flag"),
    ("double", "number"),
    ("long", "number"),
    ("byte", "octet"),
    ("char", "character"),
    ("class", "kind"),
    ("struct", "record"),
    ("enum", "choice"),
    ("event", "occurrence"),
    ("delegate", "handler"),
    ("interface", "shape"),
    ("namespace", "grouping"),
    ("operator", "operation"),
    ("override", "replacement"),
    ("readonly", "constant"),
    ("return", "answer"),
    ("switch", "choice"),
    ("case", "branch"),
    ("checked", "verified"),
    ("fixed", "pinned"),
    ("lock", "guard"),
    ("new", "created"),
    ("null", "nothing"),
    ("true", "yes"),
    ("false", "no"),
    ("is", "matches"),
    ("as", "asType"),
    ("do", "perform"),
    ("else", "otherwise"),
    ("if", "when"),
    ("while", "until"),
    ("for", "loop"),
    ("foreach", "each"),
    ("try", "attempt"),
    ("catch", "rescue"),
    ("finally", "afterwards"),
    ("throw", "raised"),
    ("using", "used"),
    ("static", "shared"),
    ("void", "nothing"),
    ("where", "condition"),
    // Names the generated bodies use themselves, which a parameter of the
    // same name would shadow.
    ("count", "howMany"),
    ("status", "outcome"),
    ("room", "capacity"),
    ("scratch", "spare"),
    ("slot", "position"),
    ("subject", "receiver"),
    ("answer", "outcome"),
];

/// What a parameter is called in C#.
fn parameter_name(name: &str) -> String {
    let spelled = names::camel_with(name, INITIALISMS);
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
/// arrays half of which name nothing; written by hand it is a dictionary from
/// the objects the caller already holds to their new ones.
///
/// A symbol here is still in the description and still checked for a name
/// collision, so the hand-written version cannot quietly diverge from the
/// call it stands for.
const BY_HAND: &[&str] = &["otio_document_absorb"];

/// Names this SDK writes by hand, which a generated one may not take.
const RESERVED: &[(&str, &str)] = &[
    ("Document", "Absorb"),
    ("Document", "Close"),
    ("Document", "Dispose"),
    ("Document", "Open"),
    ("Document", "Save"),
    ("Document", "Pointer"),
    ("object:SerializableObject", "Document"),
    ("object:SerializableObject", "DocumentPointer"),
    ("object:SerializableObject", "Handle"),
    ("object:SerializableObject", "IsA"),
    ("object:SerializableObject", "GetHashCode"),
    ("object:SerializableObject", "ToString"),
    ("object:SerializableObjectWithMetadata", "Metadata"),
];

impl Backend<'_> {
    /// Puts a header and the usings around generated C#, and names the file it
    /// goes in.
    fn assemble(&self, name: &str, body: String) -> File {
        let _ = self;
        let mut out = String::new();
        out.push_str("// Code generated by otio-sdk-gen from crates/otio-capi. DO NOT EDIT.\n\n");
        out.push_str("using System;\n");
        out.push_str("using System.Collections.Generic;\n");
        out.push_str("using System.Runtime.InteropServices;\n\n");
        out.push_str("namespace OpenTimelineIO;\n\n");
        out.push_str(body.trim_end());
        out.push('\n');
        File {
            path: PathBuf::from(DIR).join(SOURCES).join(name),
            contents: out,
        }
    }

    /// Rewrites a doc comment into C#'s shape: an XML summary, the
    /// interface's own symbols spelled the way this SDK spells them, `null`
    /// left as `null` because that is what C# says too.
    fn doc(&self, docs: &Docs, notes: &[String], symbol: Option<&str>) -> Vec<String> {
        let mut paragraphs: Vec<String> = Vec::new();
        let summary = self.rewrite(&docs.summary);
        if summary.is_empty() {
            if symbol.is_some() {
                paragraphs.push("Wraps the interface's call of the same name.".to_string());
            } else {
                paragraphs.push("Part of the OpenTimelineIO interface.".to_string());
            }
        } else {
            paragraphs.push(summary);
        }
        for paragraph in &docs.body {
            paragraphs.push(self.rewrite(paragraph));
        }
        paragraphs.extend(notes.iter().cloned());
        if let Some(symbol) = symbol {
            paragraphs.push(format!("C: <c>{symbol}</c>"));
        }

        let mut lines = vec!["/// <summary>".to_string()];
        for line in wrap(&paragraphs[0], 70) {
            lines.push(format!("/// {line}"));
        }
        lines.push("/// </summary>".to_string());
        if paragraphs.len() > 1 {
            lines.push("/// <remarks>".to_string());
            for paragraph in &paragraphs[1..] {
                lines.push("/// <para>".to_string());
                for line in wrap(paragraph, 70) {
                    lines.push(format!("/// {line}"));
                }
                lines.push("/// </para>".to_string());
            }
            lines.push("/// </remarks>".to_string());
        }
        lines
    }

    /// Replaces the interface's own names with this SDK's, and makes the text
    /// safe to sit inside an XML doc comment.
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
            let sharp = &self.spellings[symbol];
            out = out.replace(&format!("`{symbol}`"), &format!("\u{1}{sharp}\u{2}"));
            out = out.replace(symbol.as_str(), sharp);
        }
        // Rust spells a link to another item `[`name`]`, which means nothing
        // in C#'s XML.
        out = out.replace("[`", "`").replace("`]", "`");
        // Everything left between backticks is code, and everything else has
        // to survive being read as XML.
        out = escape(&out);
        let mut spelled = String::new();
        let mut code = false;
        for character in out.chars() {
            match character {
                '`' => {
                    spelled.push_str(if code { "</c>" } else { "<c>" });
                    code = !code;
                }
                '\u{1}' => spelled.push_str("<c>"),
                '\u{2}' => spelled.push_str("</c>"),
                other => spelled.push(other),
            }
        }
        if code {
            spelled.push_str("</c>");
        }
        spelled
    }
}

/// Makes text safe to sit inside an XML doc comment.
fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
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

/// Writes a member of a type: its documentation, its signature and its body.
fn write_member(out: &mut String, doc: &[String], signature: &str, body: &[String]) {
    for line in doc {
        let _ = writeln!(out, "{TAB}{line}");
    }
    let _ = writeln!(out, "{TAB}{signature}");
    let _ = writeln!(out, "{TAB}{{");
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
    /// Writes every call of one group into one C# type.
    fn emit_group(&self, out: &mut String, group: &Group) -> Result<(), String> {
        self.emit_some(out, group, |_| true)
    }

    /// Writes the calls of one group that a filter accepts.
    fn emit_some(
        &self,
        out: &mut String,
        group: &Group,
        wanted: impl Fn(&Function) -> bool,
    ) -> Result<(), String> {
        for function in &group.functions {
            if self.skipped(function) || !wanted(function) {
                continue;
            }
            self.emit_function(out, group, function)?;
        }
        Ok(())
    }

    /// Writes one call.
    fn emit_function(
        &self,
        out: &mut String,
        group: &Group,
        function: &Function,
    ) -> Result<(), String> {
        let name = member_name(self.api, group, function);
        let mut is_static = false;
        let mut extension: Option<String> = None;
        let mut wrap_as: Option<String> = None;

        let (document, receiver, owner) = match (&group.receiver, function.role) {
            (Receiver::None, _) => {
                is_static = true;
                (String::new(), String::new(), "null".to_string())
            }
            (Receiver::Document, Role::Constructor | Role::Free) => {
                is_static = true;
                (String::new(), String::new(), "null".to_string())
            }
            (Receiver::Document, _) => (
                "this.Pointer".to_string(),
                String::new(),
                "this".to_string(),
            ),
            (Receiver::Node(schema), Role::Constructor) if takes_a_document(function) => {
                wrap_as = Some(schema_name(schema));
                (
                    "this.Pointer".to_string(),
                    String::new(),
                    "this".to_string(),
                )
            }
            (Receiver::Node(_), Role::Constructor) => {
                is_static = true;
                (String::new(), String::new(), "null".to_string())
            }
            (Receiver::Node(_), _) if group.view => (
                "this.Object.DocumentPointer".to_string(),
                "this.Object.Handle".to_string(),
                "this.Object.Document".to_string(),
            ),
            (Receiver::Node(_), _) => (
                "this.DocumentPointer".to_string(),
                "this.Handle".to_string(),
                "this.Document".to_string(),
            ),
            (Receiver::Value(_), Role::Constructor | Role::Free) => {
                is_static = true;
                (String::new(), String::new(), "null".to_string())
            }
            (Receiver::Value(what), _) => {
                if self.api.enumeration(what).is_some() {
                    // An enum carries no members of its own, so its calls are
                    // extension methods and the receiver is the argument.
                    is_static = true;
                    extension = Some(format!("this {} subject", enum_name(what)));
                    (String::new(), "subject".to_string(), "null".to_string())
                } else if self.needs_scratch(what) {
                    (
                        String::new(),
                        "this.ToNative(scratch)".to_string(),
                        "null".to_string(),
                    )
                } else {
                    (
                        String::new(),
                        "this.ToNative()".to_string(),
                        "null".to_string(),
                    )
                }
            }
        };

        let site = Site {
            api: self.api,
            shadowed: &self.shadowed,
            function,
            document,
            receiver,
            owner,
            wrap: wrap_as,
        };
        let rendered = site.render()?;

        // Where the interface's prose already says an argument may be absent,
        // it is left to say it rather than said twice.
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
                    "A null {} means none.",
                    parameter_name(&param.name)
                ));
            }
        }
        let says_no_value = std::iter::once(&function.docs.summary)
            .chain(function.docs.body.iter())
            .any(|paragraph| paragraph.contains("NO_VALUE"));
        if function.optional && !says_no_value {
            notes.push(if rendered.result == "void" {
                "Where there was nothing to do this throws an OtioException whose status is \
                 Status.NoValue, which is an answer rather than a failure."
                    .to_string()
            } else {
                "Where there is nothing to report this answers null, which is an answer rather \
                 than a failure."
                    .to_string()
            });
        }
        let doc = self.doc(&function.docs, &notes, Some(&function.symbol));

        let mut params = rendered.params;
        if let Some(receiver) = extension {
            params.insert(0, receiver);
        }
        let signature = format!(
            "public {}{} {name}({})",
            if is_static { "static " } else { "" },
            rendered.result,
            params.join(", ")
        );
        write_member(out, &doc, &signature, &rendered.body);
        Ok(())
    }

    /// Whether a value struct needs the scratch space to reach C.
    fn needs_scratch(&self, c_name: &str) -> bool {
        self.api
            .structure(c_name)
            .is_some_and(|item| item.fields.iter().any(|field| field.ty == Type::Text))
    }
}

impl Backend<'_> {
    /// The `DllImport` declarations and the structs that cross the boundary.
    fn interop(&self) -> Result<String, String> {
        let mut out = String::from(INTEROP);
        let _ = writeln!(out, "internal static partial class Native\n{{");
        let _ = writeln!(
            out,
            "{TAB}/// <summary>The library every call goes to.</summary>"
        );
        let _ = writeln!(out, "{TAB}internal const string Library = \"otio\";\n");

        for item in &self.api.structs {
            let _ = writeln!(
                out,
                "{TAB}/// <summary>The C interface's own <c>{}</c>.</summary>",
                item.name
            );
            let _ = writeln!(out, "{TAB}[StructLayout(LayoutKind.Sequential)]");
            let _ = writeln!(out, "{TAB}internal struct {}", item.name);
            let _ = writeln!(out, "{TAB}{{");
            for field in &item.fields {
                let _ = writeln!(
                    out,
                    "{TAB}{TAB}internal {} {};",
                    c_type(&field.ty),
                    field.name
                );
            }
            let _ = writeln!(out, "{TAB}}}\n");
        }

        for group in &self.api.groups {
            for function in &group.functions {
                self.emit_extern(&mut out, function)?;
            }
        }
        let _ = writeln!(out, "}}");
        Ok(out)
    }

    /// Writes one `DllImport`.
    fn emit_extern(&self, out: &mut String, function: &Function) -> Result<(), String> {
        let mut params: Vec<String> = Vec::new();
        for param in &function.params {
            let name = parameter_name(&param.name);
            let spelled = match param.role {
                ParamRole::DocumentIn | ParamRole::DocumentMut => format!("IntPtr {name}"),
                ParamRole::DocumentTaken => format!("ref IntPtr {name}"),
                ParamRole::Receiver => format!("{} {name}", c_type(&param.ty)),
                ParamRole::Length | ParamRole::ListCapacity => format!("nuint {name}"),
                ParamRole::OutputCount => format!("out nuint {name}"),
                ParamRole::OutputList => {
                    let Type::List(element) = &param.ty else {
                        return Err(format!("`{}` has a list that is not one", function.symbol));
                    };
                    format!(
                        "[In, Out] {}[]? {name}",
                        c_type(element).trim_end_matches('?')
                    )
                }
                ParamRole::Output => format!("out {} {name}", out_type(&param.ty)),
                ParamRole::Bytes => format!("byte[]? {name}"),
                ParamRole::Input => match (&param.ty, param.optional) {
                    // An optional struct arrives as a pointer C reads or not,
                    // and the scratch space is what holds it still.
                    (Type::Struct(_), true) => format!("IntPtr {name}"),
                    (ty, _) => format!("{} {name}", c_type(ty)),
                },
            };
            params.push(spelled);
        }
        let returns = match &function.result {
            CResult::Void => "void".to_string(),
            CResult::Status => "Status".to_string(),
            CResult::StaticText => "IntPtr".to_string(),
            CResult::Value(ty) => c_type(ty),
        };
        let _ = writeln!(
            out,
            "{TAB}[DllImport(Library, CallingConvention = CallingConvention.Cdecl)]"
        );
        let _ = writeln!(
            out,
            "{TAB}internal static extern {returns} {}({});\n",
            function.symbol,
            params.join(", ")
        );
        Ok(())
    }

    /// The free functions and the plumbing every other file calls.
    fn runtime(&self) -> Result<String, String> {
        let mut out = String::from(RUNTIME);
        let _ = writeln!(
            out,
            "/// <summary>Everything the library offers that belongs to no object.</summary>"
        );
        let _ = writeln!(out, "public static class Otio\n{{");
        let mut body = String::new();
        for group in &self.api.groups {
            if group.receiver == Receiver::None {
                self.emit_group(&mut body, group)?;
            }
            // A constructor lifted off an enum has nowhere else to live.
            if let Receiver::Value(what) = &group.receiver {
                if self.api.enumeration(what).is_some() {
                    self.emit_some(&mut body, group, |function| {
                        matches!(function.role, Role::Constructor | Role::Free)
                    })?;
                }
            }
        }
        out.push_str(body.trim_end());
        let _ = writeln!(out, "\n}}\n");
        Ok(out)
    }

    /// The enums, as real C# enums with the C interface's own values.
    fn enums(&self) -> Result<String, String> {
        let mut out = String::new();
        for item in &self.api.enums {
            self.emit_enum(&mut out, item)?;
        }
        Ok(out)
    }

    fn emit_enum(&self, out: &mut String, item: &Enum) -> Result<(), String> {
        let sharp = enum_name(&item.name);
        for line in self.doc(&item.docs, &[], None) {
            let _ = writeln!(out, "{line}");
        }
        let _ = writeln!(out, "public enum {sharp}\n{{");
        for (index, variant) in item.variants.iter().enumerate() {
            if index > 0 {
                out.push('\n');
            }
            for line in self.doc(&variant.docs, &[], None) {
                let _ = writeln!(out, "{TAB}{line}");
            }
            let _ = writeln!(
                out,
                "{TAB}{} = {},",
                variant_name(&variant.name),
                variant.value
            );
        }
        let _ = writeln!(out, "}}\n");

        let _ = writeln!(
            out,
            "/// <summary>What a <c>{sharp}</c> can be asked.</summary>"
        );
        let _ = writeln!(out, "public static class {sharp}Extensions\n{{");
        let _ = writeln!(
            out,
            "{TAB}/// <summary>The name the C interface spells this value by.</summary>"
        );
        let _ = writeln!(
            out,
            "{TAB}public static string CName(this {sharp} subject) => subject switch"
        );
        let _ = writeln!(out, "{TAB}{{");
        for variant in &item.variants {
            let _ = writeln!(
                out,
                "{TAB}{TAB}{sharp}.{} => \"{}\",",
                variant_name(&variant.name),
                variant.c_name
            );
        }
        let _ = writeln!(out, "{TAB}{TAB}_ => $\"{sharp}({{(int)subject}})\",");
        let _ = writeln!(out, "{TAB}}};\n");
        let mut body = String::new();
        for group in &self.api.groups {
            if let Receiver::Value(what) = &group.receiver {
                if what == &item.name {
                    self.emit_some(&mut body, group, |function| {
                        !matches!(function.role, Role::Constructor | Role::Free)
                    })?;
                }
            }
        }
        out.push_str(body.trim_end());
        let _ = writeln!(out, "\n}}\n");
        Ok(())
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
            self.emit_value(&mut out, item)?;
        }
        Ok(out)
    }

    #[allow(clippy::too_many_lines)]
    fn emit_value(&self, out: &mut String, item: &Struct) -> Result<(), String> {
        let sharp = value_name(&item.name);
        for line in self.doc(&item.docs, &[], None) {
            let _ = writeln!(out, "{line}");
        }
        let _ = writeln!(out, "public readonly struct {sharp}\n{{");

        for field in &item.fields {
            for line in self.doc(&field.docs, &[], None) {
                let _ = writeln!(out, "{TAB}{line}");
            }
            let _ = writeln!(
                out,
                "{TAB}public {} {} {{ get; }}\n",
                sharp_type(&field.ty, &self.shadowed),
                member_case(&field.name)
            );
        }

        let arguments: Vec<String> = item
            .fields
            .iter()
            .map(|field| {
                format!(
                    "{} {}",
                    sharp_type(&field.ty, &self.shadowed),
                    parameter_name(&field.name)
                )
            })
            .collect();
        let _ = writeln!(out, "{TAB}/// <summary>Makes one from its parts.</summary>");
        let _ = writeln!(out, "{TAB}public {sharp}({})", arguments.join(", "));
        let _ = writeln!(out, "{TAB}{{");
        for field in &item.fields {
            let _ = writeln!(
                out,
                "{TAB}{TAB}this.{} = {};",
                member_case(&field.name),
                parameter_name(&field.name)
            );
        }
        let _ = writeln!(out, "{TAB}}}\n");

        // Out of C.
        let _ = writeln!(
            out,
            "{TAB}/// <summary>Reads the value back out of the C interface.</summary>"
        );
        let read: Vec<String> = item
            .fields
            .iter()
            .map(|field| {
                let source = format!("value.{}", field.name);
                match &field.ty {
                    Type::Text => format!("Interop.StaticText({source})"),
                    ty => from_c(ty, &source, "null", &self.shadowed),
                }
            })
            .collect();
        let _ = writeln!(
            out,
            "{TAB}internal static {sharp} FromNative(Native.{} value) =>",
            item.name
        );
        let _ = writeln!(out, "{TAB}{TAB}new {sharp}({});\n", read.join(", "));

        // Into C. Text is copied into the scratch space, which the call that
        // borrows it frees as it unwinds, so a caller cannot leak one and the
        // library cannot keep one.
        let takes_scratch = item.fields.iter().any(|field| field.ty == Type::Text);
        let _ = writeln!(
            out,
            "{TAB}/// <summary>Spells the value the way the C interface wants it.</summary>"
        );
        let _ = writeln!(
            out,
            "{TAB}internal Native.{} ToNative({}) =>",
            item.name,
            if takes_scratch {
                "Interop.Scratch scratch"
            } else {
                ""
            }
        );
        let _ = writeln!(out, "{TAB}{TAB}new Native.{}", item.name);
        let _ = writeln!(out, "{TAB}{TAB}{{");
        for field in &item.fields {
            let source = format!("this.{}", member_case(&field.name));
            let converted = match &field.ty {
                Type::Text => format!("scratch.Utf8({source}.Length == 0 ? null : {source})"),
                Type::Struct(_) => format!("{source}.ToNative()"),
                ty => to_c(ty, &source),
            };
            let _ = writeln!(out, "{TAB}{TAB}{TAB}{} = {converted},", field.name);
        }
        let _ = writeln!(out, "{TAB}{TAB}}};\n");

        let parts: Vec<String> = item
            .fields
            .iter()
            .map(|field| {
                format!(
                    "{}={{this.{}}}",
                    member_case(&field.name),
                    member_case(&field.name)
                )
            })
            .collect();
        let _ = writeln!(
            out,
            "{TAB}/// <summary>What the value holds, for a message or a log.</summary>"
        );
        let _ = writeln!(
            out,
            "{TAB}public override string ToString() => $\"{sharp}({})\";\n",
            parts.join(", ")
        );

        let mut body = String::new();
        for group in &self.api.groups {
            if let Receiver::Value(what) = &group.receiver {
                if what == &item.name {
                    self.emit_group(&mut body, group)?;
                }
            }
        }
        out.push_str(body.trim_end());
        let _ = writeln!(out, "\n}}\n");
        Ok(())
    }

    /// The schema ladder, as classes deriving as the schemas derive.
    fn schema(&self) -> String {
        let mut out = String::new();
        for schema in &self.api.schema {
            let Some(parent) = schema.parent.as_deref() else {
                continue;
            };
            let sharp = schema_name(&schema.name);
            for line in self.doc(&schema.docs, &[], None) {
                let _ = writeln!(out, "{line}");
            }
            let _ = writeln!(
                out,
                "public partial class {sharp} : {}",
                schema_name(parent)
            );
            let _ = writeln!(out, "{{");
            let _ = writeln!(
                out,
                "{TAB}internal {sharp}(Document? document, Native.OtioNode handle)"
            );
            let _ = writeln!(out, "{TAB}{TAB}: base(document, handle)");
            let _ = writeln!(out, "{TAB}{{");
            let _ = writeln!(out, "{TAB}}}");
            let _ = writeln!(out, "}}\n");
        }

        let _ = writeln!(
            out,
            "/// <summary>\n\
             /// Which schema each one derives from, so that asking whether an object is an\n\
             /// Item can say yes for a clip.\n\
             /// </summary>\n\
             internal static class Schemas\n{{"
        );
        let _ = writeln!(
            out,
            "{TAB}internal static readonly Dictionary<NodeKind, NodeKind> Parents = new()"
        );
        let _ = writeln!(out, "{TAB}{{");
        for schema in &self.api.schema {
            let Some(parent) = schema.parent.as_deref() else {
                continue;
            };
            let Some(above) = self.api.schema.iter().find(|item| item.name == parent) else {
                continue;
            };
            let _ = writeln!(
                out,
                "{TAB}{TAB}{{ NodeKind.{}, NodeKind.{} }},",
                variant_name(&schema.kind),
                variant_name(&above.kind)
            );
        }
        let _ = writeln!(out, "{TAB}}};\n");

        let _ = writeln!(
            out,
            "{TAB}/// <summary>Builds the class an object's schema names.</summary>\n\
             {TAB}internal static {ROOT} Make(Document? document, Native.OtioNode handle)\n\
             {TAB}{{\n\
             {TAB}{TAB}if (document is null || document.Pointer == IntPtr.Zero)\n\
             {TAB}{TAB}{{\n\
             {TAB}{TAB}{TAB}return new {ROOT}(document, handle);\n\
             {TAB}{TAB}}}\n\
             {TAB}{TAB}var status = Native.otio_node_kind(document.Pointer, handle, out var kind);\n\
             {TAB}{TAB}GC.KeepAlive(document);\n\
             {TAB}{TAB}if (status != Status.Ok)\n\
             {TAB}{TAB}{{\n\
             {TAB}{TAB}{TAB}return new {ROOT}(document, handle);\n\
             {TAB}{TAB}}}\n\
             {TAB}{TAB}return kind switch\n\
             {TAB}{TAB}{{"
        );
        for schema in &self.api.schema {
            if schema.parent.is_none() {
                continue;
            }
            let _ = writeln!(
                out,
                "{TAB}{TAB}{TAB}NodeKind.{} => new {}(document, handle),",
                variant_name(&schema.kind),
                schema_name(&schema.name)
            );
        }
        let _ = writeln!(out, "{TAB}{TAB}{TAB}_ => new {ROOT}(document, handle),");
        let _ = writeln!(out, "{TAB}{TAB}}};");
        let _ = writeln!(out, "{TAB}}}");
        let _ = writeln!(out, "}}\n");
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
            let mut body = String::new();
            // A constructor that needs a document to build in is written with
            // the documents; one that does not belongs to the type.
            self.emit_some(&mut body, group, |function| {
                function.role != Role::Constructor || !takes_a_document(function)
            })?;
            if body.trim().is_empty() {
                continue;
            }
            let _ = writeln!(out, "public partial class {}\n{{", schema_name(schema));
            out.push_str(body.trim_end());
            let _ = writeln!(out, "\n}}\n");
        }
        Ok(out)
    }

    /// The calls that are methods on a document, and the ones that make one.
    fn documents(&self) -> Result<String, String> {
        let mut out = String::new();
        let mut body = String::new();
        for group in &self.api.groups {
            if group.receiver == Receiver::Document {
                self.emit_group(&mut body, group)?;
            }
        }
        for group in &self.api.groups {
            if matches!(group.receiver, Receiver::Node(_)) && !group.view {
                self.emit_some(&mut body, group, |function| {
                    function.role == Role::Constructor && takes_a_document(function)
                })?;
            }
        }
        let _ = writeln!(out, "public sealed partial class Document\n{{");
        out.push_str(body.trim_end());
        let _ = writeln!(out, "\n}}\n");
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
            let sharp = &group.name;
            for line in self.doc(&group.docs, &[], None) {
                let _ = writeln!(out, "{line}");
            }
            let _ = writeln!(out, "public sealed class {sharp}\n{{");
            let _ = writeln!(
                out,
                "{TAB}internal {sharp}({ROOT} owner)\n\
                 {TAB}{{\n\
                 {TAB}{TAB}this.Object = owner;\n\
                 {TAB}}}\n"
            );
            let _ = writeln!(
                out,
                "{TAB}/// <summary>The object whose metadata this is.</summary>\n\
                 {TAB}internal {ROOT} Object {{ get; }}\n"
            );
            let mut body = String::new();
            self.emit_group(&mut body, group)?;
            out.push_str(body.trim_end());
            let _ = writeln!(out, "\n}}\n");

            let _ = writeln!(out, "public partial class {}\n{{", schema_name(schema));
            let _ = writeln!(
                out,
                "{TAB}/// <summary>\n\
                 {TAB}/// The object's metadata, which is a dictionary of its own.\n\
                 {TAB}/// </summary>\n\
                 {TAB}/// <remarks>\n\
                 {TAB}/// <para>\n\
                 {TAB}/// A path names a value inside it, a step at a time, separated by dots:\n\
                 {TAB}/// <c>cmx_3600.reel</c> reaches the reel of the dictionary the EDL adapter\n\
                 {TAB}/// left behind, and <c>takes[0]</c> the first entry of a list.\n\
                 {TAB}/// </para>\n\
                 {TAB}/// <para>\n\
                 {TAB}/// A path is followed, not created. Writing one step deep always works,\n\
                 {TAB}/// but a deeper one needs its dictionary to exist first.\n\
                 {TAB}/// </para>\n\
                 {TAB}/// </remarks>\n\
                 {TAB}public {sharp} {sharp} => new {sharp}(this);\n\
                 }}\n"
            );
        }
        Ok(out)
    }
}

/// The interop plumbing: the scratch space, the error type's helpers, and the
/// conversions every generated call uses.
const INTEROP: &str = r#"/// <summary>
/// The plumbing between this SDK and the C interface.
/// </summary>
/// <remarks>
/// <para>
/// Nothing here is part of the SDK's surface. It is the one place that knows
/// how a string, a buffer or a handle crosses the boundary, so that the
/// generated calls do not each have to.
/// </para>
/// </remarks>
internal static class Interop
{
    /// <summary>
    /// Unmanaged memory lent to one call and freed as it unwinds.
    /// </summary>
    /// <remarks>
    /// <para>
    /// The C interface borrows every string and every optional struct for the
    /// length of the call and keeps none of them, so what a call needs can be
    /// copied out, handed over and released again without the caller ever
    /// seeing it.
    /// </para>
    /// </remarks>
    internal sealed class Scratch : IDisposable
    {
        private readonly List<IntPtr> blocks = new();

        /// <summary>Copies a string out as null-terminated UTF-8.</summary>
        /// <remarks>
        /// <para>
        /// A null string is no string at all, which is how the C interface
        /// spells an argument that may be left out.
        /// </para>
        /// </remarks>
        internal IntPtr Utf8(string? text)
        {
            if (text is null)
            {
                return IntPtr.Zero;
            }
            var block = Marshal.StringToCoTaskMemUTF8(text);
            this.blocks.Add(block);
            return block;
        }

        /// <summary>Lends a struct to a call that wants a pointer to one.</summary>
        internal IntPtr Struct<T>(T value) where T : struct
        {
            var block = Marshal.AllocCoTaskMem(Marshal.SizeOf<T>());
            this.blocks.Add(block);
            Marshal.StructureToPtr(value, block, false);
            return block;
        }

        /// <summary>Releases everything lent to the call.</summary>
        public void Dispose()
        {
            foreach (var block in this.blocks)
            {
                Marshal.FreeCoTaskMem(block);
            }
            this.blocks.Clear();
        }
    }

    /// <summary>Throws what the library said, if it said anything went wrong.</summary>
    internal static void Check(Status status)
    {
        if (status != Status.Ok)
        {
            throw new OtioException(status, StaticText(Native.otio_error_message()));
        }
    }

    /// <summary>Copies a string the library owns forever and nobody frees.</summary>
    internal static string StaticText(IntPtr pointer) =>
        pointer == IntPtr.Zero ? string.Empty : Marshal.PtrToStringUTF8(pointer) ?? string.Empty;

    /// <summary>Copies a buffer of text out of the library, and frees it.</summary>
    internal static string Text(Native.OtioBuffer buffer)
    {
        if (buffer.data == IntPtr.Zero)
        {
            return string.Empty;
        }
        var text = Marshal.PtrToStringUTF8(buffer.data, (int)buffer.len);
        Native.otio_buffer_free(buffer);
        return text;
    }

    /// <summary>Copies a buffer of bytes out of the library, and frees it.</summary>
    internal static byte[] Bytes(Native.OtioBuffer buffer)
    {
        if (buffer.data == IntPtr.Zero)
        {
            return Array.Empty<byte>();
        }
        var bytes = new byte[(int)buffer.len];
        Marshal.Copy(buffer.data, bytes, 0, bytes.Length);
        Native.otio_buffer_free(buffer);
        return bytes;
    }

    /// <summary>Builds the class an object's schema names.</summary>
    /// <remarks>
    /// <para>
    /// Every handle that comes back from the library goes through this, so a
    /// cast tells the truth: <c>node as Clip</c> succeeds exactly when the
    /// object really is a clip. An object whose kind cannot be read comes back
    /// as a plain SerializableObject rather than as a guess.
    /// </para>
    /// </remarks>
    internal static SerializableObject MakeObject(Document? document, Native.OtioNode handle) =>
        Schemas.Make(document, handle);

    /// <summary>Whether every object named belongs to a document.</summary>
    /// <remarks>
    /// <para>
    /// A handle is an index into one document's arena, and two documents issue
    /// the same indices, so an object from one would resolve to an unrelated
    /// object in another rather than failing. Nothing in the handle says where
    /// it came from: the C# object carries that, and this is where it is used.
    /// An object that is none belongs to no document and means "no object", so
    /// it is allowed everywhere.
    /// </para>
    /// </remarks>
    internal static bool SameDocument(Document? owner, SerializableObject? node) =>
        node is null || ReferenceEquals(node.Document, owner) || node.IsNone();

    /// <summary>SameDocument, as something to throw rather than something to ask.</summary>
    internal static void RequireSameDocument(Document? owner, SerializableObject? node)
    {
        if (!SameDocument(owner, node))
        {
            throw new OtioException(
                Status.InvalidArgument, "otio: the object belongs to another document");
        }
    }

    /// <summary>SameDocument, for a whole list of objects.</summary>
    internal static bool SameDocumentAll(Document? owner, SerializableObject[] nodes)
    {
        foreach (var node in nodes)
        {
            if (!SameDocument(owner, node))
            {
                return false;
            }
        }
        return true;
    }

    /// <summary>RequireSameDocument, for a whole list of objects.</summary>
    internal static void RequireSameDocumentAll(Document? owner, SerializableObject[] nodes)
    {
        foreach (var node in nodes)
        {
            RequireSameDocument(owner, node);
        }
    }

    /// <summary>The part of a path after its last dot, which names a format.</summary>
    internal static string Suffix(string path)
    {
        var name = path.Replace('\\', '/');
        var slash = name.LastIndexOf('/');
        if (slash >= 0)
        {
            name = name.Substring(slash + 1);
        }
        var dot = name.LastIndexOf('.');
        return dot <= 0 ? string.Empty : name.Substring(dot + 1);
    }

    /// <summary>The format a path's suffix names, or a failure saying none does.</summary>
    internal static Format FormatOf(string path)
    {
        var suffix = Suffix(path);
        var format = Otio.FormatFromSuffix(suffix);
        if (format is null)
        {
            throw new OtioException(
                Status.NoValue, $"otio: no format is written with the suffix .{suffix}");
        }
        return format.Value;
    }
}

"#;

/// The document, the object handle, the error type, and the two calls this
/// SDK writes itself.
const RUNTIME: &str = r#"/// <summary>A failure the library reported.</summary>
/// <remarks>
/// <para>
/// Where "there is nothing here" is one of the answers — an item with no
/// source range, a clip with no active media reference — the call answers
/// null instead of throwing, because that is an answer rather than a failure.
/// </para>
/// </remarks>
public sealed class OtioException : Exception
{
    /// <summary>Makes one from what the library said.</summary>
    public OtioException(Status status, string message)
        : base(string.IsNullOrEmpty(message) ? status.CName() : message)
    {
        this.Status = status;
    }

    /// <summary>What kind of failure it was.</summary>
    public Status Status { get; }
}

/// <summary>A document owns every object in a timeline.</summary>
/// <remarks>
/// <para>
/// It is the arena the core keeps its objects in, so an object is an index
/// into it rather than a pointer, and releasing the document releases the
/// whole graph at once. Handles into a released document go stale rather than
/// dangling.
/// </para>
/// <para>
/// A document is released when it is collected, so disposing is not required;
/// it is worth doing anyway, because it frees a whole timeline at once and at
/// a moment you chose. A document is not safe to use from two threads while
/// one of them is changing it.
/// </para>
/// </remarks>
public sealed partial class Document : IDisposable
{
    private IntPtr pointer;

    internal Document(IntPtr pointer)
    {
        this.pointer = pointer;
    }

    /// <summary>Releases the document if nobody disposed of it.</summary>
    ~Document()
    {
        this.Release();
    }

    /// <summary>The document the C interface knows, or zero once it has gone.</summary>
    internal IntPtr Pointer => this.pointer;

    /// <summary>Releases the document and every object in it.</summary>
    /// <remarks>
    /// <para>
    /// Calling it twice is harmless. Using an object of a released document is
    /// not: its handle no longer resolves, and calls made with it throw.
    /// </para>
    /// </remarks>
    public void Dispose()
    {
        this.Release();
        GC.SuppressFinalize(this);
    }

    /// <summary>Releases the document, as Dispose does.</summary>
    /// <remarks>
    /// <para>
    /// It is here because every other SDK generated from this interface spells
    /// it this way, and because a reader looking for the opposite of Open
    /// looks for Close.
    /// </para>
    /// </remarks>
    public void Close() => this.Dispose();

    private void Release()
    {
        if (this.pointer != IntPtr.Zero)
        {
            Native.otio_document_free(this.pointer);
            this.pointer = IntPtr.Zero;
        }
    }

    /// <summary>Reads a document from a file, working out its format from the name.</summary>
    /// <remarks>
    /// <para>
    /// It is the short way to say ReadFromFile when the suffix already says
    /// what the file holds, which is how upstream's read_from_file behaves
    /// when no adapter is named.
    /// </para>
    /// </remarks>
    public static Document Open(string path) =>
        Document.ReadFromFile(Interop.FormatOf(path), path, null);

    /// <summary>Writes the document to a file, working out its format from the name.</summary>
    /// <remarks>
    /// <para>
    /// It is the short way to say WriteToFile, as Open is for ReadFromFile.
    /// </para>
    /// </remarks>
    public void Save(string path) => this.WriteToFile(Interop.FormatOf(path), path, null);

    /// <summary>Moves every object in another document into this one.</summary>
    /// <remarks>
    /// <para>
    /// It is how an object built on its own joins a timeline: build a Clip in
    /// a document of its own, absorb that document into the one holding the
    /// timeline, and append the clip where it belongs. A handle means nothing
    /// outside the document it was issued for, so the objects are moved rather
    /// than pointed at, and every one of them arrives under a new handle.
    /// </para>
    /// <para>
    /// The source is consumed. On success it is emptied and closed, and the
    /// dictionary returned gives the new object for each object that came from
    /// it, so a handle held from before is translated by looking it up. On
    /// failure nothing moves and the source is left alone. The source's root is
    /// not adopted, because this document has its own.
    /// </para>
    /// <para>
    /// C: <c>otio_document_absorb</c>
    /// </para>
    /// </remarks>
    public Dictionary<SerializableObject, SerializableObject> Absorb(Document source)
    {
        if (this.Pointer == IntPtr.Zero || source.Pointer == IntPtr.Zero)
        {
            throw new OtioException(Status.NullPointer, "otio: the document is closed");
        }
        // The call cannot be asked twice to size its answer, because the first
        // ask would already have consumed the source. The source's own count is
        // exactly how many objects will move.
        var moving = (int)Native.otio_document_node_count(source.Pointer);
        var from = new Native.OtioNode[moving];
        var to = new Native.OtioNode[moving];
        var sourcePointer = source.pointer;
        var status = Native.otio_document_absorb(
            this.Pointer, ref sourcePointer, from, to, (nuint)moving, out var count);
        source.pointer = sourcePointer;
        GC.KeepAlive(this);
        GC.KeepAlive(source);
        Interop.Check(status);
        var taken = Math.Min((int)count, moving);
        var translated = new Dictionary<SerializableObject, SerializableObject>(taken);
        for (int index = 0; index < taken; index++)
        {
            translated[new SerializableObject(source, from[index])] =
                Interop.MakeObject(this, to[index]);
        }
        return translated;
    }
}

/// <summary>An object in a document: which object, and which document.</summary>
/// <remarks>
/// <para>
/// It is the root of the OTIO schema ladder, and every schema below it is a
/// class deriving from it, so a Clip has every member of an Item, a Composable
/// and a SerializableObjectWithMetadata. Every handle the library hands back
/// arrives as the class its schema names, so <c>node as Clip</c> asks what an
/// object really is and gets a true answer.
/// </para>
/// <para>
/// Two objects are equal when they are the same object of the same document. A
/// handle is a value here, so there may be several wrappers for one object and
/// equality is the question worth asking.
/// </para>
/// </remarks>
public partial class SerializableObject
{
    internal SerializableObject(Document? document, Native.OtioNode handle)
    {
        this.Document = document;
        this.Handle = handle;
    }

    /// <summary>The document the object lives in, or null for one that names none.</summary>
    public Document? Document { get; }

    /// <summary>The handle itself, which only the generated calls need.</summary>
    internal Native.OtioNode Handle { get; }

    /// <summary>
    /// Zero for an object that belongs to no document, so that a call made on
    /// one fails with a message rather than reaching into nothing.
    /// </summary>
    internal IntPtr DocumentPointer => this.Document?.Pointer ?? IntPtr.Zero;

    /// <summary>Whether the object is of a schema, or of one deriving from it.</summary>
    /// <remarks>
    /// <para>
    /// An object whose document has gone, or whose handle no longer resolves,
    /// is of no schema at all, so this answers false rather than guessing.
    /// </para>
    /// </remarks>
    public bool IsA(NodeKind schema)
    {
        if (this.DocumentPointer == IntPtr.Zero)
        {
            return false;
        }
        var status = Native.otio_node_kind(this.DocumentPointer, this.Handle, out var kind);
        GC.KeepAlive(this.Document);
        if (status != Status.Ok)
        {
            return false;
        }
        while (true)
        {
            if (kind == schema)
            {
                return true;
            }
            if (!Schemas.Parents.TryGetValue(kind, out var parent))
            {
                return false;
            }
            kind = parent;
        }
    }

    /// <summary>Whether another object is the same object of the same document.</summary>
    /// <remarks>
    /// <para>
    /// The library's own <c>Equals(SerializableObject)</c>, generated from
    /// <c>otio_node_equal</c>, asks the same question and gets the same
    /// answer; this one is here because the runtime needs it, and it answers
    /// without a call so that it still works once the document has gone.
    /// </para>
    /// </remarks>
    public override bool Equals(object? other) =>
        other is SerializableObject node
        && ReferenceEquals(this.Document, node.Document)
        && this.Handle.index == node.Handle.index
        && this.Handle.generation == node.Handle.generation;

    /// <inheritdoc/>
    public override int GetHashCode() =>
        HashCode.Combine(this.Document, this.Handle.index, this.Handle.generation);

    /// <summary>Whether two wrappers name the same object of the same document.</summary>
    public static bool operator ==(SerializableObject? left, SerializableObject? right) =>
        left is null ? right is null : left.Equals((object?)right);

    /// <summary>Whether two wrappers name different objects.</summary>
    public static bool operator !=(SerializableObject? left, SerializableObject? right) =>
        !(left == right);
}

"#;

/// The project file.
const PROJECT: &str = r#"<!-- Code generated by otio-sdk-gen from crates/otio-capi. DO NOT EDIT. -->
<Project Sdk="Microsoft.NET.Sdk">

  <PropertyGroup>
    <TargetFramework>net8.0</TargetFramework>
    <AssemblyName>OpenTimelineIO</AssemblyName>
    <RootNamespace>OpenTimelineIO</RootNamespace>
    <LangVersion>12</LangVersion>
    <Nullable>enable</Nullable>
    <ImplicitUsings>disable</ImplicitUsings>
    <GenerateDocumentationFile>true</GenerateDocumentationFile>
    <TreatWarningsAsErrors>true</TreatWarningsAsErrors>
  </PropertyGroup>

  <!--
    The native library is built by cargo and copied into `sdk/csharp/lib`,
    from where it travels beside whatever is built against this project; .NET
    looks for a DllImport's library next to the assembly that asked for it.
  -->
  <ItemGroup>
    <None Include="../lib/*.so;../lib/*.dylib;../lib/*.dll" CopyToOutputDirectory="PreserveNewest" Link="%(Filename)%(Extension)" />
  </ItemGroup>

</Project>
"#;

/// What keeps what `dotnet build` leaves behind out of the repository.
const BUILD_GITIGNORE: &str = "bin/\nobj/\n";

/// What keeps the built library out of the repository.
const LIB_GITIGNORE: &str =
    "# The native library the project loads, which CI builds.\n*\n!.gitignore\n";

/// The project's own documentation.
const README: &str = r#"# OpenTimelineIO for C#

Read, write and edit OpenTimelineIO timelines from C#.

```csharp
using OpenTimelineIO;
```

This project is generated from the C interface of the otio-rust core, so it
carries the whole data model: the schemas, the composition algorithms, the ten
edit operations and the file-format adapters. Do not edit the `.cs` files under
`OpenTimelineIO/` by hand — see [`../README.md`](../README.md) for how they are
made and regenerated.

## Building

The assembly loads a native `libotio` it expects to find beside itself, which
the project copies out of `lib/`:

```sh
cargo build -p otio-capi --release
cp target/release/libotio.so sdk/csharp/lib/     # libotio.dylib on macOS

cd sdk/csharp
dotnet run --project tests
```

The library itself is not checked in; `lib/.gitignore` keeps it out.

## Using it

Everything lives in a `Document`, which owns the objects in it:

```csharp
using var document = Document.Open("cut.edl");

var root = document.Root();
if (root is not null)
{
    foreach (var child in root.FindClips())
    {
        var clip = (Clip)child;
        Console.WriteLine($"{clip.Name()} {clip.Duration()}");
    }
}
```

An object is a class of its schema, so a cast asks what one really is:

```csharp
foreach (var child in track.Children())
{
    if (child is Clip clip && clip.MediaReference(null) is ExternalReference reference)
    {
        Console.WriteLine(reference.TargetUrl());
    }
}
```

A call that can fail throws an `OtioException` carrying a `Status`. Where
"there is nothing here" is one of the answers — an item with no source range, a
clip with no active media reference — the call answers `null` instead, because
that is an answer rather than a failure:

```csharp
if (clip.SourceRange() is TimeRange span)
{
    Console.WriteLine(span);
}
```

## What this follows, and where it differs

C# has no upstream OpenTimelineIO binding to copy, so what things are *called*
follows upstream's Python and C++ — the schema names, the member names, the
bare-noun getter and the `Set` prefix — spelled the way .NET spells names, and
what the binding *is* follows upstream's Java bindings, which are the nearest
thing upstream has to a managed language. Every deliberate departure is written
down in [ADR 0003](../../docs/adr/0003-sdk-generation.md).
"#;
