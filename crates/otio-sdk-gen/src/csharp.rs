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
//! - **No document in the surface.** An object is built on its own and joins
//!   a timeline when it is put into one, which is how upstream's own
//!   bindings read. The arena underneath is held by the objects that live in
//!   it and goes when the last of them does.
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
    Api, CResult, Docs, Enum, Function, Group, Param, ParamRole, Placement, Receiver, Role, Struct,
    Type,
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
        backend.assemble("Schema.cs", backend.schema()?),
        backend.assemble("Objects.cs", backend.objects()?),
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
        // A constructor's work happens in a private static beside it, because
        // C# runs a base constructor before the body.
        for schema in &self.api.schema {
            placed.push((
                format!("object:{}", schema.name),
                format!("Make{}", schema_name(&schema.name)),
                String::new(),
            ));
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
            || HIDDEN.iter().any(|(symbol, _)| *symbol == function.symbol)
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
        if let Some((owner, _)) = rehomed(&function.symbol) {
            return owner.to_string();
        }
        match (&group.receiver, function.role) {
            (Receiver::None, _) => "Otio".to_string(),
            // Nothing hangs off the document, because there is no document
            // to hang it off: what is left is a static member of `Otio`.
            (Receiver::Document, _) => "Otio".to_string(),
            (Receiver::Node(schema), Role::Constructor) => {
                if takes_a_document(function) {
                    // A constructor of the class it builds. C# does not
                    // inherit constructors, so `Clip(name)` and `Item(name)`
                    // do not collide the way two ordinary members on one line
                    // of descent would.
                    format!("ctor:{schema}")
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
    if let Some((_, name)) = rehomed(&function.symbol) {
        return name.to_string();
    }
    let spelled = member_case(&function.name);
    match (&group.receiver, function.role) {
        // A constructor is spelled as one, which is what `new Clip("shot_01")`
        // means. The name of a C# constructor is the name of its class.
        (Receiver::Node(schema), Role::Constructor) if takes_a_document(function) => {
            schema_name(schema)
        }
        // A free function or constructor lifted off an enum keeps the enum's
        // name, since `Otio.FromSuffix` would say nothing about what it makes.
        (Receiver::Value(what), Role::Constructor | Role::Free) if enum_receiver => {
            format!("{}{spelled}", enum_name(what))
        }
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
        // A whole document read out of a file is, to a caller, the object it
        // is about.
        Type::Node | Type::Document => ROOT.to_string(),
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
/// `owner` is the arena a handle belongs to, since an object in C# carries the
/// arena it can be resolved against rather than making its caller remember.
fn from_c(ty: &Type, value: &str, owner: &str, shadowed: &BTreeSet<String>) -> String {
    match ty {
        Type::Bool => format!("{value} != 0"),
        Type::Size => format!("(int){value}"),
        Type::Node => format!("Interop.MakeObject({owner}, {value})"),
        Type::Document => format!("Interop.RootOf({value})"),
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

/// Where a call gets the arena it is made in, now that a caller no longer
/// hands one over.
///
/// The description says which object the call is anchored on — see
/// `Param::anchor` — and this is what that looks like in C#.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Anchor {
    /// The call is a member, and happens where its object is.
    Receiver(String),
    /// The call is a member of an object the C ABI passes as an ordinary
    /// argument. The argument is the receiver and not a parameter.
    Argument(usize),
    /// The call is a static member, made in the arena of one of the objects
    /// it is handed.
    Named(String),
    /// The same, for a call handed a list of objects.
    List(String),
    /// The call writes a timeline out and is handed no object to say which.
    /// It takes one, and writing starts there.
    Root,
    /// The call builds something, so it makes an arena to build it in.
    Fresh,
    /// The call touches no arena at all.
    None,
}

/// The index of the parameter a call is anchored on.
fn anchor_index(function: &Function) -> Result<usize, String> {
    function
        .params
        .iter()
        .position(|param| param.anchor)
        .ok_or_else(|| {
            format!(
                "`{}` is about one of the objects it is handed, and the description does not \
                 say which",
                function.symbol
            )
        })
}

/// One call, being written into one place.
struct Site<'a> {
    api: &'a Api,
    /// Type names a generated member also answers to.
    shadowed: &'a BTreeSet<String>,
    function: &'a Function,
    /// Where the arena the call is made in comes from.
    anchor: Anchor,
    /// The C# expression for the handle or value the call is about.
    receiver: String,
    /// Whether the call is a constructor, which hands back the arena and the
    /// handle for the constructor to pass to its base rather than a finished
    /// object.
    builds: bool,
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
    /// The C# expression naming the arena an object the call hands back
    /// belongs to. A call made in no arena hands back objects of none.
    fn holder(&self) -> &'static str {
        if self.anchor == Anchor::None {
            "null"
        } else {
            "at.Arena"
        }
    }

    /// The line that finds the arena this call is made in.
    fn reach(&self) -> Vec<String> {
        let found = |what: String| vec![format!("var at = {what};")];
        match &self.anchor {
            Anchor::None => Vec::new(),
            Anchor::Receiver(object) => found(format!("Interop.Locate({object})")),
            // The argument became the receiver, so it is `this` by the time
            // the member is written.
            Anchor::Argument(_) => found("Interop.Locate(this)".to_string()),
            Anchor::Named(name) => found(format!("Interop.Locate({name})")),
            Anchor::List(name) => found(format!("Interop.LocateAll({name})")),
            Anchor::Root => found("Interop.RootedAt(root)".to_string()),
            Anchor::Fresh => found("Interop.Fresh()".to_string()),
        }
    }

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
        // The plain question a call that cannot fail asks instead of
        // throwing, since it has no way to report the mistake.
        let mut guarded: Vec<String> = Vec::new();
        let mut scratch = false;

        for (index, param) in function.params.iter().enumerate() {
            let local = format!("c{}", names::pascal(&param.name));
            // The object the call is anchored on is the call's receiver in
            // C#, wherever the C ABI happens to put it.
            if self.anchor == Anchor::Argument(index) {
                args.push(self.receiver.clone());
                continue;
            }
            match param.role {
                ParamRole::DocumentIn | ParamRole::DocumentMut => {
                    if self.anchor == Anchor::Root {
                        params.push(format!("{ROOT} root"));
                    }
                    args.push("at.Pointer".to_string());
                }
                ParamRole::DocumentTaken => {
                    return Err(format!(
                        "`{}` consumes a document, so it cannot be emitted mechanically; hide \
                         it and write what a caller needs by hand",
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
                ParamRole::Error => args.push("TODO_OUT_ERROR".to_string()),
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
                    if self.builds && param.ty == Type::Node {
                        // A constructor hands its base the arena it built in
                        // and the handle it got, which is what a Site is.
                        results.push((
                            member_case(bare),
                            "Site".to_string(),
                            format!("new Site(at.Arena, {out})"),
                        ));
                    } else {
                        results.push((
                            member_case(bare),
                            sharp_type(&param.ty, self.shadowed),
                            from_c(&param.ty, &out, self.holder(), self.shadowed),
                        ));
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
                from_c(ty, "answer", self.holder(), self.shadowed),
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
        for line in self.reach() {
            lines.push(&line);
        }
        // A call that cannot fail has no error to hand back, so it asks the
        // plain question and answers no rather than throwing.
        for asked in &guarded {
            let zero = match &function.result {
                CResult::Value(ty) => sharp_zero(ty)?,
                other => {
                    return Err(format!(
                        "`{}` takes an object and returns `{other:?}`, so it has no way to say \
                         the object came from another timeline",
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
                    self.holder(),
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
        guarded: &mut Vec<String>,
        scratch: &mut bool,
    ) -> Result<(), String> {
        // What the call does with an object it is handed is the description's
        // answer and not this backend's: the same question decides the same
        // way in every binding that hides the document. Getting it backwards
        // is silent — moving an object the call was only going to name
        // swallows the timeline it came from.
        let bring = || match param.placement {
            Some(Placement::Adopt) => Ok("Adopt"),
            Some(Placement::Require) => Ok("RequireHere"),
            None => Err(format!(
                "`{}` takes `{}` as an object and the description does not say what it does \
                 with it",
                self.function.symbol, param.name
            )),
        };
        // A handle is an index into one arena, and two arenas issue the same
        // indices, so an object from elsewhere would resolve to an unrelated
        // object here rather than failing. Only the C# value knows where it
        // came from, so every object a caller supplies is checked. A call that
        // answers with a plain value has no exception to throw, so it is asked
        // the plain question instead.
        if self.anchor != Anchor::None && !self.function.fallible() {
            match &param.ty {
                Type::Node => guarded.push(format!("Interop.Here(at, {sharp})")),
                Type::List(inner) if **inner == Type::Node => {
                    guarded.push(format!("Interop.HereAll(at, {sharp})"));
                }
                _ => {}
            }
        }
        if self.anchor != Anchor::None {
            // A call that cannot fail has already asked `Here` and answered no
            // where the object came from elsewhere, so by now there is nothing
            // left to refuse and nothing to throw with.
            let plain = !self.function.fallible();
            if plain && param.placement == Some(Placement::Adopt) {
                return Err(format!(
                    "`{}` places `{}` and cannot fail, so it has no way to report a move it \
                     could not make",
                    self.function.symbol, param.name
                ));
            }
            match &param.ty {
                Type::Node => {
                    params.push(format!(
                        "{ROOT}{} {sharp}",
                        if param.optional { "?" } else { "" }
                    ));
                    pre.push(if plain {
                        format!("var {local} = Interop.HandleOf(at, {sharp});")
                    } else {
                        format!("var {local} = Interop.{}(at, {sharp});", bring()?)
                    });
                    args.push(local.to_string());
                    return Ok(());
                }
                Type::List(inner) if **inner == Type::Node => {
                    params.push(format!("{ROOT}[] {sharp}"));
                    pre.push(if plain {
                        format!("var {local} = Interop.HandlesOf(at, {sharp});")
                    } else {
                        format!("var {local} = Interop.{}All(at, {sharp});", bring()?)
                    });
                    args.push(local.to_string());
                    *length = Some(format!("(nuint){sharp}.Length"));
                    return Ok(());
                }
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
        // The collector may take the last reference to an arena at its last
        // use, which is the line that reads its pointer. The call has to
        // happen while it is still alive.
        let keep = if self.anchor == Anchor::None {
            None
        } else {
            Some("GC.KeepAlive(at.Arena);".to_string())
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
                ParamRole::DocumentIn | ParamRole::DocumentMut => {
                    args.push("at.Pointer".to_string());
                }
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

/// Gives the trailing run of parameters that may be left out a default of
/// none, so that `new Clip()` is a thing to write.
///
/// C# takes a default only on a trailing parameter, which is why this stops
/// at the last one that must be given.
fn defaulted(params: &mut [String]) {
    for param in params.iter_mut().rev() {
        let Some((ty, _)) = param.split_once(' ') else {
            return;
        };
        if !ty.ends_with('?') {
            return;
        }
        param.push_str(" = null");
    }
}

/// The name out of a declared parameter, to hand it on with.
fn argument_of(param: &str) -> String {
    param
        .split(" = ")
        .next()
        .unwrap_or(param)
        .rsplit(' ')
        .next()
        .unwrap_or(param)
        .to_string()
}

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

/// The calls the document took with it when it left the surface.
///
/// Each says why it is not there, because "it is missing" and "it is gone on
/// purpose" look the same from outside. A symbol here is still in the
/// description and still checked for a name collision, so hiding one cannot
/// quietly drop a call the interface grew later.
const HIDDEN: &[(&str, &str)] = &[
    (
        "otio_document_absorb",
        "how an object built on its own joins a timeline, which appending it does",
    ),
    (
        "otio_document_clone",
        "copying an object is DeepClone, which is the question a caller has",
    ),
    (
        "otio_document_new",
        "an arena is made for each object built",
    ),
    ("otio_document_node_count", "how big the arena is"),
    ("otio_document_root", "what reading a file answers with"),
    (
        "otio_document_set_root",
        "where writing starts, which is the object given",
    ),
    (
        "otio_document_to_json",
        "ToJSON, which serialises from wherever it is pointed",
    ),
];

/// The calls the C ABI hangs off the document that are really about one of
/// the objects they are handed, and where they go instead.
///
/// Each is `(symbol, owner, name)`. The owner is spelled as `owner_of` spells
/// it, so the collision check sees them where a caller does.
const REHOMED: &[(&str, &str, &str)] = &[
    (
        "otio_document_contains",
        "object:SerializableObject",
        "IsLive",
    ),
    (
        "otio_document_deep_clone",
        "object:SerializableObject",
        "DeepClone",
    ),
    (
        "otio_document_remove",
        "object:SerializableObject",
        "RemoveFromTimeline",
    ),
    (
        "otio_document_remove_recursive",
        "object:SerializableObject",
        "RemoveFromTimelineRecursive",
    ),
];

/// Where a rehomed call goes, if it is one.
fn rehomed(symbol: &str) -> Option<(&'static str, &'static str)> {
    REHOMED
        .iter()
        .find(|(name, _, _)| *name == symbol)
        .map(|(_, owner, member)| (*owner, *member))
}

/// Names this SDK writes by hand, which a generated one may not take.
const RESERVED: &[(&str, &str)] = &[
    ("Otio", "Open"),
    ("Otio", "Save"),
    ("object:SerializableObject", "Arena"),
    ("object:SerializableObject", "Handle"),
    ("object:SerializableObject", "Close"),
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
        // Where the object the call is about, and so the arena it is made in,
        // comes from. `at` is that object resolved: the arena holding it now,
        // that arena's document pointer, and its handle there.
        let mut anchor = Anchor::None;
        let mut builds = false;

        let receiver = match (&group.receiver, function.role) {
            (Receiver::None, _) => {
                is_static = true;
                String::new()
            }
            (Receiver::Document, Role::Constructor | Role::Free) => {
                is_static = true;
                String::new()
            }
            (Receiver::Document, _) if !takes_a_document(function) => {
                is_static = true;
                String::new()
            }
            (Receiver::Document, _) => {
                match rehomed(&function.symbol) {
                    // A call the C ABI hangs off the document is about one of
                    // the objects it is handed, so in C# it hangs off that.
                    Some(_) => anchor = Anchor::Argument(anchor_index(function)?),
                    None => {
                        is_static = true;
                        match function.params.iter().position(|param| param.anchor) {
                            Some(index) => {
                                let named = parameter_name(&function.params[index].name);
                                anchor = if matches!(function.params[index].ty, Type::List(_)) {
                                    Anchor::List(named)
                                } else {
                                    Anchor::Named(named)
                                };
                            }
                            // Writing is the one thing left that wants a whole
                            // timeline and is handed no object to find it by,
                            // so it takes one and starts there.
                            None => anchor = Anchor::Root,
                        }
                    }
                }
                "at.Handle".to_string()
            }
            // An object is built in an arena of its own, and moves into a
            // timeline's when it is put in one. That is what lets a clip exist
            // before the track it is going to sit on.
            (Receiver::Node(_), Role::Constructor) if takes_a_document(function) => {
                builds = true;
                anchor = Anchor::Fresh;
                String::new()
            }
            (Receiver::Node(_), Role::Constructor) => {
                is_static = true;
                String::new()
            }
            (Receiver::Node(_), _) if group.view => {
                anchor = Anchor::Receiver("this.Object".to_string());
                "at.Handle".to_string()
            }
            (Receiver::Node(_), _) => {
                anchor = Anchor::Receiver("this".to_string());
                "at.Handle".to_string()
            }
            (Receiver::Value(_), Role::Constructor | Role::Free) => {
                is_static = true;
                String::new()
            }
            (Receiver::Value(what), _) => {
                if self.api.enumeration(what).is_some() {
                    // An enum carries no members of its own, so its calls are
                    // extension methods and the receiver is the argument.
                    is_static = true;
                    extension = Some(format!("this {} subject", enum_name(what)));
                    "subject".to_string()
                } else if self.needs_scratch(what) {
                    "this.ToNative(scratch)".to_string()
                } else {
                    "this.ToNative()".to_string()
                }
            }
        };

        let site = Site {
            api: self.api,
            shadowed: &self.shadowed,
            function,
            anchor,
            receiver,
            builds,
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
        if builds {
            if rendered.result != "Site" {
                return Err(format!(
                    "`{}` builds an object and answers with `{}` rather than one handle",
                    function.symbol, rendered.result
                ));
            }
            // C# runs a base constructor before the body, so what the object
            // is built from has to be worked out before there is a `this` to
            // put it on. That is a static of its own, and the constructor is
            // the one line that hands its answer up.
            defaulted(&mut params);
            let made = format!("Make{name}");
            write_member(
                out,
                &["/// <summary>Builds the object in an arena of its own.</summary>".to_string()],
                &format!("private static Site {made}({})", params.join(", ")),
                &rendered.body,
            );
            let handed: Vec<String> = params.iter().map(|param| argument_of(param)).collect();
            write_member(
                out,
                &doc,
                &format!(
                    "public {name}({})\n{TAB}{TAB}: base({made}({}))",
                    params.join(", "),
                    handed.join(", ")
                ),
                &[],
            );
            return Ok(());
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
                ParamRole::Error => format!("TODO_OUT_ERROR {name}"),
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
        // `Otio` is partial because the runtime writes Open and Save onto it
        // and the rest of it is generated.
        let _ = writeln!(out, "public static partial class Otio\n{{");
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
        // Nothing hangs off the document any more, so what the C ABI hung
        // there is either about an object it is handed — rehomed onto
        // SerializableObject — or a whole-timeline call with no object to hang
        // off, which becomes a static member of Otio.
        for group in &self.api.groups {
            if group.receiver != Receiver::Document {
                continue;
            }
            self.emit_some(&mut body, group, |function| {
                rehomed(&function.symbol).is_none()
            })?;
        }
        out.push_str(body.trim_end());
        let _ = writeln!(out, "\n}}\n");

        let mut rehomed_body = String::new();
        for group in &self.api.groups {
            if group.receiver != Receiver::Document {
                continue;
            }
            self.emit_some(&mut rehomed_body, group, |function| {
                rehomed(&function.symbol).is_some()
            })?;
        }
        if !rehomed_body.trim().is_empty() {
            let _ = writeln!(out, "public partial class {ROOT}\n{{");
            out.push_str(rehomed_body.trim_end());
            let _ = writeln!(out, "\n}}\n");
        }
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
    fn schema(&self) -> Result<String, String> {
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
                "{TAB}internal {sharp}(Arena? arena, Native.OtioNode handle)"
            );
            let _ = writeln!(out, "{TAB}{TAB}: base(arena, handle)");
            let _ = writeln!(out, "{TAB}{{");
            let _ = writeln!(out, "{TAB}}}\n");
            // What a constructor of this class, or of one below it, built.
            let _ = writeln!(out, "{TAB}internal {sharp}(Site made)");
            let _ = writeln!(out, "{TAB}{TAB}: base(made)");
            let _ = writeln!(out, "{TAB}{{");
            let _ = writeln!(out, "{TAB}}}\n");
            // C# does not inherit constructors, so each schema declares its
            // own and `Clip(name)` and `Item(name)` cannot collide.
            out.push_str(self.constructors(&schema.name)?.trim_end());
            let _ = writeln!(out, "\n}}\n");
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
             {TAB}internal static {ROOT} Make(Arena? arena, Native.OtioNode handle)\n\
             {TAB}{{\n\
             {TAB}{TAB}if (arena is null || arena.Pointer == IntPtr.Zero)\n\
             {TAB}{TAB}{{\n\
             {TAB}{TAB}{TAB}return new {ROOT}(arena, handle);\n\
             {TAB}{TAB}}}\n\
             {TAB}{TAB}var status = Native.otio_node_kind(arena.Pointer, handle, out var kind);\n\
             {TAB}{TAB}GC.KeepAlive(arena);\n\
             {TAB}{TAB}if (status != Status.Ok)\n\
             {TAB}{TAB}{{\n\
             {TAB}{TAB}{TAB}return new {ROOT}(arena, handle);\n\
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
                "{TAB}{TAB}{TAB}NodeKind.{} => new {}(arena, handle),",
                variant_name(&schema.kind),
                schema_name(&schema.name)
            );
        }
        let _ = writeln!(out, "{TAB}{TAB}{TAB}_ => new {ROOT}(arena, handle),");
        let _ = writeln!(out, "{TAB}{TAB}}};");
        let _ = writeln!(out, "{TAB}}}");
        let _ = writeln!(out, "}}\n");
        Ok(out)
    }

    /// The constructors that build one schema, for its own class body.
    fn constructors(&self, schema: &str) -> Result<String, String> {
        let mut out = String::new();
        for group in &self.api.groups {
            if group.receiver != Receiver::Node(schema.to_string()) || group.view {
                continue;
            }
            self.emit_some(&mut out, group, |function| {
                function.role == Role::Constructor && takes_a_document(function)
            })?;
        }
        Ok(out)
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
            // A constructor is written into the class's own body, beside the
            // one the runtime uses, which `schema()` writes.
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
    internal static SerializableObject MakeObject(Arena? arena, Native.OtioNode handle) =>
        Schemas.Make(arena, handle);

    /// <summary>A handle as one number, so a translation table can be looked up.</summary>
    internal static ulong KeyOf(Native.OtioNode handle) =>
        ((ulong)handle.index << 32) | handle.generation;

    /// <summary>Makes an empty arena, for an object about to be built.</summary>
    internal static Arena NewArena()
    {
        var pointer = Native.otio_document_new();
        if (pointer == IntPtr.Zero)
        {
            throw new OtioException(
                Status.CoreError, "otio: the library could not make a timeline");
        }
        return new Arena(pointer);
    }

    /// <summary>Moves every object of one arena into another.</summary>
    /// <remarks>
    /// <para>
    /// The call consumes what it is given: it frees the source and answers with
    /// a table saying where each of its objects went. The source is left marked
    /// as moved rather than forgotten, so an object still naming it is
    /// translated through the table instead of going stale.
    /// </para>
    /// <para>
    /// C: <c>otio_document_absorb</c>
    /// </para>
    /// </remarks>
    internal static void Absorb(Arena target, Arena source)
    {
        if (target.Pointer == IntPtr.Zero || source.Pointer == IntPtr.Zero)
        {
            throw new OtioException(Status.NullPointer, "otio: the timeline has been released");
        }
        // The call cannot be asked twice to size its answer, because the first
        // ask would already have consumed the source. The source's own count is
        // exactly how many objects will move.
        var moving = (int)Native.otio_document_node_count(source.Pointer);
        var from = new Native.OtioNode[moving];
        var to = new Native.OtioNode[moving];
        var taking = source.Pointer;
        var status = Native.otio_document_absorb(
            target.Pointer, ref taking, from, to, (nuint)moving, out var count);
        // The library released the source and nulled the slot, so nothing here
        // may free it a second time.
        source.Taken(taking);
        GC.KeepAlive(target);
        Check(status);
        var moved = Math.Min((int)count, moving);
        for (int index = 0; index < moved; index++)
        {
            source.Translation[KeyOf(from[index])] = to[index];
        }
        source.MovedInto = target;
    }

    /// <summary>Follows the chain to where an object's arena, and its handle, are now.</summary>
    /// <remarks>
    /// <para>
    /// A handle means nothing outside the arena that issued it, and absorbing
    /// reissues every one of them, so an object held from before a move is
    /// translated a step at a time along the chain.
    /// </para>
    /// </remarks>
    internal static Site Locate(SerializableObject obj)
    {
        var arena = obj.Arena;
        var handle = obj.Handle;
        // Iteratively: a timeline assembled an object at a time has a chain as
        // long as it has objects, and a stack overflow would be a ridiculous
        // way to fail.
        while (arena?.MovedInto is Arena next)
        {
            if (arena.Translation.TryGetValue(KeyOf(handle), out var moved))
            {
                handle = moved;
            }
            arena = next;
        }
        return new Site(arena, handle);
    }

    /// <summary>Where a call handed a list of objects and nothing else is made.</summary>
    /// <remarks>
    /// <para>
    /// The objects are checked one at a time as they are handed over, so this
    /// only has to say where the call happens; an empty list says nothing,
    /// which is the one thing it cannot answer.
    /// </para>
    /// </remarks>
    internal static Site LocateAll(SerializableObject[] objects)
    {
        if (objects.Length == 0)
        {
            throw new OtioException(
                Status.InvalidArgument,
                "otio: no objects were given, so there is no timeline to work in");
        }
        return Locate(objects[0]);
    }

    /// <summary>Where a call that writes a whole timeline out starts.</summary>
    /// <remarks>
    /// <para>
    /// The C interface writes a document from its root. An object read out of a
    /// file is already that root; one built here is not, so it is made so —
    /// which is what writing a track rather than a whole timeline means.
    /// </para>
    /// </remarks>
    internal static Site RootedAt(SerializableObject obj)
    {
        var at = Locate(obj);
        var status = Native.otio_document_set_root(at.Pointer, at.Handle);
        GC.KeepAlive(at.Arena);
        Check(status);
        return at;
    }

    /// <summary>An arena for something about to be built.</summary>
    internal static Site Fresh() => new Site(NewArena(), Native.otio_node_none());

    /// <summary>What a whole document just read is about, as an object of its own arena.</summary>
    internal static SerializableObject RootOf(IntPtr taken)
    {
        if (taken == IntPtr.Zero)
        {
            throw new OtioException(Status.NullPointer, "otio: nothing was read");
        }
        var arena = new Arena(taken);
        var status = Native.otio_document_root(taken, out var handle);
        GC.KeepAlive(arena);
        Check(status);
        return MakeObject(arena, handle);
    }

    /// <summary>Whether an object is one this call may be handed.</summary>
    /// <remarks>
    /// <para>
    /// A handle is an index into one arena, and two arenas issue the same
    /// indices, so an object from elsewhere would resolve to an unrelated
    /// object here rather than failing. Nothing in the handle says where it
    /// came from: the C# object carries that, and this is where it is used. An
    /// object of no arena means "no object", so it is allowed everywhere.
    /// </para>
    /// </remarks>
    internal static bool Here(Site at, SerializableObject? obj)
    {
        if (obj is null)
        {
            return true;
        }
        var theirs = Locate(obj);
        return theirs.Arena is null || ReferenceEquals(theirs.Arena, at.Arena);
    }

    /// <summary>Here, for a whole list of objects.</summary>
    internal static bool HereAll(Site at, SerializableObject[] objects)
    {
        foreach (var obj in objects)
        {
            if (!Here(at, obj))
            {
                return false;
            }
        }
        return true;
    }

    /// <summary>The handle of an object this call only names, or a refusal.</summary>
    /// <remarks>
    /// <para>
    /// Used by the calls that do not place what they are given. An object from
    /// another timeline is not in this one and the honest answer is to say so,
    /// rather than to move it because somebody asked whether it was here. The
    /// refusal is made before the library is asked, so nothing has moved when
    /// it throws.
    /// </para>
    /// </remarks>
    internal static Native.OtioNode RequireHere(Site at, SerializableObject? obj)
    {
        if (obj is null)
        {
            return Native.otio_node_none();
        }
        var theirs = Locate(obj);
        if (theirs.Arena is null)
        {
            return Native.otio_node_none();
        }
        if (!ReferenceEquals(theirs.Arena, at.Arena))
        {
            throw new OtioException(
                Status.InvalidArgument,
                "otio: the object belongs to another timeline; put it in this one first");
        }
        return theirs.Handle;
    }

    /// <summary>RequireHere, for a whole list of objects.</summary>
    internal static Native.OtioNode[] RequireHereAll(Site at, SerializableObject[] objects)
    {
        var handles = new Native.OtioNode[objects.Length];
        for (int index = 0; index < objects.Length; index++)
        {
            handles[index] = RequireHere(at, objects[index]);
        }
        return handles;
    }

    /// <summary>The handle of an object this call places, moving it here if it is not.</summary>
    /// <remarks>
    /// <para>
    /// This is where <c>new Clip("shot_01")</c> followed by
    /// <c>track.AppendChild(clip)</c> turns into one timeline rather than two.
    /// </para>
    /// </remarks>
    internal static Native.OtioNode Adopt(Site at, SerializableObject? obj)
    {
        if (obj is null)
        {
            return Native.otio_node_none();
        }
        var theirs = Locate(obj);
        if (theirs.Arena is not Arena mine)
        {
            return Native.otio_node_none();
        }
        if (ReferenceEquals(mine, at.Arena))
        {
            return theirs.Handle;
        }
        if (at.Arena is not Arena target)
        {
            throw new OtioException(Status.NullPointer, "otio: the timeline has been released");
        }
        Absorb(target, mine);
        return Locate(obj).Handle;
    }

    /// <summary>Adopt, for a whole list of objects.</summary>
    internal static Native.OtioNode[] AdoptAll(Site at, SerializableObject[] objects)
    {
        var handles = new Native.OtioNode[objects.Length];
        for (int index = 0; index < objects.Length; index++)
        {
            handles[index] = Adopt(at, objects[index]);
        }
        return handles;
    }

    /// <summary>The handle an object answers to here, for a call that cannot fail.</summary>
    /// <remarks>
    /// <para>
    /// Such a call has no exception to throw, so it asks Here first and answers
    /// no where the object came from somewhere else. By the time this is
    /// reached the object is known to belong here, and an object of no arena is
    /// "no object", so there is nothing left to refuse.
    /// </para>
    /// </remarks>
    internal static Native.OtioNode HandleOf(Site at, SerializableObject? obj)
    {
        if (obj is null)
        {
            return Native.otio_node_none();
        }
        var theirs = Locate(obj);
        return theirs.Arena is null ? Native.otio_node_none() : theirs.Handle;
    }

    /// <summary>HandleOf, for a whole list of objects.</summary>
    internal static Native.OtioNode[] HandlesOf(Site at, SerializableObject[] objects)
    {
        var handles = new Native.OtioNode[objects.Length];
        for (int index = 0; index < objects.Length; index++)
        {
            handles[index] = HandleOf(at, objects[index]);
        }
        return handles;
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

/// The arena, the object handle, the error type, and the two calls this SDK
/// writes itself.
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

/// <summary>The arena the core keeps a timeline's objects in.</summary>
/// <remarks>
/// <para>
/// It is not part of this SDK's surface. An object carries the arena it lives
/// in, a new object starts in one of its own, and putting an object into a
/// timeline moves it into the timeline's — so what a caller is left holding is
/// objects. The arena goes when the last object naming it does, or earlier if
/// somebody says Close.
/// </para>
/// </remarks>
internal sealed class Arena
{
    private IntPtr pointer;

    internal Arena(IntPtr pointer)
    {
        this.pointer = pointer;
    }

    /// <summary>Releases the arena if nobody released it first.</summary>
    ~Arena()
    {
        this.Release();
    }

    /// <summary>
    /// The arena the C interface knows, or zero once it is closed or its
    /// objects have moved elsewhere.
    /// </summary>
    internal IntPtr Pointer => this.pointer;

    /// <summary>Where this arena's objects went, once another absorbed them.</summary>
    internal Arena? MovedInto { get; set; }

    /// <summary>What each of this arena's handles became on the way over.</summary>
    internal Dictionary<ulong, Native.OtioNode> Translation { get; } = new();

    /// <summary>
    /// Records what the library left in the slot it was handed, so that an
    /// arena it took over and freed is not freed a second time.
    /// </summary>
    /// <remarks>
    /// <para>
    /// A call that failed leaves the arena where it was, and this says so too:
    /// the finalizer is only let go once there is nothing left to free.
    /// </para>
    /// </remarks>
    internal void Taken(IntPtr left)
    {
        this.pointer = left;
        if (left == IntPtr.Zero)
        {
            GC.SuppressFinalize(this);
        }
    }

    /// <summary>Releases the arena and everything in it.</summary>
    /// <remarks>
    /// <para>
    /// Closing twice is harmless, and every object that lived here fails
    /// afterwards rather than reading freed memory: the pointer is zeroed, and
    /// the C interface refuses a null document.
    /// </para>
    /// </remarks>
    internal void Close()
    {
        this.Release();
        GC.SuppressFinalize(this);
    }

    private void Release()
    {
        if (this.pointer != IntPtr.Zero)
        {
            Native.otio_document_free(this.pointer);
            this.pointer = IntPtr.Zero;
        }
    }
}

/// <summary>
/// An object resolved: the arena holding it now, that arena's document, and
/// the handle it answers to there.
/// </summary>
internal readonly struct Site
{
    internal Site(Arena? arena, Native.OtioNode handle)
    {
        this.Arena = arena;
        this.Handle = handle;
    }

    /// <summary>The arena, for keeping it alive across the call.</summary>
    internal Arena? Arena { get; }

    /// <summary>The handle the object answers to in that arena.</summary>
    internal Native.OtioNode Handle { get; }

    /// <summary>The document the C interface knows, or zero once it has gone.</summary>
    internal IntPtr Pointer => this.Arena?.Pointer ?? IntPtr.Zero;
}

/// <summary>An object in a timeline: a clip, a track, a timeline, a marker.</summary>
/// <remarks>
/// <para>
/// Objects are built on their own and put together afterwards:
/// </para>
/// <code>
/// var track = new Track("V1", "Video");
/// var clip = new Clip("shot_01");
/// track.AppendChild(clip);
/// </code>
/// <para>
/// Behind that, the core keeps its objects in arenas and an object is an index
/// into one. This SDK does that bookkeeping: a new object gets an arena of its
/// own, and putting it into a timeline moves it into the timeline's. An object
/// holds the arena it lives in, so the timeline lasts as long as anything
/// naming it, and Close ends it sooner where the moment matters. An object of a
/// closed timeline names nothing and every call on it fails rather than reading
/// freed memory.
/// </para>
/// <para>
/// It is the root of the OTIO schema ladder, and every schema below it is a
/// class deriving from it, so a Clip has every member of an Item, a Composable
/// and a SerializableObjectWithMetadata. Every handle the library hands back
/// arrives as the class its schema names, so <c>node as Clip</c> asks what an
/// object really is and gets a true answer.
/// </para>
/// <para>
/// Two objects are equal when they are the same object of the same timeline. A
/// handle is a value here, so there may be several wrappers for one object and
/// equality is the question worth asking.
/// </para>
/// </remarks>
public partial class SerializableObject
{
    internal SerializableObject(Arena? arena, Native.OtioNode handle)
    {
        this.Arena = arena;
        this.Handle = handle;
    }

    /// <summary>What a constructor built, as its base receives it.</summary>
    internal SerializableObject(Site made)
        : this(made.Arena, made.Handle)
    {
    }

    /// <summary>
    /// The arena the object was issued in. This is the plumbing: Interop.Locate
    /// follows it to wherever its objects are now.
    /// </summary>
    internal Arena? Arena { get; }

    /// <summary>The handle the object is, in the arena that issued it.</summary>
    internal Native.OtioNode Handle { get; }

    /// <summary>Releases the timeline this object belongs to, and everything in it.</summary>
    /// <remarks>
    /// <para>
    /// Not required: the timeline goes when the last object naming it does.
    /// This is for code that would rather say when — a viewer opening one file
    /// after another, say. Closing twice is harmless, and every object that
    /// lived in the timeline fails afterwards.
    /// </para>
    /// </remarks>
    public void Close() => Interop.Locate(this).Arena?.Close();

    /// <summary>Whether the object is of a schema, or of one deriving from it.</summary>
    /// <remarks>
    /// <para>
    /// An object whose timeline has gone, or whose handle no longer resolves,
    /// is of no schema at all, so this answers false rather than guessing.
    /// </para>
    /// </remarks>
    public bool IsA(NodeKind schema)
    {
        var at = Interop.Locate(this);
        if (at.Pointer == IntPtr.Zero)
        {
            return false;
        }
        var status = Native.otio_node_kind(at.Pointer, at.Handle, out var kind);
        GC.KeepAlive(at.Arena);
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

    /// <summary>Whether another object is the same object of the same timeline.</summary>
    /// <remarks>
    /// <para>
    /// The library's own <c>Equals(SerializableObject)</c>, generated from
    /// <c>otio_node_equal</c>, asks the same question and gets the same answer;
    /// this one is here because the runtime needs it, and it answers without a
    /// call so that it still works once the timeline has gone.
    /// </para>
    /// </remarks>
    public override bool Equals(object? other)
    {
        if (other is not SerializableObject node)
        {
            return false;
        }
        var mine = Interop.Locate(this);
        var theirs = Interop.Locate(node);
        return ReferenceEquals(mine.Arena, theirs.Arena)
            && mine.Handle.index == theirs.Handle.index
            && mine.Handle.generation == theirs.Handle.generation;
    }

    /// <inheritdoc/>
    public override int GetHashCode()
    {
        var mine = Interop.Locate(this);
        return HashCode.Combine(mine.Arena, mine.Handle.index, mine.Handle.generation);
    }

    /// <summary>Whether two wrappers name the same object of the same timeline.</summary>
    public static bool operator ==(SerializableObject? left, SerializableObject? right) =>
        left is null ? right is null : left.Equals((object?)right);

    /// <summary>Whether two wrappers name different objects.</summary>
    public static bool operator !=(SerializableObject? left, SerializableObject? right) =>
        !(left == right);
}

/// <summary>Everything the library offers that belongs to no object.</summary>
public static partial class Otio
{
    /// <summary>Reads a timeline from a file, working out its format from the name.</summary>
    /// <remarks>
    /// <para>
    /// It is the short way to say ReadFromFile when the suffix already says
    /// what the file holds, which is how upstream's read_from_file behaves when
    /// no adapter is named.
    /// </para>
    /// </remarks>
    public static SerializableObject Open(string path) =>
        Otio.ReadFromFile(Interop.FormatOf(path), path, null);

    /// <summary>Writes a timeline to a file, working out its format from the name.</summary>
    /// <remarks>
    /// <para>
    /// It is the short way to say WriteToFile, as Open is for ReadFromFile.
    /// Writing starts at the object it is given, so handing it a track writes
    /// that track rather than the timeline around it.
    /// </para>
    /// </remarks>
    public static void Save(SerializableObject root, string path) =>
        Otio.WriteToFile(Interop.FormatOf(path), root, path, null);
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

Reading a file hands back the object it is about:

```csharp
var timeline = Otio.Open("cut.edl");

foreach (var child in timeline.FindClips())
{
    var clip = (Clip)child;
    Console.WriteLine($"{clip.Name()} {clip.Duration()}");
}
```

Building one is the other direction. Every object is made on its own and joins
a timeline when you put it into one, so nothing has to exist before the thing
it goes into:

```csharp
var timeline = new Timeline("Cut");
var stack = new Stack("tracks");
var track = new Track("V1", "Video");

timeline.SetTracks(stack);
stack.AppendChild(track);
track.AppendChild(new Clip("shot_01"));

Otio.Save(timeline, "cut.otio");
```

An object that has not joined anything is a timeline of one. Putting it into
another moves it there, and an object from a timeline it was never put into is
refused rather than quietly dragged along with everything around it.

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
thing upstream has to a managed language.

There is no document in the surface, as there is none in upstream's own
bindings. Underneath, the core keeps a timeline's objects in an arena and an
object is an index into one; this SDK does that bookkeeping. The objects hold
the arena between them, so it goes when the last of them does and there is
nothing to dispose. `Close()` is there for releasing a large timeline at a
moment you chose; every object that lived in it fails afterwards rather than
reading freed memory.

Every deliberate departure is written down in
[ADR 0003](../../docs/adr/0003-sdk-generation.md).
"#;
